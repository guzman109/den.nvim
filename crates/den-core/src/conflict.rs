//! Settling the conflicts a sync leaves behind.
//!
//! Two machines that edited the same lines leave git conflict markers in the
//! file. Sync asks git for the `diff3` style, so every conflict also carries
//! the lines as they were before either machine touched them, and Den can
//! tell *what* each side changed:
//!
//! ```text
//! <<<<<<< HEAD
//! - [x] Register the domain @done(2026-09-24)      one side
//! ||||||| base
//! - [ ] Register the domain                        before
//! =======
//! - [ ] Register the domain #admin                 the other side
//! >>>>>>> 1a2b3c4 (den: laptop, 1 change)
//! ```
//!
//! When every changed line is a task and the two sides changed different
//! things about it (one closed it, the other tagged it), the edits combine:
//! `- [x] Register the domain #admin @done(2026-09-24)`. Anything else, such
//! as both sides rewording the same sentence, is left for the person.
//!
//! During `git pull --rebase` the first side (`HEAD`) is what the other
//! machine pushed, and the second is this machine's own edit being replayed.

use std::collections::{HashMap, HashSet};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::parse::{self, State, Task};
use crate::text::TextBuf;

/// One conflict in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    /// 0-based line of `<<<<<<<`.
    pub start: usize,
    /// 0-based line of `>>>>>>>`.
    pub end: usize,
    pub ours: Vec<String>,
    /// The lines before either side changed them (diff3 style only).
    pub base: Option<Vec<String>>,
    pub theirs: Vec<String>,
    pub ours_label: String,
    pub theirs_label: String,
    /// Both sides' edits together, when they can be combined safely.
    pub combined: Option<Vec<String>>,
}

/// How to settle one hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Combine,
    Ours,
    Theirs,
    /// Ours, then theirs.
    Both,
    /// Leave the markers for editing by hand.
    Leave,
}

fn marker(line: &str, c: char) -> Option<&str> {
    let rest = line.strip_prefix(&c.to_string().repeat(7))?;
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix(' ')
    }
}

/// Every well-formed conflict in `text`, in order.
pub fn hunks(text: &str) -> Vec<Hunk> {
    hunks_in(&TextBuf::parse(text).lines)
}

fn hunks_in(lines: &[String]) -> Vec<Hunk> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(label) = marker(&lines[i], '<') else {
            i += 1;
            continue;
        };
        let start = i;
        let mut ours = Vec::new();
        let mut base: Option<Vec<String>> = None;
        let mut theirs = Vec::new();
        // 0 ours, 1 base, 2 theirs
        let mut part = 0;
        let mut end = None;
        let mut j = i + 1;
        while j < lines.len() {
            let line = &lines[j];
            if part == 0 && marker(line, '|').is_some() {
                part = 1;
                base = Some(Vec::new());
            } else if part < 2 && line == "=======" {
                part = 2;
            } else if part == 2
                && let Some(theirs_label) = marker(line, '>')
            {
                end = Some((j, theirs_label.to_string()));
                break;
            } else if marker(line, '<').is_some() {
                break;
            } else {
                match part {
                    0 => ours.push(line.clone()),
                    1 => base.get_or_insert_with(Vec::new).push(line.clone()),
                    _ => theirs.push(line.clone()),
                }
            }
            j += 1;
        }
        match end {
            Some((end, theirs_label)) => {
                let combined = combine(&ours, base.as_deref(), &theirs);
                out.push(Hunk {
                    start,
                    end,
                    ours,
                    base,
                    theirs,
                    ours_label: label.to_string(),
                    theirs_label,
                    combined,
                });
                i = end + 1;
            }
            None => i = j.max(i + 1),
        }
    }
    out
}

/// The file with each hunk settled by the matching choice. Missing choices
/// count as [`Choice::Leave`].
pub fn resolve(text: &str, choices: &[Choice]) -> Result<String> {
    let mut buf = TextBuf::parse(text);
    let found = hunks_in(&buf.lines);
    for (index, hunk) in found.iter().enumerate().rev() {
        let replacement = match choices.get(index).copied().unwrap_or(Choice::Leave) {
            Choice::Leave => continue,
            Choice::Ours => hunk.ours.clone(),
            Choice::Theirs => hunk.theirs.clone(),
            Choice::Both => hunk.ours.iter().chain(&hunk.theirs).cloned().collect(),
            Choice::Combine => hunk.combined.clone().ok_or_else(|| {
                Error::Invalid(format!(
                    "the conflict at line {} cannot be combined",
                    hunk.start + 1
                ))
            })?,
        };
        buf.lines.splice(hunk.start..=hunk.end, replacement);
    }
    Ok(buf.render())
}

