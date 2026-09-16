//! Keybindings, with vim-shaped defaults and a config file to override them.
//!
//! Den's users are Neovim users, so the defaults are the motions they already
//! have in their fingers: `g` as a go-to prefix (as in `gg`, `gt`), `j`/`k` to
//! move, `/` to search, `space` to act on the selection. Nothing here is
//! hardcoded into the update loop — the loop asks this map what a key means,
//! which is what makes the bindings replaceable rather than merely documented.
//!
//! Overrides live at `~/.config/den/keymap.json` (`$XDG_CONFIG_HOME` honoured),
//! because a vim user expects their config under `~/.config`. Anything the file
//! does not mention keeps its default, so a partial file is valid and a
//! malformed one degrades to the defaults rather than leaving Den unusable.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::screen::Kind;

/// Everything a key can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Show(Kind),
    NextTask,
    PreviousTask,
    ToggleTask,
    OpenInEditor,
    Capture,
    Edit,
    TaskView(crate::screen::TaskView),
    Search,
    Reload,
    CommandPalette,
    ToggleSidebar,
    ToggleRail,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Cancel,
}

impl Action {
    /// The name used in the config file and shown in Settings.
    pub fn name(self) -> String {
        match self {
            Action::Show(kind) => format!("show.{}", kind.as_str()),
            Action::NextTask => "task.next".into(),
            Action::PreviousTask => "task.previous".into(),
            Action::ToggleTask => "task.toggle".into(),
            Action::OpenInEditor => "task.open".into(),
            Action::Capture => "capture".into(),
            Action::Edit => "task.edit".into(),
            Action::TaskView(view) => format!("tasks.{}", view.as_str()),
            Action::Search => "search".into(),
            Action::Reload => "reload".into(),
            Action::CommandPalette => "palette".into(),
            Action::ToggleSidebar => "panel.sidebar".into(),
            Action::ToggleRail => "panel.rail".into(),
            Action::ZoomIn => "zoom.in".into(),
            Action::ZoomOut => "zoom.out".into(),
            Action::ZoomReset => "zoom.reset".into(),
            Action::Cancel => "cancel".into(),
        }
    }

    fn parse(name: &str) -> Option<Action> {
        if let Some(screen) = name.strip_prefix("show.") {
            return Kind::parse(screen).map(Action::Show);
        }
        if let Some(view) = name.strip_prefix("tasks.") {
            return crate::screen::TaskView::parse(view).map(Action::TaskView);
        }
        Some(match name {
            "task.next" => Action::NextTask,
            "task.previous" => Action::PreviousTask,
            "task.toggle" => Action::ToggleTask,
            "task.open" => Action::OpenInEditor,
            "capture" => Action::Capture,
            "task.edit" => Action::Edit,
            "search" => Action::Search,
            "reload" => Action::Reload,
            "palette" => Action::CommandPalette,
            "panel.sidebar" => Action::ToggleSidebar,
            "panel.rail" => Action::ToggleRail,
            "zoom.in" => Action::ZoomIn,
            "zoom.out" => Action::ZoomOut,
            "zoom.reset" => Action::ZoomReset,
            "cancel" => Action::Cancel,
            _ => return None,
        })
    }

    /// What Settings shows beside the keys.
    pub fn describe(self) -> String {
        match self {
            Action::Show(kind) => format!("go to {}", kind.as_str()),
            Action::NextTask => "next task".into(),
            Action::PreviousTask => "previous task".into(),
            Action::ToggleTask => "toggle done".into(),
            Action::OpenInEditor => "open in Neovim".into(),
            Action::Capture => "capture a thought".into(),
            Action::Edit => "rename the selected task".into(),
            Action::TaskView(view) => format!("tasks as {}", view.as_str()),
            Action::Search => "search".into(),
            Action::Reload => "reload the vault".into(),
            Action::CommandPalette => "command palette".into(),
            Action::ToggleSidebar => "toggle the sidebar".into(),
            Action::ToggleRail => "toggle the task rail".into(),
            Action::ZoomIn => "zoom in".into(),
            Action::ZoomOut => "zoom out".into(),
            Action::ZoomReset => "reset zoom".into(),
            Action::Cancel => "close / clear filters".into(),
        }
    }
}

