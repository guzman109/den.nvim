//! `den` as SSH's and git's askpass program, asking inside Neovim.
//!
//! When Neovim syncs in the foreground, git runs with `SSH_ASKPASS` and
//! `GIT_ASKPASS` pointing at this binary and `DEN_NVIM` holding Neovim's
//! server address. SSH calls `den "<prompt>"`; this asks Neovim (over its
//! msgpack-RPC socket) to show the prompt, waits for the answer, and prints
//! it for SSH to read. The answer is never written anywhere else.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use rmpv::Value;

/// How long a person has to answer before SSH is told no.
const WAIT: Duration = Duration::from_secs(300);

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn connect(address: &str) -> std::io::Result<Box<dyn Stream>> {
    #[cfg(unix)]
    if address.contains('/') {
        return Ok(Box::new(std::os::unix::net::UnixStream::connect(address)?));
    }
    Ok(Box::new(std::net::TcpStream::connect(address)?))
}

struct Rpc {
    stream: Box<dyn Stream>,
    next: u64,
}

impl Rpc {
    /// Runs Lua in Neovim and returns its result.
    fn lua(&mut self, code: &str, args: Vec<Value>) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let request = Value::Array(vec![
            Value::from(0),
            Value::from(id),
            Value::from("nvim_exec_lua"),
            Value::Array(vec![Value::from(code), Value::Array(args)]),
        ]);
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &request).map_err(|e| e.to_string())?;
        self.stream.write_all(&bytes).map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())?;
        loop {
            let message = rmpv::decode::read_value(&mut self.stream).map_err(|e| e.to_string())?;
            let Value::Array(parts) = message else {
                continue;
            };
            // [1, id, error, result]; anything else (a notification) is skipped.
            if parts.len() == 4 && parts[0].as_u64() == Some(1) && parts[1].as_u64() == Some(id) {
                if !parts[2].is_nil() {
                    return Err(parts[2].to_string());
                }
                return Ok(parts[3].clone());
            }
        }
    }
}

/// Asks Neovim and prints the answer. Exit status 1 means the person
/// cancelled, or no answer came.
pub fn run(prompt: &str) -> std::process::ExitCode {
    let Ok(address) = std::env::var("DEN_NVIM") else {
        eprintln!("den: no Neovim to ask (DEN_NVIM is not set)");
        return std::process::ExitCode::FAILURE;
    };
    match ask(&address, prompt) {
        Ok(Some(answer)) => {
            println!("{answer}");
            std::process::ExitCode::SUCCESS
        }
        Ok(None) => std::process::ExitCode::FAILURE,
        Err(e) => {
            eprintln!("den: could not ask Neovim: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ask(address: &str, prompt: &str) -> Result<Option<String>, String> {
    let stream = connect(address).map_err(|e| e.to_string())?;
    let mut rpc = Rpc { stream, next: 0 };
    let id = rpc.lua(
        "return require('den.askpass').begin(...)",
        vec![Value::from(prompt)],
    )?;
    let started = Instant::now();
    while started.elapsed() < WAIT {
        let answer = rpc.lua(
            "return require('den.askpass').result(...)",
            vec![id.clone()],
        )?;
        if let Value::Map(fields) = answer {
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k.as_str() == Some(key))
                    .map(|(_, v)| v.clone())
            };
            if get("ok").and_then(|v| v.as_bool()) != Some(true) {
                return Ok(None);
            }
            return Ok(get("value").and_then(|v| v.as_str().map(str::to_string)));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = rpc.lua("return require('den.askpass').cancel(...)", vec![id]);
    Ok(None)
}
