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
    /// Replaces the password copy of the key (the vault must be unlocked).
    SetPassword {
        vault: PathBuf,
        password: String,
    },
    /// Adds a YubiKey copy: the recipient and identity stub from
    /// `age-plugin-yubikey --list`.
    AddYubikey {
        vault: PathBuf,
        recipient: String,
        identity: String,
    },
    /// Stores a copy of the key in this Mac's keychain for Touch ID.
    EnableTouchId {
        vault: PathBuf,
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
    let uid = std::env::home_dir()
        .and_then(|h| std::fs::metadata(h).ok())
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap_or(0);
    tmp.join(format!("den-{uid}")).join("agent.sock")
}

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Client {
    /// Connects to a running agent.
    pub fn connect() -> Result<Client> {
        Client::connect_to(&socket_path())
    }

    pub fn connect_to(path: &Path) -> Result<Client> {
        let stream = UnixStream::connect(path)
            .map_err(|_| Error::Lock("den-agent is not running".to_string()))?;
        let writer = stream.try_clone().map_err(|e| Error::io(path, e))?;
        Ok(Client {
            reader: BufReader::new(stream),
            writer,
        })
    }

    /// Connects, starting `agent` (the den-agent program) first if none is
    /// running.
    pub fn connect_or_start(agent: &Path) -> Result<Client> {
        let path = socket_path();
        if let Ok(client) = Client::connect_to(&path) {
            return Ok(client);
        }
        std::process::Command::new(agent)
            .arg("--daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| Error::Lock(format!("could not start {}: {e}", agent.display())))?;
        let started = Instant::now();
        loop {
            if let Ok(client) = Client::connect_to(&path) {
                return Ok(client);
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
