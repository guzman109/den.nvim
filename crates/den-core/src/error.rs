//! Everything that can go wrong in the engine, as one error type.

use std::path::{Path, PathBuf};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The file no longer holds the text the change was planned against.
    #[error("{0} changed since it was read; reload and try again")]
    Stale(String),

    /// An editor holds unsaved changes for the file.
    #[error("{0} has unsaved changes in an editor")]
    Dirty(String),

    #[error("{0} already exists")]
    Exists(String),

    #[error("no project named {0}")]
    NoProject(String),

    #[error("{path}:{line} is not a task")]
    NotATask { path: String, line: usize },

    /// A path that would leave the vault, or is not a vault file at all.
    #[error("{0} is not a file in the vault")]
    OutsideVault(String),

    #[error("{0}")]
    Invalid(String),

    #[error("config {}: {message}", path.display())]
    Config { path: PathBuf, message: String },

    #[error("timer log {}: {message}", path.display())]
    Log { path: PathBuf, message: String },
}

impl Error {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Error {
        Error::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }
}
