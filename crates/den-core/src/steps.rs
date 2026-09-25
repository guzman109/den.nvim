//! Today's step count, from a small file something else keeps up to date.
//!
//! Mac apps cannot read the iPhone's Health data, so steps arrive as a file:
//! typically an iOS Shortcut that saves `{"date", "steps", "as_of"}` to
//! iCloud Drive a few times a day. Den reads it only if it is about today,
//! and always shows when it was written, because the phone only updates it
//! while unlocked.

use std::path::Path;

use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Steps {
    pub steps: u32,
    /// When the count was taken (the file's own time, else when it was
    /// written).
    pub as_of: Option<Timestamp>,
}

/// A number, or a number as text: Shortcuts writes either, sometimes with
/// decimals.
#[derive(Deserialize)]
#[serde(untagged)]
enum Count {
    Number(f64),
    Text(String),
}

#[derive(Deserialize)]
struct File {
    date: String,
    steps: Count,
    #[serde(default)]
    as_of: Option<String>,
}

fn parse_time(s: &str) -> Option<Timestamp> {
    s.parse::<Timestamp>()
        .ok()
        .or_else(|| s.parse::<jiff::Zoned>().ok().map(|z| z.timestamp()))
}

/// Today's steps from `path`, or `None` if the file is missing, unreadable
/// or about another day.
pub fn read(path: &Path, today: Date) -> Option<Steps> {
    let text = std::fs::read_to_string(path).ok()?;
    let file: File = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let date = file.date.trim().get(..10)?.parse::<Date>().ok()?;
    if date != today {
        return None;
    }
    let count = match file.steps {
        Count::Number(n) => n,
        Count::Text(t) => t.trim().replace(',', "").parse::<f64>().ok()?,
    };
    if !count.is_finite() || count < 0.0 {
        return None;
    }
    let as_of = file.as_of.as_deref().and_then(parse_time).or_else(|| {
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        Timestamp::try_from(modified).ok()
    });
    Some(Steps {
        steps: count.round().min(f64::from(u32::MAX)) as u32,
        as_of,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn file(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("steps.json");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn reads_today_in_the_shapes_shortcuts_writes() {
        let today = date(2026, 9, 24);
        let (_d, p) = file(r#"{"date":"2026-09-24","steps":6234,"as_of":"2026-09-24T14:05:00Z"}"#);
        let s = read(&p, today).unwrap();
        assert_eq!(s.steps, 6234);
        assert_eq!(s.as_of.unwrap().to_string(), "2026-09-24T14:05:00Z");

        let (_d, p) = file(r#"{"date":"2026-09-24 08:10","steps":"6,234.6"}"#);
        let s = read(&p, today).unwrap();
        assert_eq!(s.steps, 6235);
        assert!(s.as_of.is_some(), "falls back to the file's time");

        let (_d, p) = file(
            r#"{"date":"2026-09-24","steps":10,"as_of":"2026-09-24T09:00:00-05:00[America/Chicago]"}"#,
        );
        assert_eq!(
            read(&p, today).unwrap().as_of.unwrap().to_string(),
            "2026-09-24T14:00:00Z"
        );
    }

    #[test]
    fn yesterdays_or_broken_files_are_ignored() {
        let today = date(2026, 9, 24);
        let (_d, p) = file(r#"{"date":"2026-09-23","steps":9000}"#);
        assert_eq!(read(&p, today), None);
        let (_d, p) = file("not json");
        assert_eq!(read(&p, today), None);
        let (_d, p) = file(r#"{"date":"2026-09-24","steps":-5}"#);
        assert_eq!(read(&p, today), None);
        assert_eq!(read(Path::new("/nonexistent/steps.json"), today), None);
    }
}
