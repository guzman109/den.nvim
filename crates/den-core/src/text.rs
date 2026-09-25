//! A file's text as lines, keeping its line endings and final newline exactly.
//!
//! Edits work on lines and must write back every untouched byte as it was, so
//! the split has to be reversible: `TextBuf::parse(t).render() == t` for any
//! input.

/// A file split into lines, plus what it takes to join them back.
///
/// A file that uses Windows line endings throughout is split on `\r\n`. A
/// file that mixes endings is split on `\n` alone and each line keeps its
/// own `\r`, so lines nobody edits come back byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuf {
    pub lines: Vec<String>,
    /// `"\r\n"` when every line ending is Windows style, else `"\n"`.
    pub eol: &'static str,
    /// Whether the text ends with a line ending.
    pub trailing: bool,
}

impl TextBuf {
    pub fn parse(text: &str) -> TextBuf {
        let newlines = text.matches('\n').count();
        let crlf = text.matches("\r\n").count();
        let eol = if crlf > 0 && crlf == newlines {
            "\r\n"
        } else {
            "\n"
        };
        if text.is_empty() {
            return TextBuf {
                lines: Vec::new(),
                eol,
                trailing: false,
            };
        }
        let trailing = text.ends_with('\n');
        let body = if trailing {
            text.strip_suffix(eol)
                .or_else(|| text.strip_suffix('\n'))
                .unwrap_or(text)
        } else {
            text
        };
        let parts: Vec<&str> = body.split('\n').collect();
        let last = parts.len() - 1;
        let lines = parts
            .iter()
            .enumerate()
            .map(|(i, line)| {
                // Only a `\r` that came before a `\n` belongs to the ending.
                let ended = i < last || trailing;
                if eol == "\r\n" && ended {
                    line.strip_suffix('\r').unwrap_or(line).to_string()
                } else {
                    (*line).to_string()
                }
            })
            .collect();
        TextBuf {
            lines,
            eol,
            trailing,
        }
    }

    pub fn render(&self) -> String {
        let mut out = self.lines.join(self.eol);
        if self.trailing {
            out.push_str(self.eol);
        }
        out
    }

    /// A new, empty file that ends with a newline once it has content.
    pub fn empty() -> TextBuf {
        TextBuf {
            lines: Vec::new(),
            eol: "\n",
            trailing: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_shape_of_text() {
        for text in [
            "",
            "\n",
            "a",
            "a\n",
            "a\nb",
            "a\nb\n",
            "a\n\n",
            "a\r\nb\r\n",
            "a\r\nb",
            "\r\n",
            "a\r\nb\nc\n",
            "a\nb\r\n",
            "a\r\nb\r",
            "a\r\n\r\nb\n",
        ] {
            assert_eq!(TextBuf::parse(text).render(), text, "{text:?}");
        }
    }

    #[test]
    fn mixed_endings_survive_an_edit_to_one_line() {
        let mut buf = TextBuf::parse("a\r\nb\nc\r\n");
        assert_eq!(buf.eol, "\n");
        buf.lines.push("d".to_string());
        assert_eq!(buf.render(), "a\r\nb\nc\r\nd\n");
    }

    #[test]
    fn crlf_lines_do_not_keep_the_carriage_return() {
        let buf = TextBuf::parse("one\r\ntwo\r\n");
        assert_eq!(buf.lines, vec!["one", "two"]);
        assert_eq!(buf.eol, "\r\n");
    }
}
