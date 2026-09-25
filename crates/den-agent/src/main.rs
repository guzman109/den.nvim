//! den-agent: holds the unlocked vault key for a session.
//!
//! Neovim and the `den` command never hold the vault key. They ask this
//! small process, over a Unix socket that only the same user can reach, to
//! unlock the vault (with a password, the recovery key, a YubiKey or Touch
//! ID) and then to decrypt notes. The key stays here, in memory kept out of
//! swap and wiped when it is dropped (passwords and decrypted text pass
//! through ordinary memory on their way). It is forgotten:
//!
//! - after `lock.forget_after_minutes` without use (15 by default),
//! - `lock.max_hours` after unlocking, however much it is used (8),
//! - when the computer sleeps (the wall clock jumps ahead of the monotonic
//!   one) or the screen locks,
//! - on `den lock`,
//! - in strict mode, when the Neovim that unlocked it goes away.
//!
//! `den-agent --daemon` detaches from the terminal; clients start it that
//! way when none is running. A second copy exits at once.

mod platform;
mod yubikey;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use age::secrecy::SecretString;
use den_core::agent::{Request, Response, UnlockMethod, socket_path};
use den_core::lock::{self, VaultKey};
use zeroize::Zeroizing;

struct Held {
    key: Box<VaultKey>,
    unlocked_at: Instant,
    last_used: Instant,
    /// In strict mode, the connection that unlocked it.
    owner: Option<u64>,
}

struct State {
    keys: HashMap<PathBuf, Held>,
    forget_after: Option<Duration>,
    max_age: Duration,
    strict: bool,
}

type Shared = Arc<Mutex<State>>;

fn lock_state(state: &Shared) -> MutexGuard<'_, State> {
    // A panic elsewhere must not leave the key reachable or the agent stuck.
    state.lock().unwrap_or_else(|poisoned| {
        let mut s = poisoned.into_inner();
        s.keys.clear();
        s
    })
}

fn settings() -> (Option<Duration>, Duration, bool) {
    let config = den_core::Config::load().unwrap_or_default();
    let max_age = Duration::from_secs(u64::from(config.lock.max_hours.clamp(1, 24)) * 3600);
    let mut forget = match config.lock.forget_after_minutes {
        0 => None,
        m => Some(Duration::from_secs(u64::from(m) * 60)),
    };
    // Test builds can shorten the wait.
    if cfg!(debug_assertions)
        && let Some(s) = std::env::var("DEN_TEST_FORGET_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
    {
        forget = Some(Duration::from_secs(s));
    }
    let max_age = if cfg!(debug_assertions)
        && let Some(s) = std::env::var("DEN_TEST_MAX_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
    {
        Duration::from_secs(s)
    } else {
        max_age
    };
    (forget, max_age, config.lock.strict)
}

/// scrypt's work factor for new password copies: age's own choice (about a
/// second of work), except in test builds that ask for less.
fn work_factor() -> Option<u8> {
    if cfg!(debug_assertions) {
        std::env::var("DEN_TEST_SCRYPT_LOG_N")
            .ok()
            .and_then(|n| n.parse().ok())
    } else {
        None
    }
}

fn main() -> ExitCode {
    let daemon = std::env::args().any(|a| a == "--daemon");
    if daemon {
        let _ = nix::unistd::setsid();
    }
    platform::harden();

    let path = socket_path();
    if let Err(e) = prepare(&path) {
        eprintln!("den-agent: {e}");
        return ExitCode::FAILURE;
    }
    if UnixStream::connect(&path).is_ok() {
        // One agent per user.
        return ExitCode::SUCCESS;
    }
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("den-agent: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    let (forget_after, max_age, strict) = settings();
    let state: Shared = Arc::new(Mutex::new(State {
        keys: HashMap::new(),
        forget_after,
        max_age,
        strict,
    }));
    {
        let state = state.clone();
        std::thread::spawn(move || watch(state));
    }

    let me = nix::unistd::getuid();
    for (id, conn) in listener.incoming().enumerate() {
        let Ok(stream) = conn else { continue };
        if !platform::same_user(&stream, me) {
            continue;
        }
        let state = state.clone();
        let path = path.clone();
        std::thread::spawn(move || serve(id as u64, stream, &state, &path));
    }
    ExitCode::SUCCESS
}

/// The socket's folder must exist, belong to this user, and let nobody else
/// in.
fn prepare(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let dir = path.parent().ok_or("the socket path has no folder")?;
    // Looked at before anything is done to it: a planted symlink or someone
    // else's folder is refused, never chmodded.
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(format!(
                "{} is a symlink; refusing to use it",
                dir.display()
            ));
        }
        Ok(meta) if meta.is_dir() && meta.uid() == nix::unistd::getuid().as_raw() => {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        Ok(_) => {}
        Err(_) => {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if !meta.is_dir() || meta.uid() != nix::unistd::getuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(format!(
            "{} must be a folder only this user can open",
            dir.display()
        ));
    }
    Ok(())
}

/// Forgets keys left unused too long, and all keys after a sleep or while
/// the screen is locked.
fn watch(state: Shared) {
    let tick = Duration::from_secs(1);
    let mut ticks: u64 = 0;
    let mut wall = SystemTime::now();
    let mut mono = Instant::now();
    loop {
        std::thread::sleep(tick);
        let (now_wall, now_mono) = (SystemTime::now(), Instant::now());
        let wall_passed = now_wall.duration_since(wall).unwrap_or_default();
        let mono_passed = now_mono.duration_since(mono);
        let slept = wall_passed > mono_passed + Duration::from_secs(30);
        (wall, mono) = (now_wall, now_mono);
        ticks += 1;
        // The screen is asked about every few seconds, and only while there
        // is a key to forget.
        let holding = !lock_state(&state).keys.is_empty();
        let locked = holding && ticks % 3 == 0 && platform::screen_locked();
        let mut s = lock_state(&state);
        if slept || locked {
            s.keys.clear();
            continue;
        }
        if let Some(limit) = s.forget_after {
            s.keys.retain(|_, held| held.last_used.elapsed() < limit);
        }
        let max_age = s.max_age;
        s.keys
            .retain(|_, held| held.unlocked_at.elapsed() < max_age);
    }
}

fn serve(id: u64, stream: UnixStream, state: &Shared, socket: &Path) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;
    loop {
        let mut line = Zeroizing::new(String::new());
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                if matches!(request, Request::Stop) {
                    lock_state(state).keys.clear();
                    let _ = writer.write_all(b"{\"ok\":true}\n");
                    let _ = std::fs::remove_file(socket);
                    std::process::exit(0);
                }
                handle(id, request, state)
            }
            Err(e) => Response::fail(format!("den-agent could not read the request: {e}")),
        };
        let mut response = response;
        let mut out = Zeroizing::new(
            serde_json::to_string(&response)
                .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"reply\"}".to_string()),
        );
        // Decrypted text and recovery keys are wiped once sent.
        if let Some(text) = response.text.take() {
            drop(Zeroizing::new(text));
        }
        drop(response);
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            break;
        }
    }
    // Strict mode: the key goes with the session that unlocked it.
    let mut s = lock_state(state);
    if s.strict {
        s.keys.retain(|_, held| held.owner != Some(id));
    }
}

