//! Port of den.nvim's `lua/den/task.lua` and `lua/den/parse.lua`.
//!
//! Behaviour here must match the plugin exactly: the same vault is parsed by
//! both, and a disagreement shows up as a task that the desktop can see but
//! Neovim cannot edit (or worse, the reverse). `tests/parity.rs` runs this
//! against the plugin's own `tests/fixtures/tasks.json`.
//!
//! Lua patterns are ASCII-classed, so the character predicates below are too.
//! Using Rust's Unicode-aware equivalents would silently widen what counts as
//! a separator or an identifier.

/// The maximum `@order(...)` den.nvim accepts.
pub const MAX_ORDER: u64 = 9_000_000_000_000;

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

fn is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Lua's `%w`: ASCII alphanumeric.
fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn trim(value: &str) -> &str {
    value.trim_matches(is_space)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    Backlog,
    Doing,
    Done,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Backlog => "backlog",
            Status::Doing => "doing",
            Status::Done => "done",
        }
    }

    fn from_str(value: &str) -> Option<Status> {
        match value {
            "backlog" => Some(Status::Backlog),
            "doing" => Some(Status::Doing),
            "done" => Some(Status::Done),
            _ => None,
        }
    }

    /// The `[ ]` / `[~]` / `[x]` mark the design draws for each status.
    pub fn mark(self) -> &'static str {
        match self {
            Status::Backlog => "[ ]",
            Status::Doing => "[~]",
            Status::Done => "[x]",
        }
    }
}

/// A task line, parsed exactly as den.nvim parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTask {
    /// Everything after the checkbox, verbatim.
    pub text: String,
    /// den.nvim's caption: `text` with only id/status/order stripped.
    pub caption: String,
    /// What the desktop shows: `caption` with `@tag()`/`@due()` also stripped.
    pub display: String,
    /// Desktop-side convention, read from `@tag(name)` tokens.
    pub tags: Vec<String>,
    pub completed: bool,
    pub status: Status,
    pub id: Option<String>,
    pub order: Option<u64>,
    pub due: Option<String>,
    pub invalid_metadata: bool,
}

/// Splits `@key(value)` when the whole token is exactly that, per
/// `^@([%a]+)%(([^%s()]*)%)$`.
fn metadata_token(token: &str) -> Option<(&str, &str)> {
    let rest = token.strip_prefix('@')?;
    let open = rest.find('(')?;
    let (key, rest) = rest.split_at(open);
    if key.is_empty() || !key.chars().all(is_alpha) {
        return None;
    }
    let value = rest.strip_prefix('(')?.strip_suffix(')')?;
    if value.chars().any(|c| is_space(c) || c == '(' || c == ')') {
        return None;
    }
    Some((key, value))
}

/// Finds the next `@key(value)` anywhere in `text` from `from`, matching
/// Lua's unanchored `@[%a]+%([^%s()]*%)`. Returns `(start, end, key, value)`.
fn find_metadata(text: &str, from: usize) -> Option<(usize, usize, &str, &str)> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < bytes.len() {
        let Some(offset) = text[at..].find('@') else {
            return None;
        };
        let start = at + offset;
        let after = start + 1;
        let key_len = text[after..]
            .chars()
            .take_while(|c| is_alpha(*c))
            .map(char::len_utf8)
            .sum::<usize>();
        if key_len > 0 && bytes.get(after + key_len) == Some(&b'(') {
            let value_start = after + key_len + 1;
            let value_len = text[value_start..]
                .chars()
                .take_while(|c| !is_space(*c) && *c != '(' && *c != ')')
                .map(char::len_utf8)
                .sum::<usize>();
            let close = value_start + value_len;
            if bytes.get(close) == Some(&b')') {
                return Some((
                    start,
                    close + 1,
                    &text[after..after + key_len],
                    &text[value_start..close],
                ));
            }
        }
        at = start + 1;
    }
    None
}

fn is_plugin_metadata(key: &str) -> bool {
    matches!(key, "id" | "status" | "order")
}

/// Rebuilds a caption by dropping the metadata tokens `drop` selects, then
/// collapsing whitespace runs and trimming — Lua's two chained `gsub`s.
fn strip_metadata(text: &str, drop: impl Fn(&str) -> bool) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut at = 0;
    while let Some((start, end, key, _)) = find_metadata(text, at) {
        kept.push_str(&text[at..start]);
        if !drop(key) {
            kept.push_str(&text[start..end]);
        }
        at = end;
    }
    kept.push_str(&text[at..]);

    let mut collapsed = String::with_capacity(kept.len());
    let mut in_space = false;
    for c in kept.chars() {
        if is_space(c) {
            in_space = true;
        } else {
            if in_space {
                collapsed.push(' ');
                in_space = false;
            }
            collapsed.push(c);
        }
    }
    if in_space {
        collapsed.push(' ');
    }
    trim(&collapsed).to_string()
}

