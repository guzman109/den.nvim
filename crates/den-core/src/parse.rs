//! Reading one Markdown file: its fields, title, headings, tasks and tags.
//!
//! A task is a list item with a checkbox:
//!
//! ```text
//! - [ ] open        - [/] doing        - [-] dropped        - [x] done
//! ```
//!
//! Its text may carry `#tags`, `@due(YYYY-MM-DD)` and `@done(YYYY-MM-DD)`.
//! Nothing inside a fenced code block is parsed, and fields count only in the
//! frontmatter at the top.

use jiff::civil::Date;
use serde::Serialize;

use crate::frontmatter::Frontmatter;
use crate::text::TextBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Open,
    Doing,
    Dropped,
    Done,
}

impl State {
    /// The character between the brackets.
    pub fn mark(self) -> char {
        match self {
            State::Open => ' ',
            State::Doing => '/',
            State::Dropped => '-',
            State::Done => 'x',
        }
    }

    pub fn from_mark(c: char) -> Option<State> {
        match c {
            ' ' => Some(State::Open),
            '/' => Some(State::Doing),
            '-' => Some(State::Dropped),
            'x' | 'X' => Some(State::Done),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Doing => "doing",
            State::Dropped => "dropped",
            State::Done => "done",
        }
    }

    pub fn parse(name: &str) -> Option<State> {
        [State::Open, State::Doing, State::Dropped, State::Done]
            .into_iter()
            .find(|s| s.as_str() == name)
    }

