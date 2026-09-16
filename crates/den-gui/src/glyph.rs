//! The icon set, as inline SVG.
//!
//! Two rules keep this from becoming decoration:
//!
//! 1. **Every glyph stands for something the engine knows.** The four note
//!    kinds are `den_core::vault::Kind`; the stack marks come from a note's
//!    `Stack:` line. An icon that means nothing is a sticker.
//! 2. **Monochrome, drawn in `currentColor`.** Brand logos arrive in their own
//!    colours — React blue, Docker blue, Rust orange — and dropping those into
//!    the ember palette would put a cold backlog row in competition with the
//!    lit task, which is the one rule the whole design rests on. Tinting a
//!    single-path silhouette keeps colour meaning what it means everywhere else.
//!
//! These are stand-ins in the same line weight as the rest of the UI. The
//! shipping set should come from Simple Icons (CC0, single-path, monochrome),
//! vendored as bytes — not from a brand-coloured icon library and not from an
//! icon font, which would reintroduce the glyph-fallback problems Nerd Fonts
//! have.

use iced::widget::svg;
use iced::{Color, Length};

/// Wraps a path in the stroke style every glyph here shares.
macro_rules! stroked {
    ($body:expr) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" "#,
            r#"stroke="currentColor" stroke-width="1.5" stroke-linecap="round" "#,
            r#"stroke-linejoin="round">"#,
            $body,
            "</svg>"
        )
    };
}

// ── note kinds: den_core::vault::Kind ───────────────────────────────────────
pub const PROJECTS: &str = stroked!(
    r#"<path d="M2 5.2c0-.7.5-1.2 1.2-1.2h2.3c.4 0 .8.2 1 .5l.6.8h4.7c.7 0 1.2.5 1.2 1.2v4.3c0 .7-.5 1.2-1.2 1.2H3.2c-.7 0-1.2-.5-1.2-1.2z"/>"#
);
pub const DAILY: &str = stroked!(
    r#"<circle cx="8" cy="8" r="3.1"/><path d="M8 1.6v1.5M8 12.9v1.5M1.6 8h1.5M12.9 8h1.5M3.5 3.5l1 1M11.5 11.5l1 1M12.5 3.5l-1 1M4.5 11.5l-1 1"/>"#
);
pub const INBOX: &str = stroked!(
    r#"<path d="M2 9.5h3l1 1.8h4l1-1.8h3M2 9.5 3.8 3.6a1 1 0 0 1 1-.7h6.4a1 1 0 0 1 1 .7L14 9.5v2.7a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/>"#
);
pub const NOTES: &str = stroked!(
    r#"<path d="M3.5 2.2h6l3 3v8.6a.4.4 0 0 1-.4.4H3.5a.4.4 0 0 1-.4-.4V2.6a.4.4 0 0 1 .4-.4z"/><path d="M9.3 2.3v3.2h3.1M5.4 8.4h5.2M5.4 10.9h3.4"/>"#
);

