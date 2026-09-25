//! The vault: every Markdown file Den knows about, parsed and kept current.
//!
//! ```text
//! <root>/
//!   projects/<name>.md      one file per project (no subfolders)
//!   notes/**/*.md           everything else, any depth
//!   daily/<date>.md         one journal page per day
//!   templates/*.md          templates, such as daily.md
//!   inbox.md                captures made outside any project
//! ```
//!
//! A `.md.age` file in the same places is a locked note: Den knows it exists
//! but cannot read it without the key.
//!
//! When an editor holds unsaved text for a file, that text can be laid over
//! the file (an *overlay*). Den then reads the editor's version, and refuses
//! to write the file on disk until the overlay is cleared.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::parse::{Parsed, parse};
use crate::text::TextBuf;
use crate::worktree;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Project,
    Note,
    Daily,
    Template,
    Inbox,
}

/// One file in the vault.
#[derive(Debug, Clone)]
pub struct Doc {
    /// Relative to the vault root, with `/` separators.
    pub path: String,
    pub kind: Kind,
    /// Stored encrypted; `text` is empty until it is unlocked.
    pub locked: bool,
    /// The text Den reads: the editor's unsaved buffer when overlaid, else disk.
    pub text: String,
    pub buf: TextBuf,
    pub parsed: Parsed,
    pub overlaid: bool,
}

impl Doc {
    fn new(path: String, kind: Kind, locked: bool, text: String, overlaid: bool) -> Doc {
        let buf = TextBuf::parse(&text);
        let parsed = parse(&buf);
        Doc {
            path,
            kind,
            locked,
            text,
            buf,
            parsed,
            overlaid,
        }
    }

    /// The file name without its extension: `projects/website.md` → `website`.
    pub fn name(&self) -> &str {
        let file = self.path.rsplit('/').next().unwrap_or(&self.path);
        file.strip_suffix(".md.age")
            .or_else(|| file.strip_suffix(".md"))
            .unwrap_or(file)
    }

    /// The first `# ` heading, or the file name.
    pub fn title(&self) -> String {
        self.parsed
            .title
            .clone()
            .unwrap_or_else(|| self.name().to_string())
    }

    pub fn field(&self, key: &str) -> Option<String> {
        self.parsed.frontmatter.text(key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Active,
    Paused,
    Archived,
}

/// A project: a file in `projects/`, seen through its fields.
#[derive(Debug, Clone, Copy)]
pub struct Project<'a> {
    pub doc: &'a Doc,
}

impl<'a> Project<'a> {
    pub fn name(&self) -> &'a str {
        self.doc.name()
    }

    pub fn title(&self) -> String {
        self.doc.title()
    }

    /// The code folder this project belongs to, with `~` expanded.
    pub fn root(&self) -> Option<PathBuf> {
        self.doc.field("root").map(|r| expand_home(&r))
    }

    /// `active` unless the file says `paused` or `archived`.
    pub fn status(&self) -> ProjectStatus {
        match self.doc.field("status").as_deref() {
            Some("paused") => ProjectStatus::Paused,
            Some("archived") => ProjectStatus::Archived,
            _ => ProjectStatus::Active,
        }
    }

    pub fn due(&self) -> Option<Date> {
        self.doc.parsed.frontmatter.date("due")
    }

    pub fn created(&self) -> Option<Date> {
        self.doc.parsed.frontmatter.date("created")
    }
}

/// A file Den found but could not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
    docs: BTreeMap<String, Doc>,
    problems: Vec<Problem>,
}