    /// Done or dropped: no longer work to do.
    pub fn is_closed(self) -> bool {
        matches!(self, State::Done | State::Dropped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    /// 0-based line index in the file.
    pub line: usize,
    /// Leading whitespace, in bytes.
    pub indent: usize,
    pub state: State,
    /// Everything after the checkbox, as written.
    pub text: String,
    /// The text without `@due`/`@done` and without the trailing run of tags.
    pub title: String,
    pub tags: Vec<String>,
    pub due: Option<Date>,
    pub done: Option<Date>,
    /// The nearest heading above the task (level 2 or deeper).
    pub section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

/// Something in a file Den could not read the way the author probably meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub frontmatter: Frontmatter,
    /// The first `# ` heading.
    pub title: Option<String>,
    pub headings: Vec<Heading>,
    pub tasks: Vec<Task>,
    /// Tags anywhere in the body and in a `tags:` field, in order of appearance.
    pub tags: Vec<String>,
    pub issues: Vec<Issue>,
}

pub fn parse(buf: &TextBuf) -> Parsed {
    let lines = &buf.lines;
    let frontmatter = Frontmatter::parse(lines);
    let mut out = Parsed::default();
    if let Some(e) = &frontmatter.error {
        out.issues.push(Issue {
            line: 0,
            message: format!("frontmatter: {e}"),
        });
    }
    let start = frontmatter.range.map_or(0, |(_, close)| close + 1);
    let mut fence: Option<(char, usize)> = None;
    let mut section: Option<String> = None;

    for (i, line) in lines.iter().enumerate().skip(start) {
        if let Some((c, n, bare)) = fence_marker(line) {
            match fence {
                None => fence = Some((c, n)),
                Some((open, len)) if c == open && n >= len && bare => fence = None,
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some((level, text)) = heading(line) {
            if level == 1 && out.title.is_none() {
                out.title = Some(text.clone());
            }
            if level >= 2 {
                section = Some(text.clone());
            }
            out.headings.push(Heading {
                level,
                text,
                line: i,
            });
            continue;
        }
        if let Some(item) = task_line(line) {
            let tokens = tokens(&item.text);
            for message in tokens.problems {
                out.issues.push(Issue { line: i, message });
            }
            out.tasks.push(Task {
                line: i,
                indent: item.indent,
                state: item.state,
                title: tokens.title,
                tags: tokens.tags,
                due: tokens.due,
                done: tokens.done,
                text: item.text,
                section: section.clone(),
            });
        }
        for tag in line_tags(line) {
            push_unique(&mut out.tags, tag);
        }
    }
    if let Some((c, _)) = fence {
        out.issues.push(Issue {
            line: lines.len().saturating_sub(1),
            message: format!("a code block opened with {c}{c}{c} is never closed"),
        });
    }
    for tag in frontmatter.list("tags") {
        push_unique(&mut out.tags, tag.trim_start_matches('#').to_string());
    }
    out.frontmatter = frontmatter;
    out
}

/// One line read as a task on its own, outside any file (line 0, no section).
pub fn task(line: &str) -> Option<Task> {
    let item = task_line(line)?;
    let tokens = tokens(&item.text);
    Some(Task {
        line: 0,
        indent: item.indent,
        state: item.state,
        title: tokens.title,
        tags: tokens.tags,
        due: tokens.due,
        done: tokens.done,
        text: item.text,
        section: None,
    })
}

/// A list item with a checkbox, split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLine {
    pub indent: usize,
    /// Byte index of the character between the brackets.
    pub mark_at: usize,
    pub state: State,
    pub text: String,
}

pub fn task_line(line: &str) -> Option<TaskLine> {
    let rest = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - rest.len();
    let mut chars = rest.chars();
    if !matches!(chars.next()?, '-' | '*' | '+') || chars.next()? != ' ' || chars.next()? != '[' {
        return None;
    }
    let state = State::from_mark(chars.next()?)?;
    if chars.next()? != ']' {
        return None;
    }
    let after = chars.as_str();
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let text = after.trim();
    if text.is_empty() {
        return None;
    }
    Some(TaskLine {
        indent,
        mark_at: indent + 3,
        state,
        text: text.to_string(),
    })
}

#[derive(Debug, Default)]
struct Tokens {
    title: String,
    tags: Vec<String>,
    due: Option<Date>,
    done: Option<Date>,
    problems: Vec<String>,
}

enum Word<'a> {
    Plain(&'a str),
    Tag(&'a str),
    Date,
}

fn tokens(text: &str) -> Tokens {
    let mut out = Tokens::default();
    let mut words = Vec::new();
    let mut in_code = false;
    for word in text.split_whitespace() {
        let ticks = word.matches('`').count();
        let classified = if in_code || ticks > 0 {
            Word::Plain(word)
        } else if let Some(value) = meta(word, "due") {
            match value.parse::<Date>() {
                Ok(date) => {
                    out.due = Some(date);
                    Word::Date
                }
                Err(_) => {
                    out.problems.push(format!("@due({value}) is not a date"));
                    Word::Plain(word)
                }
            }
        } else if let Some(value) = meta(word, "done") {
            match value.parse::<Date>() {
                Ok(date) => {
                    out.done = Some(date);
                    Word::Date
                }
                Err(_) => {
                    out.problems.push(format!("@done({value}) is not a date"));
                    Word::Plain(word)
                }
            }
        } else if let Some(tag) = tag_of(word) {
            push_unique(&mut out.tags, tag);
            Word::Tag(word)
        } else {
            Word::Plain(word)
        };
        if ticks % 2 == 1 {
            in_code = !in_code;
        }
        words.push(classified);
    }
    let kept: Vec<&Word> = words.iter().filter(|w| !matches!(w, Word::Date)).collect();
    let end = kept
        .iter()
        .rposition(|w| !matches!(w, Word::Tag(_)))
        .map_or(0, |i| i + 1);
    let title: Vec<&str> = kept[..end]
        .iter()
        .map(|w| match w {
            Word::Plain(s) | Word::Tag(s) => *s,
            Word::Date => "",
        })
        .collect();
    out.title = if title.is_empty() {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        title.join(" ")
    };
    out
}

/// `@key(value)` → `value`.
fn meta<'a>(word: &'a str, key: &str) -> Option<&'a str> {
    word.strip_prefix('@')?
        .strip_prefix(key)?
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// `#tag` → `tag`. A tag starts with a letter, so `#1` is not one, and
/// trailing punctuation is not part of it.
pub fn tag_of(word: &str) -> Option<String> {
    let rest = word.strip_prefix('#')?;
    let rest = rest.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']);
    let first = rest.chars().next()?;
    if !first.is_alphabetic() {
        return None;
    }
    if !rest
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/'))
    {
        return None;
    }
    Some(rest.to_string())
}

fn line_tags(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_code = false;
    for word in line.split_whitespace() {
        let ticks = word.matches('`').count();
        if !in_code
            && ticks == 0
            && let Some(tag) = tag_of(word)
        {
            out.push(tag);
        }
        if ticks % 2 == 1 {
            in_code = !in_code;
        }
    }
    out
}

/// A fence line: its character, run length, and whether nothing follows.
fn fence_marker(line: &str) -> Option<(char, usize, bool)> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let c = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let run = rest.chars().take_while(|x| *x == c).count();
    if run < 3 {
        return None;
    }
    let after = &rest[run..];
    Some((c, run, after.trim().is_empty()))
}

/// An ATX heading: `## Text` → `(2, "Text")`.
fn heading(line: &str) -> Option<(u8, String)> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let level = rest.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &rest[level..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let mut text = after.trim();
    let without_closing = text.trim_end_matches('#');
    if without_closing.len() != text.len()
        && (without_closing.is_empty() || without_closing.ends_with([' ', '\t']))
    {
        text = without_closing.trim_end();
    }
    Some((level as u8, text.to_string()))
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn parsed(text: &str) -> Parsed {
        parse(&TextBuf::parse(text))
    }

    #[test]
    fn reads_the_four_states() {
        let p = parsed("- [ ] a\n- [/] b\n- [-] c\n- [x] d\n- [X] e\n");
        let states: Vec<State> = p.tasks.iter().map(|t| t.state).collect();
        assert_eq!(
            states,
            [
                State::Open,
                State::Doing,
                State::Dropped,
                State::Done,
                State::Done
            ]
        );
    }

    #[test]
    fn ignores_non_tasks() {
        let p = parsed("- [ ]\n- [?] odd\n-[ ] tight\n- [ ]tight\n1. [ ] numbered\n- plain\n");
        assert!(p.tasks.is_empty(), "{:?}", p.tasks);
    }

    #[test]
    fn splits_title_tags_and_dates() {
        let p = parsed("- [/] Draft the homepage story #writing #deep-work @due(2026-09-28)\n");
        let t = &p.tasks[0];
        assert_eq!(t.title, "Draft the homepage story");
        assert_eq!(t.tags, ["writing", "deep-work"]);
        assert_eq!(t.due, Some(date(2026, 9, 28)));
        assert_eq!(t.done, None);
    }

    #[test]
    fn keeps_tags_that_are_part_of_the_sentence() {
        let p = parsed("- [ ] Fix the #rust build tonight\n");
        assert_eq!(p.tasks[0].title, "Fix the #rust build tonight");
        assert_eq!(p.tasks[0].tags, ["rust"]);
    }

    #[test]
    fn a_bad_date_is_reported_and_left_visible() {
        let p = parsed("- [ ] Ship @due(2026-02-30)\n");
        assert_eq!(p.tasks[0].due, None);
        assert_eq!(p.tasks[0].title, "Ship @due(2026-02-30)");
        assert_eq!(p.issues.len(), 1);
    }

    #[test]
    fn nothing_inside_a_code_block_counts() {
        let p = parsed(
            "# T\n```md\n- [ ] not a task\n## Not a heading\n#nottag\n```\n- [ ] real #tag\n",
        );
        assert_eq!(p.tasks.len(), 1);
        assert_eq!(p.headings.len(), 1);
        assert_eq!(p.tags, ["tag"]);
    }

    #[test]
    fn a_longer_fence_needs_a_longer_close() {
        let p = parsed("````\n```\n- [ ] inside\n```\n````\n- [ ] outside\n");
        assert_eq!(p.tasks.len(), 1);
        assert_eq!(p.tasks[0].text, "outside");
    }

    #[test]
    fn numbers_and_urls_are_not_tags() {
        let p = parsed("See #1 and https://x.org/#anchor and `#code` but #real.\n");
        assert_eq!(p.tags, ["real"]);
    }

    #[test]
    fn title_and_sections() {
        let p = parsed(
            "---\nstatus: active\n---\n# Site\n\n## Inbox\n- [ ] a\n## Next actions ##\n- [ ] b\n",
        );
        assert_eq!(p.title.as_deref(), Some("Site"));
        assert_eq!(p.tasks[0].section.as_deref(), Some("Inbox"));
        assert_eq!(p.tasks[1].section.as_deref(), Some("Next actions"));
    }

    #[test]
    fn frontmatter_lines_are_not_body() {
        let p = parsed("---\ntags: [alpha, beta]\n---\n# T\n");
        assert_eq!(p.tags, ["alpha", "beta"]);
        assert!(p.headings.iter().all(|h| h.line == 3));
    }

    #[test]
    fn task_line_finds_the_mark() {
        let t = task_line("  * [x] done thing").unwrap();
        assert_eq!(t.indent, 2);
        assert_eq!(&"  * [x] done thing"[t.mark_at..t.mark_at + 1], "x");
    }
}
