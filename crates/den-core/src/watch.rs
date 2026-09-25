//! Noticing changes made outside Den: a `git pull`, another editor, another
//! Den on the same machine.
//!
//! The watcher runs on its own thread and reports vault paths. It never
//! touches the vault itself; whoever receives the report decides when to
//! reload, on its own thread.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::vault::classify;

/// What changed on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    /// Vault paths of Markdown files that were written, created or removed.
    pub docs: BTreeSet<String>,
    /// A timer log changed.
    pub log: bool,
}

/// Keeps the watcher alive; dropping it stops watching.
pub struct Watch {
    _watcher: RecommendedWatcher,
}

/// Watches `root` and calls `on_change` from the watcher's thread.
pub fn watch(root: &Path, on_change: impl Fn(Changed) + Send + 'static) -> Result<Watch> {
    let roots: Vec<PathBuf> = [
        std::fs::canonicalize(root).ok(),
        Some(root.to_path_buf()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let mut changed = Changed::default();
        for path in &event.paths {
            let Some(rel) = roots.iter().find_map(|r| relative(r, path)) else {
                continue;
            };
            if rel.starts_with(".den/log/") && rel.ends_with(".jsonl") {
                changed.log = true;
            } else if classify(&rel).is_some() {
                changed.docs.insert(rel);
            }
        }
        if changed.log || !changed.docs.is_empty() {
            on_change(changed);
        }
    })
    .map_err(|e| Error::Invalid(format!("cannot watch {}: {e}", root.display())))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| Error::Invalid(format!("cannot watch {}: {e}", root.display())))?;
    Ok(Watch { _watcher: watcher })
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let inner = path.strip_prefix(root).ok()?;
    let parts = inner
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn reports_a_note_written_from_outside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("notes")).unwrap();
        let (tx, rx) = mpsc::channel();
        let _watch = watch(dir.path(), move |c| {
            let _ = tx.send(c);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.path().join("notes/new.md"), "# New\n").unwrap();
        std::fs::write(dir.path().join("notes/ignored.txt"), "x").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = BTreeSet::new();
        while std::time::Instant::now() < deadline && !seen.contains("notes/new.md") {
            if let Ok(c) = rx.recv_timeout(Duration::from_millis(200)) {
                seen.extend(c.docs);
            }
        }
        assert!(seen.contains("notes/new.md"), "{seen:?}");
        assert!(!seen.iter().any(|p| p.ends_with(".txt")));
    }
}