// ── chrome ──────────────────────────────────────────────────────────────────
pub const SEARCH: &str =
    stroked!(r#"<circle cx="7.1" cy="7.1" r="4.4"/><path d="m10.4 10.4 3 3"/>"#);
pub const CLOCK: &str = stroked!(r#"<circle cx="8" cy="8" r="6"/><path d="M8 4.6V8l2.3 1.5"/>"#);
pub const DUE: &str = stroked!(
    r#"<rect x="2.3" y="3.3" width="11.4" height="10.4" rx="1.2"/><path d="M2.3 6.4h11.4M5.4 2v2.4M10.6 2v2.4"/>"#
);
pub const VAULT: &str = stroked!(
    r#"<path d="M2.4 5.6 8 2.5l5.6 3.1v4.8L8 13.5 2.4 10.4z"/><path d="M8 7.2v6.3M2.4 5.6 8 8.8l5.6-3.2"/>"#
);
pub const LOCK: &str = stroked!(
    r#"<rect x="3.4" y="7" width="9.2" height="6.7" rx="1.3"/><path d="M5.5 7V5.2a2.5 2.5 0 0 1 5 0V7"/>"#
);
pub const NVIM: &str = stroked!(
    r#"<path d="M2.6 4.1 6 2.2v9.5l-3.4 2.1zM13.4 11.9 10 13.8V4.3l3.4-2.1z"/><path d="M6 2.2 13.4 12M2.6 4.1 10 13.8"/>"#
);
pub const PLUS: &str = stroked!(r#"<path d="M8 3.4v9.2M3.4 8h9.2"/>"#);
pub const SETTINGS: &str = stroked!(
    r#"<circle cx="8" cy="8" r="2.3"/><path d="M12.9 9.8a1.3 1.3 0 0 0 .3 1.4l.1.1a1.5 1.5 0 1 1-2.2 2.2l-.1-.1a1.3 1.3 0 0 0-1.4-.3 1.3 1.3 0 0 0-.8 1.2v.2a1.5 1.5 0 0 1-3 0v-.1a1.3 1.3 0 0 0-.9-1.2 1.3 1.3 0 0 0-1.4.3l-.1.1a1.5 1.5 0 1 1-2.2-2.2l.1-.1a1.3 1.3 0 0 0 .3-1.4 1.3 1.3 0 0 0-1.2-.8h-.2a1.5 1.5 0 0 1 0-3h.1a1.3 1.3 0 0 0 1.2-.9 1.3 1.3 0 0 0-.3-1.4l-.1-.1a1.5 1.5 0 1 1 2.2-2.2l.1.1a1.3 1.3 0 0 0 1.4.3h.1a1.3 1.3 0 0 0 .8-1.2v-.2a1.5 1.5 0 0 1 3 0v.1a1.3 1.3 0 0 0 .8 1.2 1.3 1.3 0 0 0 1.4-.3l.1-.1a1.5 1.5 0 1 1 2.2 2.2l-.1.1a1.3 1.3 0 0 0-.3 1.4v.1a1.3 1.3 0 0 0 1.2.8h.2a1.5 1.5 0 0 1 0 3h-.1a1.3 1.3 0 0 0-1.2.8z"/>"#
);
pub const THEME: &str = stroked!(
    r#"<circle cx="8" cy="8" r="5.8"/><path d="M8 2.2v11.6a5.8 5.8 0 0 0 0-11.6z" fill="currentColor" stroke="none"/>"#
);

// ── stacks: from a note's `Stack:` line ─────────────────────────────────────
const RUST: &str = stroked!(
    r#"<circle cx="8" cy="8" r="5.2"/><circle cx="8" cy="8" r="2"/><path d="M8 .9v1.7M8 13.4v1.7M.9 8h1.7M13.4 8h1.7M2.9 2.9l1.2 1.2M11.9 11.9l1.2 1.2M13.1 2.9l-1.2 1.2M4.1 11.9l-1.2 1.2"/>"#
);
const LUA: &str = stroked!(
    r#"<circle cx="8" cy="8" r="5.4"/><circle cx="8" cy="8" r="1.7" fill="currentColor" stroke="none"/><circle cx="11.7" cy="4.3" r="1.6" fill="currentColor" stroke="none"/>"#
);
const REACT: &str = stroked!(
    r#"<ellipse cx="8" cy="8" rx="6.6" ry="2.6"/><ellipse cx="8" cy="8" rx="6.6" ry="2.6" transform="rotate(60 8 8)"/><ellipse cx="8" cy="8" rx="6.6" ry="2.6" transform="rotate(120 8 8)"/><circle cx="8" cy="8" r="1.2" fill="currentColor" stroke="none"/>"#
);
const TAILWIND: &str = stroked!(
    r#"<path d="M5 5.6c.8-2 1.9-3 3.3-3 2.1 0 2.4 1.5 3.4 1.9.7.2 1.3-.1 1.8-.8-.8 2-1.9 3-3.3 3-2.1 0-2.4-1.5-3.4-1.9-.7-.2-1.3.1-1.8.8z"/><path d="M1.5 10.6c.8-2 1.9-3 3.3-3 2.1 0 2.4 1.5 3.4 1.9.7.2 1.3-.1 1.8-.8-.8 2-1.9 3-3.3 3-2.1 0-2.4-1.5-3.4-1.9-.7-.2-1.3.1-1.8.8z"/>"#
);
const VITE: &str = stroked!(
    r#"<path d="M8 14.6 1.6 3.3l5-.9L8 1.4l1.4 1 5 .9z"/><path d="M9.6 4.6 6.4 9.1h2.2l-.7 3.1 3.3-4.7H9z"/>"#
);
const DOCKER: &str = stroked!(
    r#"<path d="M2 8.4h10.2c0 2.6-1.6 4.4-4.4 4.4-3 0-5.3-1.6-5.8-4.4z"/><path d="M4.3 8.4V6.2h2.1v2.2M6.9 8.4V6.2H9v2.2M6.9 5.7V3.5H9v2.2M9.5 8.4V6.2h2.1v2.2M12.2 7.6c.9-.6 1.6-.5 2.2-.2"/>"#
);
const DATABASE: &str = stroked!(
    r#"<ellipse cx="8" cy="4" rx="5.2" ry="2.1"/><path d="M2.8 4v8c0 1.2 2.3 2.1 5.2 2.1s5.2-.9 5.2-2.1V4M2.8 8c0 1.2 2.3 2.1 5.2 2.1s5.2-.9 5.2-2.1"/>"#
);
const CODE: &str = stroked!(r#"<path d="M5.6 4.4 2 8l3.6 3.6M10.4 4.4 14 8l-3.6 3.6"/>"#);

/// The glyph for a `Stack:` token.
///
/// Unknown tokens fall back to a generic code mark rather than to nothing: a
/// note that declares a stack should look like it has one even when Den has
/// never heard of the tool.
pub fn stack(token: &str) -> &'static str {
    match token.trim().to_ascii_lowercase().as_str() {
        "rust" | "cargo" => RUST,
        "typescript" | "ts" | "javascript" | "js" | "node" | "deno" | "bun" => CODE,
        "lua" | "neovim" | "nvim" => LUA,
        "react" | "preact" | "next" | "nextjs" => REACT,
        "tailwind" | "tailwindcss" | "css" => TAILWIND,
        "vite" | "esbuild" | "rollup" | "webpack" => VITE,
        "docker" | "podman" | "compose" => DOCKER,
        "postgres" | "postgresql" | "sqlite" | "mysql" | "redis" => DATABASE,
        _ => CODE,
    }
}

/// The glyph for a note kind.
pub fn kind(kind: den_core::vault::Kind) -> &'static str {
    use den_core::vault::Kind;
    match kind {
        Kind::Inbox => INBOX,
        Kind::Notes => NOTES,
        Kind::Projects => PROJECTS,
        Kind::Daily => DAILY,
    }
}

/// The glyph a note leads with: its primary stack, or its kind.
///
/// A project that says what it is built with gets to show it; a note about a
/// desk keeps the folder. The fallback is what stops this becoming decoration.
pub fn for_entry(entry: &den_core::vault::Entry) -> &'static str {
    match entry.stack().first() {
        Some(token) => stack(token),
        None => kind(entry.kind),
    }
}

/// Renders one glyph at `size`, tinted.
pub fn view<'a, Message: 'a>(
    source: &'static str,
    size: f32,
    color: Color,
) -> iced::Element<'a, Message> {
    svg(svg::Handle::from_memory(source.as_bytes()))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_well_formed_and_tintable() {
        let all = [
            PROJECTS, DAILY, INBOX, NOTES, SEARCH, CLOCK, DUE, VAULT, LOCK, NVIM, PLUS, THEME,
            RUST, LUA, REACT, TAILWIND, VITE, DOCKER, DATABASE, CODE,
        ];
        for source in all {
            assert!(source.starts_with("<svg"), "not an svg: {source}");
            assert!(source.ends_with("</svg>"));
            // Tinting only works if nothing hardcodes its own colour.
            assert!(
                source.contains("currentColor"),
                "a glyph that names its own colour cannot follow the palette"
            );
            assert!(!source.contains('#'), "no literal hex in a glyph");
        }
    }

    #[test]
    fn an_unknown_stack_token_still_gets_a_mark() {
        assert_eq!(stack("rust"), RUST);
        assert_eq!(stack("  Rust  "), RUST, "tokens arrive untrimmed and cased");
        assert_eq!(stack("brainfuck"), CODE);
        assert_eq!(stack(""), CODE);
    }
}