/// The file with every hunk combined, or `None` when any of them needs the
/// person.
pub fn combine_all(text: &str) -> Option<String> {
    let found = hunks(text);
    if found.is_empty() || found.iter().any(|h| h.combined.is_none()) {
        return None;
    }
    resolve(text, &vec![Choice::Combine; found.len()]).ok()
}

/// A task line and what it says, keyed by where it sits and what it is called.
struct Entry<'a> {
    line: &'a str,
    task: Task,
}

type Key = (usize, String);

/// Task lines by key, or `None` when a side holds the same task twice (which
/// line matches which would be a guess).
fn entries(lines: &[String]) -> Option<HashMap<Key, Entry<'_>>> {
    let mut out = HashMap::new();
    for line in lines {
        if let Some(task) = parse::task(line) {
            let key = (task.indent, task.title.clone());
            if out.insert(key, Entry { line, task }).is_some() {
                return None;
            }
        }
    }
    Some(out)
}

/// Lines that are neither tasks nor blank, sorted, to compare sides.
fn prose(lines: &[String]) -> Vec<&str> {
    let mut out: Vec<&str> = lines
        .iter()
        .filter(|l| !l.trim().is_empty() && parse::task(l).is_none())
        .map(String::as_str)
        .collect();
    out.sort_unstable();
    out
}

fn key_of(line: &str) -> Option<Key> {
    parse::task(line).map(|t| (t.indent, t.title))
}

/// Combines two sides' edits to the same lines, when they do not overlap.
pub fn combine(ours: &[String], base: Option<&[String]>, theirs: &[String]) -> Option<Vec<String>> {
    if ours == theirs {
        return Some(ours.to_vec());
    }
    let base = base?;
    if ours == base {
        return Some(theirs.to_vec());
    }
    if theirs == base {
        return Some(ours.to_vec());
    }
    // Sentences, headings and anything else that is not a task must be the
    // same on all three sides; only task changes are combined.
    let p = prose(base);
    if prose(ours) != p || prose(theirs) != p {
        return None;
    }
    let (b, o, t) = (entries(base)?, entries(ours)?, entries(theirs)?);

    let mut merged: HashMap<Key, Option<String>> = HashMap::new();
    let keys: HashSet<&Key> = b.keys().chain(o.keys()).chain(t.keys()).collect();
    for key in keys {
        let result = match (b.get(key), o.get(key), t.get(key)) {
            (_, Some(x), Some(y)) => Some(merge_task(b.get(key), x, y)?),
            // Removed on one side: fine if the other left it alone.
            (Some(was), Some(kept), None) | (Some(was), None, Some(kept)) => {
                if kept.line != was.line {
                    return None;
                }
                None
            }
            (Some(_), None, None) => None,
            (None, Some(added), None) | (None, None, Some(added)) => Some(added.line.to_string()),
            (None, None, None) => None,
        };
        merged.insert(key.clone(), result);
    }

    // Ours gives the order; lines only theirs added follow the line they
    // follow on their side.
    let mut out: Vec<String> = Vec::new();
    let mut placed: HashMap<Key, usize> = HashMap::new();
    for line in ours {
        match key_of(line) {
            Some(key) => {
                if let Some(Some(text)) = merged.get(&key) {
                    placed.insert(key, out.len());
                    out.push(text.clone());
                }
            }
            None => out.push(line.clone()),
        }
    }
    let mut after: Option<Key> = None;
    for line in theirs {
        let Some(key) = key_of(line) else { continue };
        if !o.contains_key(&key) && !b.contains_key(&key) {
            let at = after
                .as_ref()
                .and_then(|k| placed.get(k))
                .map_or(0, |i| i + 1);
            if let Some(Some(text)) = merged.get(&key) {
                out.insert(at, text.clone());
                for index in placed.values_mut() {
                    if *index >= at {
                        *index += 1;
                    }
                }
                placed.insert(key.clone(), at);
            }
        }
        if placed.contains_key(&key) {
            after = Some(key);
        }
    }
    Some(out)
}

