//! Changing a task, and getting it onto disk without ever losing a byte.
//!
//! Two separate concerns live here, and both are compatibility contracts.
//!
//! [`change_line`] is a port of `den.nvim/lua/den/task.lua::change`. Its output
//! must stay **byte-identical** to the Lua, because both interfaces edit the
//! same files and a user will have notes written by either. It is the reason
//! `tests/mutation_parity.rs` exists.
//!
//! [`apply`] is the file write. The engine now owns the file — Den no longer
//! routes edits through a running editor — so every failure mode that used to
//! be Neovim's problem is ours: a half-written file, a write that lands on top
//! of someone else's edit, or a reformat that quietly rewrites lines the user
//! did not touch. The rules are: only the target line may differ, the write is
//! atomic, and a file that moved underneath us is refused rather than
//! overwritten.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::vault::parse::{self, MAX_ORDER, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The line is not a Markdown task.
    NotATask,
    /// Duplicate or malformed `@id`/`@status`/`@order` on the line. den.nvim
    /// refuses these too: rewriting them would silently pick a winner.
    InvalidMetadata,
    InvalidOrder(u64),
    InvalidId(String),
    /// The file changed since it was read. The caller should reload and retry
    /// rather than clobber whatever arrived in the meantime.
    Stale {
        path: PathBuf,
    },
    /// An editor is holding unsaved changes for this file.
    ///
    /// Routing writes through Neovim used to make this impossible; now that the
    /// engine writes directly, it has to be checked explicitly.
    Dirty {
        path: PathBuf,
    },
    NoSuchLine {
        path: PathBuf,
        line: usize,
    },
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotATask => f.write_str("Den: not a Markdown task"),
            Error::InvalidMetadata => {
                f.write_str("Den: repair duplicate or invalid task metadata first")
            }
            Error::InvalidOrder(order) => write!(f, "Den: invalid task order {order}"),
            Error::InvalidId(id) => write!(f, "Den: invalid task ID {id:?}"),
            Error::Stale { path } => {
                write!(f, "Den: {} changed; refresh and try again", path.display())
            }
            Error::Dirty { path } => write!(
                f,
                "Den: {} has unsaved changes in your editor; save or discard them first",
                path.display()
            ),
            Error::NoSuchLine { path, line } => {
                write!(f, "Den: {}:{line} no longer exists", path.display())
            }
            Error::Io(message) => write!(f, "Den: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// Rewrites one task line, exactly as `task.lua::change` does.
///
/// The checkbox mark follows the status, then `@status`, `@order` and `@id` are
/// set in that order — replaced in place when already present, appended at the
/// end when not. Every other byte of the line, including unknown tokens like
/// `@tag(...)` and the user's own spacing, is preserved untouched.
pub fn change_line(
    line: &str,
    status: Status,
    order: Option<u64>,
    id: Option<&str>,
) -> Result<String, Error> {
    let task = parse::parse_task(line).ok_or(Error::NotATask)?;
    if task.invalid_metadata {
        return Err(Error::InvalidMetadata);
    }

    let mut rewritten = set_mark(line, status);
    rewritten = set_metadata(&rewritten, "status", status.as_str());

    if let Some(order) = order {
        if order > MAX_ORDER {
            return Err(Error::InvalidOrder(order));
        }
        rewritten = set_metadata(&rewritten, "order", &order.to_string());
    }

    // den.nvim only ever *adds* an id; it never rewrites one the user set.
    if task.id.is_none()
        && let Some(id) = id
    {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(Error::InvalidId(id.to_string()));
        }
        rewritten = set_metadata(&rewritten, "id", id);
    }

    Ok(rewritten)
}

