//! The screens Den can show.
//!
//! Stateless screens are unit variants that render from the shared [`Den`]
//! state; [`Settings`] owns state of its own and follows the usual
//! message/action split.
//!
//! [`Den`]: crate::den::Den

pub mod board;
/// Kept for `Burndown::spark`, which Notes draws as a panel. Not a screen.
pub mod burndown;
pub mod roadmap;
pub mod settings;
pub mod table;
pub mod today;

pub use settings::Settings;

/// How the tasks screen is drawn.
///
/// Board and table are two readings of one set of tasks, not two destinations:
/// the board answers "what state is everything in", the table answers "show me
/// all of it lined up". Making them one screen with a switch is what stops the
/// app growing a second half-empty view — the mistake that gave it five screens
/// and not enough content for any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskView {
    #[default]
    Board,
    Table,
}

impl TaskView {
    pub const ALL: [TaskView; 2] = [TaskView::Board, TaskView::Table];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskView::Board => "board",
            TaskView::Table => "table",
        }
    }

    pub fn parse(value: &str) -> Option<TaskView> {
        TaskView::ALL.into_iter().find(|v| v.as_str() == value)
    }
}

/// The live screen, with whatever state it owns.
#[derive(Default)]
pub enum Screen {
    #[default]
    Today,
    Tasks,
    Roadmap,
    Settings(Settings),
}

impl Screen {
    pub fn kind(&self) -> Kind {
        match self {
            Screen::Today => Kind::Today,
            Screen::Tasks => Kind::Tasks,
            Screen::Roadmap => Kind::Roadmap,
            Screen::Settings(_) => Kind::Settings,
        }
    }
}

/// Which screen, without its state — for tabs, shortcuts and the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Today,
    Tasks,
    Roadmap,
    Settings,
}

impl Kind {
    /// The screens that get a tab in the command bar. Settings is reached from
    /// its own control, as the design has no sixth tab.
    /// The destinations the sidebar lists. Settings is reached from the status
    /// line and from `,`, because it is about the app rather than the vault.
    pub const TABS: [Kind; 3] = [Kind::Today, Kind::Tasks, Kind::Roadmap];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Today => "today",
            Kind::Tasks => "tasks",
            Kind::Roadmap => "roadmap",
            Kind::Settings => "settings",
        }
    }

    pub fn parse(value: &str) -> Option<Kind> {
        [Kind::Today, Kind::Tasks, Kind::Roadmap, Kind::Settings]
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }

    /// The letter that follows `g` to reach this screen.
    pub fn shortcut(self) -> char {
        match self {
            Kind::Today => 't',
            Kind::Tasks => 'k',
            Kind::Roadmap => 'r',
            Kind::Settings => ',',
        }
    }

    /// Whether the screen wants the shared right rail.
    ///
    /// Today brings its own aside — cooling, then the week — so the rail would
    /// be a second right-hand column drawn on top of the first. Settings has
    /// its own two-pane layout for the same reason.
    /// Nothing does any more.
    ///
    /// Every screen now owns its own right-hand column — Today has cooling and
    /// the week, Notes has the vault shape and the pace — so a shared rail
    /// would be a second column drawn on top of the first.
    pub fn shows_rail(self) -> bool {
        false
    }
}
