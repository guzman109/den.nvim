//! The SQLite index.
//!
//! **The index is a cache, never the truth.** The Markdown files are the store;
//! everything here is derived from them and can be rebuilt by deleting the
//! database. That is what makes it safe to change the schema, and why nothing
//! is ever written to the index that could not be recovered from disk.
//!
//! The schema deliberately mirrors `den.nvim/lua/den/index.lua` — same tables,
//! same `user_version` — so the Lua side can read the same file during the
//! transition, and so a vault indexed by one interface is not re-scanned by the
//! other.
//!
//! It lives outside the vault. `den.nvim/AGENTS.md` is explicit that caches do
//! not belong in the user's Markdown tree.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::vault::{Entry, Vault};

/// Bumping this discards the old database and reindexes from disk. Because the
/// index holds nothing authoritative, that is always safe.
const SCHEMA_VERSION: i64 = 2;

#[derive(Debug)]
pub enum Error {
    Database(String),
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Database(message) | Error::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Error::Database(error.to_string())
    }
}

/// What a refresh changed, so callers can skip work when nothing moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Refreshed {
    pub updated: usize,
    pub removed: usize,
}

impl Refreshed {
    pub fn is_empty(self) -> bool {
        self.updated == 0 && self.removed == 0
    }
}

pub struct Index {
    db: Connection,
    root: PathBuf,
}

impl Index {
    /// Opens (or creates) the index for `root`.
    pub fn open(root: &Path) -> Result<Index, Error> {
        let path = Self::location(root).ok_or_else(|| Error::Io("no cache directory".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io(e.to_string()))?;
        }
        Index::open_at(&path, root)
    }

    /// Opens an index at an explicit path. Tests use this to stay hermetic.
    pub fn open_at(path: &Path, root: &Path) -> Result<Index, Error> {
        let db = Connection::open(path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;

        let mut index = Index {
            db,
            root: root.to_path_buf(),
        };
        index.migrate()?;
        Ok(index)
    }

    /// An in-memory index, for tests.
    pub fn in_memory(root: &Path) -> Result<Index, Error> {
        let db = Connection::open_in_memory()?;
        let mut index = Index {
            db,
            root: root.to_path_buf(),
        };
        index.migrate()?;
        Ok(index)
    }

    /// Where the index lives: inside the vault, under `.den/`.
    ///
    /// This used to sit in the user's cache directory, "always outside the
    /// Markdown tree", which is the right instinct for a plaintext vault — do
    /// not litter someone's notes. It is the wrong one the moment the vault is
    /// encrypted: the FTS5 table holds every note body and task title in the
    /// clear, and a cache directory survives unmounting. You would encrypt your
    /// notes and leave a searchable copy behind.
    ///
    /// Derived data belongs inside the encryption boundary, so it follows the
    /// data. `mutate::write_atomically` already writes its temp files beside
    /// their target, so the vault was never untouched anyway. Add `.den/` to
    /// the vault's `.gitignore`; [`cache_outside`] restores the old location.
    pub fn location(root: &Path) -> Option<PathBuf> {
        Some(root.join(".den").join("index-v2.sqlite3"))
    }

    /// The pre-`.den/` location, for a vault that should stay pristine.
    ///
    /// Only appropriate for a plaintext vault: see [`Index::location`].
    pub fn cache_outside(root: &Path) -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let base = if cfg!(target_os = "macos") {
            home.join("Library/Caches/den")
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache"))
                .join("den")
        };
        Some(base.join(fingerprint(root)).join("index-v2.sqlite3"))
    }

    /// Creates the schema, or drops and recreates it when the version moved.
    fn migrate(&mut self) -> Result<(), Error> {
        let version: i64 = self
            .db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0);