/// `^(%s*[-*+] %[)[ xX](%])` — flip the checkbox, touching nothing else.
fn set_mark(line: &str, status: Status) -> String {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - trimmed.len();
    let mut chars = trimmed.char_indices();

    let Some((_, bullet)) = chars.next() else {
        return line.to_string();
    };
    if !matches!(bullet, '-' | '*' | '+') {
        return line.to_string();
    }
    // bullet, space, '[', mark, ']'
    let bytes = trimmed.as_bytes();
    if bytes.len() < 5 || bytes[1] != b' ' || bytes[2] != b'[' || bytes[4] != b']' {
        return line.to_string();
    }
    if !matches!(bytes[3], b' ' | b'x' | b'X') {
        return line.to_string();
    }

    let mark = if status == Status::Done { 'x' } else { ' ' };
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..indent]);
    out.push_str(&trimmed[..3]);
    out.push(mark);
    out.push_str(&trimmed[4..]);
    out
}

/// den.nvim's `metadata()`: replace `@key(...)` in place when it is preceded by
/// whitespace, otherwise append ` @key(value)`.
///
/// The leading-whitespace requirement is load-bearing — it is why `word@id(x)`
/// is left alone and a fresh token is appended instead, matching the parser,
/// which does not treat an embedded token as metadata either.
fn set_metadata(line: &str, key: &str, value: &str) -> String {
    let needle = format!("@{key}(");
    let mut at = 0;

    while let Some(offset) = line[at..].find(&needle) {
        let start = at + offset;
        let preceded_by_space =
            start > 0 && line[..start].ends_with([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
        let close = line[start..].find(')').map(|end| start + end + 1);

        match (preceded_by_space, close) {
            (true, Some(close)) => {
                // Only a token with no whitespace or parens inside counts.
                let inner = &line[start + needle.len()..close - 1];
                if !inner
                    .chars()
                    .any(|c| c.is_whitespace() || c == '(' || c == ')')
                {
                    let mut out = String::with_capacity(line.len() + value.len());
                    out.push_str(&line[..start]);
                    out.push_str(&format!("@{key}({value})"));
                    out.push_str(&line[close..]);
                    return out;
                }
                at = close;
            }
            (_, Some(close)) => at = close,
            (_, None) => break,
        }
    }

    format!("{line} @{key}({value})")
}

/// How the caller proves the file has not moved since it was parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub content: String,
}

impl Expected {
    pub fn of(content: impl Into<String>) -> Expected {
        Expected {
            content: content.into(),
        }
    }
}

/// Applies a status/order/id change to one line of a file.
///
/// `dirty` is whatever the caller knows about unsaved editor buffers; when the
/// target is among them the write is refused. The desktop app passes what
/// den.nvim reported; the Lua module passes its own buffer state.
pub fn apply(
    path: &Path,
    expected: &Expected,
    line_number: usize,
    status: Status,
    order: Option<u64>,
    id: Option<&str>,
    dirty: &[PathBuf],
) -> Result<String, Error> {
    if dirty.iter().any(|candidate| candidate == path) {
        return Err(Error::Dirty {
            path: path.to_path_buf(),
        });
    }

    let current = std::fs::read_to_string(path).map_err(|e| Error::Io(e.to_string()))?;
    if current != expected.content {
        return Err(Error::Stale {
            path: path.to_path_buf(),
        });
    }

    // Split without losing the file's exact line endings or its final newline.
    let mut lines: Vec<&str> = current.split('\n').collect();
    let index = line_number.checked_sub(1).ok_or(Error::NoSuchLine {
        path: path.to_path_buf(),
        line: line_number,
    })?;
    if index >= lines.len() {
        return Err(Error::NoSuchLine {
            path: path.to_path_buf(),
            line: line_number,
        });
    }

    let rewritten = change_line(lines[index], status, order, id)?;
    lines[index] = &rewritten;
    let updated = lines.join("\n");

    write_atomically(path, &updated)?;
    Ok(updated)
}

/// Writes via a temp file in the same directory, then renames.
///
/// Same directory matters: `rename` is only atomic within a filesystem, and a
/// temp file elsewhere would silently degrade to a copy. The `fsync` before the
/// rename is what makes a crash leave either the old file or the new one, never
/// a truncated mixture of both.
pub fn write_atomically(path: &Path, content: &str) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Io("no parent directory".into()))?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::Io(e.to_string()))?
        .as_nanos();
    let temp = parent.join(format!(
        ".{}.den-{unique}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();

    if let Err(error) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(Error::Io(error.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact assertion `den.nvim/tests/desktop.lua` makes about
    /// `task.change`. If this drifts, notes edited in the GUI and in Neovim
    /// stop matching.
    #[test]
    fn matches_den_nvims_own_change_assertion() {
        let line = "  * [ ]  Keep  **my words** @custom(x) @status(backlog)";
        let changed = change_line(line, Status::Doing, Some(200), Some("stable-id")).expect("ok");
        assert_eq!(
            changed,
            "  * [ ]  Keep  **my words** @custom(x) @status(doing) @order(200) @id(stable-id)"
        );
    }

    #[test]
    fn duplicate_metadata_is_refused_like_the_plugin() {
        // `assert(not pcall(task.change, '- [ ] Duplicate @id(a) @id(b)', ...))`
        let result = change_line(
            "- [ ] Duplicate @id(a) @id(b)",
            Status::Done,
            Some(1),
            Some("c"),
        );
        assert_eq!(result, Err(Error::InvalidMetadata));
    }

    #[test]
    fn the_checkbox_follows_the_status() {
        let done = change_line("- [ ] Ship it", Status::Done, None, None).expect("ok");
        assert!(done.starts_with("- [x] Ship it"));

        let reopened =
            change_line("- [x] Ship it @status(done)", Status::Backlog, None, None).expect("ok");
        assert!(reopened.starts_with("- [ ] Ship it"));
        assert!(reopened.contains("@status(backlog)"));
    }

    #[test]
    fn unknown_tokens_and_spacing_survive() {
        // Exactly one space between bullet and `[` — den.nvim's pattern is
        // `^%s*[-*+] %[`, so anything else is not a task at all.
        let line = "- [ ]   Draft   @tag(writing) @due(2026-09-20) @status(backlog)";
        let changed = change_line(line, Status::Doing, None, None).expect("ok");
        assert!(
            changed.contains("@tag(writing)"),
            "tags must survive: {changed}"
        );
        assert!(
            changed.contains("@due(2026-09-20)"),
            "due must survive: {changed}"
        );
        assert!(
            changed.contains("   Draft   "),
            "spacing must survive: {changed}"
        );
    }

    #[test]
    fn an_existing_id_is_never_rewritten() {
        let changed =
            change_line("- [ ] Ship @id(mine)", Status::Doing, None, Some("theirs")).expect("ok");
        assert!(changed.contains("@id(mine)"));
        assert!(!changed.contains("theirs"));
    }

    #[test]
    fn an_embedded_token_is_not_treated_as_metadata() {
        // The parser ignores `word@status(x)`, so the writer must not rewrite
        // it either — it appends a real token instead.
        let changed =
            change_line("- [ ] Odd word@status(doing)", Status::Done, None, None).expect("ok");
        assert!(changed.contains("word@status(doing)"), "{changed}");
        assert!(changed.ends_with("@status(done)"), "{changed}");
    }

    #[test]
    fn an_out_of_range_order_is_refused() {
        let result = change_line("- [ ] Ship", Status::Doing, Some(MAX_ORDER + 1), None);
        assert_eq!(result, Err(Error::InvalidOrder(MAX_ORDER + 1)));
    }

    // ── the file write ──────────────────────────────────────────────────────

    fn scratch(body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("den-mutate-test-{unique}"));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("p.md");
        std::fs::write(&file, body).expect("write");
        (dir, file)
    }

    #[test]
    fn only_the_target_line_changes() {
        let body = "# P\n\nSome prose.\n\n- [ ] One @id(a)\n- [ ] Two @id(b)\n\nTrailing prose.\n";
        let (dir, file) = scratch(body);

        let updated = apply(
            &file,
            &Expected::of(body),
            5,
            Status::Doing,
            Some(500),
            None,
            &[],
        )
        .expect("applied");

        let before: Vec<&str> = body.split('\n').collect();
        let after: Vec<&str> = updated.split('\n').collect();
        assert_eq!(before.len(), after.len(), "line count must not change");
        for (index, (a, b)) in before.iter().zip(&after).enumerate() {
            if index == 4 {
                assert_ne!(a, b, "the target line should have changed");
            } else {
                assert_eq!(a, b, "line {} must be untouched", index + 1);
            }
        }
        assert_eq!(std::fs::read_to_string(&file).expect("read"), updated);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stale_write_is_refused() {
        let body = "# P\n\n- [ ] One @id(a)\n";
        let (dir, file) = scratch(body);
        std::fs::write(&file, "# P\n\n- [ ] One @id(a) @status(doing)\n").expect("external edit");

        let result = apply(&file, &Expected::of(body), 3, Status::Done, None, None, &[]);
        assert!(matches!(result, Err(Error::Stale { .. })), "{result:?}");
        // The external edit must survive untouched.
        assert!(
            std::fs::read_to_string(&file)
                .expect("read")
                .contains("@status(doing)"),
            "the other writer's change was clobbered"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_dirty_buffer_refuses_the_write() {
        let body = "# P\n\n- [ ] One @id(a)\n";
        let (dir, file) = scratch(body);

        let result = apply(
            &file,
            &Expected::of(body),
            3,
            Status::Done,
            None,
            None,
            std::slice::from_ref(&file),
        );
        assert!(matches!(result, Err(Error::Dirty { .. })), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(&file).expect("read"),
            body,
            "file must be untouched"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_final_newline_is_preserved_either_way() {
        for body in ["# P\n\n- [ ] One\n", "# P\n\n- [ ] One"] {
            let (dir, file) = scratch(body);
            let updated =
                apply(&file, &Expected::of(body), 3, Status::Done, None, None, &[]).expect("ok");
            assert_eq!(
                updated.ends_with('\n'),
                body.ends_with('\n'),
                "trailing newline changed for {body:?}"
            );
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn no_temp_files_are_left_behind() {
        let body = "# P\n\n- [ ] One\n";
        let (dir, file) = scratch(body);
        apply(&file, &Expected::of(body), 3, Status::Done, None, None, &[]).expect("ok");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("den-") && name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left temp files: {leftovers:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The heading new tasks are filed under.
///
/// den.nvim's own project template uses this, so a task Den appends lands where
/// the plugin expects to find it.
pub const NEXT_ACTIONS: &str = "## Next actions";

/// Appends a task to a note's `## Next actions` section.
///
/// The section is created at the end of the file when it is absent, because a
/// capture that fails because a heading is missing is a capture lost.
///
/// Everything else about the file is preserved byte for byte: the same stale
/// and dirty guards as [`apply`], the same atomic write, and the same rule that
/// only the intended lines differ. The inserted line is built by `change_line`
/// so that a task Den writes is parsed identically by den.nvim.
pub fn insert_task(
    path: &Path,
    expected: &Expected,
    text: &str,
    status: Status,
    order: Option<u64>,
    id: Option<&str>,
    dirty: &[PathBuf],
) -> Result<String, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::NotATask);
    }
    if dirty.iter().any(|candidate| candidate == path) {
        return Err(Error::Dirty {
            path: path.to_path_buf(),
        });
    }

    let current = std::fs::read_to_string(path).map_err(|e| Error::Io(e.to_string()))?;
    if current != expected.content {
        return Err(Error::Stale {
            path: path.to_path_buf(),
        });
    }

    // Built through the shared rewriter, so the metadata spelling and spacing
    // match what den.nvim would have written for the same task.
    let line = change_line(&format!("- [ ] {text}"), status, order, id)?;

    let mut lines: Vec<String> = current.split('\n').map(str::to_string).collect();
    match section_end(&lines) {
        Some(at) => lines.insert(at, line),
        None => {
            // Keep exactly one blank line before a heading we are adding, and
            // do not disturb a trailing newline the file already had.
            while lines.last().is_some_and(|last| last.trim().is_empty()) {
                lines.pop();
            }
            lines.push(String::new());
            lines.push(NEXT_ACTIONS.to_string());
            lines.push(String::new());
            lines.push(line);
            lines.push(String::new());
        }
    }

    let updated = lines.join("\n");
    write_atomically(path, &updated)?;
    Ok(updated)
}

/// Where a new task goes.
///
/// After the **last task line** in `## Next actions`, so a capture lands with
/// its siblings. Falling back to the end of the section would be defensible but
/// unhelpful: a note whose section ends with a code block or a paragraph would
/// file the new task underneath it, away from every other task.
///
/// Fence-aware, because a `##` inside a code block is not a heading — the same
/// rule the parser follows.
fn section_end(lines: &[String]) -> Option<usize> {
    let mut fence = parse::Fence::default();
    let mut start = None;
    let mut last_task = None;
    let mut section_end = None;

    for (index, line) in lines.iter().enumerate() {
        let fenced = fence.consume(line) || fence.is_open();
        if fenced {
            continue;
        }
        if start.is_none() {
            if line.trim_end() == NEXT_ACTIONS {
                start = Some(index);
            }
            continue;
        }
        // The section ends at the next heading of any level.
        if line.starts_with('#') {
            section_end = Some(index);
            break;
        }
        if parse::parse_task(line).is_some() {
            last_task = Some(index);
        }
    }

    let start = start?;
    if let Some(last) = last_task {
        return Some(last + 1);
    }

    // No tasks yet: sit at the end of the section, above the blank padding that
    // separates it from whatever follows.
    let mut at = section_end.unwrap_or(lines.len());
    while at > start + 1 && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    Some(at)
}

#[cfg(test)]
mod insert_tests {
    use super::*;

    /// One directory per test: these run in parallel, and a sibling's in-flight
    /// temp file would otherwise show up in the stray-file check.
    fn scratch(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "den-insert-{}-{}",
            std::process::id(),
            name.trim_end_matches(".md")
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write");
        path
    }

    #[test]
    fn a_task_lands_under_next_actions_and_nothing_else_moves() {
        let body = "# N\n\n## Next actions\n\n- [ ] First @id(a) @status(backlog) @order(1000000)\n\n## Notes\n\nprose\n";
        let path = scratch("under.md", body);
        let updated = insert_task(
            &path,
            &Expected::of(body),
            "Second",
            Status::Backlog,
            Some(2_000_000),
            Some("b"),
            &[],
        )
        .expect("insert");

        assert_eq!(
            updated,
            "# N\n\n## Next actions\n\n- [ ] First @id(a) @status(backlog) @order(1000000)\n- [ ] Second @status(backlog) @order(2000000) @id(b)\n\n## Notes\n\nprose\n"
        );
        // The prose section is untouched.
        assert!(updated.ends_with("## Notes\n\nprose\n"));
    }

    #[test]
    fn a_note_without_the_heading_gets_one() {
        let body = "# N\n\nSome prose.\n";
        let path = scratch("heading.md", body);
        let updated = insert_task(
            &path,
            &Expected::of(body),
            "First",
            Status::Backlog,
            None,
            Some("a"),
            &[],
        )
        .expect("insert");

        assert!(updated.contains(NEXT_ACTIONS), "heading was not created");
        assert!(updated.contains("- [ ] First @status(backlog) @id(a)"));
        assert!(updated.starts_with("# N\n\nSome prose."), "prose survived");
    }

    #[test]
    fn the_same_guards_as_apply() {
        let body = "# N\n\n## Next actions\n\n";
        let path = scratch("guards.md", body);

        // A dirty buffer refuses.
        assert!(matches!(
            insert_task(
                &path,
                &Expected::of(body),
                "x",
                Status::Backlog,
                None,
                None,
                &[path.clone()]
            ),
            Err(Error::Dirty { .. })
        ));
        // A stale hash refuses.
        assert!(matches!(
            insert_task(
                &path,
                &Expected::of("something else"),
                "x",
                Status::Backlog,
                None,
                None,
                &[]
            ),
            Err(Error::Stale { .. })
        ));
        // An empty capture is not a task.
        assert!(matches!(
            insert_task(
                &path,
                &Expected::of(body),
                "   ",
                Status::Backlog,
                None,
                None,
                &[]
            ),
            Err(Error::NotATask)
        ));
        // None of that wrote anything.
        assert_eq!(std::fs::read_to_string(&path).expect("read"), body);
    }

    #[test]
    fn a_heading_inside_a_fence_is_not_a_section_boundary() {
        let body =
            "# N\n\n## Next actions\n\n- [ ] First @id(a) @status(backlog)\n\n```\n## Notes\n```\n";
        let path = scratch("fence.md", body);
        let updated = insert_task(
            &path,
            &Expected::of(body),
            "Second",
            Status::Backlog,
            None,
            Some("b"),
            &[],
        )
        .expect("insert");
        // The fenced `## Notes` must not have ended the section early.
        // The new task sits with its sibling, not after the code block.
        assert!(updated.contains("@id(a) @status(backlog)\n- [ ] Second"));
        assert!(updated.contains("```\n## Notes\n```"), "the fence survived");
    }

    #[test]
    fn no_temp_files_are_left_behind() {
        let body = "# N\n\n## Next actions\n\n";
        let path = scratch("clean.md", body);
        insert_task(
            &path,
            &Expected::of(body),
            "x",
            Status::Backlog,
            None,
            None,
            &[],
        )
        .expect("insert");
        let strays: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .expect("read dir")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".den-"))
            .collect();
        assert!(strays.is_empty(), "temp files survived: {strays:?}");
    }
}

/// Rewrites a task's caption, preserving every `@token` on the line.
///
/// This is what makes editing a title from the desktop safe: den.nvim wrote
/// those tokens, other tools may have added their own, and none of them are
/// Den's to discard. Only the words before the first metadata token change.
///
/// The token run is found the same way [`set_metadata`] finds one — a `@key(`
/// preceded by whitespace with no whitespace or parens inside — so a caption
/// containing `word@id(x)` is treated as prose by both, exactly as the parser
/// treats it.
pub fn retitle_line(line: &str, text: &str) -> Result<String, Error> {
    let task = parse::parse_task(line).ok_or(Error::NotATask)?;
    if task.invalid_metadata {
        return Err(Error::InvalidMetadata);
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::NotATask);
    }

    // The checkbox and everything before it survive verbatim, including indent
    // and the bullet character the file happens to use.
    let trimmed = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - trimmed.len()];
    let head_len = trimmed
        .char_indices()
        .nth(5)
        .map(|(at, _)| at)
        .unwrap_or(trimmed.len());
    let (head, body) = trimmed.split_at(head_len);

    let tokens = token_run(body);
    let mut out = String::with_capacity(line.len() + text.len());
    out.push_str(indent);
    out.push_str(head);
    // Exactly one space after `]`: den.nvim's pattern requires it, and dropping
    // it makes the line stop being a task at all.
    out.push(' ');
    out.push_str(text);
    if !tokens.is_empty() {
        out.push(' ');
        out.push_str(tokens.trim());
    }
    Ok(out)
}

/// The trailing `@key(value)` run of a task body, or `""`.
fn token_run(body: &str) -> &str {
    let mut at = 0;
    while let Some(offset) = body[at..].find('@') {
        let start = at + offset;
        let preceded_by_space =
            start == 0 || body[..start].ends_with([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
        let rest = &body[start + 1..];
        let opens = rest.find('(');
        let closes = rest.find(')');

        if let (true, Some(open), Some(close)) = (preceded_by_space, opens, closes)
            && open < close
        {
            let key = &rest[..open];
            let inner = &rest[open + 1..close];
            let well_formed = !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && !inner
                    .chars()
                    .any(|c| c.is_whitespace() || c == '(' || c == ')');
            if well_formed {
                return &body[start..];
            }
        }
        at = start + 1;
    }
    ""
}

/// Applies a caption change to one line of a file.
pub fn retitle(
    path: &Path,
    expected: &Expected,
    line_number: usize,
    text: &str,
    dirty: &[PathBuf],
) -> Result<String, Error> {
    if dirty.iter().any(|candidate| candidate == path) {
        return Err(Error::Dirty {
            path: path.to_path_buf(),
        });
    }

    let current = std::fs::read_to_string(path).map_err(|e| Error::Io(e.to_string()))?;
    if current != expected.content {
        return Err(Error::Stale {
            path: path.to_path_buf(),
        });
    }

    let mut lines: Vec<&str> = current.split('\n').collect();
    let index = line_number.checked_sub(1).ok_or(Error::NoSuchLine {
        path: path.to_path_buf(),
        line: line_number,
    })?;
    if index >= lines.len() {
        return Err(Error::NoSuchLine {
            path: path.to_path_buf(),
            line: line_number,
        });
    }

    let rewritten = retitle_line(lines[index], text)?;
    lines[index] = &rewritten;
    let updated = lines.join("\n");

    write_atomically(path, &updated)?;
    Ok(updated)
}

#[cfg(test)]
mod retitle_tests {
    use super::*;

    /// The property that matters: after a retitle the line still parses to the
    /// same task, with only the words changed.
    fn round_trip(line: &str, text: &str) {
        let before = parse::parse_task(line).expect("parses before");
        let after_line = retitle_line(line, text).expect("retitles");
        let after = parse::parse_task(&after_line).expect("parses after");

        assert_eq!(after.display, text, "in {after_line:?}");
        assert_eq!(after.id, before.id, "id changed in {after_line:?}");
        assert_eq!(after.status, before.status, "status changed");
        assert_eq!(after.order, before.order, "order changed");
        assert_eq!(after.due, before.due, "due changed");
        assert_eq!(after.tags, before.tags, "tags changed");
        assert_eq!(after.completed, before.completed, "mark changed");
    }

    #[test]
    fn every_token_survives_a_new_caption() {
        round_trip(
            "- [ ] Old words @tag(writing) @due(2026-09-20) @id(a) @status(backlog) @order(1000000)",
            "New words",
        );
        round_trip("  * [x] Indented star @id(b) @status(done)", "Rewritten");
        round_trip("+ [ ] No metadata at all", "Still none");
        round_trip("- [ ] Unicode — em dash ✓ @id(u)", "Plain again");
    }

    #[test]
    fn an_unknown_token_is_preserved_too() {
        // Den does not own every token on the line and must not drop one.
        let out = retitle_line("- [ ] Old @custom(x) @id(a)", "New").expect("retitle");
        assert!(out.contains("@custom(x)"), "{out}");
        assert!(out.contains("@id(a)"), "{out}");
    }

    #[test]
    fn an_embedded_token_stays_prose() {
        // `word@id(x)` is not metadata to the parser, so it is part of the
        // caption and goes when the caption goes.
        let out = retitle_line("- [ ] Embedded word@id(x) @status(backlog)", "New").expect("ok");
        assert!(!out.contains("word@id(x)"), "{out}");
        assert!(out.contains("@status(backlog)"), "{out}");
    }

    #[test]
    fn an_empty_or_non_task_line_is_refused() {
        assert!(matches!(
            retitle_line("- [ ] x", "   "),
            Err(Error::NotATask)
        ));
        assert!(matches!(
            retitle_line("not a task", "x"),
            Err(Error::NotATask)
        ));
        assert!(matches!(
            retitle_line("- [ ] x @id(a) @id(b)", "y"),
            Err(Error::InvalidMetadata)
        ));
    }
}
