//! Talking to den-agent, the process that holds the unlocked vault key.
//!
//! The agent listens on a Unix socket only its own user can reach. Requests
//! and replies are one JSON object per line. The key never crosses the
//! socket: the agent decrypts notes and wraps new copies of the key itself,
//! and hands back only plaintext (and, once, the recovery key at setup).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlockMethod {
    Password,
    Recovery,
    Yubikey,
    TouchId,
}

/// A request to the agent. Secrets travel only inside these, over the
/// user-only socket, and are wiped by the agent after use.
#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status {
        vault: PathBuf,
    },
    /// Creates the vault's key pair; the reply's `text` is the recovery key,
    /// shown once.
    Setup {
        vault: PathBuf,
        password: String,
    },
    Unlock {
        vault: PathBuf,
        method: UnlockMethod,
        /// The password, the recovery key, or a YubiKey PIN.
        #[serde(default)]
        secret: Option<String>,
    },
    /// Decrypts a locked note's armored text.
    Decrypt {
        vault: PathBuf,
        text: String,
    },
    /// Replaces the password copy of the key. `current` is the current
    /// password or the recovery key: an unlocked vault alone is not enough
    /// to add a way in.
    SetPassword {
        vault: PathBuf,
        current: String,
        password: String,
    },
    /// Adds a YubiKey copy: the recipient and identity stub from
    /// `age-plugin-yubikey --list`. Needs `current`, as above.
    AddYubikey {
        vault: PathBuf,
        current: String,
        recipient: String,
        identity: String,
    },
    /// Stores a copy of the key in this Mac's keychain for Touch ID. Needs
    /// `current`, as above.
    EnableTouchId {
        vault: PathBuf,
        current: String,
    },
    /// Forgets every key now.
    Lock,
    /// Forgets every key and exits.
    Stop,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = match self {
            Request::Status { .. } => "status",
            Request::Setup { .. } => "setup",
            Request::Unlock { .. } => "unlock",
            Request::Decrypt { .. } => "decrypt",
            Request::SetPassword { .. } => "set_password",
            Request::AddYubikey { .. } => "add_yubikey",
            Request::EnableTouchId { .. } => "enable_touch_id",
            Request::Lock => "lock",
            Request::Stop => "stop",
        };
        write!(f, "Request::{op}(…)")
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub set_up: bool,
    #[serde(default)]
    pub unlocked: bool,
    /// Unlock methods available: "password", "recovery", "yubikey",
    /// "touch_id".
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub strict: bool,
    /// Decrypted text, or the recovery key after setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("ok", &self.ok)
            .field("error", &self.error)
            .field("set_up", &self.set_up)
            .field("unlocked", &self.unlocked)
            .field("methods", &self.methods)
            .field("text", &self.text.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Response {
    pub fn fail(message: impl Into<String>) -> Response {
        Response {
            ok: false,
            error: Some(message.into()),
            ..Response::default()
        }
    }
}

/// The agent's socket: `$DEN_AGENT_SOCKET`, else in the per-user runtime or
/// temporary folder.
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DEN_AGENT_SOCKET") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("den").join("agent.sock");
    }
    // macOS gives every user a private temporary folder; elsewhere /tmp is
    // shared, so the folder carries the user id.
    let tmp = std::env::temp_dir();
    if cfg!(target_os = "macos") {
        return tmp.join("den").join("agent.sock");
    }
    let uid = nix::unistd::getuid().as_raw();
    tmp.join(format!("den-{uid}")).join("agent.sock")
}

fn refused(message: impl Into<String>) -> Error {
    Error::Lock(message.into())
}

/// The socket's folder must belong to this user and let nobody else in, or
/// someone else could stand in for the agent.
fn check_folder(socket: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let dir = socket
        .parent()
        .ok_or_else(|| refused("den-agent's socket has no folder"))?;
    let meta = std::fs::symlink_metadata(dir).map_err(|_| refused("den-agent is not running"))?;
    if !meta.is_dir() || meta.uid() != nix::unistd::getuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(refused(format!(
            "{} is not a folder only you can open; refusing to talk to whatever listens there",
            dir.display()
        )));
    }
    Ok(())
}

/// The listening process's user and program.
fn peer(stream: &UnixStream) -> (Option<u32>, Option<PathBuf>) {
    #[cfg(target_os = "macos")]
    {
        use nix::sys::socket::{getsockopt, sockopt::LocalPeerPid};
        let uid = nix::unistd::getpeereid(stream)
            .ok()
            .map(|(u, _)| u.as_raw());
        let exe = getsockopt(stream, LocalPeerPid)
            .ok()
            .and_then(|pid| libproc::proc_pid::pidpath(pid).ok())
            .map(PathBuf::from);
        (uid, exe)
    }
    #[cfg(target_os = "linux")]
    {
        use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
        let Ok(cred) = getsockopt(stream, PeerCredentials) else {
            return (None, None);
        };
        let exe = std::fs::read_link(format!("/proc/{}/exe", cred.pid()))
            .ok()
            .map(|p| {
                // A program replaced by an update since it started.
                let s = p.to_string_lossy();
                PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s).to_string())
            });
        (Some(cred.uid()), exe)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = stream;
        (None, None)
    }
}

