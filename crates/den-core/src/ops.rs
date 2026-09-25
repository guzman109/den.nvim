//! Planning changes to the vault.
//!
//! Nothing here touches the disk. Each function returns the [`Change`]s that
//! would make the edit, each holding the file's full text before and after.
//! [`crate::write::apply`] writes them; an editor can instead apply `after`
//! to an open buffer.
//!
//! When a change spans two files (moving a task), the file gaining the task
//! comes first and the one losing it second, so an interruption between the
//! two leaves a duplicate rather than a lost task.

use std::path::Path;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::frontmatter::set_field;
use crate::parse::{Parsed, State, parse, task_line};
use crate::slug::slug;
use crate::text::TextBuf;
use crate::vault::{Doc, Kind, Vault, classify, contract_home};

/// One file's text before and after an edit. `before: None` creates the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

/// A task as the caller last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRef {
    pub path: String,
    pub line: usize,
    /// The whole line as read.
    pub raw: String,
}

/// The two sections Den writes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Inbox,
    NextActions,
}

impl Section {
    pub fn heading(self) -> &'static str {
        match self {
            Section::Inbox => "Inbox",
            Section::NextActions => "Next actions",
        }
    }
}

pub const GENERAL_INBOX: &str = "inbox.md";

pub const DEFAULT_DAILY_TEMPLATE: &str = "---\ndate: {{date}}\nmood:\n---\n# {{title}}\n\n## On my mind\n\n## Went well\n\n## Tomorrow\n";

impl Vault {
    /// Finds a task by reference. If lines above it moved, a single line
    /// elsewhere in the file with exactly the same text is taken to be it.
    fn locate(&self, task: &TaskRef) -> Result<(&Doc, usize)> {
        let doc = self
            .doc(&task.path)
            .ok_or_else(|| Error::OutsideVault(task.path.clone()))?;
        if doc.locked {
            return Err(Error::Invalid(format!("{} is locked", task.path)));
        }
        let lines = &doc.buf.lines;
        let index = if lines.get(task.line) == Some(&task.raw) {
            task.line
        } else {
            let mut found = lines.iter().enumerate().filter(|(_, l)| **l == task.raw);
            match (found.next(), found.next()) {
                (Some((i, _)), None) => i,
                _ => return Err(Error::Stale(task.path.clone())),
            }
        };
        if task_line(&lines[index]).is_none() {
            return Err(Error::NotATask {
                path: task.path.clone(),
                line: index + 1,
            });
        }
        Ok((doc, index))
    }

    /// Start, pause (`Doing`), finish, drop or reopen a task. Finishing adds
    /// `@done(today)`; any other state removes it.
    pub fn plan_state(&self, task: &TaskRef, state: State, today: Date) -> Result<Vec<Change>> {
        let (doc, index) = self.locate(task)?;
        let mut buf = doc.buf.clone();
        buf.lines[index] =
            with_state(&buf.lines[index], state, today).ok_or_else(|| Error::NotATask {
                path: task.path.clone(),
                line: index + 1,
            })?;
        Ok(vec![change(doc, &buf)])
    }

    /// Replace a task's text, keeping its checkbox and indentation.
    pub fn plan_edit(&self, task: &TaskRef, text: &str) -> Result<Vec<Change>> {
        let text = one_line(text)?;
        let (doc, index) = self.locate(task)?;
        let mut buf = doc.buf.clone();
        let line = &buf.lines[index];
        let item = task_line(line).ok_or_else(|| Error::NotATask {
            path: task.path.clone(),
            line: index + 1,
        })?;
        buf.lines[index] = format!("{}] {text}", &line[..=item.mark_at]);
        Ok(vec![change(doc, &buf)])
    }

    /// Save a quick thought to a project's Inbox, or to the general inbox.
    pub fn plan_capture(&self, project: Option<&str>, text: &str) -> Result<Vec<Change>> {
        match project {
            Some(name) => self.plan_add(name, text, Section::Inbox),
            None => {
                let line = format!("- [ ] {}", one_line(text)?);
                match self.doc(GENERAL_INBOX) {
                    Some(doc) => {
                        let mut buf = doc.buf.clone();
                        let at =
                            last_content_line(&buf.lines, 0, buf.lines.len()).map_or(0, |i| i + 1);
                        buf.lines.insert(at, line);
                        buf.trailing = true;
                        Ok(vec![change(doc, &buf)])
                    }
                    None => Ok(vec![Change {
                        path: GENERAL_INBOX.to_string(),
                        before: None,
                        after: format!("# Inbox\n\n{line}\n"),
                    }]),
                }
            }
        }
    }

