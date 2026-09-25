//! Settings, read from `~/.config/den/config.yaml`.
//!
//! Whether break nudges are on is deliberately not a setting: turning them off
//! takes a person, not an edit to a file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::vault::expand_home;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Where the vault lives. `$DEN_VAULT` overrides it.
    pub vault: String,
    /// This machine's name in the timer log. Defaults to the host name.
    pub machine: Option<String>,
    /// For sunset times.
    pub location: Option<Location>,
    pub nudges: Nudges,
    pub focus: Focus,
    pub sync: Sync,
    pub lock: Lock,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Nudges {
    /// Minutes at the keyboard before Den suggests a break.
    pub chair_minutes: u32,
    pub snooze_minutes: u32,
    /// Minutes away that count as a break on their own.
    pub break_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Focus {
    pub session_minutes: u32,
    pub daily_goal_minutes: u32,
    pub steps_goal: u32,
    /// A JSON file with today's steps (`{"date", "steps", "as_of"}`), kept
    /// up to date by something else, such as an iOS Shortcut saving to
    /// iCloud Drive. See steps.rs.
    pub steps_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sync {
    pub enabled: bool,
    /// Commit once the vault has been quiet this long.
    pub commit_after_seconds: u32,
    /// Pull and push this often.
    pub every_minutes: u32,
}

/// How long den-agent keeps the vault unlocked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lock {
    /// Forget the key after this long without use; 0 keeps it until sleep
    /// or `den lock`.
    pub forget_after_minutes: u32,
    /// Each Neovim unlocks for itself, and the key is forgotten when it
    /// quits.
    pub strict: bool,
}

impl Default for Lock {
    fn default() -> Lock {
        Lock {
            forget_after_minutes: 15,
            strict: false,
        }
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            vault: "~/Notes/den".to_string(),
            machine: None,
            location: None,
            nudges: Nudges::default(),
            focus: Focus::default(),
            sync: Sync::default(),
            lock: Lock::default(),
        }
    }
}

impl Default for Nudges {
    fn default() -> Nudges {
        Nudges {
            chair_minutes: 90,
            snooze_minutes: 30,
            break_minutes: 5,
        }
    }
}

impl Default for Focus {
    fn default() -> Focus {
        Focus {
            session_minutes: 50,
            daily_goal_minutes: 240,
            steps_goal: 10_000,
            steps_file: None,
        }
    }
}

impl Default for Sync {
    fn default() -> Sync {
        Sync {
            enabled: true,
            commit_after_seconds: 30,
            every_minutes: 5,
        }
    }
}

impl Config {
    /// `$DEN_CONFIG`, else `$XDG_CONFIG_HOME/den/config.yaml`, else
    /// `~/.config/den/config.yaml`.
    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("DEN_CONFIG") {
            return Some(PathBuf::from(p));
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|h| h.join(".config")))?;
        Some(base.join("den").join("config.yaml"))
    }

    /// The config file, or defaults when there is none.
    pub fn load() -> Result<Config> {
        match Config::path() {
            Some(path) => Config::load_from(&path),
            None => Ok(Config::default()),
        }
    }

    pub fn load_from(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) if text.trim().is_empty() => Ok(Config::default()),
            Ok(text) => serde_norway::from_str(&text).map_err(|e| {
                let mut message = e.to_string();
                if message.contains("nudges") && message.contains("unknown field") {
                    message = format!(
                        "{message}. Break nudges cannot be turned off from the config file. {}",
                        crate::nudge::AGENT_NOTICE
                    );
                }
                Error::Config {
                    path: path.to_path_buf(),
                    message,
                }
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Today's steps, if a steps file is set and current.
    pub fn steps(&self, today: jiff::civil::Date) -> Option<crate::steps::Steps> {
        let path = expand_home(self.focus.steps_file.as_deref()?);
        crate::steps::read(&path, today)
    }

    pub fn vault_root(&self) -> PathBuf {
        match std::env::var("DEN_VAULT") {
            Ok(v) if !v.is_empty() => expand_home(&v),
            _ => expand_home(&self.vault),
        }
    }

    /// A file-name-safe name for this machine.
    pub fn machine_name(&self) -> String {
        let raw = self
            .machine
            .clone()
            .unwrap_or_else(|| gethostname::gethostname().to_string_lossy().into_owned());
        let raw = raw.strip_suffix(".local").unwrap_or(&raw).to_lowercase();
        let name: String = raw
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let name = name.trim_matches('-').to_string();
        if name.is_empty() {
            "machine".to_string()
        } else {
            name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_means_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_from(&dir.path().join("nope.yaml")).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn reads_partial_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(
            &path,
            "vault: ~/vault\nlocation: { lat: 40.0, lon: -83.0 }\nnudges:\n  chair_minutes: 60\n",
        )
        .unwrap();
        let config = Config::load_from(&path).unwrap();
        assert_eq!(config.nudges.chair_minutes, 60);
        assert_eq!(config.nudges.snooze_minutes, 30);
        assert_eq!(
            config.location,
            Some(Location {
                lat: 40.0,
                lon: -83.0
            })
        );
    }

    #[test]
    fn typos_are_errors_not_silence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "nudge:\n  chair_minutes: 60\n").unwrap();
        assert!(Config::load_from(&path).is_err());
    }

    #[test]
    fn nudges_cannot_be_switched_off_here() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "nudges:\n  enabled: false\n").unwrap();
        let message = Config::load_from(&path).unwrap_err().to_string();
        assert!(message.contains("unknown field `enabled`"), "{message}");
        assert!(
            message.contains("can only be turned off by a person"),
            "{message}"
        );
    }

    #[test]
    fn machine_names_are_safe_file_names() {
        let config = Config {
            machine: Some("Sam's MacBook.local".to_string()),
            ..Config::default()
        };
        assert_eq!(config.machine_name(), "sam-s-macbook");
    }
}
