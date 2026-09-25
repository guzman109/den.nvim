//! den-agent: holds the unlocked vault key for a session.
//!
//! Neovim and the `den` command never hold the vault key. They ask this
//! small process, over a Unix socket that only the same user can reach, to
//! unlock the vault (with a password, the recovery key, a YubiKey or Touch
//! ID) and then to decrypt notes. The key stays here, in memory kept out of
//! swap and wiped when it is dropped. It is forgotten:
//!
//! - after `lock.forget_after_minutes` without use (15 by default),
//! - when the computer sleeps (the wall clock jumps ahead of the monotonic
//!   one),
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
    last_used: Instant,
    /// In strict mode, the connection that unlocked it.
    owner: Option<u64>,
}

struct State {
    keys: HashMap<PathBuf, Held>,
    forget_after: Option<Duration>,
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

fn settings() -> (Option<Duration>, bool) {
    let config = den_core::Config::load().unwrap_or_default();
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
    (forget, config.lock.strict)
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

    let (forget_after, strict) = settings();
    let state: Shared = Arc::new(Mutex::new(State {
        keys: HashMap::new(),
        forget_after,
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
    use std::os::unix::fs::MetadataExt;
    let dir = path.parent().ok_or("the socket path has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if !meta.is_dir() || meta.uid() != nix::unistd::getuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(format!(
            "{} must be a folder only this user can open",
            dir.display()
        ));
    }
    Ok(())
}

/// Forgets keys left unused too long, and all keys after a sleep.
fn watch(state: Shared) {
    let tick = Duration::from_secs(1);
    let mut wall = SystemTime::now();
    let mut mono = Instant::now();
    loop {
        std::thread::sleep(tick);
        let (now_wall, now_mono) = (SystemTime::now(), Instant::now());
        let wall_passed = now_wall.duration_since(wall).unwrap_or_default();
        let mono_passed = now_mono.duration_since(mono);
        let slept = wall_passed > mono_passed + Duration::from_secs(30);
        (wall, mono) = (now_wall, now_mono);
        let mut s = lock_state(&state);
        if slept {
            s.keys.clear();
            continue;
        }
        if let Some(limit) = s.forget_after {
            s.keys.retain(|_, held| held.last_used.elapsed() < limit);
        }
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
        let mut out = Zeroizing::new(
            serde_json::to_string(&response)
                .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"reply\"}".to_string()),
        );
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

fn keep(state: &Shared, conn: u64, root: PathBuf, key: VaultKey) {
    let key = Box::new(key);
    platform::keep_out_of_swap(&*key);
    let mut s = lock_state(state);
    let owner = s.strict.then_some(conn);
    s.keys.insert(
        root,
        Held {
            key,
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
        Request::SetPassword { vault, password } => {
            let root = vault_root(&vault)?;
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
            recipient,
            identity,
        } => {
            let root = vault_root(&vault)?;
            with_key(state, conn, &root, |key| {
                yubikey::wrap(&root, key, &recipient, &identity).map_err(err)
            })?;
            Ok(Response {
                ok: true,
                unlocked: true,
                ..Response::default()
            })
        }
        Request::EnableTouchId { vault } => {
            let root = vault_root(&vault)?;
            with_key(state, conn, &root, |key| {
                platform::touch_id_store(&root, key)
            })?;
            Ok(Response {
                ok: true,
                unlocked: true,
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