/// One binding: either a single chord, or a prefix followed by a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// `cmd` modifier, as in `cmd+k`.
    pub command: bool,
    /// A leading key held as a prefix, as in vim's `g`.
    pub prefix: Option<char>,
    /// The key that completes the binding.
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Enter,
    Escape,
}

impl Binding {
    /// Parses `g h`, `cmd+k`, `space`, `enter`, `/`.
    pub fn parse(spec: &str) -> Option<Binding> {
        let spec = spec.trim();
        let (command, rest) = match spec.strip_prefix("cmd+") {
            Some(rest) => (true, rest),
            None => (false, spec),
        };

        let parts: Vec<&str> = rest.split_whitespace().collect();
        let (prefix, last) = match parts.as_slice() {
            [single] => (None, *single),
            [prefix, last] => (prefix.chars().next(), *last),
            _ => return None,
        };

        let key = match last {
            "space" => Key::Space,
            "enter" | "cr" | "return" => Key::Enter,
            "esc" | "escape" => Key::Escape,
            other => Key::Char(other.chars().next()?),
        };
        Some(Binding {
            command,
            prefix,
            key,
        })
    }

    /// How the binding is written in the config and shown in Settings.
    pub fn spec(&self) -> String {
        let key = match self.key {
            Key::Char(c) => c.to_string(),
            Key::Space => "space".into(),
            Key::Enter => "enter".into(),
            Key::Escape => "esc".into(),
        };
        let body = match self.prefix {
            Some(prefix) => format!("{prefix} {key}"),
            None => key,
        };
        if self.command {
            format!("cmd+{body}")
        } else {
            body
        }
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<(Binding, Action)>,
    /// Where an override was read from, for Settings to show.
    source: Option<PathBuf>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap {
            bindings: Keymap::defaults(),
            source: None,
        }
    }
}

impl Keymap {
    /// The vim-shaped defaults: `g` to go somewhere, `j`/`k` to move, `/` to
    /// search, `space` to act.
    fn defaults() -> Vec<(Binding, Action)> {
        let mut bindings = Vec::new();
        let bind =
            |spec: &str, action: Action| (Binding::parse(spec).expect("valid default"), action);

        for kind in [Kind::Today, Kind::Tasks, Kind::Roadmap, Kind::Settings] {
            bindings.push(bind(&format!("g {}", kind.shortcut()), Action::Show(kind)));
        }

        bindings.extend([
            bind("j", Action::NextTask),
            bind("k", Action::PreviousTask),
            bind("space", Action::ToggleTask),
            bind("enter", Action::OpenInEditor),
            // `o` is vim's "open a line below", which is exactly what this does.
            // Two ways into one screen: `g b` for the board, `g l` for the
            // table. They are readings of the same tasks, not destinations.
            bind("g b", Action::TaskView(crate::screen::TaskView::Board)),
            bind("g l", Action::TaskView(crate::screen::TaskView::Table)),
            bind("o", Action::Capture),
            // `i` is vim's insert, on the selected task.
            bind("i", Action::Edit),
            bind("/", Action::Search),
            bind("esc", Action::Cancel),
            bind("cmd+k", Action::CommandPalette),
            bind("cmd+r", Action::Reload),
            bind("cmd+b", Action::ToggleSidebar),
            bind("cmd+i", Action::ToggleRail),
            bind("cmd+=", Action::ZoomIn),
            bind("cmd+-", Action::ZoomOut),
            bind("cmd+0", Action::ZoomReset),
        ]);
        bindings
    }

