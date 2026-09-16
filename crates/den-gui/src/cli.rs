//! Command-line options.

use std::path::PathBuf;

use crate::screen::Kind;
use crate::theme::{Accent, Variant};

pub const USAGE: &str = "\
Den — a desktop client for the den.nvim Markdown vault

USAGE:
    den [OPTIONS]

OPTIONS:
    --vault <PATH>     Vault root (default: ~/Notes/den)
    --nvim <SOCKET>    Neovim RPC socket to attach to. Optional.
                       Start Neovim with `nvim --listen <SOCKET>`.
    --demo             Open a throwaway vault with the demo project in it
    --view <NAME>      today | tasks | roadmap | settings
    --tasks <NAME>     board | table — which reading the tasks screen opens on
    --theme <NAME>     ember | ember-soft | ember-light | ember-lighter
    --mode <NAME>      auto | dark | light. Auto follows the system (default).
    --accent <NAME>    coral | gold | steel | olive
    --font <NAME>      Monospace family to draw with; --font list shows what
                       is installed. Defaults to MonoLisa Text if present.
    --scale <FACTOR>   UI zoom, 0.8-2.0 (default 1.15). ⌘+ / ⌘- adjust it live.
    -h, --help         Show this message

Den reads and writes the vault itself, so --nvim is never required. Attaching
to Neovim adds one thing: Den can see which buffers have unsaved changes, and
refuses to write those files rather than clobbering your edits. Unattached,
that check is unavailable — a write is still refused if the file changed on
disk since Den last read it.
";

#[derive(Debug, Clone)]
pub struct Options {
    pub root: PathBuf,
    pub socket: Option<PathBuf>,
    pub screen: Kind,
    pub variant: Variant,
    pub mode: crate::theme::Mode,
    pub task_view: crate::screen::TaskView,
    pub accent: Accent,
    /// Resolved against the installed monospace families at startup.
    pub font: &'static str,
    /// UI zoom; see `Den::scale`.
    pub scale: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            root: default_root(),
            socket: None,
            screen: Kind::default(),
            variant: Variant::default(),
            mode: crate::theme::Mode::default(),
            task_view: crate::screen::TaskView::default(),
            accent: Accent::default(),
            font: crate::fonts::choose(None),
            scale: DEFAULT_SCALE,
        }
    }
}

/// A modest bump over the design's own sizes, which were drawn for a mockup
/// viewed scaled down. Much past this and the columnar views start wrapping.
pub const DEFAULT_SCALE: f32 = 1.15;

/// den.nvim's own default, from `lua/den/init.lua`.
pub fn default_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Notes/den")
}

pub enum Outcome {
    Run(Options),
    ShowUsage,
    /// `--font list`: print the installed monospace families and exit.
    ListFonts,
    Invalid(String),
}

pub fn parse(arguments: impl Iterator<Item = String>) -> Outcome {
    let mut options = Options::default();
    let mut demo = false;
    let mut font = None;
    let mut arguments = arguments.peekable();

    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| -> Result<String, String> {
            arguments
                .next()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match argument.as_str() {
            "-h" | "--help" => return Outcome::ShowUsage,
            "--demo" => demo = true,
            "--vault" => match value("--vault") {
                Ok(path) => options.root = expand(&path),
                Err(error) => return Outcome::Invalid(error),
            },
            "--nvim" => match value("--nvim") {
                Ok(path) => options.socket = Some(expand(&path)),
                Err(error) => return Outcome::Invalid(error),
            },
            "--view" => match value("--view") {
                Ok(name) => match Kind::parse(&name) {
                    Some(kind) => options.screen = kind,
                    None => return Outcome::Invalid(format!("unknown view {name:?}")),
                },
                Err(error) => return Outcome::Invalid(error),
            },
            "--tasks" => match value("--tasks") {
                Ok(name) => match crate::screen::TaskView::parse(&name) {
                    Some(view) => options.task_view = view,
                    None => return Outcome::Invalid(format!("unknown tasks view {name:?}")),
                },
                Err(error) => return Outcome::Invalid(error),
            },
            "--mode" => match value("--mode") {
                Ok(name) => match crate::theme::Mode::parse(&name) {
                    Some(mode) => options.mode = mode,
                    None => return Outcome::Invalid(format!("unknown mode {name:?}")),
                },
                Err(error) => return Outcome::Invalid(error),
            },
            "--theme" => match value("--theme") {
                Ok(name) => match Variant::parse(&name) {
                    Some(variant) => {
                        options.variant = variant;
                        // Naming a variant pins it: asking for ember-light and
                        // being handed ember because the system is dark would
                        // be baffling.
                        options.mode = if variant.is_dark() {
                            crate::theme::Mode::Dark
                        } else {
                            crate::theme::Mode::Light
                        };
                    }
                    None => return Outcome::Invalid(format!("unknown theme {name:?}")),
                },
                Err(error) => return Outcome::Invalid(error),
            },
            "--font" => match value("--font") {
                Ok(name) if name == "list" => return Outcome::ListFonts,
                Ok(name) => match crate::fonts::find(&name) {
                    Some(found) => font = Some(found),
                    None => {
                        // Never hand iced a family it does not have: it would
                        // silently fall back to a proportional face.
                        return Outcome::Invalid(format!(
                            "{name:?} is not an installed monospace font; try --font list"
                        ));
                    }
                },
                Err(error) => return Outcome::Invalid(error),
            },
            "--scale" => match value("--scale") {
                Ok(raw) => match raw.parse::<f32>() {
                    Ok(scale) if (0.8..=2.0).contains(&scale) => options.scale = scale,
                    _ => return Outcome::Invalid(format!("--scale must be 0.8-2.0, got {raw:?}")),
                },
                Err(error) => return Outcome::Invalid(error),
            },
            "--accent" => match value("--accent") {
                Ok(name) => match Accent::parse(&name) {
                    Some(accent) => options.accent = accent,
                    None => return Outcome::Invalid(format!("unknown accent {name:?}")),
                },
                Err(error) => return Outcome::Invalid(error),
            },
            other => return Outcome::Invalid(format!("unknown option {other:?}")),
        }
    }

    options.font = crate::fonts::choose(font);

    if demo {
        match demo_vault() {
            Ok(root) => options.root = root,
            Err(error) => return Outcome::Invalid(format!("cannot prepare demo vault: {error}")),
        }
    }

    Outcome::Run(options)
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None => PathBuf::from(path),
    }
}