impl Vault {
    /// Reads every file in the vault. The folder must exist; it may be empty.
    pub fn open(root: impl Into<PathBuf>) -> Result<Vault> {
        let root = root.into();
        let meta = std::fs::metadata(&root).map_err(|e| Error::io(&root, e))?;
        if !meta.is_dir() {
            return Err(Error::Invalid(format!(
                "{} is not a folder",
                root.display()
            )));
        }
        let mut vault = Vault {
            root,
            docs: BTreeMap::new(),
            problems: Vec::new(),
        };
        vault.rescan()?;
        Ok(vault)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn docs(&self) -> impl Iterator<Item = &Doc> {
        self.docs.values()
    }

    pub fn doc(&self, path: &str) -> Option<&Doc> {
        self.docs.get(path)
    }

    /// Files that exist but could not be read.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// Finds added and removed files and re-reads everything from disk,
    /// keeping any overlays.
    pub fn rescan(&mut self) -> Result<()> {
        let mut found = Vec::new();
        collect(&self.root, &self.root, &mut found).map_err(|e| Error::io(&self.root, e))?;
        let overlays: BTreeMap<String, String> = self
            .docs
            .values()
            .filter(|d| d.overlaid)
            .map(|d| (d.path.clone(), d.text.clone()))
            .collect();
        self.docs.clear();
        self.problems.clear();
        for (path, kind, locked) in found {
            if let Some(text) = overlays.get(&path) {
                self.docs.insert(
                    path.clone(),
                    Doc::new(path, kind, locked, text.clone(), true),
                );
            } else {
                self.load(path, kind, locked);
            }
        }
        Ok(())
    }

    /// Re-reads one file from disk (or drops it if it is gone). Overlaid files
    /// keep the editor's text.
    pub fn reload(&mut self, path: &str) {
        self.problems.retain(|p| p.path != path);
        if self.docs.get(path).is_some_and(|d| d.overlaid) {
            return;
        }
        match classify(path) {
            Some((kind, locked)) if self.abs(path).is_file() => {
                self.load(path.to_string(), kind, locked);
            }
            _ => {
                self.docs.remove(path);
            }
        }
    }

    /// Like [`Vault::reload`], but says whether anything changed: `false` when
    /// the file on disk still holds exactly what Den already has, as after
    /// Den's own writes.
    pub fn reload_if_changed(&mut self, path: &str) -> bool {
        let before = self.docs.get(path).map(|d| (d.text.clone(), d.overlaid));
        if let Some((_, true)) = before {
            return false;
        }
        let on_disk = std::fs::read(self.abs(path)).ok();
        match (&before, &on_disk) {
            (Some((text, _)), Some(bytes)) if text.as_bytes() == bytes.as_slice() => false,
            (None, None) => false,
            _ => {
                self.reload(path);
                true
            }
        }
    }

    fn load(&mut self, path: String, kind: Kind, locked: bool) {
        if locked {
            self.docs.insert(
                path.clone(),
                Doc::new(path, kind, true, String::new(), false),
            );
            return;
        }
        let abs = self.abs(&path);
        match std::fs::read(&abs) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => {
                    self.docs
                        .insert(path.clone(), Doc::new(path, kind, false, text, false));
                }
                Err(_) => {
                    self.docs.remove(&path);
                    self.problems.push(Problem {
                        path,
                        message: "not UTF-8 text".to_string(),
                    });
                }
            },
            Err(e) => {
                self.docs.remove(&path);
                self.problems.push(Problem {
                    path,
                    message: e.to_string(),
                });
            }
        }
    }

    /// Lays an editor's unsaved text over a file, or clears it with `None`.
    pub fn set_overlay(&mut self, path: &str, text: Option<String>) -> Result<()> {
        let (kind, locked) = classify(path).ok_or_else(|| Error::OutsideVault(path.to_string()))?;
        match text {
            Some(text) => {
                self.docs.insert(
                    path.to_string(),
                    Doc::new(path.to_string(), kind, locked, text, true),
                );
            }
            None => {
                if let Some(doc) = self.docs.get_mut(path) {
                    doc.overlaid = false;
                }
                self.reload(path);
            }
        }
        Ok(())
    }

    /// Files with an overlay: the ones Den must not write.
    pub fn dirty(&self) -> BTreeSet<String> {
        self.docs
            .values()
            .filter(|d| d.overlaid)
            .map(|d| d.path.clone())
            .collect()
    }

    pub fn abs(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// The vault path of an absolute path, if it names a vault file.
    pub fn rel(&self, abs: &Path) -> Option<String> {
        let root = std::fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        let abs = std::fs::canonicalize(abs).unwrap_or_else(|_| abs.to_path_buf());
        let inner = abs
            .strip_prefix(&root)
            .or_else(|_| abs.strip_prefix(&self.root))
            .ok()?;
        let path = inner
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()?
            .join("/");
        classify(&path).map(|_| path)
    }

    pub fn projects(&self) -> impl Iterator<Item = Project<'_>> {
        self.docs
            .values()
            .filter(|d| d.kind == Kind::Project)
            .map(|doc| Project { doc })
    }

    pub fn project(&self, name: &str) -> Option<Project<'_>> {
        let plain = format!("projects/{name}.md");
        self.docs
            .get(&plain)
            .or_else(|| self.docs.get(&format!("{plain}.age")))
            .map(|doc| Project { doc })
    }

    /// The project a document belongs to: itself, or the one its `project:`
    /// field names.
    pub fn project_of(&self, doc: &Doc) -> Option<Project<'_>> {
        match doc.kind {
            Kind::Project => self.project(doc.name()),
            _ => self.project(&doc.field("project")?),
        }
    }

    /// Notes that name `project` in their `project:` field.
    pub fn notes_of(&self, project: &str) -> Vec<&Doc> {
        self.docs
            .values()
            .filter(|d| d.kind == Kind::Note && d.field("project").as_deref() == Some(project))
            .collect()
    }

    /// Every note that names a project, grouped by project name. One pass,
    /// for views that need the notes of many projects.
    pub fn notes_by_project(&self) -> BTreeMap<String, Vec<&Doc>> {
        let mut out: BTreeMap<String, Vec<&Doc>> = BTreeMap::new();
        for doc in self.docs.values().filter(|d| d.kind == Kind::Note) {
            if let Some(project) = doc.field("project") {
                out.entry(project).or_default().push(doc);
            }
        }
        out
    }

    /// The project whose `root:` contains `dir`, looking through git
    /// worktrees to their main checkout. The deepest root wins.
    pub fn project_for_dir(&self, dir: &Path) -> Option<Project<'_>> {
        let mut candidates = vec![std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())];
        if let Some(main) = worktree::in_main_checkout(&candidates[0]) {
            candidates.push(main);
        }
        self.projects()
            .filter_map(|p| {
                let root = p.root()?;
                let root = std::fs::canonicalize(&root).unwrap_or(root);
                candidates
                    .iter()
                    .any(|c| c.starts_with(&root))
                    .then(|| (root.components().count(), p))
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, p)| p)
    }
}

