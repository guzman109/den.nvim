//! The agent as a program: started on a private socket, driven through the
//! same client Neovim and the `den` command use.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use den_core::agent::{Client, Request, UnlockMethod};
use den_core::lock;

struct Agent {
    child: Child,
    socket: PathBuf,
    _dir: tempfile::TempDir,
    vault: PathBuf,
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const AGENT: &str = env!("CARGO_BIN_EXE_den-agent");

fn start(config: &str, forget_seconds: Option<u64>) -> Agent {
    start_with(
        config,
        &[(
            "DEN_TEST_FORGET_SECONDS",
            forget_seconds.map(|s| s.to_string()),
        )],
    )
}

fn start_with(config: &str, extra: &[(&str, Option<String>)]) -> Agent {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let socket = base.join("run/agent.sock");
    let vault = base.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(base.join("config.yaml"), config).unwrap();
    let mut cmd = Command::new(AGENT);
    cmd.env("DEN_AGENT_SOCKET", &socket)
        .env("DEN_CONFIG", base.join("config.yaml"))
        .env("DEN_TEST_SCRYPT_LOG_N", "10");
    for (key, value) in extra {
        if let Some(v) = value {
            cmd.env(key, v);
        }
    }
    let child = cmd.spawn().unwrap();
    let started = Instant::now();
    while Client::connect_to(&socket, Some(Path::new(AGENT))).is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the agent starts"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    Agent {
        child,
        socket,
        _dir: dir,
        vault,
    }
}

fn client(a: &Agent) -> Client {
    Client::connect_to(&a.socket, Some(Path::new(AGENT))).unwrap()
}

fn status(c: &mut Client, vault: &Path) -> (bool, bool, Vec<String>) {
    let r = c
        .call(&Request::Status {
            vault: vault.to_path_buf(),
        })
        .unwrap();
    (r.set_up, r.unlocked, r.methods.clone())
}

fn setup(c: &mut Client, vault: &Path) -> String {
    c.call(&Request::Setup {
        vault: vault.to_path_buf(),
        password: "correct horse".into(),
    })
    .unwrap()
    .text
    .clone()
    .unwrap()
}

fn locked_note(vault: &Path, text: &str) -> String {
    lock::encrypt(&lock::recipient(vault).unwrap(), text).unwrap()
}

fn decrypt(c: &mut Client, vault: &Path, armored: &str) -> Result<String, String> {
    c.call(&Request::Decrypt {
        vault: vault.to_path_buf(),
        text: armored.to_string(),
    })
    .map(|r| r.text.clone().unwrap())
    .map_err(|e| e.to_string())
}

fn unlock(c: &mut Client, vault: &Path, method: UnlockMethod, secret: &str) -> Result<(), String> {
    c.call(&Request::Unlock {
        vault: vault.to_path_buf(),
        method,
        secret: Some(secret.to_string()),
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[test]
fn the_socket_is_for_this_user_only() {
    let a = start("", None);
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&a.socket), 0o600);
    assert_eq!(mode(a.socket.parent().unwrap()), 0o700);
}

#[test]
fn setup_unlocks_and_the_key_stays_in_the_agent() {
    let a = start("", None);
    let mut c = client(&a);
    assert_eq!(status(&mut c, &a.vault), (false, false, vec![]));
    let recovery = setup(&mut c, &a.vault);
    assert!(recovery.starts_with("AGE-SECRET-KEY-1"));
    assert_eq!(
        status(&mut c, &a.vault),
        (
            true,
            true,
            vec!["password".to_string(), "recovery".to_string()]
        )
    );
    let note = locked_note(&a.vault, "# Private\n\nthe plan\n");
    assert_eq!(
        decrypt(&mut c, &a.vault, &note).unwrap(),
        "# Private\n\nthe plan\n"
    );

    // Nothing on disk holds the key in the clear.
    for entry in std::fs::read_dir(a.vault.join(".den/keys")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!text.contains("AGE-SECRET-KEY"), "{text}");
    }
}

#[test]
fn lock_forgets_and_the_password_or_recovery_key_unlocks_again() {
    let a = start("", None);
    let mut c = client(&a);
    let recovery = setup(&mut c, &a.vault);
    let note = locked_note(&a.vault, "secret");

    c.call(&Request::Lock).unwrap();
    assert!(!status(&mut c, &a.vault).1);
    assert!(
        decrypt(&mut c, &a.vault, &note)
            .unwrap_err()
            .contains("locked")
    );

    let wrong = unlock(&mut c, &a.vault, UnlockMethod::Password, "wrong horse").unwrap_err();
    assert!(wrong.contains("wrong password"), "{wrong}");
    unlock(&mut c, &a.vault, UnlockMethod::Password, "correct horse").unwrap();
    assert_eq!(decrypt(&mut c, &a.vault, &note).unwrap(), "secret");

    c.call(&Request::Lock).unwrap();
    unlock(&mut c, &a.vault, UnlockMethod::Recovery, &recovery).unwrap();
    assert_eq!(decrypt(&mut c, &a.vault, &note).unwrap(), "secret");
}

