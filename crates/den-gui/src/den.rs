//! The root: shared state, the message loop, and the screen it is showing.
//!
//! Screens that render straight from this state are unit variants of
//! [`Screen`]; [`settings::Settings`] owns state of its own and reports changes
//! back as [`settings::Action`]s, which is where they get applied.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::{Subscription, Task};
use jiff::civil::Date;

use crate::editor::Editor;
use crate::screen::{Kind, Screen, TaskView, settings};
use crate::theme::{Accent, Palette, Variant};
use den_core::history::History;
use den_core::index::Index;
use den_core::mutate::{self, Expected};
use den_core::vault::{Status, Task as VaultTask, Vault};

/// How often we stat the vault for changes. Files are the store, so this is
/// the only thing Den watches.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const FOCUS_SESSION: Duration = Duration::from_secs(25 * 60);

pub const SEARCH_INPUT: &str = "den-search";
pub const CAPTURE_INPUT: &str = "den-capture";
pub const EDIT_INPUT: &str = "den-edit";

/// Points at a task by file and line, which is how den.nvim's bridge addresses
/// them too.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
}

impl Location {
    pub fn of(task: &VaultTask) -> Location {
        Location {
            file: task.file.clone(),
            line: task.line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DueFilter {
    #[default]
    Any,
    Overdue,
    Today,
    Week,
    Undated,
}

impl DueFilter {
    pub const ALL: [DueFilter; 5] = [
        DueFilter::Any,
        DueFilter::Overdue,
        DueFilter::Today,
        DueFilter::Week,
        DueFilter::Undated,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DueFilter::Any => "due",
            DueFilter::Overdue => "overdue",
            DueFilter::Today => "today",
            DueFilter::Week => "this week",
            DueFilter::Undated => "no date",
        }
    }

    fn matches(self, task: &VaultTask, today: Date) -> bool {
        let due = task.due().and_then(den_core::date::parse);
        match (self, due) {
            (DueFilter::Any, _) => true,
            (DueFilter::Undated, due) => due.is_none(),
            (_, None) => false,
            (DueFilter::Overdue, Some(due)) => den_core::date::days_between(today, due) < 0,
            (DueFilter::Today, Some(due)) => due == today,
            (DueFilter::Week, Some(due)) => {
                (0..=7).contains(&den_core::date::days_between(today, due))
            }
        }
    }
}

/// Whether an editor is attached.
///
/// Den writes files itself, so this no longer gates editing — it says whether
/// Den can see unsaved buffers and jump the editor to a task.
#[derive(Debug, Clone)]
pub enum Link {
    /// No `--nvim` socket. Den is standalone; nothing can be dirty.
    Standalone,
    Attached,
    Unreachable,
}

impl Link {
    pub fn label(&self) -> String {
        match self {
            Link::Standalone => "nvim: standalone".to_string(),
            Link::Attached => "nvim: attached".to_string(),
            Link::Unreachable => "nvim: not responding".to_string(),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Link::Attached)
    }
}

/// The 25-minute focus session the design's rail drives.
#[derive(Debug, Clone)]
pub struct Focus {
    pub remaining: Duration,
    pub running: bool,
    pub task: Option<Location>,
    pub logged: Duration,
}

impl Default for Focus {
    fn default() -> Self {
        Focus {
            remaining: FOCUS_SESSION,
            running: false,
            task: None,
            logged: Duration::ZERO,
        }
    }
}

impl Focus {
    pub fn elapsed(&self) -> Duration {
        FOCUS_SESSION.saturating_sub(self.remaining)
    }

    pub fn fraction(&self) -> f32 {
        self.elapsed().as_secs_f32() / FOCUS_SESSION.as_secs_f32()
    }

    pub fn session() -> Duration {
        FOCUS_SESSION
    }
}

/// A transient message across the top of the main area — bridge rejections
/// mostly, which are usually the user's problem to fix rather than a bug.
#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Show(Kind),
    SelectTask(Location),
    FocusEntry(PathBuf),

    QueryChanged(String),
    SearchFocused,

    SetTaskView(TaskView),
    CursorMoved(iced::Point),

    DragStarted(Location),
    DragOver(Status),
    DragDropped,

    EditOpened,
    EditChanged(String),
    EditCommitted,
    EditCancelled,

    CaptureOpened,
    CaptureChanged(String),
    CaptureCommitted,
    CaptureCancelled,
    SetTagFilter(Option<String>),
    SetDueFilter(DueFilter),

    SetVariant(Variant),
    SetAccent(Accent),
    SetFont(&'static str),
    SetScale(f32),
    ToggleSidebar,
    ToggleRail,
    Settings(settings::Message),

    SetStatus(Location, Status),
    Reorder(Location, Direction),
    ToggleSelected,
    OpenInNvim(Location),
    Wrote(Result<(), String>),

    Reload,
    Poll,

    FocusStart,
    FocusPause,
    FocusStop,
    Tick,

    TogglePalette,
    PaletteQuery(String),
    RunCommand(usize),

    DragWindow,
    DismissNotice,
    /// Raw key presses. Shortcut meaning depends on state (is the search box
    /// live? is `g` pending?), and a subscription closure cannot see state —
    /// iced hashes subscriptions by type, so a closure capturing state keeps
    /// running with a stale copy. So the decision happens in `update`.
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

pub struct Den {
    pub palette: Palette,
    /// UI zoom. The design's type sizes were drawn for a mockup viewed scaled
    /// down; at a real 1440pt window they render physically tiny, so the whole
    /// interface is scaled rather than every size being retuned by hand.
    pub scale: f32,
    pub screen: Screen,
    /// Both side panels collapse to a thin strip with a reopen button.
    pub sidebar_open: bool,
    pub rail_open: bool,
    pub vault: Vault,
    /// Probed at load: false only when the vault directory itself refuses
    /// writes. Never a consequence of running without Neovim.
    pub writable: bool,
    /// The full-text index. `None` when it could not be opened — search then
    /// falls back to the substring scan rather than the app refusing to run.
    pub index: Option<Index>,
    pub history: History,
    pub root: PathBuf,
    pub editor: Option<Editor>,
    pub link: Link,

    pub selected: Option<Location>,
    pub home_entry: Option<PathBuf>,

    pub query: String,
    pub search_active: bool,
    /// The capture line, when open. `None` means the status bar shows its
    /// prompt instead of an input.
    pub capture: Option<String>,
    /// The task whose title is being edited, and the draft text.
    pub editing: Option<(Location, String)>,
    /// The card under the pointer during a drag, and the column it is over.
    ///
    /// iced has no drag-and-drop, so this is assembled from `mouse_area`
    /// press/move/release. Holding the target here rather than in the board
    /// screen keeps the board a pure function of state, like every other view.
    /// Where the pointer is, in window coordinates.
    ///
    /// Only meaningful while a drag is running — it is what lets the dragged
    /// card be drawn under the cursor rather than the drag being invisible.
    pub cursor: iced::Point,
    pub drag: Option<Location>,
    pub drop_target: Option<Status>,
    /// Auto, or pinned. In auto the variant is re-resolved on each poll, so a
    /// system appearance change lands without a restart.
    /// Which reading of the tasks screen is showing.
    pub task_view: TaskView,
    pub mode: crate::theme::Mode,
    /// Which variant each side of `Mode::Auto` maps to.
    pub dark_variant: Variant,
    pub light_variant: Variant,
    pub tag_filter: Option<String>,
    pub due_filter: DueFilter,

    pub focus: Focus,
    pub notice: Option<Notice>,
    pub palette_open: bool,
    pub palette_query: String,

    pub today: Date,
    pub loaded_at: Instant,
    /// Set when a prefix key is waiting for the rest of its chord.
    pub pending_goto: bool,
    /// Vim-shaped by default, overridable from `~/.config/den/keymap.json`.
    pub keymap: crate::keymap::Keymap,

    fingerprint: Vec<(PathBuf, u64, u64)>,
}

impl Den {
    pub fn new(options: crate::Options) -> (Den, Task<Message>) {
        crate::fonts::set(options.font);
        let (keymap, keymap_problem) = crate::keymap::Keymap::load();

        let root = options.root;
        let palette = Palette::new(options.variant, options.accent);
        let vault = Vault::load(&root);
        let today = den_core::date::today();

        let mut history = History::load(&root);
        history.observe(&vault, today);
        history.save();

        let editor = options.socket.map(Editor::new);
        let home_entry = default_home_entry(&vault);

        let den = Den {
            palette,
            scale: options.scale,
            sidebar_open: true,
            rail_open: true,
            screen: match options.screen {
                Kind::Settings => Screen::Settings(settings::Settings::new()),
                kind => Den::blank_screen(kind),
            },
            fingerprint: Vault::fingerprint(&root),
            writable: Vault::is_writable(&root),
            index: Index::open(&root).ok(),
            vault,
            history,
            root,
            link: if editor.is_some() {
                Link::Attached
            } else {
                Link::Standalone
            },
            editor,
            selected: None,
            home_entry,
            query: String::new(),
            search_active: false,
            capture: None,
            editing: None,
            cursor: iced::Point::ORIGIN,
            drag: None,
            drop_target: None,
            task_view: options.task_view,
            mode: options.mode,
            dark_variant: if options.variant.is_dark() {
                options.variant
            } else {
                Variant::Ember
            },
            light_variant: if options.variant.is_dark() {
                Variant::EmberLight
            } else {
                options.variant
            },
            tag_filter: None,
            due_filter: DueFilter::Any,
            focus: Focus::default(),
            notice: keymap_problem.map(|text| Notice {
                text,
                is_error: true,
            }),
            palette_open: false,
            palette_query: String::new(),
            today,
            loaded_at: Instant::now(),
            pending_goto: false,
            keymap,
        };

        (den, Task::none())
    }

    pub const MIN_SCALE: f32 = 0.8;
    pub const MAX_SCALE: f32 = 2.0;
    /// The steps the settings screen offers.
    pub const SCALES: [f32; 6] = [1.0, 1.15, 1.25, 1.4, 1.6, 1.8];

    pub fn scale_factor(&self) -> f32 {
        self.scale
    }

    pub fn title(&self) -> String {
        match &self.screen {
            Screen::Settings(settings) => format!("{} — Den", settings.title()),
            _ => format!("Den — {}", self.vault.label()),
        }
    }

    /// The screen currently showing, without its state.
    pub fn kind(&self) -> Kind {
        self.screen.kind()
    }

    fn blank_screen(kind: Kind) -> Screen {
        match kind {
            Kind::Today => Screen::Today,
            Kind::Tasks => Screen::Tasks,
            Kind::Roadmap => Screen::Roadmap,
            Kind::Settings => Screen::Settings(settings::Settings::new()),
        }
    }

    /// Switches screens, keeping an open settings screen's section if it is
    /// already the one being asked for.
    fn show(&mut self, kind: Kind) {
        if self.kind() == kind {
            return;
        }
        self.screen = Den::blank_screen(kind);
    }

    /// The vault root as the command bar shows it: `$HOME` collapsed to `~`,
    /// and anything still long elided to its last two segments. A demo vault
    /// under the system temp directory is otherwise a wall of noise.
    pub fn root_label(&self) -> String {
        let root = self.root.to_string_lossy().into_owned();
        let collapsed = match std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned()) {
            Some(home) if root.starts_with(&home) => format!("~{}", &root[home.len()..]),
            _ => root,
        };
        if collapsed.chars().count() <= 44 {
            return collapsed;
        }
        let tail: Vec<&str> = collapsed.rsplit('/').take(2).collect();
        match tail.len() {
            2 => format!("…/{}/{}", tail[1], tail[0]),
            _ => collapsed,
        }
    }

    pub fn theme(&self) -> iced::Theme {
        self.palette.iced_theme()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Show(kind) => {
                self.show(kind);
                self.pending_goto = false;
            }
            Message::SelectTask(location) => {
                self.selected = Some(location);
            }
            Message::FocusEntry(file) => {
                self.home_entry = Some(file);
                self.show(Kind::Tasks);
            }

            Message::CursorMoved(point) => self.cursor = point,
            Message::SetTaskView(view) => {
                self.task_view = view;
                self.show(Kind::Tasks);
            }

            Message::DragStarted(location) => {
                self.drag = Some(location);
                self.drop_target = None;
            }
            Message::DragOver(status) => {
                if self.drag.is_some() {
                    self.drop_target = Some(status);
                }
            }
            Message::DragDropped => {
                let (Some(location), Some(status)) = (self.drag.take(), self.drop_target.take())
                else {
                    // A release with no target is a cancelled drag, not an error.
                    self.drag = None;
                    self.drop_target = None;
                    return Task::none();
                };
                // Dropping onto the column a card already sits in is a no-op
                // rather than a pointless rewrite of the file.
                if self
                    .vault
                    .task_at(&location.file, location.line)
                    .is_some_and(|task| task.status() == status)
                {
                    return Task::none();
                }
                return self.write_status(&location, status, None);
            }

            Message::EditOpened => {
                let Some(location) = self.selected.clone() else {
                    return Task::none();
                };
                let Some(task) = self.vault.task_at(&location.file, location.line) else {
                    return Task::none();
                };
                self.editing = Some((location, task.title().to_string()));
                return iced::widget::operation::focus(iced::widget::Id::new(EDIT_INPUT));
            }
            Message::EditChanged(text) => {
                if let Some((_, draft)) = &mut self.editing {
                    *draft = text;
                }
            }
            Message::EditCancelled => self.editing = None,
            Message::EditCommitted => {
                let Some((location, text)) = self.editing.take() else {
                    return Task::none();
                };
                return self.retitle_task(&location, &text);
            }

            Message::CaptureOpened => {
                self.capture = Some(String::new());
                return iced::widget::operation::focus(iced::widget::Id::new(CAPTURE_INPUT));
            }
            Message::CaptureChanged(text) => {
                if self.editing.is_some() {
                    self.editing = None;
                } else if self.capture.is_some() {
                    self.capture = Some(text);
                }
            }
            Message::CaptureCancelled => self.capture = None,
            Message::CaptureCommitted => {
                let Some(text) = self.capture.take() else {
                    return Task::none();
                };
                return self.capture_task(&text);
            }

            Message::QueryChanged(query) => self.query = query,
            Message::SearchFocused => self.search_active = true,
            Message::SetTagFilter(tag) => self.tag_filter = tag,
            Message::SetDueFilter(filter) => self.due_filter = filter,

            Message::SetVariant(variant) => {
                if variant.is_dark() {
                    self.dark_variant = variant;
                } else {
                    self.light_variant = variant;
                }
                self.palette = Palette::new(variant, self.palette.accent);
                // The bear mark is drawn per variant, so the window icon in the
                // Dock and switcher follows the theme too.
                if let Some(icon) = crate::icon::window_icon(variant) {
                    return with_window(move |id| iced::window::set_icon(id, icon.clone()));
                }
            }
            Message::SetAccent(accent) => {
                self.palette = Palette::new(self.palette.variant, accent);
            }
            Message::SetFont(name) => crate::fonts::set(name),
            Message::SetScale(scale) => self.scale = scale.clamp(Den::MIN_SCALE, Den::MAX_SCALE),
            Message::ToggleSidebar => self.sidebar_open = !self.sidebar_open,
            Message::ToggleRail => self.rail_open = !self.rail_open,
            Message::Settings(message) => {
                let Screen::Settings(settings) = &mut self.screen else {
                    return Task::none();
                };
                return match settings.update(message) {
                    settings::Action::None => Task::none(),
                    settings::Action::ChangeMode(mode) => {
                        self.mode = mode;
                        let wanted = mode.resolve(self.dark_variant, self.light_variant);
                        self.palette = Palette::new(wanted, self.palette.accent);
                        Task::none()
                    }
                    settings::Action::ChangeVariant(variant) => {
                        self.update(Message::SetVariant(variant))
                    }
                    settings::Action::ChangeAccent(accent) => {
                        self.update(Message::SetAccent(accent))
                    }
                    settings::Action::ChangeFont(name) => self.update(Message::SetFont(name)),
                    settings::Action::Reload => self.update(Message::Reload),
                };
            }

            Message::SetStatus(location, status) => {
                return self.write_status(&location, status, None);
            }
            Message::Reorder(location, direction) => return self.reorder(&location, direction),
            Message::ToggleSelected => {
                if let Some(location) = self.selected.clone()
                    && let Some(task) = self.vault.task_at(&location.file, location.line)
                {
                    let next = match task.status() {
                        Status::Done => Status::Backlog,
                        _ => Status::Done,
                    };
                    return self.write_status(&location, next, None);
                }
            }
            Message::OpenInNvim(location) => {
                let Some(editor) = self.editor.clone() else {
                    self.notice = Some(Notice {
                        text: "No editor attached — start Den with --nvim <socket>.".to_string(),
                        is_error: true,
                    });
                    return Task::none();
                };
                let (path, line) = (location.file.clone(), location.line);
                return Task::perform(async move { editor.open(&path, line) }, Message::Wrote);
            }
            Message::Wrote(result) => {
                match result {
                    Ok(()) => self.notice = None,
                    Err(error) => self.note_error(&error),
                }
                // Either way the file may have moved: re-read rather than
                // trusting what we had.
                return self.reload();
            }

            Message::Reload => return self.reload(),
            Message::Poll => {
                self.follow_system_theme();
                if let Some(index) = &mut self.index {
                    let _ = index.refresh(&self.vault);
                }
                return self.poll();
            }

            Message::FocusStart => {
                self.focus.running = true;
                self.focus.task = self.selected.clone();
            }
            Message::FocusPause => self.focus.running = false,
            Message::FocusStop => {
                self.focus.logged += self.focus.elapsed();
                self.focus = Focus {
                    logged: self.focus.logged,
                    ..Focus::default()
                };
            }
            Message::Tick => {
                if self.focus.running {
                    self.focus.remaining =
                        self.focus.remaining.saturating_sub(Duration::from_secs(1));
                    if self.focus.remaining.is_zero() {
                        self.focus.running = false;
                        self.focus.logged += Focus::session();
                        self.notice = Some(Notice {
                            text: "Focus session finished.".to_string(),
                            is_error: false,
                        });
                    }
                }
            }

            Message::TogglePalette => {
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
            }
            Message::PaletteQuery(query) => self.palette_query = query,
            Message::RunCommand(index) => {
                let command = self.commands().get(index).map(|c| c.message.clone());
                self.palette_open = false;
                self.palette_query.clear();
                if let Some(message) = command {
                    return Task::done(message);
                }
            }

            Message::DragWindow => return with_window(iced::window::drag),
            Message::DismissNotice => self.notice = None,
            Message::Key(key, modifiers) => return self.on_key(key, modifiers),
        }
        Task::none()
    }

    /// Resolves a key press against the current state.
    fn on_key(
        &mut self,
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Task<Message> {
        use crate::keymap::Key as Chord;
        use iced::keyboard::{Key, Modifiers, key::Named};

        let command = modifiers.contains(Modifiers::COMMAND);
        let chord = match key.as_ref() {
            Key::Named(Named::Escape) => Some(Chord::Escape),
            Key::Named(Named::Enter) => Some(Chord::Enter),
            Key::Named(Named::Space) => Some(Chord::Space),
            // `+` and `=` share a key; treat them as one so zoom works either
            // way without the user needing shift.
            Key::Character("+") => Some(Chord::Char('=')),
            Key::Character(text) => text.chars().next().map(Chord::Char),
            _ => None,
        };
        let Some(chord) = chord else {
            return Task::none();
        };

        // Escape steps out of whatever is open, innermost first, before the
        // keymap gets a say — a rebind should never be able to trap the user.
        if chord == Chord::Escape {
            self.pending_goto = false;
            if self.capture.is_some() {
                // Innermost first: an open capture is what Escape means here,
                // and abandoning a half-typed thought must never also wipe the
                // user's filters.
                self.capture = None;
            } else if self.palette_open {
                self.palette_open = false;
            } else if self.search_active {
                // Unmounting the input is what releases focus; iced has no
                // blur operation.
                self.search_active = false;
            } else if self.notice.is_some() {
                self.notice = None;
            } else {
                self.tag_filter = None;
                self.due_filter = DueFilter::Any;
                self.query.clear();
            }
            return Task::none();
        }

        // While typing, every key belongs to the text field except the
        // command-modified ones. Without this, `o` inside a capture would open
        // another capture instead of typing a letter.
        if (self.search_active
            || self.palette_open
            || self.capture.is_some()
            || self.editing.is_some())
            && !command
        {
            if chord == Chord::Enter {
                if self.palette_open {
                    return self.update(Message::RunCommand(0));
                }
                if self.capture.is_some() {
                    return self.update(Message::CaptureCommitted);
                }
                if self.editing.is_some() {
                    return self.update(Message::EditCommitted);
                }
                self.search_active = false;
            }
            return Task::none();
        }

        // A pending prefix consumes the next key, whatever it is.
        let prefix = self.pending_goto.then_some('g');
        if prefix.is_some() {
            self.pending_goto = false;
        }

        if let Some(action) = self.keymap.action(command, prefix, &chord) {
            return self.run(action);
        }
        if prefix.is_none() && self.keymap.is_prefix(command, &chord) {
            self.pending_goto = true;
        }
        Task::none()
    }

    /// Carries out a bound action. Every keybinding routes through here, so a
    /// rebind needs no change anywhere else.
    fn run(&mut self, action: crate::keymap::Action) -> Task<Message> {
        use crate::keymap::Action;

        match action {
            Action::Show(kind) => {
                self.show(kind);
                Task::none()
            }
            Action::TaskView(view) => self.update(Message::SetTaskView(view)),
            Action::NextTask => {
                self.step_selection(1);
                Task::none()
            }
            Action::PreviousTask => {
                self.step_selection(-1);
                Task::none()
            }
            Action::ToggleTask => self.update(Message::ToggleSelected),
            Action::OpenInEditor => match self.selected.clone() {
                Some(location) => self.update(Message::OpenInNvim(location)),
                None => Task::none(),
            },
            Action::Capture => self.update(Message::CaptureOpened),
            Action::Edit => self.update(Message::EditOpened),
            Action::Search => {
                self.search_active = true;
                iced::widget::operation::focus(iced::widget::Id::new(SEARCH_INPUT))
            }
            Action::Reload => self.update(Message::Reload),
            Action::CommandPalette => self.update(Message::TogglePalette),
            Action::ToggleSidebar => self.update(Message::ToggleSidebar),
            Action::ToggleRail => self.update(Message::ToggleRail),
            Action::ZoomIn => self.update(Message::SetScale(self.scale + 0.05)),
            Action::ZoomOut => self.update(Message::SetScale(self.scale - 0.05)),
            Action::ZoomReset => self.update(Message::SetScale(crate::cli::DEFAULT_SCALE)),
            Action::Cancel => Task::none(),
        }
    }

    /// Moves the selection through whatever the current view is listing.
    fn step_selection(&mut self, delta: isize) {
        let ordered: Vec<Location> = match self.kind() {
            Kind::Tasks => [Status::Doing, Status::Backlog, Status::Done]
                .into_iter()
                .flat_map(|status| self.tasks_with_status(status))
                .map(Location::of)
                .collect(),
            _ => self.visible_tasks().into_iter().map(Location::of).collect(),
        };
        if ordered.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|location| ordered.iter().position(|candidate| candidate == location));
        let next = match current {
            Some(index) => (index as isize + delta).rem_euclid(ordered.len() as isize) as usize,
            None if delta < 0 => ordered.len() - 1,
            None => 0,
        };
        self.selected = Some(ordered[next].clone());
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut streams = vec![keyboard()];

        if self.focus.running {
            streams.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick));
        }

