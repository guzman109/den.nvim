//! The timer log: when you worked on what.
//!
//! Markdown records what is true now, never when it happened, so timer
//! sessions live in their own append-only log, one file per machine:
//!
//! ```text
//! <vault>/.den/log/<machine>.jsonl
//! {"at":"2026-09-24T18:02:11Z","event":"start","file":"projects/haste.md","task":"Wire up the renderer"}
//! {"at":"2026-09-24T18:26:21Z","event":"stop"}
//! ```
//!
//! Each machine only ever appends to its own file, so git merges the logs of
//! several machines without conflicts. Every machine reads all of them.
//!
//! A task is identified by its file and title. When Den sees a task renamed or
//! moved it logs a `rename`, and earlier sessions follow the new name.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Start,
    Stop,
    Rename,
    WalkStart,
    WalkEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub at: Timestamp,
    pub event: Event,
    /// Vault path of the file holding the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_task: Option<String>,
    /// Which machine's log the entry came from.
    #[serde(skip)]
    pub machine: String,
}

impl Entry {
    fn new(at: Timestamp, event: Event) -> Entry {
        Entry {
            at,
            event,
            file: None,
            task: None,
            from_file: None,
            from_task: None,
            machine: String::new(),
        }
    }
}

/// The timer running on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Running {
    pub file: String,
    pub task: String,
    pub since: Timestamp,
}

/// One stretch of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Session {
    pub machine: String,
    pub file: String,
    pub task: String,
    pub start: Timestamp,
    pub end: Timestamp,
    /// Still running; `end` is "now".
    pub open: bool,
}

impl Session {
    /// Seconds of this session inside `[from, to)`.
    pub fn seconds_within(&self, from: Timestamp, to: Timestamp) -> i64 {
        let start = self.start.max(from);
        let end = self.end.min(to);
        (end.as_second() - start.as_second()).max(0)
    }
}

#[derive(Debug, Clone)]
pub struct TimerLog {
    dir: PathBuf,
    machine: String,
    entries: Vec<Entry>,
    /// Lines that could not be read, as `file:line: why`.
    pub problems: Vec<String>,
}

impl TimerLog {
    pub fn dir(vault_root: &Path) -> PathBuf {
        vault_root.join(".den").join("log")
    }

    /// Reads every machine's log. A missing folder is an empty log.
    pub fn load(vault_root: &Path, machine: &str) -> Result<TimerLog> {
        let mut log = TimerLog {
            dir: TimerLog::dir(vault_root),
            machine: machine.to_string(),
            entries: Vec::new(),
            problems: Vec::new(),
        };
        log.reload()?;
        Ok(log)
    }