        if version != 0 && version != SCHEMA_VERSION {
            // Nothing here is authoritative, so starting over costs a rescan.
            self.db.execute_batch(
                "DROP TABLE IF EXISTS task_tags;
                 DROP TABLE IF EXISTS tasks;
                 DROP TABLE IF EXISTS links;
                 DROP TABLE IF EXISTS search;
                 DROP TABLE IF EXISTS entries;",
            )?;
        }

        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                 file TEXT PRIMARY KEY,
                 kind TEXT NOT NULL,
                 title TEXT NOT NULL,
                 content TEXT NOT NULL,
                 hash TEXT NOT NULL,
                 archived INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tasks (
                 file TEXT NOT NULL REFERENCES entries(file) ON DELETE CASCADE,
                 line INTEGER NOT NULL,
                 text TEXT NOT NULL,
                 caption TEXT NOT NULL,
                 status TEXT NOT NULL,
                 id TEXT,
                 ord INTEGER,
                 due TEXT,
                 PRIMARY KEY(file, line)
             );
             CREATE TABLE IF NOT EXISTS task_tags (
                 file TEXT NOT NULL,
                 line INTEGER NOT NULL,
                 tag TEXT NOT NULL,
                 PRIMARY KEY(file, line, tag),
                 FOREIGN KEY(file, line) REFERENCES tasks(file, line) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS links (
                 file TEXT NOT NULL REFERENCES entries(file) ON DELETE CASCADE,
                 line INTEGER NOT NULL,
                 target TEXT NOT NULL,
                 PRIMARY KEY(file, line, target)
             );
             CREATE INDEX IF NOT EXISTS tasks_due ON tasks(due);
             CREATE INDEX IF NOT EXISTS tasks_status ON tasks(status);
             CREATE INDEX IF NOT EXISTS task_tags_tag ON task_tags(tag);
             CREATE INDEX IF NOT EXISTS links_target ON links(target);",
        )?;

        // Full-text search over note bodies. The Lua index has no equivalent;
        // it is additive, so an older reader simply ignores it.
        self.db.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS search
                 USING fts5(file UNINDEXED, title, content, tokenize = 'unicode61');",
        )?;

        self.db
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Reconciles the index against what is on disk.
    ///
    /// Hashing rather than trusting mtime means an external sync, a `git
    /// checkout` or an editor that rewrites timestamps is all handled the same
    /// way, and only genuinely changed entries are reparsed.
    pub fn refresh(&mut self, vault: &Vault) -> Result<Refreshed, Error> {
        let known: HashMap<String, String> = {
            let mut statement = self.db.prepare("SELECT file, hash FROM entries")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };

        let transaction = self.db.transaction()?;
        let mut changed = Refreshed::default();
        let mut present = Vec::with_capacity(vault.entries.len());

        for entry in &vault.entries {
            let file = entry.file.to_string_lossy().into_owned();
            let content = entry.content();
            let hash = fingerprint_str(&content);
            present.push(file.clone());

            if known.get(&file).is_some_and(|seen| *seen == hash) {
                continue;
            }
            put(&transaction, entry, &file, &content, &hash)?;
            changed.updated += 1;
        }

        for file in known.keys() {
            if !present.contains(file) {
                transaction.execute("DELETE FROM entries WHERE file = ?1", params![file])?;
                transaction.execute("DELETE FROM search WHERE file = ?1", params![file])?;
                changed.removed += 1;
            }
        }

        transaction.commit()?;
        Ok(changed)
    }

    /// Notes whose title or body match `needle`, best match first.
    pub fn search(&self, needle: &str) -> Result<Vec<PathBuf>, Error> {
        if needle.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self
            .db
            .prepare("SELECT file FROM search WHERE search MATCH ?1 ORDER BY rank LIMIT 200")?;
        // Treat the query as a literal phrase prefix: a user typing `foo(` is
        // asking for text, not writing FTS5 syntax that would fail to parse.
        let query = format!("\"{}\"*", needle.replace('"', ""));
        let rows = statement.query_map(params![query], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).map(PathBuf::from).collect())
    }

    /// Notes linking to `target`.
    pub fn backlinks(&self, target: &Path) -> Result<Vec<(PathBuf, usize)>, Error> {
        let mut statement = self
            .db
            .prepare("SELECT file, line FROM links WHERE target = ?1 ORDER BY file, line")?;
        let rows = statement.query_map(params![target.to_string_lossy()], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                row.get::<_, i64>(1)? as usize,
            ))
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Every tag in the vault with its task count.
    pub fn tags(&self) -> Result<Vec<(String, usize)>, Error> {
        let mut statement = self.db.prepare(
            "SELECT tag, COUNT(*) FROM task_tags GROUP BY tag ORDER BY COUNT(*) DESC, tag",
        )?;
        let rows =
            statement.query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as usize)))?;
        Ok(rows
            .filter_map(Result::ok)
            .map(|(tag, n): (String, usize)| (tag, n))
            .collect())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The hash the index holds for a file, if it knows it.
    pub fn hash_of(&self, file: &Path) -> Option<String> {
        self.db
            .query_row(
                "SELECT hash FROM entries WHERE file = ?1",
                params![file.to_string_lossy()],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }
}