    /// Loads the user's overrides, falling back to the defaults.
    ///
    /// A missing file is the normal case. A malformed one is reported through
    /// the returned error but still yields a usable map — bad config should
    /// never leave the app without keys.
    pub fn load() -> (Keymap, Option<String>) {
        let mut keymap = Keymap::default();
        let Some(path) = Keymap::path() else {
            return (keymap, None);
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return (keymap, None);
        };

        let parsed: Result<BTreeMap<String, String>, _> = serde_json::from_str(&raw);
        let Ok(overrides) = parsed else {
            return (keymap, Some(format!("{}: not valid JSON", path.display())));
        };

        let mut problems = Vec::new();
        for (action_name, spec) in overrides {
            let Some(action) = Action::parse(&action_name) else {
                problems.push(format!("unknown action {action_name:?}"));
                continue;
            };
            let Some(binding) = Binding::parse(&spec) else {
                problems.push(format!("unreadable binding {spec:?}"));
                continue;
            };
            keymap.bindings.retain(|(_, existing)| *existing != action);
            keymap.bindings.push((binding, action));
        }

        keymap.source = Some(path);
        let report = (!problems.is_empty()).then(|| format!("keymap: {}", problems.join("; ")));
        (keymap, report)
    }

    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("den/keymap.json"))
    }

    pub fn source(&self) -> Option<&PathBuf> {
        self.source.as_ref()
    }

    /// The action for a completed chord, if any.
    pub fn action(&self, command: bool, prefix: Option<char>, key: &Key) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(binding, _)| {
                binding.command == command && binding.prefix == prefix && binding.key == *key
            })
            .map(|(_, action)| *action)
    }

    /// True when this key starts a multi-key binding, so the next key should be
    /// held for it rather than acted on.
    pub fn is_prefix(&self, command: bool, key: &Key) -> bool {
        let Key::Char(c) = key else {
            return false;
        };
        !command
            && self
                .bindings
                .iter()
                .any(|(binding, _)| binding.prefix == Some(*c))
    }

    /// Bindings in display order, for Settings.
    pub fn listing(&self) -> Vec<(String, String)> {
        self.bindings
            .iter()
            .map(|(binding, action)| (binding.spec(), action.describe()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_vim_shaped() {
        let keymap = Keymap::default();
        assert_eq!(
            keymap.action(false, Some('g'), &Key::Char('k')),
            Some(Action::Show(Kind::Tasks))
        );
        // `g b` and `g l` are two readings of the tasks screen, not two places.
        assert_eq!(
            keymap.action(false, Some('g'), &Key::Char('b')),
            Some(Action::TaskView(crate::screen::TaskView::Board))
        );
        assert_eq!(
            keymap.action(false, Some('g'), &Key::Char('l')),
            Some(Action::TaskView(crate::screen::TaskView::Table))
        );
        assert_eq!(
            keymap.action(false, None, &Key::Char('j')),
            Some(Action::NextTask)
        );
        assert_eq!(
            keymap.action(false, None, &Key::Space),
            Some(Action::ToggleTask)
        );
        assert_eq!(
            keymap.action(false, None, &Key::Char('/')),
            Some(Action::Search)
        );
        assert_eq!(
            keymap.action(true, None, &Key::Char('k')),
            Some(Action::CommandPalette)
        );
        // `g` opens a chord, so it must not act on its own.
        assert!(keymap.is_prefix(false, &Key::Char('g')));
        assert!(!keymap.is_prefix(false, &Key::Char('j')));
    }

    #[test]
    fn specs_round_trip() {
        for spec in ["g h", "cmd+k", "space", "enter", "esc", "/", "j"] {
            let binding = Binding::parse(spec).expect(spec);
            assert_eq!(binding.spec(), spec, "{spec} did not round-trip");
        }
    }

    #[test]
    fn an_override_replaces_only_its_own_action() {
        let mut keymap = Keymap::default();
        let before = keymap.bindings.len();

        // What `load` does for one entry.
        let action = Action::Show(Kind::Tasks);
        keymap.bindings.retain(|(_, existing)| *existing != action);
        keymap
            .bindings
            .push((Binding::parse("cmd+2").expect("valid"), action));

        assert_eq!(
            keymap.bindings.len(),
            before,
            "a rebind must not add a binding"
        );
        assert_eq!(keymap.action(true, None, &Key::Char('2')), Some(action));
        assert_eq!(keymap.action(false, Some('g'), &Key::Char('k')), None);
        // Everything else is untouched.
        assert_eq!(
            keymap.action(false, None, &Key::Char('j')),
            Some(Action::NextTask)
        );
    }

    #[test]
    fn unparseable_specs_are_rejected_rather_than_guessed() {
        assert!(Binding::parse("").is_none());
        assert!(Binding::parse("g h j").is_none());
        assert!(Action::parse("show.nonsense").is_none());
        assert!(Action::parse("tasks.nonsense").is_none());
        assert!(Action::parse("wat").is_none());
    }
}