        if self.drag.is_some() {
            streams.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorMoved(position))
                }
                // A release anywhere ends the drag, including outside a column.
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::DragDropped),
                _ => None,
            }));
        }

        streams.push(iced::time::every(POLL_INTERVAL).map(|_| Message::Poll));

        Subscription::batch(streams)
    }

    /// Tasks matching the command bar's query and filters, in the order the
    /// design lists them.
    pub fn visible_tasks(&self) -> Vec<&VaultTask> {
        let needle = self.query.trim().to_lowercase();

        // The substring scan answers "a task called this"; the FTS index
        // answers "a note that mentions this". They are different questions and
        // a search box should answer both, so the index widens the result
        // rather than replacing the scan. A failed query degrades to the scan
        // alone — search returning nothing because SQLite hiccuped would be
        // worse than search being merely literal.
        let matching_files: Vec<std::path::PathBuf> = match (&self.index, needle.is_empty()) {
            (Some(index), false) => index.search(&needle).unwrap_or_default(),
            _ => Vec::new(),
        };

        let mut tasks: Vec<&VaultTask> = self
            .vault
            .tasks()
            .filter(|task| self.due_filter.matches(task, self.today))
            .filter(|task| match &self.tag_filter {
                Some(tag) => task.tags().iter().any(|t| t == tag),
                None => true,
            })
            .filter(|task| {
                if needle.is_empty() {
                    return true;
                }
                task.title().to_lowercase().contains(&needle)
                    || task.entry_title.to_lowercase().contains(&needle)
                    || task
                        .tags()
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&needle))
                    || matching_files.iter().any(|file| *file == task.file)
            })
            .collect();
        // The design's table is sorted by task, ascending.
        tasks.sort_by(|a, b| a.title().cmp(b.title()));
        tasks
    }

    pub fn tasks_with_status(&self, status: Status) -> Vec<&VaultTask> {
        let mut tasks: Vec<&VaultTask> = self
            .visible_tasks()
            .into_iter()
            .filter(|t| t.status() == status)
            .collect();
        // Within a column, den.nvim's `@order` decides; ties fall back to file
        // position so the board never reshuffles arbitrarily.
        tasks.sort_by(|a, b| {
            a.parsed
                .order
                .unwrap_or(u64::MAX)
                .cmp(&b.parsed.order.unwrap_or(u64::MAX))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.line.cmp(&b.line))
        });
        tasks
    }

    pub fn selected_task(&self) -> Option<&VaultTask> {
        let location = self.selected.as_ref()?;
        self.vault.task_at(&location.file, location.line)
    }

    /// The entry the home view is showing.
    pub fn home_entry(&self) -> Option<&den_core::vault::Entry> {
        self.home_entry
            .as_ref()
            .and_then(|file| self.vault.entry(file))
            .or_else(|| self.vault.active().next())
    }

    /// True only when the vault directory refuses writes.
    ///
    /// Den owns the files, so this is never a consequence of running without
    /// Neovim — an unattached Den is fully writable. A write can also be
    /// refused for one *specific* file with unsaved changes in an attached
    /// editor; that is reported when it happens rather than disabling the UI.
    pub fn is_read_only(&self) -> bool {
        !self.writable
    }

    // ── writes ──────────────────────────────────────────────────────────────

    /// Moves a task, writing the Markdown ourselves.
    ///
    /// The engine does the editing; an attached editor is only asked which
    /// files it is holding unsaved. That check is what replaced routing writes
    /// through Neovim, and it is the one thing standing between a click here
    /// and a clobbered buffer.
    /// Writes a captured thought into the vault.
    ///
    /// It goes to `inbox/` — the point of a capture is that you do not have to
    /// decide where it belongs yet, and making the user choose a note is the
    /// thing that stops people capturing at all. The inbox note is created if
    /// absent, never overwritten if present.
    /// Re-resolves the palette when the system appearance has moved.
    ///
    /// Polled rather than subscribed: the app already ticks, the check is a
    /// cheap platform query, and it avoids a second subscription whose closure
    /// would capture stale state — the bug that made the first keymap subtly
    /// wrong.
    fn follow_system_theme(&mut self) {
        if self.mode != crate::theme::Mode::Auto {
            return;
        }
        let wanted = self.mode.resolve(self.dark_variant, self.light_variant);
        if wanted != self.palette.variant {
            self.palette = Palette::new(wanted, self.palette.accent);
        }
    }

    /// Rewrites a task's title in place.
    ///
    /// Routed through `den_core::mutate::retitle`, which preserves every
    /// `@token` on the line — the tags, the due date, the id den.nvim assigned.
    /// None of those are Den's to discard because the user edited some words.
    fn retitle_task(&mut self, location: &Location, text: &str) -> Task<Message> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Task::none();
        }
        let Some(entry) = self.vault.entry(&location.file) else {
            return Task::none();
        };

        let (path, expected, line) = (location.file.clone(), entry.content(), location.line);
        let editor = self.editor.clone();

        Task::perform(
            async move {
                let dirty = editor
                    .map(|editor| editor.dirty_files())
                    .unwrap_or_default();
                mutate::retitle(&path, &Expected::of(expected), line, &text, &dirty)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            Message::Wrote,
        )
    }

    fn capture_task(&mut self, text: &str) -> Task<Message> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Task::none();
        }

        let path = match self.inbox_note() {
            Ok(path) => path,
            Err(error) => {
                self.notice = Some(Notice {
                    text: format!("could not open the inbox: {error}"),
                    is_error: true,
                });
                return Task::none();
            }
        };

        let expected = std::fs::read_to_string(&path).unwrap_or_default();
        // One step past the last task in that file, leaving room to reorder
        // between any two without renumbering.
        let order = self
            .vault
            .entry(&path)
            .map(|entry| {
                entry
                    .tasks
                    .iter()
                    .filter_map(|task| task.parsed.order)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
            .saturating_add(1_000_000)
            .min(den_core::vault::MAX_ORDER);
        let id = fresh_id(&text);
        let editor = self.editor.clone();

        Task::perform(
            async move {
                let dirty = editor
                    .map(|editor| editor.dirty_files())
                    .unwrap_or_default();
                mutate::insert_task(
                    &path,
                    &Expected::of(expected),
                    &text,
                    Status::Backlog,
                    Some(order),
                    Some(&id),
                    &dirty,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            },
            Message::Wrote,
        )
    }

    /// The inbox note, created on first capture.
    ///
    /// No-clobber: an existing file is returned untouched, and a new one gets
    /// only a title and the heading `insert_task` files under.
    fn inbox_note(&self) -> std::io::Result<std::path::PathBuf> {
        let directory = self.root.join("inbox");
        if let Ok(listing) = std::fs::read_dir(&directory)
            && let Some(existing) = listing
                .flatten()
                .map(|file| file.path())
                .filter(|path| path.extension().is_some_and(|e| e == "md"))
                .min()
        {
            return Ok(existing);
        }

        std::fs::create_dir_all(&directory)?;
        let path = directory.join("inbox.md");
        if !path.exists() {
            std::fs::write(&path, "# Inbox\n\n## Next actions\n\n")?;
        }
        Ok(path)
    }

    fn write_status(
        &mut self,
        location: &Location,
        status: Status,
        order: Option<u64>,
    ) -> Task<Message> {
        let Some(entry) = self.vault.entry(&location.file) else {
            return Task::none();
        };
        let Some(task) = self.vault.task_at(&location.file, location.line) else {
            return Task::none();
        };
        if task.parsed.invalid_metadata {
            self.notice = Some(Notice {
                text: mutate::Error::InvalidMetadata.to_string(),
                is_error: true,
            });
            return Task::none();
        }

        let (path, expected, line) = (location.file.clone(), entry.content(), location.line);
        let id = task.parsed.id.clone();
        let editor = self.editor.clone();

        Task::perform(
            async move {
                let dirty = editor
                    .map(|editor| editor.dirty_files())
                    .unwrap_or_default();
                mutate::apply(
                    &path,
                    &Expected::of(expected),
                    line,
                    status,
                    order,
                    id.as_deref(),
                    &dirty,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            },
            Message::Wrote,
        )
    }

    /// Moves a task within its column by renumbering `@order`.
    ///
    /// den.nvim stores order as an integer, so we take the midpoint between the
    /// neighbours we are stepping over — the same trick that keeps a list
    /// reorderable without rewriting every sibling.
    fn reorder(&mut self, location: &Location, direction: Direction) -> Task<Message> {
        let Some(task) = self.vault.task_at(&location.file, location.line) else {
            return Task::none();
        };
        let status = task.status();
        let siblings = self.tasks_with_status(status);
        let Some(index) = siblings
            .iter()
            .position(|t| t.line == location.line && t.file == location.file)
        else {
            return Task::none();
        };

        let target = match direction {
            Direction::Up if index > 0 => index - 1,
            Direction::Down if index + 1 < siblings.len() => index + 1,
            _ => return Task::none(),
        };

        let order_of = |task: &VaultTask| task.parsed.order.unwrap_or(0);
        let neighbour = order_of(siblings[target]);
        let beyond = match direction {
            Direction::Up => target
                .checked_sub(1)
                .map(|i| order_of(siblings[i]))
                .unwrap_or(0),
            Direction::Down => siblings
                .get(target + 1)
                .map(|t| order_of(t))
                .unwrap_or_else(|| neighbour.saturating_add(2_000_000)),
        };
        let order = (neighbour / 2 + beyond / 2).clamp(0, den_core::vault::MAX_ORDER);

        self.write_status(location, status, Some(order))
    }

    // ── loading ─────────────────────────────────────────────────────────────

    /// Re-reads the vault from disk. The files are the store, so this is the
    /// only way data enters Den.
    fn reload(&mut self) -> Task<Message> {
        self.fingerprint = Vault::fingerprint(&self.root);
        let mut vault = Vault::load(&self.root);

        if let Some(editor) = &self.editor {
            // Informational: the sidebar marks entries an editor is holding
            // unsaved, so a refused write is never a surprise.
            let dirty = editor.dirty_files();
            self.link = if dirty.is_empty() && !editor.is_reachable() {
                Link::Unreachable
            } else {
                Link::Attached
            };
            vault.mark_modified(&dirty);
        }

        self.adopt(vault);
        Task::none()
    }

    /// Cheap change detection, run on a timer.
    fn poll(&mut self) -> Task<Message> {
        if Vault::fingerprint(&self.root) == self.fingerprint {
            return Task::none();
        }
        self.reload()
    }

    fn adopt(&mut self, vault: Vault) {
        self.today = den_core::date::today();
        self.history.observe(&vault, self.today);
        self.history.save();
        self.vault = vault;
        self.writable = Vault::is_writable(&self.root);
        // Re-index after a reload so search never answers from a stale vault.
        if let Some(index) = &mut self.index {
            let _ = index.refresh(&self.vault);
        }
        self.loaded_at = Instant::now();

        // Drop a selection whose task has gone, and keep the home view pointed
        // at something that still exists.
        if let Some(location) = &self.selected
            && self.vault.task_at(&location.file, location.line).is_none()
        {
            self.selected = None;
        }
        if self
            .home_entry
            .as_ref()
            .is_none_or(|file| self.vault.entry(file).is_none())
        {
            self.home_entry = default_home_entry(&self.vault);
        }
    }

    fn note_error(&mut self, error: &str) {
        self.notice = Some(Notice {
            text: error.to_string(),
            is_error: true,
        });
    }

    // ── command palette ─────────────────────────────────────────────────────

    pub fn commands(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        for kind in [Kind::Today, Kind::Tasks, Kind::Roadmap, Kind::Settings] {
            commands.push(Command {
                label: format!("Go to {}", kind.as_str()),
                hint: format!("g {}", kind.shortcut()),
                message: Message::Show(kind),
            });
        }
        for variant in Variant::ALL {
            commands.push(Command {
                label: format!("Theme: {}", variant.as_str()),
                hint: String::new(),
                message: Message::SetVariant(variant),
            });
        }
        for accent in Accent::ALL {
            commands.push(Command {
                label: format!("Accent: {}", accent.as_str()),
                hint: String::new(),
                message: Message::SetAccent(accent),
            });
        }
        for family in crate::fonts::installed() {
            commands.push(Command {
                label: format!("Font: {family}"),
                hint: String::new(),
                message: Message::SetFont(family),
            });
        }
        for entry in self.vault.active() {
            commands.push(Command {
                label: format!("Open {}", entry.title),
                hint: entry.file_name(),
                message: Message::FocusEntry(entry.file.clone()),
            });
        }

        let needle = self.palette_query.trim().to_lowercase();
        if needle.is_empty() {
            return commands;
        }
        commands.retain(|command| command.label.to_lowercase().contains(&needle));
        commands
    }
}

#[derive(Debug, Clone)]
pub struct Command {
    pub label: String,
    pub hint: String,
    pub message: Message,
}

fn default_home_entry(vault: &Vault) -> Option<PathBuf> {
    // Prefer a project with open work; the home view is built around one.
    vault
        .active()
        .find(|entry| {
            entry.kind == den_core::vault::Kind::Projects && entry.open_tasks().count() > 0
        })
        .or_else(|| vault.active().find(|entry| entry.open_tasks().count() > 0))
        .or_else(|| vault.active().next())
        .map(|entry| entry.file.clone())
}

/// Raw key presses; `Den::on_key` gives them meaning.
fn keyboard() -> Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            Some(Message::Key(key, modifiers))
        }
        _ => None,
    })
}

