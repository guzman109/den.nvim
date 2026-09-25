//! Which vault files an editor holds unsaved changes for, across processes.
//!
//! Neovim knows its own unsaved buffers (overlays), but another program
//! writing to the vault, such as the MCP server an AI agent uses, does not.
//! Each Neovim publishes its list here, one small file per process in the
//! person's state folder, and writers treat those files as off limits until
//! they are saved. A list whose process has exited is ignored.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The person's state folder: `$XDG_STATE_HOME`, else `~/.local/state`.
pub fn state_home() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".local/state")))
}

/// The folder for one vault's lists, under a state folder. Never inside the
/// synced vault.
pub fn dir(state: &Path, root: &Path) -> PathBuf {
    let real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    state.join("den").join("editing").join(fingerprint(&real))
}

/// A short, stable name for a path (FNV-1a).
fn fingerprint(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Records the vault paths this process holds unsaved changes for (empty
/// removes the record).
pub fn publish(state: &Path, root: &Path, paths: &BTreeSet<String>) {
    let dir = dir(state, root);
    let file = dir.join(std::process::id().to_string());
    if paths.is_empty() {
        let _ = std::fs::remove_file(&file);
        return;
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let text: String = paths.iter().map(|p| format!("{p}\n")).collect();
    let _ = crate::write::write_atomically(&file, text.as_bytes());
}

fn alive(pid: i32) -> bool {
    #[cfg(unix)]
    {
        !matches!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        )
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Every path a running editor (other than this process) holds unsaved
/// changes for.
pub fn others(state: &Path, root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(dir(state, root)) else {
        return out;
    };
    let me = std::process::id().to_string();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(pid) = name.parse::<i32>() else {
            continue;
        };
        if name == me {
            continue;
        }
        if !alive(pid) {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            out.extend(text.lines().filter(|l| !l.is_empty()).map(str::to_string));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_shared_and_cleared() {
        let state = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let dir = dir(state.path(), vault.path());
        std::fs::create_dir_all(&dir).unwrap();

        // Another live process (this test's parent stands in for Neovim).
        let parent = std::os::unix::process::parent_id();
        std::fs::write(dir.join(parent.to_string()), "projects/website.md\n").unwrap();
        let seen = others(state.path(), vault.path());
        assert_eq!(seen, ["projects/website.md".to_string()].into());

        // This process's own list is not "someone else editing".
        let own: BTreeSet<String> = ["notes/mine.md".to_string()].into();
        publish(state.path(), vault.path(), &own);
        assert!(!others(state.path(), vault.path()).contains("notes/mine.md"));
        publish(state.path(), vault.path(), &BTreeSet::new());
        assert!(!dir.join(std::process::id().to_string()).exists());

        // A list left by a process that is gone does not count.
        std::fs::write(dir.join("999999"), "notes/x.md\n").unwrap();
        assert!(!others(state.path(), vault.path()).contains("notes/x.md"));
        assert!(!dir.join("999999").exists());
    }
}