    /// Add a new open task to a project section.
    pub fn plan_add(&self, project: &str, text: &str, section: Section) -> Result<Vec<Change>> {
        let line = format!("- [ ] {}", one_line(text)?);
        let doc = self.project_doc(project)?;
        let mut buf = doc.buf.clone();
        insert_in_section(&mut buf, section, &[line]);
        Ok(vec![change(doc, &buf)])
    }

    /// Move a task (with any lines indented under it) into a section of a
    /// project — another one, or its own when `project` is `None`.
    pub fn plan_move(
        &self,
        task: &TaskRef,
        project: Option<&str>,
        section: Section,
    ) -> Result<Vec<Change>> {
        let (source, index) = self.locate(task)?;
        let target = match project {
            Some(name) => self.project_doc(name)?,
            None if source.kind == Kind::Project => source,
            None => match self.project_of(source) {
                Some(p) => p.doc,
                None => {
                    return Err(Error::Invalid(format!(
                        "{} belongs to no project",
                        source.path
                    )));
                }
            },
        };
        let mut source_buf = source.buf.clone();
        let end = block_end(&source_buf.lines, index);
        let block: Vec<String> = source_buf.lines.drain(index..end).collect();
        let blank = |i: usize, lines: &[String]| lines.get(i).is_some_and(|l| l.trim().is_empty());
        if index > 0 && blank(index - 1, &source_buf.lines) && blank(index, &source_buf.lines) {
            source_buf.lines.remove(index);
        }
        let indent = block[0].len() - block[0].trim_start_matches([' ', '\t']).len();
        let block: Vec<String> = block
            .iter()
            .map(|l| l.get(indent..).unwrap_or(l).to_string())
            .collect();
        if source.path == target.path {
            insert_in_section(&mut source_buf, section, &block);
            return Ok(vec![change(source, &source_buf)]);
        }
        let mut target_buf = target.buf.clone();
        insert_in_section(&mut target_buf, section, &block);
        Ok(vec![
            change(target, &target_buf),
            change(source, &source_buf),
        ])
    }

    /// A new project file, optionally linked to a code folder.
    pub fn plan_new_project(
        &self,
        title: &str,
        root: Option<&Path>,
        today: Date,
    ) -> Result<Vec<Change>> {
        let title = one_line(title)?;
        let path = format!("projects/{}.md", slug(&title));
        self.ensure_free(&path)?;
        let mut text = String::from("---\n");
        if let Some(root) = root {
            text.push_str(&format!("root: {}\n", contract_home(root)));
        }
        text.push_str(&format!(
            "status: active\ncreated: {today}\n---\n# {title}\n\n## Inbox\n\n## Next actions\n"
        ));
        Ok(vec![Change {
            path,
            before: None,
            after: text,
        }])
    }

    /// Set or remove one frontmatter field.
    pub fn plan_field(&self, path: &str, key: &str, value: Option<&str>) -> Result<Vec<Change>> {
        let doc = self
            .doc(path)
            .ok_or_else(|| Error::OutsideVault(path.to_string()))?;
        if doc.locked {
            return Err(Error::Invalid(format!("{path} is locked")));
        }
        let mut buf = doc.buf.clone();
        set_field(&mut buf, key, value);
        Ok(vec![change(doc, &buf)])
    }

    /// Point a project at a code folder.
    pub fn plan_link(&self, project: &str, root: &Path) -> Result<Vec<Change>> {
        let path = self.project_doc(project)?.path.clone();
        self.plan_field(&path, "root", Some(&contract_home(root)))
    }

    pub fn plan_new_note(
        &self,
        title: &str,
        project: Option<&str>,
        today: Date,
    ) -> Result<Vec<Change>> {
        let title = one_line(title)?;
        let path = format!("notes/{}.md", slug(&title));
        self.ensure_free(&path)?;
        if let Some(name) = project {
            self.project_doc(name)?;
        }
        let mut text = String::from("---\n");
        if let Some(name) = project {
            text.push_str(&format!("project: {name}\n"));
        }
        text.push_str(&format!("created: {today}\n---\n# {title}\n\n"));
        Ok(vec![Change {
            path,
            before: None,
            after: text,
        }])
    }

    /// The journal page for `date`, from `templates/daily.md`. Empty when the
    /// page already exists.
    pub fn plan_daily(&self, date: Date) -> Result<Vec<Change>> {
        let path = daily_path(date);
        if self.doc(&path).is_some() || self.doc(&format!("{path}.age")).is_some() {
            return Ok(Vec::new());
        }
        let template = self
            .doc("templates/daily.md")
            .map_or(DEFAULT_DAILY_TEMPLATE, |d| d.text.as_str());
        let text = template
            .replace("{{date}}", &date.to_string())
            .replace("{{title}}", &long_date(date))
            .replace("{{weekday}}", weekday(date));
        Ok(vec![Change {
            path,
            before: None,
            after: text,
        }])
    }