    pub fn reload(&mut self) -> Result<()> {
        self.entries.clear();
        self.problems.clear();
        let listing = match std::fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(Error::io(&self.dir, e)),
        };
        let mut files: Vec<PathBuf> = listing
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .collect();
        files.sort();
        for file in files {
            let machine = file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = std::fs::read_to_string(&file).map_err(|e| Error::io(&file, e))?;
            for (i, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Entry>(line) {
                    Ok(mut entry) => {
                        entry.machine = machine.clone();
                        self.entries.push(entry);
                    }
                    Err(e) => self
                        .problems
                        .push(format!("{}:{}: {e}", file.display(), i + 1)),
                }
            }
        }
        self.entries.sort_by_key(|e| e.at);
        Ok(())
    }

    pub fn machine(&self) -> &str {
        &self.machine
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn own_file(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.machine))
    }

    fn append(&mut self, mut entry: Entry) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::io(&self.dir, e))?;
        let path = self.own_file();
        let line = serde_json::to_string(&entry).map_err(|e| Error::Log {
            path: path.clone(),
            message: e.to_string(),
        })?;
        let mut file = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::io(&path, e))?;
        file.write_all(format!("{line}\n").as_bytes())
            .and_then(|()| file.sync_data())
            .map_err(|e| Error::io(&path, e))?;
        entry.machine = self.machine.clone();
        let at = self.entries.partition_point(|e| e.at <= entry.at);
        self.entries.insert(at, entry);
        Ok(())
    }

    /// What this machine's timer is running, if anything.
    pub fn running(&self) -> Option<Running> {
        let mut running: Option<Running> = None;
        for e in self.entries.iter().filter(|e| e.machine == self.machine) {
            match e.event {
                Event::Start => {
                    running = Some(Running {
                        file: e.file.clone().unwrap_or_default(),
                        task: e.task.clone().unwrap_or_default(),
                        since: e.at,
                    });
                }
                Event::Stop => running = None,
                Event::Rename => {
                    if let Some(r) = running.as_mut()
                        && Some(&r.file) == e.from_file.as_ref()
                        && Some(&r.task) == e.from_task.as_ref()
                    {
                        r.file = e.file.clone().unwrap_or_default();
                        r.task = e.task.clone().unwrap_or_default();
                    }
                }
                Event::WalkStart | Event::WalkEnd => {}
            }
        }
        running
    }

    /// Starts timing a task, stopping whatever was running.
    pub fn start(&mut self, file: &str, task: &str, at: Timestamp) -> Result<()> {
        self.reload()?;
        if let Some(r) = self.running() {
            if r.file == file && r.task == task {
                return Ok(());
            }
            self.append(Entry::new(at, Event::Stop))?;
        }
        let mut entry = Entry::new(at, Event::Start);
        entry.file = Some(file.to_string());
        entry.task = Some(task.to_string());
        self.append(entry)
    }

    /// Stops the timer. Returns what was running.
    pub fn stop(&mut self, at: Timestamp) -> Result<Option<Running>> {
        self.reload()?;
        let running = self.running();
        if running.is_some() {
            self.append(Entry::new(at, Event::Stop))?;
        }
        Ok(running)
    }

    /// Records that a task was renamed or moved, so its history follows it.
    pub fn rename(
        &mut self,
        from_file: &str,
        from_task: &str,
        file: &str,
        task: &str,
        at: Timestamp,
    ) -> Result<()> {
        if from_file == file && from_task == task {
            return Ok(());
        }
        let mut entry = Entry::new(at, Event::Rename);
        entry.from_file = Some(from_file.to_string());
        entry.from_task = Some(from_task.to_string());
        entry.file = Some(file.to_string());
        entry.task = Some(task.to_string());
        self.append(entry)
    }

    pub fn walk_start(&mut self, at: Timestamp) -> Result<()> {
        self.append(Entry::new(at, Event::WalkStart))
    }

    pub fn walk_end(&mut self, at: Timestamp) -> Result<()> {
        self.append(Entry::new(at, Event::WalkEnd))
    }

    /// Every session on every machine, with renames applied. Sessions still
    /// running end at `now`.
    pub fn sessions(&self, now: Timestamp) -> Vec<Session> {
        let mut done: Vec<Session> = Vec::new();
        let mut open: BTreeMap<String, Session> = BTreeMap::new();
        for e in &self.entries {
            match e.event {
                Event::Start => {
                    if let Some(mut s) = open.remove(&e.machine) {
                        s.end = e.at;
                        done.push(s);
                    }
                    open.insert(
                        e.machine.clone(),
                        Session {
                            machine: e.machine.clone(),
                            file: e.file.clone().unwrap_or_default(),
                            task: e.task.clone().unwrap_or_default(),
                            start: e.at,
                            end: e.at,
                            open: true,
                        },
                    );
                }
                Event::Stop => {
                    if let Some(mut s) = open.remove(&e.machine) {
                        s.end = e.at;
                        s.open = false;
                        done.push(s);
                    }
                }
                Event::Rename => {
                    let (Some(ff), Some(ft), Some(f), Some(t)) =
                        (&e.from_file, &e.from_task, &e.file, &e.task)
                    else {
                        continue;
                    };
                    for s in done.iter_mut().chain(open.values_mut()) {
                        if &s.file == ff && &s.task == ft {
                            s.file = f.clone();
                            s.task = t.clone();
                        }
                    }
                }
                Event::WalkStart | Event::WalkEnd => {}
            }
        }
        for (_, mut s) in open {
            s.end = now.max(s.start);
            s.open = true;
            done.push(s);
        }
        for s in &mut done {
            if !s.open {
                continue;
            }
            s.end = now.max(s.start);
        }
        done.sort_by_key(|s| s.start);
        done
    }

    /// Seconds worked inside `[from, to)`, per file.
    pub fn seconds_by_file(
        &self,
        from: Timestamp,
        to: Timestamp,
        now: Timestamp,
    ) -> BTreeMap<String, i64> {
        let mut out = BTreeMap::new();
        for s in self.sessions(now) {
            let secs = s.seconds_within(from, to);
            if secs > 0 {
                *out.entry(s.file).or_insert(0) += secs;
            }
        }
        out
    }

    /// Walks and breaks on this machine as `(start, end)`, an unfinished one
    /// ending at `now`.
    pub fn walks(&self, now: Timestamp) -> Vec<(Timestamp, Timestamp)> {
        let mut out = Vec::new();
        let mut start: Option<Timestamp> = None;
        for e in self.entries.iter().filter(|e| e.machine == self.machine) {
            match e.event {
                Event::WalkStart => start = start.or(Some(e.at)),
                Event::WalkEnd => {
                    if let Some(s) = start.take() {
                        out.push((s, e.at));
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push((s, now.max(s)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn starting_a_task_stops_the_last_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = TimerLog::load(dir.path(), "mac").unwrap();
        log.start("projects/a.md", "one", at("2026-09-24T10:00:00Z"))
            .unwrap();
        log.start("projects/a.md", "two", at("2026-09-24T10:30:00Z"))
            .unwrap();
        let running = log.running().unwrap();
        assert_eq!(running.task, "two");

        let sessions = log.sessions(at("2026-09-24T11:00:00Z"));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].task, "one");
        assert_eq!(
            sessions[0].seconds_within(at("2026-09-24T00:00:00Z"), at("2026-09-25T00:00:00Z")),
            1800
        );
        assert!(sessions[1].open);
    }

    #[test]
    fn history_follows_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = TimerLog::load(dir.path(), "mac").unwrap();
        log.start("projects/a.md", "old", at("2026-09-24T10:00:00Z"))
            .unwrap();
        log.stop(at("2026-09-24T10:20:00Z")).unwrap();
        log.rename(
            "projects/a.md",
            "old",
            "projects/b.md",
            "new",
            at("2026-09-24T12:00:00Z"),
        )
        .unwrap();
        let sessions = log.sessions(at("2026-09-24T13:00:00Z"));
        assert_eq!(sessions[0].file, "projects/b.md");
        assert_eq!(sessions[0].task, "new");
    }

    #[test]
    fn reads_other_machines_and_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut mac = TimerLog::load(dir.path(), "mac").unwrap();
        mac.start("projects/a.md", "x", at("2026-09-24T10:00:00Z"))
            .unwrap();
        mac.stop(at("2026-09-24T11:00:00Z")).unwrap();
        let mut linux = TimerLog::load(dir.path(), "linux").unwrap();
        linux
            .start("projects/a.md", "x", at("2026-09-24T12:00:00Z"))
            .unwrap();
        assert!(linux.running().is_some());
        assert!(
            TimerLog::load(dir.path(), "mac")
                .unwrap()
                .running()
                .is_none()
        );

        let totals = linux.seconds_by_file(
            at("2026-09-24T00:00:00Z"),
            at("2026-09-25T00:00:00Z"),
            at("2026-09-24T12:30:00Z"),
        );
        assert_eq!(totals["projects/a.md"], 3600 + 1800);
    }

    #[test]
    fn bad_lines_are_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let logs = TimerLog::dir(dir.path());
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("mac.jsonl"),
            "not json\n{\"at\":\"2026-09-24T10:00:00Z\",\"event\":\"stop\"}\n",
        )
        .unwrap();
        let log = TimerLog::load(dir.path(), "mac").unwrap();
        assert_eq!(log.entries().len(), 1);
        assert_eq!(log.problems.len(), 1);
    }
}
