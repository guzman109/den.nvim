//! The vault model: Markdown entries on disk (or in Neovim buffers), parsed
//! into tasks the way den.nvim parses them.

pub mod parse;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use parse::{MAX_ORDER, ParsedTask, Status};

/// The four vault folders den.nvim indexes, in `lua/den/index.lua`'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Inbox,
    Notes,
    Projects,
    Daily,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Inbox, Kind::Notes, Kind::Projects, Kind::Daily];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Inbox => "inbox",
            Kind::Notes => "notes",
            Kind::Projects => "projects",
            Kind::Daily => "daily",
        }
    }

    /// Parses a folder name, which is also how a kind is stored in the index.
    pub fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A task, located in its file.
#[derive(Debug, Clone)]
pub struct Task {
    pub parsed: ParsedTask,
    pub file: PathBuf,
    /// 1-based, matching the `line` den.nvim's desktop bridge expects.
    pub line: usize,
    /// The owning entry's `# Title`.
    pub entry_title: String,
}

impl Task {
    pub fn status(&self) -> Status {
        self.parsed.status
    }

    /// What the UI shows: the caption with `@tag()`/`@due()` also removed.
    pub fn title(&self) -> &str {
        &self.parsed.display
    }

    pub fn tags(&self) -> &[String] {
        &self.parsed.tags
    }

    pub fn due(&self) -> Option<&str> {
        self.parsed.due.as_deref()
    }

    /// `website.md:12`, as the design labels task sources.
    pub fn location(&self) -> String {
        let name = self
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{name}:{}", self.line)
    }

    /// The slot of the task's first tag, which interfaces use to colour its
    /// stripe. `None` for a finished task, which is drawn neutrally.
    pub fn slot(&self, tags: &TagIndex) -> Option<u8> {
        match self.parsed.status {
            Status::Done => None,
            _ => self.parsed.tags.first().map(|tag| tags.slot(tag)),
        }
    }
}

/// One Markdown file.
#[derive(Debug, Clone)]
pub struct Entry {
    pub file: PathBuf,
    pub kind: Kind,
    pub title: String,
    pub lines: Vec<String>,
    pub archived: bool,
    pub tasks: Vec<Task>,
    /// True when Neovim holds unsaved changes for this file.
    pub modified: bool,
}

impl Entry {
    /// den.nvim's `parse.entry`, over an already-read file.
    pub fn parse(file: PathBuf, kind: Kind, content: &str, modified: bool) -> Entry {
        let lines: Vec<String> = content.split('\n').map(str::to_string).collect();
        let title = lines
            .first()
            .and_then(|line| parse::heading_title(line))
            .map(str::to_string)
            .unwrap_or_else(|| {
                file.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });

        let mut archived = false;
        let mut tasks = Vec::new();
        let mut fence = parse::Fence::default();
        for (index, line) in lines.iter().enumerate() {
            if fence.consume(line) || fence.is_open() {
                continue;
            }
            if parse::is_archived_line(line) {
                archived = true;
            }
            if let Some(parsed) = parse::parse_task(line) {
                tasks.push(Task {
                    parsed,
                    file: file.clone(),
                    line: index + 1,
                    entry_title: title.clone(),
                });
            }
        }

        Entry {
            file,
            kind,
            title,
            lines,
            archived,
            tasks,
            modified,
        }
    }

    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// `website.md`
    pub fn file_name(&self) -> String {
        self.file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn open_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks
            .iter()
            .filter(|t| t.parsed.status != Status::Done)
    }