/// Runs a window task against whichever window is current.
///
/// `Task<Option<T>>::and_then` already drops the `None` case, so this is just
/// the `latest()` lookup plus the action.
fn with_window(
    action: impl Fn(iced::window::Id) -> Task<Message> + Send + 'static,
) -> Task<Message> {
    iced::window::latest().and_then(action)
}

/// A stable-looking id for a captured task.
///
/// den.nvim's ids are slugs, so this keeps that shape — the caption, lowercased
/// and hyphenated — with a short time suffix so two captures of the same words
/// never collide. `change_line` validates the result, so anything malformed is
/// refused rather than written.
fn fresh_id(text: &str) -> String {
    let slug: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("-");

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() % 1_000_000)
        .unwrap_or(0);

    if slug.is_empty() {
        format!("capture-{stamp}")
    } else {
        format!("{slug}-{stamp}")
    }
}

#[cfg(test)]
mod capture_tests {
    use super::fresh_id;

    #[test]
    fn an_id_is_a_slug_with_a_suffix() {
        let id = fresh_id("Draft the homepage story");
        assert!(id.starts_with("draft-the-homepage-"), "{id}");
        // Only characters den.nvim's `@id(...)` accepts.
        assert!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "{id}"
        );
    }

    #[test]
    fn punctuation_and_emptiness_still_yield_a_usable_id() {
        let id = fresh_id("!!! ??? ***");
        assert!(id.starts_with("capture-"), "{id}");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        // Unicode must not leak into an id the plugin has to parse.
        let id = fresh_id("Unicode — em dash ✓");
        assert!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "{id}"
        );
    }

    #[test]
    fn two_captures_of_the_same_words_do_not_collide_in_shape() {
        // Same slug, and the suffix is what separates them.
        let a = fresh_id("same words");
        assert!(a.starts_with("same-words-"));
    }
}
