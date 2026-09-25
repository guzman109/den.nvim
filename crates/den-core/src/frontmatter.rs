//! YAML frontmatter: the fields block at the very top of a file.
//!
//! Only a block that starts on the first line with `---` and closes with a
//! later `---` (or `...`) counts. Anything that looks like a field further down
//! is prose, so a sentence in a note can never change a project's status.

use jiff::civil::Date;
use serde_norway::{Mapping, Value};

use crate::text::TextBuf;

#[derive(Debug, Clone, Default)]
pub struct Frontmatter {
    /// Line indices of the opening and closing fences, when present.
    pub range: Option<(usize, usize)>,
    pub fields: Mapping,
    /// Why the block could not be read, when it could not.
    pub error: Option<String>,
}

impl Frontmatter {
    pub fn parse(lines: &[String]) -> Frontmatter {
        if lines.first().map(|l| l.trim_end()) != Some("---") {
            return Frontmatter::default();
        }
        let Some(close) = lines
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, l)| matches!(l.trim_end(), "---" | "..."))
            .map(|(i, _)| i)
        else {
            return Frontmatter {
                error: Some("frontmatter is never closed with ---".to_string()),
                ..Frontmatter::default()
            };
        };
        let yaml = lines[1..close].join("\n");
        let (fields, error) = if yaml.trim().is_empty() {
            (Mapping::new(), None)
        } else {
            match serde_norway::from_str::<Value>(&yaml) {
                Ok(Value::Mapping(map)) => (map, None),
                Ok(Value::Null) => (Mapping::new(), None),
                Ok(_) => (
                    Mapping::new(),
                    Some("frontmatter must be key: value pairs".to_string()),
                ),
                Err(e) => (Mapping::new(), Some(e.to_string())),
            }
        };
        Frontmatter {
            range: Some((0, close)),
            fields,
            error,
        }
    }

    /// A field as text. Numbers and booleans are written as they read; an empty
    /// value is `None`.
    pub fn text(&self, key: &str) -> Option<String> {
        match self.fields.get(key)? {
            Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    pub fn date(&self, key: &str) -> Option<Date> {
        self.text(key)?.parse().ok()
    }

    /// A list field; a single value counts as a list of one.
    pub fn list(&self, key: &str) -> Vec<String> {
        match self.fields.get(key) {
            Some(Value::Sequence(items)) => items
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.trim().to_string()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .filter(|s| !s.is_empty())
                .collect(),
            _ => self.text(key).into_iter().collect(),
        }
    }
}

/// Sets `key: value` in the file's frontmatter, or removes it when `value` is
/// `None`, touching no other line. Creates the block when the file has none.
pub fn set_field(buf: &mut TextBuf, key: &str, value: Option<&str>) {
    let fm = Frontmatter::parse(&buf.lines);
    let rendered = value.map(|v| format!("{key}: {}", scalar(v)));
    let Some((_, close)) = fm.range else {
        if let Some(line) = rendered {
            buf.lines
                .splice(0..0, ["---".to_string(), line, "---".to_string()]);
            buf.trailing = true;
        }
        return;
    };
    let existing = (1..close).find(|&i| {
        let line = &buf.lines[i];
        !line.starts_with([' ', '\t'])
            && line
                .split_once(':')
                .is_some_and(|(k, _)| k.trim_end() == key)
    });
    match (existing, rendered) {
        (Some(i), Some(line)) => buf.lines[i] = line,
        (Some(i), None) => {
            buf.lines.remove(i);
        }
        (None, Some(line)) => buf.lines.insert(close, line),
        (None, None) => {}
    }
}

/// Writes a value as a plain YAML scalar when that reads back unchanged, and
/// quoted otherwise.
fn scalar(value: &str) -> String {
    let plain = !value.is_empty()
        && !value.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%',
            '@', '`', ' ',
        ])
        && !value.ends_with(' ')
        && !value.contains(": ")
        && !value.contains(" #")
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off"
        );
    if plain {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        TextBuf::parse(text).lines
    }

    #[test]
    fn reads_fields_only_at_the_top() {
        let fm = Frontmatter::parse(&lines(
            "---\nroot: ~/code/site\nstatus: active\ndue: 2026-10-01\n---\n# Site\nstatus: archived\n",
        ));
        assert_eq!(fm.range, Some((0, 4)));
        assert_eq!(fm.text("status").as_deref(), Some("active"));
        assert_eq!(fm.date("due"), Some(jiff::civil::date(2026, 10, 1)));

        let none = Frontmatter::parse(&lines("# Site\n---\nstatus: archived\n---\n"));
        assert!(none.range.is_none());
        assert!(none.text("status").is_none());
    }

    #[test]
    fn an_unclosed_block_is_reported_not_guessed() {
        let fm = Frontmatter::parse(&lines("---\nstatus: active\n# Title\n"));
        assert!(fm.range.is_none());
        assert!(fm.error.is_some());
    }

    #[test]
    fn broken_yaml_is_reported() {
        let fm = Frontmatter::parse(&lines("---\nstatus: [active\n---\n"));
        assert!(fm.error.is_some());
        assert!(fm.fields.is_empty());
    }

    #[test]
    fn set_field_touches_only_its_line() {
        let mut buf = TextBuf::parse("---\nroot: ~/a\nstatus: active\n---\n# T\n");
        set_field(&mut buf, "status", Some("archived"));
        assert_eq!(buf.render(), "---\nroot: ~/a\nstatus: archived\n---\n# T\n");
        set_field(&mut buf, "due", Some("2026-10-01"));
        assert_eq!(
            buf.render(),
            "---\nroot: ~/a\nstatus: archived\ndue: 2026-10-01\n---\n# T\n"
        );
        set_field(&mut buf, "root", None);
        assert_eq!(
            buf.render(),
            "---\nstatus: archived\ndue: 2026-10-01\n---\n# T\n"
        );
    }

    #[test]
    fn set_field_creates_the_block() {
        let mut buf = TextBuf::parse("# T\n");
        set_field(&mut buf, "status", Some("active"));
        assert_eq!(buf.render(), "---\nstatus: active\n---\n# T\n");
    }

    #[test]
    fn values_that_would_change_meaning_are_quoted() {
        let mut buf = TextBuf::parse("# T\n");
        set_field(&mut buf, "mood", Some("yes"));
        set_field(&mut buf, "note", Some("a: b"));
        let fm = Frontmatter::parse(&buf.lines);
        assert_eq!(fm.text("mood").as_deref(), Some("yes"));
        assert_eq!(fm.text("note").as_deref(), Some("a: b"));
    }
}