    pub fn done_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks
            .iter()
            .filter(|t| t.parsed.status == Status::Done)
    }

    /// The entry's `Status:` line value, if it has one. The design shows this
    /// as the pill beside the project title.
    pub fn status_label(&self) -> Option<&str> {
        self.lines
            .iter()
            .find_map(|line| line.strip_prefix("Status:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// The entry's `Stack:` line, split into tokens.
    ///
    /// A note may declare what it is built with:
    ///
    /// ```text
    /// Stack: rust, typescript, tailwind
    /// ```
    ///
    /// The interface draws one icon per token. Parsing it here keeps the engine
    /// the only thing that reads the file format, and returning plain strings
    /// keeps the engine from knowing what an icon is — the same split that
    /// makes `TagIndex` hand out slots rather than colours.
    ///
    /// Absent on most notes, and deliberately so: a note about a desk has no
    /// stack, and the interface falls back to the note's kind.
    pub fn stack(&self) -> Vec<&str> {
        self.lines
            .iter()
            .find_map(|line| line.strip_prefix("Stack:"))
            .into_iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .collect()
    }

    /// Splits the body into `## Heading` sections for the home view's note
    /// panel. Lines before the first subheading land under an empty heading.
    pub fn sections(&self) -> Vec<Section<'_>> {
        let mut sections: Vec<Section<'_>> = Vec::new();
        let mut fence = parse::Fence::default();
        for (index, line) in self.lines.iter().enumerate() {
            let fenced = fence.consume(line) || fence.is_open();
            if !fenced && let Some(heading) = line.strip_prefix("## ") {
                sections.push(Section {
                    heading: heading.trim(),
                    lines: Vec::new(),
                });
                continue;
            }
            // Skip the title, the header lines and blank padding.
            if index == 0
                || line.trim().is_empty()
                || (!fenced && (line.starts_with("Status:") || line.starts_with("Stack:")))
            {
                continue;
            }
            if let Some(section) = sections.last_mut() {
                section.lines.push((index + 1, line.as_str()));
            }
        }
        sections.retain(|section| !section.lines.is_empty());
        sections
    }
}

#[derive(Debug, Clone)]
pub struct Section<'a> {
    pub heading: &'a str,
    /// `(1-based line number, text)`
    pub lines: Vec<(usize, &'a str)>,
}

/// Assigns a stable color to every tag in the vault.
///
/// The seven tags the design names keep the hues it drew them in, so `--demo`
/// reproduces the mockup exactly. Everything else is hashed, which is stable
/// across runs and independent of what else is in the vault.
#[derive(Debug, Clone, Default)]
pub struct TagIndex {
    counts: BTreeMap<String, usize>,
}

impl TagIndex {
    /// How many distinct colours an interface is expected to offer. The engine
    /// does not know what they look like — it only guarantees that a given tag
    /// always lands in the same slot.
    pub const SLOTS: u8 = 8;

    /// The well-known tags, pinned so a vault's colours never shuffle when an
    /// unrelated tag is added.
    const PINNED: [(&'static str, u8); 7] = [
        ("design", 0),
        ("orange", 1),
        ("writing", 2),
        ("deep-work", 3),
        ("web", 4),
        ("research", 5),
        ("admin", 7),
    ];

    /// A stable slot in `0..SLOTS` for this tag.
    ///
    /// Colour lives in the interface; the engine only provides the identity it
    /// is keyed by, so the desktop and the editor can agree on which tag is
    /// which without the engine knowing what a colour is.
    pub fn slot(&self, tag: &str) -> u8 {
        if let Some((_, slot)) = Self::PINNED.iter().find(|(name, _)| *name == tag) {
            return *slot;
        }
        // FNV-1a, so the choice does not move between runs or platforms.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in tag.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        (hash % u64::from(Self::SLOTS)) as u8
    }

    /// Tags with their task counts, most used first, then alphabetical.
    pub fn ranked(&self) -> Vec<(&str, u8, usize)> {
        let mut ranked: Vec<_> = self
            .counts
            .iter()
            .map(|(tag, count)| (tag.as_str(), self.slot(tag), *count))
            .collect();
        ranked.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)));
        ranked
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Vault {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub tags: TagIndex,
}

impl Vault {
    /// Reads the four vault folders from disk. Unreadable files are skipped
    /// rather than failing the load — a vault is user-owned and may contain
    /// anything.
    pub fn load(root: &Path) -> Vault {
        let mut sources = Vec::new();
        for kind in Kind::ALL {
            let directory = root.join(kind.as_str());
            let Ok(listing) = std::fs::read_dir(&directory) else {
                continue;
            };
            for file in listing.flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e == "md")
                    && file.file_type().is_ok_and(|t| t.is_file())
                    && let Ok(content) = std::fs::read_to_string(&path)
                {
                    sources.push((path, kind, content, false));
                }
            }
        }
        Vault::from_sources(root.to_path_buf(), sources)
    }

    /// Whether Den can write into the vault at all.
    ///
    /// This is the one condition that genuinely makes Den read-only, and it has
    /// nothing to do with Neovim: the engine writes Markdown itself, so an
    /// unattached Den is fully writable. What makes it read-only is the
    /// directory — a read-only mount, a vault owned by another user, or a
    /// sandbox that has not been granted the folder.
    ///
    /// Probed by creating and removing a file rather than by reading permission
    /// bits, because the bits do not account for mount flags, ACLs or sandbox
    /// policy. `mutate::write_atomically` already creates a temp file beside its
    /// target, so this exercises the same mechanism the real write path uses.
    pub fn is_writable(root: &Path) -> bool {
        let probe = root.join(format!(".den-write-probe-{}", std::process::id()));
        match std::fs::File::create(&probe) {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    }

    /// A cheap stat-only summary of the vault on disk, used to decide whether a
    /// reload is worth doing. Re-reading every file on a timer would be wasteful
    /// for a large vault; comparing this is two syscalls per file.
    pub fn fingerprint(root: &Path) -> Vec<(PathBuf, u64, u64)> {
        let mut marks = Vec::new();
        for kind in Kind::ALL {
            let Ok(listing) = std::fs::read_dir(root.join(kind.as_str())) else {
                continue;
            };
            for file in listing.flatten() {
                let path = file.path();
                if !path.extension().is_some_and(|e| e == "md") {
                    continue;
                }
                let Ok(meta) = file.metadata() else { continue };
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or_default();
                marks.push((path, modified, meta.len()));
            }
        }
        marks.sort();
        marks
    }

    /// Marks entries that an editor is holding unsaved changes for.
    ///
    /// The engine reads from disk, so a dirty buffer is not a *source* of
    /// content — it is a reason to refuse a write. The interface that knows
    /// about buffers (den.nvim) reports the paths; everything else just shows
    /// the flag.
    pub fn mark_modified(&mut self, dirty: &[PathBuf]) {
        for entry in &mut self.entries {
            entry.modified = dirty.contains(&entry.file);
        }
    }

    fn from_sources(root: PathBuf, sources: Vec<(PathBuf, Kind, String, bool)>) -> Vault {
        let mut entries: Vec<Entry> = sources
            .into_iter()
            .map(|(path, kind, content, modified)| Entry::parse(path, kind, &content, modified))
            .collect();
        // den.nvim sorts entries by path; matching it keeps the two UIs in step.
        entries.sort_by(|a, b| a.file.cmp(&b.file));

        // Counts cover every task, finished ones included: a tag that only ever
        // appears on closed work still belongs in the index, and the design's
        // own sidebar counts it that way (`#studio 3` includes "Clear the desk").
        let mut tags = TagIndex::default();
        for entry in entries.iter().filter(|e| !e.archived) {
            for task in &entry.tasks {
                for tag in task.tags() {
                    *tags.counts.entry(tag.clone()).or_default() += 1;
                }
            }
        }

        Vault {
            root,
            entries,
            tags,
        }
    }

    /// Entries that are not archived, which is what every view works from.
    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| !entry.archived)
    }

    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.active().flat_map(|entry| entry.tasks.iter())
    }

    pub fn entry(&self, file: &Path) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.file == file)
    }

    pub fn task_at(&self, file: &Path, line: usize) -> Option<&Task> {
        self.entry(file)?
            .tasks
            .iter()
            .find(|task| task.line == line)
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for task in self.tasks() {
            counts.total += 1;
            match task.status() {
                Status::Backlog => counts.backlog += 1,
                Status::Doing => counts.doing += 1,
                Status::Done => counts.done += 1,
            }
        }
        counts
    }

    /// The name shown in the title bar and status bar.
    pub fn label(&self) -> String {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Counts {
    pub total: usize,
    pub backlog: usize,
    pub doing: usize,
    pub done: usize,
}

impl Counts {
    pub fn open(self) -> usize {
        self.backlog + self.doing
    }

    /// 0.0–1.0, for the HEARTH meters.
    pub fn done_fraction(self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        self.done as f32 / self.total as f32
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;

    fn entry(body: &str) -> Entry {
        let vault = Vault::from_sources(
            PathBuf::from("/tmp/den-test"),
            vec![(
                PathBuf::from("/tmp/den-test/projects/n.md"),
                Kind::Projects,
                body.to_string(),
                false,
            )],
        );
        vault.entries.into_iter().next().expect("one entry")
    }

    #[test]
    fn stack_is_split_on_commas_and_trimmed() {
        let e = entry("# N\n\nStatus: active\nStack: rust,  typescript , tailwind\n");
        assert_eq!(e.stack(), vec!["rust", "typescript", "tailwind"]);
        assert_eq!(e.status_label(), Some("active"));
    }

    #[test]
    fn a_note_without_a_stack_reports_none_rather_than_an_empty_token() {
        // The common case: most notes are not code projects.
        assert!(entry("# N\n\nStatus: active\n").stack().is_empty());
        assert!(entry("# N\n\nStack:\n").stack().is_empty());
        assert!(entry("# N\n\nStack:   ,  ,\n").stack().is_empty());
    }

    #[test]
    fn header_lines_stay_out_of_the_body_sections() {
        let e = entry("# N\n\nStatus: active\nStack: rust\n\n## Notes\n\nreal prose\n");
        let sections = e.sections();
        let text: Vec<&str> = sections
            .iter()
            .flat_map(|s| s.lines.iter().map(|(_, l)| *l))
            .collect();
        assert_eq!(text, vec!["real prose"], "headers must not leak into prose");
    }

    #[test]
    fn a_writable_directory_probes_writable_and_a_missing_one_does_not() {
        let dir = std::env::temp_dir().join(format!("den-writable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        assert!(Vault::is_writable(&dir));
        // The probe must leave nothing behind.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);

        std::fs::remove_dir_all(&dir).ok();
        assert!(!Vault::is_writable(&dir), "a missing vault is not writable");
    }
}