fn put(db: &Connection, entry: &Entry, file: &str, content: &str, hash: &str) -> Result<(), Error> {
    db.execute("DELETE FROM entries WHERE file = ?1", params![file])?;
    db.execute("DELETE FROM search WHERE file = ?1", params![file])?;
    db.execute(
        "INSERT INTO entries (file, kind, title, content, hash, archived)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            file,
            entry.kind.as_str(),
            entry.title,
            content,
            hash,
            entry.archived as i64
        ],
    )?;
    db.execute(
        "INSERT INTO search (file, title, content) VALUES (?1, ?2, ?3)",
        params![file, entry.title, content],
    )?;

    for task in &entry.tasks {
        db.execute(
            "INSERT INTO tasks (file, line, text, caption, status, id, ord, due)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                file,
                task.line as i64,
                task.parsed.text,
                task.parsed.caption,
                task.parsed.status.as_str(),
                task.parsed.id,
                task.parsed.order.map(|order| order as i64),
                task.parsed.due,
            ],
        )?;
        for tag in task.tags() {
            db.execute(
                "INSERT OR IGNORE INTO task_tags (file, line, tag) VALUES (?1, ?2, ?3)",
                params![file, task.line as i64, tag],
            )?;
        }
    }
    Ok(())
}

/// FNV-1a. Not a security hash — it only has to notice that a file changed and
/// give each vault its own cache directory.
fn fingerprint_str(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn fingerprint(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    fingerprint_str(&canonical.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_of(files: &[(&str, &str)]) -> (tempdir::Dir, Vault) {
        let dir = tempdir::Dir::new();
        let projects = dir.path().join("projects");
        std::fs::create_dir_all(&projects).expect("projects");
        for (name, body) in files {
            std::fs::write(projects.join(name), body).expect("write");
        }
        let vault = Vault::load(dir.path());
        (dir, vault)
    }

    /// A minimal scratch directory. den.nvim's AGENTS.md forbids pointing any
    /// test at a real vault, and the engine writes now, so this matters.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct Dir(PathBuf);

        /// Distinguishes concurrent scratch directories.
        ///
        /// A timestamp alone is not enough: `SystemTime::now()` has coarser
        /// resolution than the gap between two threads entering this function,
        /// so two tests could land in the same directory and see each other's
        /// files. That made the suite flaky in a way that only showed up under
        /// load — the failure looked like an indexing bug and was not one.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        impl Dir {
            pub fn new() -> Dir {
                let unique = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos();
                let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "den-index-test-{}-{unique}-{seq}",
                    std::process::id()
                ));
                std::fs::create_dir_all(&path).expect("scratch dir");
                Dir(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn indexes_entries_tasks_and_tags() {
        let (dir, vault) = vault_of(&[(
            "p.md",
            "# Project\n\n- [ ] Ship it @tag(web) @id(a) @due(2026-09-20)\n- [x] Done @tag(admin)\n",
        )]);
        let mut index = Index::in_memory(dir.path()).expect("index");

        let changed = index.refresh(&vault).expect("refresh");
        assert_eq!(changed.updated, 1);
        assert_eq!(changed.removed, 0);

        let tags = index.tags().expect("tags");
        assert!(tags.iter().any(|(tag, n)| tag == "web" && *n == 1));
        assert!(tags.iter().any(|(tag, n)| tag == "admin" && *n == 1));
    }

    #[test]
    fn refreshing_an_unchanged_vault_does_nothing() {
        let (dir, vault) = vault_of(&[("p.md", "# P\n\n- [ ] One\n")]);
        let mut index = Index::in_memory(dir.path()).expect("index");

        assert_eq!(index.refresh(&vault).expect("first").updated, 1);
        // The second pass must be a no-op: hashing is what keeps a poll cheap.
        assert!(index.refresh(&vault).expect("second").is_empty());
    }

    #[test]
    fn a_deleted_entry_leaves_the_index() {
        let (dir, vault) = vault_of(&[("p.md", "# P\n\n- [ ] One\n"), ("q.md", "# Q\n")]);
        let mut index = Index::in_memory(dir.path()).expect("index");
        index.refresh(&vault).expect("seed");

        std::fs::remove_file(dir.path().join("projects/q.md")).expect("remove");
        let changed = index.refresh(&Vault::load(dir.path())).expect("refresh");
        assert_eq!(changed.removed, 1);
    }

    #[test]
    fn search_finds_note_bodies() {
        let (dir, vault) = vault_of(&[("p.md", "# Widgets\n\nThe shelf is the slow part.\n")]);
        let mut index = Index::in_memory(dir.path()).expect("index");
        index.refresh(&vault).expect("refresh");

        assert_eq!(index.search("shelf").expect("search").len(), 1);
        assert!(
            index
                .search("nothing-like-this")
                .expect("search")
                .is_empty()
        );
        // A query with FTS5 punctuation must be treated as text, not syntax.
        assert!(index.search("shelf(").is_ok());
    }
}