/// den.nvim's `task.parse`: `^%s*[-*+] %[([ xX])%]%s+(.+)`, then a token scan.
pub fn parse_task(line: &str) -> Option<ParsedTask> {
    let rest = line.trim_start_matches(is_space);
    let mut chars = rest.chars();
    if !matches!(chars.next()?, '-' | '*' | '+') {
        return None;
    }
    if chars.next()? != ' ' || chars.next()? != '[' {
        return None;
    }
    let mark = chars.next()?;
    if !matches!(mark, ' ' | 'x' | 'X') || chars.next()? != ']' {
        return None;
    }
    let after = chars.as_str();
    // `%s+(.+)` needs at least one separator; an all-whitespace remainder is
    // rejected by the emptiness check below, as `vim.trim(text) == ''` does.
    let text = after.trim_start_matches(is_space);
    if text.len() == after.len() || trim(text).is_empty() {
        return None;
    }

    let completed = mark != ' ';
    let mut status = Status::Backlog;
    let mut id = None;
    let mut order = None;
    let mut invalid_metadata = false;
    let mut tags = Vec::new();
    let mut seen: Vec<&str> = Vec::new();

    for token in text.split(is_space).filter(|t| !t.is_empty()) {
        let Some((key, value)) = metadata_token(token) else {
            continue;
        };
        if key == "tag" && !value.is_empty() {
            // Desktop-side only. den.nvim preserves unknown tokens verbatim.
            let tag = value.to_string();
            if !tags.contains(&tag) {
                tags.push(tag);
            }
            continue;
        }
        if !is_plugin_metadata(key) {
            continue;
        }
        if seen.contains(&key) {
            invalid_metadata = true;
        }
        seen.push(key);
        match key {
            "id" if !value.is_empty()
                && value.chars().all(|c| is_alnum(c) || c == '_' || c == '-') =>
            {
                id = Some(value.to_string());
            }
            "status" if Status::from_str(value).is_some() => {
                status = Status::from_str(value).expect("checked by the guard");
            }
            "order"
                if !value.is_empty()
                    && value.chars().all(|c| c.is_ascii_digit())
                    && value.parse::<u64>().is_ok_and(|n| n <= MAX_ORDER) =>
            {
                order = value.parse::<u64>().ok();
            }
            _ => invalid_metadata = true,
        }
    }

    // A checked box always wins; a `@status(done)` on an unchecked box does not.
    if completed {
        status = Status::Done;
    } else if status == Status::Done {
        status = Status::Backlog;
    }

    let caption = strip_metadata(text, is_plugin_metadata);
    let display = strip_metadata(text, |key| {
        is_plugin_metadata(key) || matches!(key, "tag" | "due")
    });
    let due = find_due(text);

    Some(ParsedTask {
        text: text.to_string(),
        caption,
        display,
        tags,
        completed,
        status,
        id,
        order,
        due,
        invalid_metadata,
    })
}

/// `@due(YYYY-MM-DD)` anywhere in the text, validated the way parse.lua does.
/// An unparseable date leaves the token in place but yields no due date.
fn find_due(text: &str) -> Option<String> {
    let mut at = 0;
    while let Some(offset) = text[at..].find("@due(") {
        let start = at + offset + "@due(".len();
        let rest = &text[start..];
        if let Some(close) = rest.find(')') {
            let value = &rest[..close];
            if is_date_shaped(value) && valid_date(value) {
                return Some(value.to_string());
            }
        }
        at = start;
    }
    None
}

fn is_date_shaped(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..].iter().all(u8::is_ascii_digit)
}

pub fn valid_date(value: &str) -> bool {
    if !is_date_shaped(value) {
        return false;
    }
    let year: u32 = value[0..4].parse().expect("four digits");
    let month: u32 = value[5..7].parse().expect("two digits");
    let day: u32 = value[8..10].parse().expect("two digits");
    if !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    day >= 1 && day <= days[month as usize - 1]
}

/// Tracks Markdown fences the way parse.lua does: the first ``` or ~~~ opens,
/// and a run of the same character at least as long closes it.
#[derive(Default)]
pub struct Fence {
    open: Option<(char, usize)>,
}

impl Fence {
    /// Returns true when `line` is a fence marker, which is never itself a task.
    pub fn consume(&mut self, line: &str) -> bool {
        let rest = line.trim_start_matches(is_space);
        let marker = ['`', '~'].into_iter().find_map(|c| {
            let run = rest.chars().take_while(|x| *x == c).count();
            (run >= 3).then_some((c, run))
        });
        let Some((char, len)) = marker else {
            return false;
        };
        match self.open {
            None => self.open = Some((char, len)),
            Some((open_char, open_len)) if char == open_char && len >= open_len => self.open = None,
            Some(_) => {}
        }
        true
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// `Status: archived` on its own line, per parse.lua's `^Status:%s*archived%s*$`.
pub fn is_archived_line(line: &str) -> bool {
    line.strip_prefix("Status:")
        .is_some_and(|rest| trim(rest) == "archived")
}

/// `# Title` on the first line, per `^#%s+(.+)`.
pub fn heading_title(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?;
    if !rest.starts_with(is_space) {
        return None;
    }
    let title = rest.trim_start_matches(is_space);
    (!title.is_empty()).then_some(title)
}
