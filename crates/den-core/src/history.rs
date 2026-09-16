//! A local ledger of when tasks appeared and when they closed.
//!
//! The vault format records a task's *state* (`@status(done)`) but never when
//! it changed — den.nvim has no closure timestamp, and inventing one would mean
//! writing to files the user owns. So the burndown cannot be reconstructed from
//! a vault alone.
//!
//! Instead Den keeps its own append-only ledger outside the vault: each time it
//! loads a snapshot it records the date it first saw a task, and the date it
//! first saw that task done. History therefore starts the day Den is first run
//! and fills in from there; before that the burndown honestly shows only the
//! scope line and today's point rather than a fabricated curve.
//!
//! Nothing here is authoritative. Deleting the ledger loses chart history and
//! costs nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::civil::Date;

use crate::vault::{Status, Task, Vault};

#[derive(Debug, Clone, Default)]
pub struct History {
    path: Option<PathBuf>,
    /// Task key -> the first date we saw it.
    opened: BTreeMap<String, Date>,
    /// Task key -> the first date we saw it done.
    closed: BTreeMap<String, Date>,
    dirty: bool,
}

/// Identifies a task across reloads.
///
/// `@id(...)` is stable by design and survives edits and moves, so it is always
/// preferred. Without one we fall back to the file plus the task's text, which
/// is stable until the task is reworded — acceptable, because the consequence
/// is one chart point, not data loss.
pub fn key(task: &Task) -> String {
    match &task.parsed.id {
        Some(id) => format!("id:{id}"),
        None => format!("at:{}:{}", task.file.display(), task.parsed.caption),
    }
}

impl History {
    /// Loads the ledger for `root`, or an empty one if it cannot be read.
    pub fn load(root: &Path) -> History {
        let Some(path) = ledger_path(root) else {
            return History::default();
        };
        let mut history = History {
            path: Some(path.clone()),
            ..History::default()
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return history;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return history;
        };
        for (field, target) in [
            ("opened", &mut history.opened),
            ("closed", &mut history.closed),
        ] {
            let Some(entries) = value.get(field).and_then(serde_json::Value::as_object) else {
                continue;
            };
            for (key, date) in entries {
                if let Some(date) = date.as_str().and_then(|d| d.parse::<Date>().ok()) {
                    target.insert(key.clone(), date);
                }
            }
        }
        history
    }

    /// Records today's view of the vault. Only ever adds facts: a task that
    /// reopens keeps its original closure date until it closes again, which is
    /// what makes the "closed per day" series monotonic.
    pub fn observe(&mut self, vault: &Vault, today: Date) {
        for task in vault.tasks() {
            let key = key(task);
            self.opened.entry(key.clone()).or_insert_with(|| {
                self.dirty = true;
                today
            });
            if task.status() == Status::Done && !self.closed.contains_key(&key) {
                self.closed.insert(key, today);
                self.dirty = true;
            }
        }
    }

    /// The date a task closed, if we ever saw it open first.
    pub fn closed_on(&self, task: &Task) -> Option<Date> {
        self.closed.get(&key(task)).copied()
    }

    pub fn opened_on(&self, task: &Task) -> Option<Date> {
        self.opened.get(&key(task)).copied()
    }

    /// The earliest date the ledger knows anything about; the burndown cannot
    /// show a curve before this.
    pub fn starts_on(&self) -> Option<Date> {
        self.opened.values().min().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.opened.is_empty()
    }

    /// Writes the ledger back, if anything changed. Failures are silent: this
    /// is a cache, and a vault must never be held hostage to it.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = &self.path else { return };
        let encode = |entries: &BTreeMap<String, Date>| {
            entries
                .iter()
                .map(|(key, date)| {
                    (
                        key.clone(),
                        serde_json::Value::from(crate::date::iso(*date)),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        };
        let document = serde_json::json!({
            "version": 1,
            "opened": encode(&self.opened),
            "closed": encode(&self.closed),
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(path, document.to_string()).is_ok() {
            self.dirty = false;
        }
    }
}

/// Per-vault, and deliberately outside it — den.nvim's own `AGENTS.md` keeps
/// caches out of the Markdown tree, and so do we.
fn ledger_path(root: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/den-desktop")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("den-desktop")
    };

    // FNV-1a of the canonical root, so two vaults never share a ledger.
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(base.join(format!("{hash:016x}")).join("history-v1.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{Entry, Kind};

    fn vault_with(markdown: &str) -> Vault {
        let entry = Entry::parse(PathBuf::from("/tmp/p.md"), Kind::Projects, markdown, false);
        let mut vault = Vault::default();
        vault.entries.push(entry);
        vault
    }

    fn date(y: i16, m: i8, d: i8) -> Date {
        Date::new(y, m, d).expect("valid")
    }

    #[test]
    fn records_first_sighting_and_first_closure() {
        let mut history = History::default();
        let open = vault_with("# P\n- [ ] Ship it @id(a)\n");
        history.observe(&open, date(2026, 9, 10));

        let task = &open.entries[0].tasks[0];
        assert_eq!(history.opened_on(task), Some(date(2026, 9, 10)));
        assert_eq!(history.closed_on(task), None);

        let done = vault_with("# P\n- [x] Ship it @id(a)\n");
        history.observe(&done, date(2026, 9, 12));
        assert_eq!(
            history.closed_on(&done.entries[0].tasks[0]),
            Some(date(2026, 9, 12))
        );
        // The opening date is not rewritten by a later sighting.
        assert_eq!(
            history.opened_on(&done.entries[0].tasks[0]),
            Some(date(2026, 9, 10))
        );
    }

    #[test]
    fn reopening_keeps_the_original_closure() {
        let mut history = History::default();
        history.observe(
            &vault_with("# P\n- [x] Ship it @id(a)\n"),
            date(2026, 9, 12),
        );
        let reopened = vault_with("# P\n- [ ] Ship it @id(a)\n");
        history.observe(&reopened, date(2026, 9, 14));
        assert_eq!(
            history.closed_on(&reopened.entries[0].tasks[0]),
            Some(date(2026, 9, 12))
        );
    }

    #[test]
    fn tasks_without_ids_are_still_tracked() {
        let mut history = History::default();
        let vault = vault_with("# P\n- [ ] No id here\n");
        history.observe(&vault, date(2026, 9, 10));
        assert_eq!(
            history.opened_on(&vault.entries[0].tasks[0]),
            Some(date(2026, 9, 10))
        );
    }
}