/// Which state wins when both sides changed it: finishing beats dropping
/// beats starting.
fn rank(state: State) -> u8 {
    match state {
        State::Open => 0,
        State::Doing => 1,
        State::Dropped => 2,
        State::Done => 3,
    }
}

/// One field, from whichever side changed it. `None` when both changed it
/// differently.
fn pick<T: PartialEq + Clone>(base: Option<&T>, ours: &T, theirs: &T) -> Option<T> {
    if ours == theirs {
        Some(ours.clone())
    } else if base == Some(ours) {
        Some(theirs.clone())
    } else if base == Some(theirs) {
        Some(ours.clone())
    } else {
        None
    }
}

fn merge_task(base: Option<&Entry>, ours: &Entry, theirs: &Entry) -> Option<String> {
    if ours.line == theirs.line {
        return Some(ours.line.to_string());
    }
    if let Some(b) = base {
        if b.line == ours.line {
            return Some(theirs.line.to_string());
        }
        if b.line == theirs.line {
            return Some(ours.line.to_string());
        }
    }
    let (o, t) = (&ours.task, &theirs.task);
    let b = base.map(|e| &e.task);

    let state = pick(b.map(|b| &b.state), &o.state, &t.state).unwrap_or(
        if rank(o.state) >= rank(t.state) {
            o.state
        } else {
            t.state
        },
    );
    let due: Option<Date> = pick(b.map(|b| &b.due), &o.due, &t.due)?;
    let done = if state == State::Done {
        match (o.state == State::Done, t.state == State::Done) {
            (true, _) => o.done.or(t.done),
            (false, _) => t.done.or(o.done),
        }
    } else {
        None
    };

    // A tag stays unless one side removed it; a tag either side added joins.
    let base_tags: Vec<&String> = b.map(|b| b.tags.iter().collect()).unwrap_or_default();
    let mut tags: Vec<String> = Vec::new();
    for tag in o.tags.iter().chain(&t.tags) {
        let in_o = o.tags.contains(tag);
        let in_t = t.tags.contains(tag);
        let in_b = base_tags.contains(&tag);
        let keep = (in_o && in_t) || !in_b;
        if keep && !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }

    let merged = Task {
        state,
        due,
        done,
        tags,
        ..o.clone()
    };
    for side in [ours, theirs] {
        let s = &side.task;
        if s.state == merged.state
            && s.due == merged.due
            && s.done == merged.done
            && s.tags == merged.tags
        {
            return Some(side.line.to_string());
        }
    }
    Some(render(ours.line, &merged))
}