/// The bundled demo project, written into a scratch vault.
///
/// It lives outside any real vault on purpose: den.nvim's `AGENTS.md` is
/// explicit that development must never touch a user's notes.
fn demo_vault() -> std::io::Result<PathBuf> {
    const DEMO: [(&str, &str); 3] = [
        ("website.md", include_str!("../../../demo/website.md")),
        ("studio.md", include_str!("../../../demo/studio.md")),
        ("plugin.md", include_str!("../../../demo/plugin.md")),
    ];

    let root = std::env::temp_dir().join("den-desktop-demo");
    let projects = root.join("projects");
    std::fs::create_dir_all(&projects)?;
    for (name, content) in DEMO {
        // Rewritten on each launch so the demo always starts from a known state.
        std::fs::write(projects.join(name), content)?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Options {
        match parse(args.iter().map(|a| a.to_string())) {
            Outcome::Run(options) => options,
            Outcome::ShowUsage | Outcome::ListFonts => panic!("unexpected early exit"),
            Outcome::Invalid(error) => panic!("unexpected error: {error}"),
        }
    }

    #[test]
    fn defaults_match_den_nvim() {
        let options = Options::default();
        assert!(options.root.ends_with("Notes/den"));
        assert!(options.socket.is_none());
        assert_eq!(options.screen, Kind::Today);
        assert_eq!(options.variant, Variant::Ember);
    }

    #[test]
    fn reads_every_option() {
        let options = parse_args(&[
            "--vault",
            "/tmp/v",
            "--nvim",
            "/tmp/s",
            "--view",
            "tasks",
            "--theme",
            "ember-light",
            "--accent",
            "steel",
        ]);
        assert_eq!(options.root, PathBuf::from("/tmp/v"));
        assert_eq!(options.socket, Some(PathBuf::from("/tmp/s")));
        assert_eq!(options.screen, Kind::Tasks);
        assert_eq!(options.variant, Variant::EmberLight);
        assert_eq!(options.accent, Accent::Steel);
    }

    #[test]
    fn rejects_unknown_values() {
        assert!(matches!(
            parse(["--view", "gantt"].iter().map(|a| a.to_string())),
            Outcome::Invalid(_)
        ));
        assert!(matches!(
            parse(["--vault"].iter().map(|a| a.to_string())),
            Outcome::Invalid(_)
        ));
    }

    #[test]
    fn demo_vault_is_loadable() {
        let root = demo_vault().expect("demo vault");
        let vault = den_core::vault::Vault::load(&root);
        assert_eq!(vault.entries.len(), 3, "every demo project loads");

        let counts = vault.counts();
        assert_eq!(counts.total, 11, "every demo task loads");
        assert_eq!(counts.done, 3);
        assert_eq!(counts.doing, 3);
        assert_eq!(counts.backlog, 5);

        // The tag set and counts the design draws in the sidebar.
        let ranked = vault.tags.ranked();
        let counts: std::collections::BTreeMap<&str, usize> = ranked
            .iter()
            .map(|(tag, _, count)| (*tag, *count))
            .collect();
        for (tag, expected) in [
            ("studio", 3),
            ("design", 2),
            ("writing", 2),
            ("admin", 2),
            ("deep-work", 2),
            ("research", 2),
            ("web", 1),
            ("rust", 2),
        ] {
            assert_eq!(counts.get(tag), Some(&expected), "count for #{tag}");
        }
    }
}
