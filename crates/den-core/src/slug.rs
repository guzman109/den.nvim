//! File names made from titles.

/// `"Personal website!"` → `"personal-website"`.
///
/// Letters and digits in any script are kept (lowercased); everything else
/// becomes a single `-`. The result is never empty and at most 60 characters.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.extend(c.to_lowercase());
        } else {
            dash = true;
        }
        if out.chars().count() >= 60 {
            break;
        }
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn slugs_read_like_the_title() {
        assert_eq!(slug("Personal website"), "personal-website");
        assert_eq!(slug("  den.nvim  "), "den-nvim");
        assert_eq!(slug("Café & crème"), "café-crème");
        assert_eq!(slug("!!!"), "untitled");
        assert_eq!(slug("a--b"), "a-b");
    }
}