    fn project_doc(&self, name: &str) -> Result<&Doc> {
        let project = self
            .project(name)
            .ok_or_else(|| Error::NoProject(name.to_string()))?;
        if project.doc.locked {
            return Err(Error::Invalid(format!("{name} is locked")));
        }
        Ok(project.doc)
    }

    fn ensure_free(&self, path: &str) -> Result<()> {
        if self.doc(path).is_some() || self.abs(path).exists() {
            return Err(Error::Exists(path.to_string()));
        }
        debug_assert!(classify(path).is_some());
        Ok(())
    }
}

pub fn daily_path(date: Date) -> String {
    format!("daily/{date}.md")
}

/// `Thursday 24 September`.
pub fn long_date(date: Date) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month = MONTHS
        .get(usize::try_from(date.month() - 1).unwrap_or(0))
        .copied()
        .unwrap_or("");
    format!("{} {} {}", weekday(date), date.day(), month)
}

fn weekday(date: Date) -> &'static str {
    use jiff::civil::Weekday::*;
    match date.weekday() {
        Monday => "Monday",
        Tuesday => "Tuesday",
        Wednesday => "Wednesday",
        Thursday => "Thursday",
        Friday => "Friday",
        Saturday => "Saturday",
        Sunday => "Sunday",
    }
}

fn change(doc: &Doc, buf: &TextBuf) -> Change {
    Change {
        path: doc.path.clone(),
        before: Some(doc.text.clone()),
        after: buf.render(),
    }
}

fn one_line(text: &str) -> Result<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        Err(Error::Invalid("the text is empty".to_string()))
    } else {
        Ok(text)
    }
}

/// The task line with a new state, `@done` added or removed to match.
pub fn with_state(line: &str, state: State, today: Date) -> Option<String> {
    let item = task_line(line)?;
    let mut out = String::with_capacity(line.len() + 20);
    out.push_str(&line[..item.mark_at]);
    out.push(state.mark());
    out.push_str(&line[item.mark_at + 1..]);
    let has_done = out.split_whitespace().any(|w| is_meta(w, "done"));
    Some(match (state, has_done) {
        (State::Done, false) => format!("{} @done({today})", out.trim_end()),
        (State::Done, true) => out,
        (_, _) => remove_meta(&out, "done"),
    })
}

fn is_meta(word: &str, key: &str) -> bool {
    word.strip_prefix('@')
        .and_then(|w| w.strip_prefix(key))
        .and_then(|w| w.strip_prefix('('))
        .is_some_and(|w| w.ends_with(')'))
}

/// Removes every `@key(...)` word and the space before it.
fn remove_meta(line: &str, key: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while !rest.is_empty() {
        let start = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        let (space, word_and_rest) = rest.split_at(start);
        let end = word_and_rest
            .find(char::is_whitespace)
            .unwrap_or(word_and_rest.len());
        let (word, after) = word_and_rest.split_at(end);
        if !is_meta(word, key) {
            out.push_str(space);
            out.push_str(word);
        }
        rest = after;
    }
    out
}

/// The line after the last non-blank line in `lines[from..to]`.
fn last_content_line(lines: &[String], from: usize, to: usize) -> Option<usize> {
    (from..to).rev().find(|&i| !lines[i].trim().is_empty())
}

/// Where a task's block ends: the task line plus following lines indented
/// deeper than it.
fn block_end(lines: &[String], index: usize) -> usize {
    let indent = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
    let base = indent(&lines[index]);
    let mut end = index + 1;
    while end < lines.len() && !lines[end].trim().is_empty() && indent(&lines[end]) > base {
        end += 1;
    }
    end
}

