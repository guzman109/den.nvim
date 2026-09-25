//! `den lock` and `den unlock`, and the helpers git calls for locked notes.
//!
//! Everything that needs the vault key goes through den-agent: this program
//! asks, the agent unwraps and decrypts, and the key never comes here.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Subcommand;
use den_core::agent::{Client, Request, UnlockMethod};
use den_core::{Vault, lock};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, String>;

#[derive(Subcommand)]
pub enum LockCommand {
    /// Whether locking is set up, the vault is unlocked, and how it unlocks.
    Status,
    /// Create the vault's key: asks for a password and shows a recovery key
    /// once.
    Setup,
    /// Lock a note: `x.md` becomes `x.md.age`. Needs no unlock.
    Note { file: PathBuf },
    /// Change the password (the vault must be unlocked).
    Password,
    /// Add your YubiKey as a way to unlock (needs age-plugin-yubikey).
    Yubikey,
    /// Keep a copy of the key in this Mac's keychain, for Touch ID.
    TouchId,
    /// Teach this clone's git to diff and merge locked notes.
    Git,
    /// New notes in this folder (and below) start locked.
    Folder { dir: PathBuf },
}

#[derive(Subcommand)]
pub enum UnlockCommand {
    /// Unlock a note for good: `x.md.age` becomes a plain `x.md`.
    Note { file: PathBuf },
}

fn agent_program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("den-agent"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("den-agent"))
}

fn agent() -> Result<Client> {
    Client::connect_or_start(&agent_program()).map_err(|e| e.to_string())
}

fn ask(prompt: &str) -> Result<Zeroizing<String>> {
    rpassword::prompt_password(prompt)
        .map(Zeroizing::new)
        .map_err(|e| format!("could not read from the terminal: {e}"))
}

/// A vault path from a file argument (relative to the current folder or
/// already relative to the vault).
fn vault_path(root: &Path, file: &Path) -> Result<String> {
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        let here = std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(file);
        if here.exists() { here } else { root.join(file) }
    };
    let abs = std::fs::canonicalize(&abs).map_err(|e| format!("{}: {e}", file.display()))?;
    let root = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    abs.strip_prefix(&root)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| format!("{} is not in the vault", file.display()))
}