/// A task line rebuilt from its parts, keeping the original indent and bullet.
fn render(original: &str, task: &Task) -> String {
    let bullet = original
        .trim_start_matches([' ', '\t'])
        .chars()
        .next()
        .unwrap_or('-');
    let mut line = format!(
        "{}{bullet} [{}] {}",
        &original[..task.indent],
        task.state.mark(),
        task.title
    );
    let in_title: Vec<String> = task
        .title
        .split_whitespace()
        .filter_map(parse::tag_of)
        .collect();
    for tag in &task.tags {
        if !in_title.contains(tag) {
            line.push_str(&format!(" #{tag}"));
        }
    }
    if let Some(due) = task.due {
        line.push_str(&format!(" @due({due})"));
    }
    if let Some(done) = task.done {
        line.push_str(&format!(" @done({done})"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    fn conflicted(ours: &str, base: &str, theirs: &str) -> String {
        format!(
            "# Project\n\n## Next actions\n\n<<<<<<< HEAD\n{ours}||||||| base\n{base}=======\n{theirs}>>>>>>> abc1234 (den: laptop, 1 change)\n- [ ] After\n"
        )
    }

    #[test]
    fn reads_diff3_hunks_with_labels() {
        let text = conflicted("- [x] A\n", "- [ ] A\n", "- [ ] A #x\n");
        let found = hunks(&text);
        assert_eq!(found.len(), 1);
        let h = &found[0];
        assert_eq!((h.start, h.end), (4, 10));
        assert_eq!(h.ours, lines("- [x] A"));
        assert_eq!(h.base, Some(lines("- [ ] A")));
        assert_eq!(h.theirs, lines("- [ ] A #x"));
        assert_eq!(h.ours_label, "HEAD");
        assert_eq!(h.theirs_label, "abc1234 (den: laptop, 1 change)");
    }

    #[test]
    fn done_on_one_machine_and_tagged_on_the_other_keeps_both() {
        let text = conflicted(
            "- [x] Register the domain @done(2026-09-24)\n",
            "- [ ] Register the domain\n",
            "- [ ] Register the domain #admin\n",
        );
        let out = combine_all(&text).unwrap();
        assert!(
            out.contains("\n- [x] Register the domain #admin @done(2026-09-24)\n- [ ] After\n"),
            "{out}"
        );
        assert!(!out.contains("<<<<<<<"));
    }

    #[test]
    fn additions_from_both_sides_are_kept_in_place() {
        let text = conflicted(
            "- [ ] A\n- [ ] From the other machine\n- [ ] B\n",
            "- [ ] A\n- [ ] B\n",
            "- [ ] A\n- [ ] B\n- [ ] From this machine\n",
        );
        let out = combine_all(&text).unwrap();
        assert!(
            out.contains(
                "- [ ] A\n- [ ] From the other machine\n- [ ] B\n- [ ] From this machine\n- [ ] After"
            ),
            "{out}"
        );
    }

    #[test]
    fn a_removed_task_stays_removed_unless_the_other_side_changed_it() {
        let clean = conflicted("- [ ] B\n", "- [ ] A\n- [ ] B\n", "- [ ] A\n- [x] B\n");
        let out = combine_all(&clean).unwrap();
        assert!(out.contains("\n- [x] B\n- [ ] After"), "{out}");
        assert!(!out.contains("- [ ] A\n"), "{out}");

        let clash = conflicted("- [ ] B\n", "- [ ] A\n- [ ] B\n", "- [x] A\n- [ ] B #y\n");
        assert!(combine_all(&clash).is_none());
    }

    #[test]
    fn two_different_due_dates_need_the_person() {
        let text = conflicted(
            "- [ ] A @due(2026-10-01)\n",
            "- [ ] A\n",
            "- [ ] A @due(2026-10-02)\n",
        );
        assert!(combine_all(&text).is_none());
    }

    #[test]
    fn finishing_wins_over_dropping() {
        let text = conflicted("- [-] A\n", "- [ ] A\n", "- [x] A @done(2026-09-24)\n");
        let out = combine_all(&text).unwrap();
        assert!(out.contains("\n- [x] A @done(2026-09-24)\n"), "{out}");
    }

    #[test]
    fn a_removed_tag_stays_removed() {
        let text = conflicted(
            "- [x] A #a @done(2026-09-24)\n",
            "- [ ] A #a #b\n",
            "- [ ] A #b\n",
        );
        let out = combine_all(&text).unwrap();
        assert!(out.contains("\n- [x] A @done(2026-09-24)\n"), "{out}");
    }

    #[test]
    fn prose_edited_on_both_sides_is_never_combined() {
        let text = conflicted(
            "The plan is red.\n",
            "The plan is blue.\n",
            "The plan is green.\n",
        );
        let h = &hunks(&text)[0];
        assert!(h.combined.is_none());
        let settled = resolve(&text, &[Choice::Theirs]).unwrap();
        assert!(
            settled.contains("\nThe plan is green.\n- [ ] After\n"),
            "{settled}"
        );
        let both = resolve(&text, &[Choice::Both]).unwrap();
        assert!(
            both.contains("The plan is red.\nThe plan is green.\n"),
            "{both}"
        );
    }

    #[test]
    fn without_a_base_only_identical_sides_combine() {
        let text = "<<<<<<< HEAD\n- [x] A\n=======\n- [ ] A #x\n>>>>>>> abc\n";
        assert!(hunks(text)[0].combined.is_none());
        let same = "<<<<<<< HEAD\n- [x] A\n=======\n- [x] A\n>>>>>>> abc\n";
        assert_eq!(combine_all(same).unwrap(), "- [x] A\n");
    }

    #[test]
    fn several_hunks_and_windows_line_endings_survive() {
        let one = conflicted("- [x] A\n", "- [ ] A\n", "- [ ] A\n").replace('\n', "\r\n");
        let text = format!("{one}{one}");
        assert_eq!(hunks(&text).len(), 2);
        let out = resolve(&text, &[Choice::Ours, Choice::Leave]).unwrap();
        assert_eq!(hunks(&out).len(), 1);
        assert!(out.contains("\r\n- [x] A\r\n- [ ] After\r\n"), "{out:?}");
    }

    #[test]
    fn an_unfinished_marker_is_not_a_hunk() {
        assert!(hunks("<<<<<<< HEAD\n- [ ] A\n=======\n").is_empty());
    }
}