/// Appends lines to the end of a section, creating the section when missing:
/// Inbox goes before the first `##` heading, Next actions after everything.
fn insert_in_section(buf: &mut TextBuf, section: Section, lines: &[String]) {
    let parsed: Parsed = parse(buf);
    let name = section.heading();
    let found = parsed
        .headings
        .iter()
        .enumerate()
        .find(|(_, h)| h.level >= 2 && h.text.eq_ignore_ascii_case(name));
    match found {
        Some((i, heading)) => {
            let end = parsed.headings[i + 1..]
                .iter()
                .find(|h| h.level <= heading.level)
                .map_or(buf.lines.len(), |h| h.line);
            let at = match last_content_line(&buf.lines, heading.line + 1, end) {
                Some(last) => last + 1,
                None if buf
                    .lines
                    .get(heading.line + 1)
                    .is_some_and(|l| l.trim().is_empty()) =>
                {
                    heading.line + 2
                }
                None => heading.line + 1,
            };
            let at = at.min(buf.lines.len());
            buf.lines.splice(at..at, lines.iter().cloned());
            let after = at + lines.len();
            let before_heading = heading.line + 1 == at;
            if before_heading {
                buf.lines.insert(at, String::new());
            }
            let after = if before_heading { after + 1 } else { after };
            if after < buf.lines.len() && !buf.lines[after].trim().is_empty() {
                buf.lines.insert(after, String::new());
            }
        }
        None => {
            let first_h2 = parsed
                .headings
                .iter()
                .find(|h| h.level >= 2)
                .map(|h| h.line);
            let mut block = vec![format!("## {name}"), String::new()];
            block.extend(lines.iter().cloned());
            match (section, first_h2) {
                (Section::Inbox, Some(at)) => {
                    block.push(String::new());
                    buf.lines.splice(at..at, block);
                }
                _ => {
                    if buf.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                        buf.lines.push(String::new());
                    }
                    buf.lines.extend(block);
                }
            }
        }
    }
    buf.trailing = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    const TODAY: Date = date(2026, 9, 24);

    #[test]
    fn done_adds_the_date_and_reopening_removes_it() {
        let line = "- [ ] Ship it #web @due(2026-09-30)";
        let done = with_state(line, State::Done, TODAY).unwrap();
        assert_eq!(
            done,
            "- [x] Ship it #web @due(2026-09-30) @done(2026-09-24)"
        );
        assert_eq!(with_state(&done, State::Done, TODAY).unwrap(), done);
        assert_eq!(with_state(&done, State::Open, TODAY).unwrap(), line);
        assert_eq!(
            with_state(&done, State::Dropped, TODAY).unwrap(),
            "- [-] Ship it #web @due(2026-09-30)"
        );
    }

    #[test]
    fn state_changes_keep_indentation_and_bullets() {
        assert_eq!(
            with_state("   * [ ] nested", State::Doing, TODAY).unwrap(),
            "   * [/] nested"
        );
    }

    fn insert(text: &str, section: Section, line: &str) -> String {
        let mut buf = TextBuf::parse(text);
        insert_in_section(&mut buf, section, &[line.to_string()]);
        buf.render()
    }

    #[test]
    fn appends_after_the_last_item_of_a_section() {
        let text = "# P\n\n## Inbox\n\n- [ ] a\n\n## Next actions\n\n- [ ] b\n";
        assert_eq!(
            insert(text, Section::Inbox, "- [ ] new"),
            "# P\n\n## Inbox\n\n- [ ] a\n- [ ] new\n\n## Next actions\n\n- [ ] b\n"
        );
        assert_eq!(
            insert(text, Section::NextActions, "- [ ] new"),
            "# P\n\n## Inbox\n\n- [ ] a\n\n## Next actions\n\n- [ ] b\n- [ ] new\n"
        );
    }

    #[test]
    fn fills_an_empty_section() {
        assert_eq!(
            insert(
                "# P\n\n## Inbox\n\n## Next actions\n",
                Section::Inbox,
                "- [ ] a"
            ),
            "# P\n\n## Inbox\n\n- [ ] a\n\n## Next actions\n"
        );
        assert_eq!(
            insert(
                "# P\n\n## Inbox\n## Next actions\n",
                Section::Inbox,
                "- [ ] a"
            ),
            "# P\n\n## Inbox\n\n- [ ] a\n\n## Next actions\n"
        );
    }

    #[test]
    fn creates_a_missing_section_in_its_place() {
        assert_eq!(
            insert(
                "# P\n\n## Next actions\n\n- [ ] b\n",
                Section::Inbox,
                "- [ ] a"
            ),
            "# P\n\n## Inbox\n\n- [ ] a\n\n## Next actions\n\n- [ ] b\n"
        );
        assert_eq!(
            insert("# P\n\nSome prose.\n", Section::NextActions, "- [ ] a"),
            "# P\n\nSome prose.\n\n## Next actions\n\n- [ ] a\n"
        );
    }

    #[test]
    fn remove_meta_leaves_other_words_alone() {
        assert_eq!(remove_meta("a @done(x) b", "done"), "a b");
        assert_eq!(remove_meta("a @donex b", "done"), "a @donex b");
        assert_eq!(remove_meta("a @done(2026-01-01)", "done"), "a");
    }

    #[test]
    fn long_dates_read_naturally() {
        assert_eq!(long_date(TODAY), "Thursday 24 September");
    }
}