fn same_program(a: &Path, b: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    real(a) == real(b)
}

/// Programs the agent may use (age-plugin-yubikey) are found only in the
/// usual places, whatever PATH the first caller had.
fn agent_path() -> String {
    let home = std::env::home_dir().unwrap_or_default();
    [
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
        home.join(".cargo/bin").display().to_string(),
        home.join(".local/bin").display().to_string(),
    ]
    .join(":")
}

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    /// Connects to a running agent, checking it is `agent` (the den-agent
    /// program) running as this user.
    pub fn connect(agent: &Path) -> Result<Client> {
        Client::connect_to(&socket_path(), Some(agent))
    }

    /// Connects to the socket at `path`. The folder must be private to this
    /// user and the listener must run as this user; with `agent`, it must
    /// also be that program, so nothing else can collect a password.
    pub fn connect_to(path: &Path, agent: Option<&Path>) -> Result<Client> {
        check_folder(path)?;
        let stream = UnixStream::connect(path).map_err(|_| refused("den-agent is not running"))?;
        let (uid, exe) = peer(&stream);
        if uid != Some(nix::unistd::getuid().as_raw()) {
            return Err(refused(
                "the process on den-agent's socket is not running as you",
            ));
        }
        if let Some(agent) = agent {
            match exe {
                Some(exe) if same_program(&exe, agent) => {}
                Some(exe) => {
                    return Err(refused(format!(
                        "{} is listening on den-agent's socket, not den-agent; refusing to send it anything",
                        exe.display()
                    )));
                }
                None => {
                    return Err(refused(
                        "could not tell which program is listening on den-agent's socket",
                    ));
                }
            }
        }
        let writer = stream.try_clone().map_err(|e| Error::io(path, e))?;
        Ok(Client {
            reader: BufReader::new(stream),
            writer,
        })
    }

    /// Connects, starting `agent` (the den-agent program) first if none is
    /// running. The agent starts with a small, fixed environment rather than
    /// whatever the caller had.
    pub fn connect_or_start(agent: &Path) -> Result<Client> {
        let path = socket_path();
        if let Ok(client) = Client::connect_to(&path, Some(agent)) {
            return Ok(client);
        }
        let mut cmd = std::process::Command::new(agent);
        cmd.arg("--daemon")
            .env_clear()
            .env("PATH", agent_path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let kept = [
            "HOME",
            "USER",
            "LOGNAME",
            "LANG",
            "TMPDIR",
            "XDG_RUNTIME_DIR",
            "XDG_CONFIG_HOME",
            "DEN_AGENT_SOCKET",
            "DEN_CONFIG",
        ];
        for key in kept {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        if cfg!(debug_assertions) {
            for (key, value) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("DEN_TEST_") {
                    cmd.env(key, value);
                }
            }
        }
        cmd.spawn()
            .map_err(|e| Error::Lock(format!("could not start {}: {e}", agent.display())))?;
        let started = Instant::now();
        loop {
            match Client::connect_to(&path, Some(agent)) {
                Ok(client) => return Ok(client),
                // Something that is not den-agent answered: say so at once.
                Err(e)
                    if e.to_string().contains("refusing")
                        || e.to_string().contains("not running as you") =>
                {
                    return Err(e);
                }
                Err(_) => {}
            }
            if started.elapsed() > Duration::from_secs(3) {
                return Err(Error::Lock("den-agent did not start".to_string()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Sends one request and waits for the reply. A reply with `ok: false`
    /// becomes an error.
    pub fn call(&mut self, request: &Request) -> Result<Response> {
        let mut line =
            Zeroizing::new(serde_json::to_string(request).map_err(|e| Error::Lock(e.to_string()))?);
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| Error::Lock(format!("den-agent: {e}")))?;
        let mut reply = Zeroizing::new(String::new());
        let n = self
            .reader
            .read_line(&mut reply)
            .map_err(|e| Error::Lock(format!("den-agent: {e}")))?;
        if n == 0 {
            return Err(Error::Lock("den-agent closed the connection".to_string()));
        }
        let response: Response =
            serde_json::from_str(&reply).map_err(|e| Error::Lock(format!("den-agent: {e}")))?;
        if !response.ok {
            return Err(Error::Lock(
                response
                    .error
                    .clone()
                    .unwrap_or_else(|| "den-agent refused".to_string()),
            ));
        }
        Ok(response)
    }
}
