//! A file's text as lines, keeping its line endings and final newline exactly.
//!
//! Edits work on lines and must write back every untouched byte as it was, so
//! the split has to be reversible: `TextBuf::parse(t).render() == t` for any
//! input.

/// A file split into lines, plus what it takes to join them back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuf {
    pub lines: Vec<String>,
    /// `"\r\n"` when the file uses Windows line endings, else `"\n"`.
    pub eol: &'static str,
    /// Whether the text ends with a line ending.
    pub trailing: bool,
}

impl TextBuf {
    pub fn parse(text: &str) -> TextBuf {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
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
        let lines = body
            .split('\n')
            .map(|line| {
                if eol == "\r\n" {
                    line.strip_suffix('\r').unwrap_or(line).to_string()
                } else {
                    line.to_string()
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
        ] {
            assert_eq!(TextBuf::parse(text).render(), text, "{text:?}");
        }
    }

    #[test]
    fn crlf_lines_do_not_keep_the_carriage_return() {
        let buf = TextBuf::parse("one\r\ntwo\r\n");
        assert_eq!(buf.lines, vec!["one", "two"]);
        assert_eq!(buf.eol, "\r\n");
    }
}
