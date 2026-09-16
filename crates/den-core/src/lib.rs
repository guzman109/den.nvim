//! The Den engine.
//!
//! Den's store is plain Markdown the user owns — git-diffable, editable in any
//! editor, readable without Den. This crate is the single implementation of
//! everything that reads or changes it: the parser, the index, and the rules
//! for mutating a task line.
//!
//! Both interfaces sit on top of it. The desktop app links it directly, and
//! den.nvim loads it as a native Lua module. Neither owns any parsing of its
//! own, which is what stops the two drifting apart — they cannot disagree
//! about what a task is if there is only one definition of one.
//!
//! Nothing here knows about a user interface, an editor, or a window.

pub mod date;
pub mod history;
pub mod index;
pub mod metrics;
pub mod mutate;
pub mod vault;

pub use history::History;
pub use index::Index;
pub use metrics::Burndown;
pub use vault::{Entry, Kind, Status, Task, Vault};