fn vault_root(vault: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(vault).map_err(|e| format!("{}: {e}", vault.display()))
}

fn secret(s: Option<String>) -> Option<SecretString> {
    s.map(SecretString::from)
}

fn handle(conn: u64, request: Request, state: &Shared) -> Response {
    match respond(conn, request, state) {
        Ok(r) => r,
        Err(e) => Response::fail(e),
    }
}

/// Runs `f` with this connection's key for the vault, if unlocked.
fn with_key<T>(
    state: &Shared,
    conn: u64,
    root: &Path,
    f: impl FnOnce(&VaultKey) -> Result<T, String>,
) -> Result<T, String> {
    let mut s = lock_state(state);
    let strict = s.strict;
    let held = s
        .keys
        .get_mut(root)
        .filter(|h| !strict || h.owner == Some(conn))
        .ok_or("the vault is locked")?;
    held.last_used = Instant::now();
    f(&held.key)
}

/// The person proves again that it is them (the current password, or the
/// recovery key) before a new way to unlock is added: an unlocked vault is
/// not enough, or any program running meanwhile could add its own.
fn confirm(root: &Path, current: String) -> Result<(), String> {
    let current = SecretString::from(current);
    let held = lock::unwrap_password(root, current.clone())
        .or_else(|_| lock::unwrap_recovery(root, &current))
        .map_err(|_| "that is not the vault password or the recovery key".to_string())?;
    drop(held);
    Ok(())
}

fn keep(state: &Shared, conn: u64, root: PathBuf, key: VaultKey) {
    // This clone remembers the real key's public half, so a swapped key
    // file in the synced vault is noticed (see den_core::lock::recipient).
    let _ = lock::pin(&root, &key);
    let key = Box::new(key);
    platform::keep_out_of_swap(&*key);
    let mut s = lock_state(state);
    let owner = s.strict.then_some(conn);
    s.keys.insert(
        root,
        Held {
            key,
            unlocked_at: Instant::now(),
            last_used: Instant::now(),
            owner,
        },
    );
}

