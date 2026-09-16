//! Den — a desktop client for the [den.nvim] Markdown vault.
//!
//! The vault is plain Markdown owned by the user and shared with Neovim, so
//! Den reads it the way the plugin does ([`data::vault::parse`]) and routes
//! every write back through the plugin ([`data::bridge`]) rather than rewriting
//! files behind the editor's back.
//!
//! The layout follows the usual iced shape: [`data`] is the domain and knows
//! nothing about the UI, [`screen`] holds one module per screen, [`widget`] the
//! pieces they share, and [`den::Den`] is the root that ties them together.
//!
//! [den.nvim]: https://github.com/den-nvim

pub mod cli;
pub mod den;
pub mod editor;
pub mod fonts;
pub mod glyph;
pub mod icon;
pub mod keymap;
pub mod screen;
pub mod theme;
pub mod widget;

pub use cli::Options;
pub use den::Den;