#[test]
fn a_new_password_needs_the_vault_unlocked() {
    let a = start("", None);
    let mut c = client(&a);
    setup(&mut c, &a.vault);
    c.call(&Request::Lock).unwrap();
    let refused = c
        .call(&Request::SetPassword {
            vault: a.vault.clone(),
            current: "correct horse".into(),
            password: "battery staple".into(),
        })
        .unwrap_err();
    assert!(refused.to_string().contains("locked"));
    unlock(&mut c, &a.vault, UnlockMethod::Password, "correct horse").unwrap();
    // Unlocked is not enough: adding a way in needs the current password.
    let refused = c
        .call(&Request::SetPassword {
            vault: a.vault.clone(),
            current: "a guess".into(),
            password: "attacker's".into(),
        })
        .unwrap_err();
    assert!(
        refused.to_string().contains("not the vault password"),
        "{refused}"
    );
    c.call(&Request::SetPassword {
        vault: a.vault.clone(),
        current: "correct horse".into(),
        password: "battery staple".into(),
    })
    .unwrap();
    c.call(&Request::Lock).unwrap();
    assert!(unlock(&mut c, &a.vault, UnlockMethod::Password, "correct horse").is_err());
    unlock(&mut c, &a.vault, UnlockMethod::Password, "battery staple").unwrap();
}

#[test]
fn an_unused_key_is_forgotten() {
    let a = start("", Some(1));
    let mut c = client(&a);
    setup(&mut c, &a.vault);
    assert!(status(&mut c, &a.vault).1);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(
        !status(&mut c, &a.vault).1,
        "forgotten after a second unused"
    );
}

#[test]
fn strict_mode_ties_the_key_to_the_session_that_unlocked_it() {
    let a = start("lock:\n  strict: true\n", None);
    let mut first = client(&a);
    setup(&mut first, &a.vault);
    let note = locked_note(&a.vault, "secret");
    assert_eq!(decrypt(&mut first, &a.vault, &note).unwrap(), "secret");

    let mut other = client(&a);
    assert!(!status(&mut other, &a.vault).1);
    assert!(decrypt(&mut other, &a.vault, &note).is_err());

    drop(first);
    std::thread::sleep(Duration::from_millis(200));
    let mut again = client(&a);
    unlock(
        &mut again,
        &a.vault,
        UnlockMethod::Password,
        "correct horse",
    )
    .unwrap();
    assert!(status(&mut again, &a.vault).1);
    drop(again);
    std::thread::sleep(Duration::from_millis(200));
    // The unlocking session is gone, so is the key: nobody can read.
    let mut later = client(&a);
    assert!(decrypt(&mut later, &a.vault, &note).is_err());
}

#[test]
fn a_second_agent_leaves_the_first_alone() {
    let a = start("", None);
    let mut c = client(&a);
    setup(&mut c, &a.vault);
    let out = Command::new(AGENT)
        .env("DEN_AGENT_SOCKET", &a.socket)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(status(&mut c, &a.vault).1, "still unlocked");
}

#[test]
fn stop_forgets_and_exits() {
    let mut a = start("", None);
    let mut c = client(&a);
    setup(&mut c, &a.vault);
    c.call(&Request::Stop).unwrap();
    let status = a.child.wait().unwrap();
    assert!(status.success());
    assert!(!a.socket.exists());
}

#[test]
fn touch_id_and_yubikey_explain_themselves_when_not_set_up() {
    let a = start("", None);
    let mut c = client(&a);
    setup(&mut c, &a.vault);
    c.call(&Request::Lock).unwrap();
    let touch = c
        .call(&Request::Unlock {
            vault: a.vault.clone(),
            method: UnlockMethod::TouchId,
            secret: None,
        })
        .unwrap_err()
        .to_string();
    assert!(touch.contains("Touch ID"), "{touch}");
    let yubi = c
        .call(&Request::Unlock {
            vault: a.vault.clone(),
            method: UnlockMethod::Yubikey,
            secret: None,
        })
        .unwrap_err()
        .to_string();
    assert!(yubi.contains("yubikey.identity"), "{yubi}");
}

#[test]
fn an_unlock_ends_after_its_time_limit_even_when_used() {
    let a = start_with("", &[("DEN_TEST_MAX_SECONDS", Some("2".into()))]);
    let mut c = client(&a);
    setup(&mut c, &a.vault);
    let note = locked_note(&a.vault, "secret");
    for _ in 0..8 {
        let _ = decrypt(&mut c, &a.vault, &note);
        std::thread::sleep(Duration::from_millis(400));
    }
    assert!(!status(&mut c, &a.vault).1, "forgotten despite steady use");
}

#[test]
fn clients_refuse_anything_but_den_agent_on_the_socket() {
    use std::os::unix::net::UnixListener;
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let run = base.join("run");
    std::fs::create_dir(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = run.join("agent.sock");
    // This test program stands in for an impostor collecting passwords.
    let listener = UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            drop(conn);
        }
    });
    let err = Client::connect_to(&socket, Some(Path::new(AGENT)))
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("not den-agent"), "{err}");

    // A folder others can enter is refused before connecting at all.
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = Client::connect_to(&socket, None).err().unwrap().to_string();
    assert!(err.contains("only you can open"), "{err}");
}
