//! The Den engine.
//!
//! Den's store is a folder of Markdown files in one git repository. This crate
//! is the only code that reads or changes it: it parses the files, answers
//! questions about them, plans edits, and writes those edits to disk without
//! ever leaving a file half written.
//!
//! It knows nothing about screens, editors or colours. Every query returns
//! plain data, and every interface — Neovim, the `den` command, a desktop app,
//! an agent — draws that data its own way.
//!
//! Changes are planned first and applied second. A plan is a list of
//! [`Change`]s, each carrying the exact text the file must still contain, so
//! an interface can apply it to an open editor buffer instead of the disk, and
//! a file that moved underneath us is refused rather than overwritten.

pub mod config;
pub mod conflict;
pub mod error;
pub mod frontmatter;
pub mod nudge;
pub mod ops;
pub mod parse;
pub mod query;
pub mod review;
pub mod slug;
pub mod sun;
pub mod sync;
pub mod text;
pub mod timer;
pub mod vault;
pub mod watch;
pub mod worktree;
pub mod write;

pub use config::Config;
pub use error::{Error, Result};
pub use ops::{Change, Section, TaskRef};
pub use parse::{State, Task};
pub use query::{Scope, TaskRow};
pub use timer::TimerLog;
pub use vault::{Doc, Kind, Vault};
pub use write::apply;
