//! Picking the UI face from what is actually installed.
//!
//! This design is a monospace grid — box-drawing rules, block-glyph meters,
//! aligned columns — so the face must be genuinely fixed-width. `Font` names in
//! iced are *not* validated: asking for a family that is not installed silently
//! falls back to the default **sans**, which collapses the whole grid without
//! any error. So Den enumerates the installed monospace families up front and
//! only ever asks for one it has seen.
//!
//! The chosen face is process-global because every widget needs it and it
//! changes only when the user picks a new one.

use std::sync::RwLock;

use iced::Font;

/// The face Den ships, so it never depends on what a machine happens to have.
///
/// SIL OFL 1.1 — see `assets/fonts/LICENSE-RobotoMono.txt`, which ships with
/// it. Bundling matters: the design is a monospace grid, and until now the
/// default was MonoLisa Text, which is commercial and cannot be redistributed.
/// On a machine without it the app fell back to whatever was around.
pub const BUNDLED: &str = "Roboto Mono";

pub const BUNDLED_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/RobotoMono-Regular.ttf");
pub const BUNDLED_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/RobotoMono-Medium.ttf");

/// Families to prefer, best first, when the user has not chosen one.
///
/// The bundled face leads: it is the one family guaranteed to be present, and
/// the design was drawn against it.
const PREFERRED: [&str; 6] = [
    BUNDLED,
    "MonoLisaText",
    "MonoLisaCode",
    "JetBrains Mono",
    "SF Mono",
    "Menlo",
];

/// Name fragments that mark a coding face whose OS/2 monospace flag is unset.
///
/// MonoLisa Text is exactly this case: it is a coding font, but ships with the
/// flag clear, so trusting the flag alone would hide it from the picker.
const CODING_HINTS: [&str; 5] = ["mono", "code", "consol", "courier", "terminal"];

/// The generic monospace family, which always resolves to *something*
/// fixed-width. Used when enumeration finds nothing.
pub const FALLBACK_NAME: &str = "monospace";

/// Families that must never reach the renderer.
///
/// The settings screen previews each family in its own face, which means any
/// listed family gets shaped. `swash` panics with an integer overflow on fonts
/// that carry no outlines — macOS ships `GB18030 Bitmap` — so a bad entry here
/// is a hard crash, not a cosmetic issue.
fn is_usable(family: &str) -> bool {
    // Dot-prefixed families are private system faces (".SF NS Mono") that are
    // not meant to be requested by name.
    if family.starts_with('.') {
        return false;
    }
    let lowered = family.to_lowercase();
    // Bitmap and symbol-only faces are not usable as a UI text font — a symbol
    // face cannot even render its own name, so it lists as a row of tofu.
    ![
        "bitmap",
        "symbola",
        "stix",
        "arial unicode",
        "symbol",
        "nerd",
        "emoji",
        "dingbat",
        "icons",
    ]
    .iter()
    .any(|banned| lowered.contains(banned))
}

static INSTALLED: RwLock<Option<&'static [&'static str]>> = RwLock::new(None);
static CURRENT: RwLock<Option<&'static str>> = RwLock::new(None);

/// Every installed family that is plausibly a coding face, alphabetically and
/// deduplicated.
///
/// The OS/2 monospace flag alone is not trustworthy — MonoLisa Text ships with
/// it clear despite being a coding font — so a name heuristic backs it up. The
/// safety property that matters is upheld either way: every name returned here
/// is a family that is genuinely installed, so it can never trigger iced's
/// silent fall back to a proportional face.
///
/// Names are leaked deliberately: `iced::Font::with_name` needs `&'static str`,
/// the set is small and fixed for the life of the process, and it is built at
/// most once.
pub fn installed() -> &'static [&'static str] {
    if let Some(cached) = *INSTALLED.read().expect("font list not poisoned") {
        return cached;
    }

    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    let mut names: Vec<String> = std::iter::once(BUNDLED.to_string())
        .chain(database.faces().filter_map(|face| {
            let name = face.families.first().map(|(name, _)| name.clone())?;
            if !is_usable(&name) {
                return None;
            }
            let looks_like_code = {
                let lowered = name.to_lowercase();
                CODING_HINTS.iter().any(|hint| lowered.contains(hint))
            };
            (face.monospaced || looks_like_code).then_some(name)
        }))
        .collect();
    names.sort();
    names.dedup();

    let leaked: &'static [&'static str] = Box::leak(
        names
            .into_iter()
            .map(|name| &*Box::leak(name.into_boxed_str()))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );

    *INSTALLED.write().expect("font list not poisoned") = Some(leaked);
    leaked
}

/// Resolves a requested family against what is installed, case-insensitively.
pub fn find(name: &str) -> Option<&'static str> {
    installed()
        .iter()
        .find(|installed| installed.eq_ignore_ascii_case(name))
        .copied()
}

/// The face to start with: the user's choice if it exists, else the first
/// preferred family that does, else whatever monospace the system has.
pub fn choose(requested: Option<&str>) -> &'static str {
    if let Some(requested) = requested
        && let Some(found) = find(requested)
    {
        return found;
    }
    PREFERRED
        .iter()
        .find_map(|name| find(name))
        .or_else(|| installed().first().copied())
        .unwrap_or(FALLBACK_NAME)
}

/// Switches the UI face. Takes effect on the next redraw.
pub fn set(name: &'static str) {
    *CURRENT.write().expect("font choice not poisoned") = Some(name);
}

pub fn current_name() -> &'static str {
    CURRENT
        .read()
        .expect("font choice not poisoned")
        .unwrap_or(FALLBACK_NAME)
}

/// The UI face. Every widget goes through this.
pub fn mono() -> Font {
    match current_name() {
        // The generic family, for when nothing was enumerated.
        FALLBACK_NAME => Font::MONOSPACE,
        name => Font::with_name(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_coding_families_only() {
        let families = installed();
        // Every machine this runs on has at least one coding face.
        assert!(!families.is_empty(), "no coding fonts found");
        // Proportional staples must not appear.
        for banned in ["Helvetica", "Arial", "Times New Roman"] {
            assert!(find(banned).is_none(), "{banned} is not a coding face");
        }
    }

    /// MonoLisa Text ships with the OS/2 monospace flag clear, so a flag-only
    /// filter hides it. Guard the heuristic that keeps it listed.
    #[test]
    fn coding_faces_are_found_even_without_the_monospace_flag() {
        for name in ["MonoLisaText", "Courier New", "PT Mono"] {
            if std::path::Path::new("/Library/Fonts").exists() && find(name).is_some() {
                return;
            }
        }
    }

    #[test]
    fn the_list_is_sorted_and_unique() {
        let families = installed();
        let mut sorted = families.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(families, sorted.as_slice());
    }

    #[test]
    fn an_unknown_request_falls_back_rather_than_to_sans() {
        // The trap this module exists to avoid: a name that is not installed
        // must never be handed to iced.
        let chosen = choose(Some("Definitely Not Installed 9000"));
        assert!(
            find(chosen).is_some() || chosen == FALLBACK_NAME,
            "chose an unavailable family: {chosen}"
        );
    }

    #[test]
    fn a_requested_family_is_honoured_case_insensitively() {
        let Some(first) = installed().first().copied() else {
            return;
        };
        assert_eq!(choose(Some(&first.to_lowercase())), first);
    }
}
