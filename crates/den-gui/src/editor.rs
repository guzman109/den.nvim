//! Talking to a running Neovim — for the two things Den cannot answer itself.
//!
//! The engine writes files directly now, so this is no longer the write path.
//! What is left is exactly what needs the editor's own knowledge:
//!
//! - **Which files have unsaved changes.** Routing writes through Neovim used
//!   to make clobbering a dirty buffer impossible. Writing directly gives that
//!   up, so Den has to ask before it writes. This is the safety check the whole
//!   module exists for.
//! - **Open this task.** Jumping the user's editor to a line.
//!
//! Neovim speaks msgpack-rpc over a Unix socket. Each call opens its own
//! short-lived connection: call volume is a handful per second at most, and it
//! removes any need for msgid correlation or a worker thread.
//!
//! Every call is best-effort. A missing socket is normal — Den is a standalone
//! app — and simply means "no editor is attached", never an error the user has
//! to act on.

use std::io::BufWriter;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct Editor {
    socket: PathBuf,
}

impl Editor {
    pub fn new(socket: PathBuf) -> Editor {
        Editor { socket }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// True when Neovim answers.
    pub fn is_reachable(&self) -> bool {
        self.call("return 1", &[]).is_some()
    }

    /// Files Neovim is holding unsaved changes for.
    ///
    /// Returned as a list rather than a yes/no so one round trip covers a whole
    /// vault. On any failure this returns empty — callers must treat that as
    /// "no editor attached", which is the same state as running standalone.
    pub fn dirty_files(&self) -> Vec<PathBuf> {
        let lua = "\
            local dirty = {} \
            for _, b in ipairs(vim.api.nvim_list_bufs()) do \
              if vim.api.nvim_buf_is_loaded(b) and vim.bo[b].modified and vim.bo[b].buftype == '' then \
                local name = vim.api.nvim_buf_get_name(b) \
                if name ~= '' then dirty[#dirty + 1] = vim.uv.fs_realpath(name) or name end \
              end \
            end \
            return dirty";

        let Some(value) = self.call(lua, &[]) else {
            return Vec::new();
        };
        match value {
            rmpv::Value::Array(items) => items
                .iter()
                .filter_map(rmpv::Value::as_str)
                .map(PathBuf::from)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Jumps the editor to a file and line.
    pub fn open(&self, path: &Path, line: usize) -> Result<(), String> {
        let lua = "\
            local path, line = ... \
            vim.cmd.edit(vim.fn.fnameescape(path)) \
            vim.api.nvim_win_set_cursor(0, { math.min(line, vim.api.nvim_buf_line_count(0)), 0 }) \
            return true";

        self.call(
            lua,
            &[
                rmpv::Value::from(path.to_string_lossy().into_owned()),
                rmpv::Value::from(line as i64),
            ],
        )
        .map(|_| ())
        .ok_or_else(|| format!("could not reach Neovim at {}", self.socket.display()))
    }

    /// One `nvim_exec_lua` round trip. `None` on any transport failure.
    fn call(&self, lua: &str, args: &[rmpv::Value]) -> Option<rmpv::Value> {
        let stream = UnixStream::connect(&self.socket).ok()?;
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        stream.set_write_timeout(Some(TIMEOUT)).ok();

        let request = rmpv::Value::Array(vec![
            rmpv::Value::from(0),
            rmpv::Value::from(1),
            rmpv::Value::from("nvim_exec_lua"),
            rmpv::Value::Array(vec![
                rmpv::Value::from(lua),
                rmpv::Value::Array(args.to_vec()),
            ]),
        ]);

        let mut writer = BufWriter::new(&stream);
        rmpv::encode::write_value(&mut writer, &request).ok()?;
        std::io::Write::flush(&mut writer).ok()?;
        drop(writer);

        let mut reader = std::io::BufReader::new(&stream);
        let response = rmpv::decode::read_value(&mut reader).ok()?;

        // msgpack-rpc replies are `[1, msgid, error, result]`.
        let rmpv::Value::Array(parts) = response else {
            return None;
        };
        if parts.first().and_then(rmpv::Value::as_u64) != Some(1) || parts.len() < 4 {
            return None;
        }
        if !parts[2].is_nil() {
            return None;
        }
        Some(parts[3].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_socket_reads_as_no_editor_attached() {
        let editor = Editor::new(PathBuf::from("/tmp/den-definitely-not-a-socket"));
        assert!(!editor.is_reachable());
        // Crucially empty, not an error: standalone is a supported mode.
        assert!(editor.dirty_files().is_empty());
        assert!(editor.open(Path::new("/tmp/x.md"), 1).is_err());
    }
}