/// What a vault path holds, or `None` if Den does not read it.
pub fn classify(path: &str) -> Option<(Kind, bool)> {
    let (stem_ok, locked) = if path.ends_with(".md.age") {
        (true, true)
    } else {
        (path.ends_with(".md"), false)
    };
    if !stem_ok
        || path
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part == "..")
    {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    let kind = match parts.as_slice() {
        ["inbox.md"] => Kind::Inbox,
        ["projects", _] => Kind::Project,
        ["notes", ..] if parts.len() >= 2 => Kind::Note,
        ["daily", _] => Kind::Daily,
        ["templates", _] => Kind::Template,
        _ => return None,
    };
    Some((kind, locked))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Kind, bool)>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && dir != root => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file_type = std::fs::metadata(&path).map(|m| m.file_type());
        let Ok(file_type) = file_type else { continue };
        if file_type.is_dir() {
            collect(root, &path, out)?;
        } else if file_type.is_file() {
            let Ok(inner) = path.strip_prefix(root) else {
                continue;
            };
            let Some(rel) = inner
                .components()
                .map(|c| c.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()
                .map(|parts| parts.join("/"))
            else {
                continue;
            };
            if let Some((kind, locked)) = classify(&rel) {
                out.push((rel, kind, locked));
            }
        }
    }
    Ok(())
}

/// `~/code` → `/Users/me/code`.
pub fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => std::env::home_dir().unwrap_or_else(|| PathBuf::from(path)),
        _ => PathBuf::from(path),
    }
}

/// `/Users/me/code` → `~/code`, for writing paths people read.
pub fn contract_home(path: &Path) -> String {
    if let Some(home) = std::env::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_vault_paths() {
        assert_eq!(classify("projects/site.md"), Some((Kind::Project, false)));
        assert_eq!(classify("projects/sub/site.md"), None);
        assert_eq!(classify("notes/a/b/c.md"), Some((Kind::Note, false)));
        assert_eq!(
            classify("daily/2026-09-24.md.age"),
            Some((Kind::Daily, true))
        );
        assert_eq!(classify("inbox.md"), Some((Kind::Inbox, false)));
        assert_eq!(classify("README.md"), None);
        assert_eq!(classify("notes/.hidden.md"), None);
        assert_eq!(classify("notes/../x.md"), None);
        assert_eq!(classify("notes/a.txt"), None);
    }
}