fn respond(conn: u64, request: Request, state: &Shared) -> Result<Response, String> {
    let err = |e: den_core::Error| e.to_string();
    match request {
        Request::Status { vault } => {
            let root = vault_root(&vault)?;
            let s = lock_state(state);
            let unlocked = s
                .keys
                .get(&root)
                .is_some_and(|h| !s.strict || h.owner == Some(conn));
            let mut methods: Vec<String> = lock::methods(&root)
                .into_iter()
                .map(|m| {
                    match m {
                        lock::Method::Password => "password",
                        lock::Method::Recovery => "recovery",
                        lock::Method::Yubikey => "yubikey",
                    }
                    .to_string()
                })
                .collect();
            if lock::is_set_up(&root) && platform::touch_id_ready(&root) {
                methods.push("touch_id".to_string());
            }
            Ok(Response {
                ok: true,
                set_up: lock::is_set_up(&root),
                unlocked,
                methods,
                strict: s.strict,
                ..Response::default()
            })
        }
        Request::Setup { vault, password } => {
            let root = vault_root(&vault)?;
            let setup =
                lock::setup(&root, SecretString::from(password), work_factor()).map_err(err)?;
            let recovery = age::secrecy::ExposeSecret::expose_secret(&setup.recovery).to_string();
            keep(state, conn, root, setup.key);
            Ok(Response {
                ok: true,
                set_up: true,
                unlocked: true,
                text: Some(recovery),
                ..Response::default()
            })
        }
        Request::Unlock {
            vault,
            method,
            secret: given,
        } => {
            let root = vault_root(&vault)?;
            let given = secret(given);
            // Slow work (scrypt, a finger, a YubiKey touch) happens without
            // holding the lock.
            let key = match method {
                UnlockMethod::Password => {
                    lock::unwrap_password(&root, given.ok_or("type the vault password")?)
                        .map_err(err)?
                }
                UnlockMethod::Recovery => {
                    lock::unwrap_recovery(&root, &given.ok_or("give the recovery key")?)
                        .map_err(err)?
                }
                UnlockMethod::Yubikey => yubikey::unwrap(&root, given).map_err(err)?,
                UnlockMethod::TouchId => platform::touch_id_unlock(&root)?,
            };
            keep(state, conn, root, key);
            Ok(Response {
                ok: true,
                set_up: true,
                unlocked: true,
                ..Response::default()
            })
        }
        Request::Decrypt { vault, text } => {
            let root = vault_root(&vault)?;
            let plain = with_key(state, conn, &root, |key| key.decrypt(&text).map_err(err))?;
            Ok(Response {
                ok: true,
                unlocked: true,
                text: Some(plain.as_str().to_string()),
                ..Response::default()
            })
        }
        Request::SetPassword {
            vault,
            current,
            password,
        } => {
            let root = vault_root(&vault)?;
            confirm(&root, current)?;
            let password = SecretString::from(password);
            with_key(state, conn, &root, |key| {
                lock::wrap_password(&root, key, password, work_factor()).map_err(err)
            })?;
            Ok(Response {
                ok: true,
                unlocked: true,
                ..Response::default()
            })
        }
        Request::AddYubikey {
            vault,
            current,
            recipient,
            identity,
        } => {
            let root = vault_root(&vault)?;
            confirm(&root, current)?;
            with_key(state, conn, &root, |key| {
                yubikey::wrap(&root, key, &recipient, &identity).map_err(err)
            })?;
            Ok(Response {
                ok: true,
                unlocked: true,
                ..Response::default()
            })
        }
        Request::EnableTouchId { vault, current } => {
            let root = vault_root(&vault)?;
            confirm(&root, current)?;
            with_key(state, conn, &root, |key| {
                platform::touch_id_store(&root, key)
            })?;
            Ok(Response {
                ok: true,
                unlocked: true,
                ..Response::default()
            })
        }
        Request::ReadConfirmed { vault, path } => {
            let root = vault_root(&vault)?;
            if !den_core::vault::classify(&path).is_some_and(|(_, locked)| locked) {
                return Err(format!("{path} is not a locked note"));
            }
            let file = den_core::write::resolve(&root, &path).map_err(err)?;
            let armored = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
            // Only an unlocked vault can be read, and never without the
            // person: the confirmation comes first, whoever unlocked it.
            if !lock_state(state).keys.contains_key(&root) {
                return Err("the vault is locked; the person has to unlock it first".into());
            }
            let shown: String = path.chars().filter(|c| !c.is_control()).take(120).collect();
            platform::confirm_presence(&format!("let an AI agent read {shown}"))?;
            let mut s = lock_state(state);
            let held = s.keys.get_mut(&root).ok_or("the vault is locked")?;
            held.last_used = Instant::now();
            let plain = held.key.decrypt(&armored).map_err(err)?;
            Ok(Response {
                ok: true,
                unlocked: true,
                text: Some(plain.as_str().to_string()),
                ..Response::default()
            })
        }
        Request::Lock => {
            lock_state(state).keys.clear();
            Ok(Response {
                ok: true,
                ..Response::default()
            })
        }
        Request::Stop => Ok(Response {
            ok: true,
            ..Response::default()
        }),
    }
}