/// Whether git already has earlier versions of this file.
fn in_history(root: &Path, path: &str) -> bool {
    std::process::Command::new("git")
        .args(["log", "-1", "--format=%H", "--", path])
        .current_dir(root)
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

pub fn lock(root: &Path, what: Option<LockCommand>) -> Result<()> {
    match what {
        None => {
            // Forgetting needs no agent start: none running means nothing held.
            if let Ok(mut c) = Client::connect(&agent_program()) {
                c.call(&Request::Lock).map_err(|e| e.to_string())?;
            }
            println!("locked");
            Ok(())
        }
        Some(LockCommand::Status) => {
            let r = agent()?
                .call(&Request::Status {
                    vault: root.to_path_buf(),
                })
                .map_err(|e| e.to_string())?;
            if !r.set_up {
                println!("locking is not set up (den lock setup)");
                return Ok(());
            }
            println!(
                "vault    {}",
                if r.unlocked { "unlocked" } else { "locked" }
            );
            println!("unlock   {}", r.methods.join(", "));
            if r.strict {
                println!("strict   each Neovim unlocks for itself");
            }
            Ok(())
        }
        Some(LockCommand::Setup) => {
            let first = ask("New vault password: ")?;
            let again = ask("Again: ")?;
            if *first != *again {
                return Err("the passwords differ".into());
            }
            let r = agent()?
                .call(&Request::Setup {
                    vault: root.to_path_buf(),
                    password: first.to_string(),
                })
                .map_err(|e| e.to_string())?;
            let recovery = Zeroizing::new(r.text.clone().unwrap_or_default());
            if let Ok(den) = std::env::current_exe() {
                lock::git_setup(root, &den).map_err(|e| e.to_string())?;
            }
            println!("Locking is set up, and the vault is unlocked.\n");
            println!("Your recovery key. Write it down or print it, and keep it somewhere");
            println!("safe away from this computer. It is shown only this once, and it is");
            println!("the only way back in if the password is lost:\n");
            println!("    {}\n", recovery.as_str());
            Ok(())
        }
        Some(LockCommand::Note { file }) => {
            let path = vault_path(root, &file)?;
            let vault = Vault::open(root).map_err(|e| e.to_string())?;
            let changes = vault.plan_lock(&path).map_err(|e| e.to_string())?;
            den_core::apply(root, &changes, &vault.dirty()).map_err(|e| e.to_string())?;
            println!("locked {path}.age");
            if in_history(root, &path) {
                println!(
                    "Earlier versions of {path} are still in the vault's git history, in the clear."
                );
            }
            Ok(())
        }
        Some(LockCommand::Password) => {
            let current = ask("Current vault password (or recovery key): ")?;
            let first = ask("New vault password: ")?;
            let again = ask("Again: ")?;
            if *first != *again {
                return Err("the passwords differ".into());
            }
            agent()?
                .call(&Request::SetPassword {
                    vault: root.to_path_buf(),
                    current: current.to_string(),
                    password: first.to_string(),
                })
                .map_err(|e| e.to_string())?;
            println!("password changed");
            Ok(())
        }
        Some(LockCommand::Yubikey) => {
            let (recipient, identity) = yubikey_list()?;
            let current = ask("Vault password (or recovery key): ")?;
            agent()?
                .call(&Request::AddYubikey {
                    vault: root.to_path_buf(),
                    current: current.to_string(),
                    recipient,
                    identity,
                })
                .map_err(|e| e.to_string())?;
            println!("your YubiKey can unlock the vault now (den unlock --yubikey)");
            Ok(())
        }
        Some(LockCommand::TouchId) => {
            let current = ask("Vault password (or recovery key): ")?;
            agent()?
                .call(&Request::EnableTouchId {
                    vault: root.to_path_buf(),
                    current: current.to_string(),
                })
                .map_err(|e| e.to_string())?;
            println!("Touch ID can unlock the vault on this Mac now (den unlock --touch-id)");
            Ok(())
        }
        Some(LockCommand::Folder { dir }) => {
            let path = vault_path(root, &dir)?;
            let target =
                den_core::write::resolve(root, &format!("{path}/{}", lock::LOCKED_FOLDER_MARKER))
                    .map_err(|e| e.to_string())?;
            den_core::write::write_atomically(&target, b"").map_err(|e| e.to_string())?;
            println!("new notes in {path} start locked");
            Ok(())
        }
        Some(LockCommand::Git) => {
            let den = std::env::current_exe().map_err(|e| e.to_string())?;
            lock::git_setup(root, &den).map_err(|e| e.to_string())?;
            println!("git diffs and merges locked notes in this clone");
            Ok(())
        }
    }
}

/// The first YubiKey identity `age-plugin-yubikey --list` reports.
fn yubikey_list() -> Result<(String, String)> {
    let out = std::process::Command::new("age-plugin-yubikey")
        .arg("--list")
        .output()
        .map_err(|e| format!("age-plugin-yubikey: {e} (install it first)"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let recipient = text
        .lines()
        .find_map(|l| l.split_whitespace().find(|w| w.starts_with("age1yubikey1")))
        .map(str::to_string);
    let identity = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("AGE-PLUGIN-YUBIKEY-1"))
        .map(str::to_string);
    match (recipient, identity) {
        (Some(r), Some(i)) => Ok((r, i)),
        _ => Err(
            "no YubiKey identity found; run age-plugin-yubikey once to create one, then try again"
                .into(),
        ),
    }
}

pub fn unlock(root: &Path, what: Option<UnlockCommand>, method: UnlockMethod) -> Result<()> {
    if let Some(UnlockCommand::Note { file }) = what {
        let path = vault_path(root, &file)?;
        let armored = std::fs::read_to_string(root.join(&path)).map_err(|e| e.to_string())?;
        let text = decrypt(root, &armored)?;
        let vault = Vault::open(root).map_err(|e| e.to_string())?;
        let changes = vault
            .plan_unlock(&path, &armored, &text)
            .map_err(|e| e.to_string())?;
        den_core::apply(root, &changes, &vault.dirty()).map_err(|e| e.to_string())?;
        println!("{path} is a plain note again");
        return Ok(());
    }
    let secret = match method {
        UnlockMethod::Password => Some(ask("Vault password: ")?),
        UnlockMethod::Recovery => Some(ask("Recovery key: ")?),
        UnlockMethod::Yubikey => {
            let pin = ask("YubiKey PIN (enter if none): ")?;
            (!pin.is_empty()).then_some(pin)
        }
        UnlockMethod::TouchId => None,
    };
    if method == UnlockMethod::Yubikey {
        eprintln!("touch your YubiKey when it blinks");
    }
    agent()?
        .call(&Request::Unlock {
            vault: root.to_path_buf(),
            method,
            secret: secret.map(|s| s.to_string()),
        })
        .map_err(|e| e.to_string())?;
    println!("unlocked");
    Ok(())
}

fn decrypt(root: &Path, armored: &str) -> Result<Zeroizing<String>> {
    let r = agent()?
        .call(&Request::Decrypt {
            vault: root.to_path_buf(),
            text: armored.to_string(),
        })
        .map_err(|e| e.to_string())?;
    Ok(Zeroizing::new(r.text.clone().unwrap_or_default()))
}

/// `den show`: a locked note on standard output.
pub fn show(root: &Path, file: &Path) -> Result<()> {
    let path = vault_path(root, file)?;
    let armored = std::fs::read_to_string(root.join(&path)).map_err(|e| e.to_string())?;
    let text = decrypt(root, &armored)?;
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

fn repo_root() -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|d| std::fs::canonicalize(d).ok())
        .unwrap_or_default()
}

/// git's textconv: a placeholder, or the decrypted note when the vault is
/// unlocked and `DEN_SHOW_LOCKED=1` asks for it. Opt-in, so a routine
/// `git log -p` by any tool never prints locked notes. Never starts the
/// agent or asks anything.
pub fn git_textconv(file: &Path) -> Result<()> {
    let placeholder = || {
        Zeroizing::new(
            "(locked note; DEN_SHOW_LOCKED=1 shows what changed while unlocked)\n".to_string(),
        )
    };
    if std::env::var("DEN_SHOW_LOCKED").as_deref() != Ok("1") {
        let mut out = std::io::stdout().lock();
        return out
            .write_all(placeholder().as_bytes())
            .map_err(|e| e.to_string());
    }
    let armored = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let text = Client::connect(&agent_program())
        .and_then(|mut c| {
            c.call(&Request::Decrypt {
                vault: repo_root(),
                text: armored,
            })
        })
        .ok()
        .and_then(|r| r.text.clone())
        .map(Zeroizing::new)
        .unwrap_or_else(placeholder);
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

/// git's merge driver for locked notes: `%O %A %B %P`. Decrypts the three
/// versions in memory, merges them, and writes the result, encrypted, to
/// `%A`. Exits with failure (a conflict) when the vault is locked or the
/// edits clash.
pub fn git_merge(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    let root = repo_root();
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| e.to_string());
    let (b, o, t) = (read(base)?, read(ours)?, read(theirs)?);
    if o == t || b == t {
        return Ok(());
    }
    if b == o {
        return den_core::write::write_atomically(ours, t.as_bytes()).map_err(|e| e.to_string());
    }
    let mut client = Client::connect(&agent_program()).map_err(|_| {
        "the vault is locked; unlock it and sync again to combine this note".to_string()
    })?;
    let mut open = |armored: String| -> Result<Zeroizing<String>> {
        client
            .call(&Request::Decrypt {
                vault: root.clone(),
                text: armored,
            })
            .map(|r| Zeroizing::new(r.text.clone().unwrap_or_default()))
            .map_err(|e| e.to_string())
    };
    let (pb, po, pt) = (open(b)?, open(o)?, open(t)?);
    let merged = Zeroizing::new(
        lock::merge_text(&pb, &po, &pt)
            .ok_or("both machines changed the same lines of this locked note")?,
    );
    let recipient = lock::recipient(&root).map_err(|e| e.to_string())?;
    let armored = lock::encrypt(&recipient, &merged).map_err(|e| e.to_string())?;
    den_core::write::write_atomically(ours, armored.as_bytes()).map_err(|e| e.to_string())
}
