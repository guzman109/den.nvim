//! Dates, in the shapes the design writes them.
//!
//! Due dates in the vault are plain civil dates (`@due(2026-09-20)`) with no
//! time or zone, so "is this due today" has to be answered against the user's
//! *local* calendar day — hence `jiff`, since `std` has no local time at all.

use jiff::civil::Date;

/// The user's current local date.
pub fn today() -> Date {
    jiff::Zoned::now().date()
}

/// Parses a vault `@due(...)` value. den.nvim has already validated the shape,
/// so this is only reached for dates that parsed there too.
pub fn parse(value: &str) -> Option<Date> {
    value.parse().ok()
}

/// `20 Sep` — the design's compact due label.
pub fn short(date: Date) -> String {
    format!("{} {}", date.day(), month_name(date.month()))
}

/// `8 Sep 2026` — used for ranges that span a year boundary.
pub fn short_with_year(date: Date) -> String {
    format!(
        "{} {} {}",
        date.day(),
        month_name(date.month()),
        date.year()
    )
}

/// `Wednesday 16 September` — the day line's heading.
///
/// Spelled out rather than abbreviated: it appears once, at the top of the
/// screen, and it is the only place the app says what day it is.
pub fn long(date: Date) -> String {
    format!(
        "{} {} {}",
        weekday_name(date.weekday()),
        date.day(),
        month_full(date.month())
    )
}

/// `2026-09-20` — the rail's detail rows echo the vault's own spelling.
pub fn iso(date: Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

/// `08` — the burndown axis ticks.
pub fn day_tick(date: Date) -> String {
    format!("{:02}", date.day())
}

/// Whole days from `from` to `to`; negative when `to` is in the past.
pub fn days_between(from: Date, to: Date) -> i64 {
    to.since(from)
        .map(|span| i64::from(span.get_days()))
        .unwrap_or(0)
}

pub fn add_days(date: Date, days: i64) -> Date {
    date.checked_add(jiff::Span::new().days(days))
        .unwrap_or(date)
}

/// How the design phrases a due date's distance: `in 5 days`, `today`,
/// `2 days ago`.
pub fn relative(due: Date, today: Date) -> String {
    match days_between(today, due) {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        days if days > 0 => format!("in {}", day_count(days)),
        days => format!("{} ago", day_count(days)),
    }
}

/// `1 day` / `3 days`, so generated sentences read properly.
pub fn day_count(days: i64) -> String {
    let days = days.abs();
    if days == 1 {
        "1 day".to_string()
    } else {
        format!("{days} days")
    }
}

fn month_name(month: i8) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    NAMES
        .get((month as usize).saturating_sub(1))
        .copied()
        .unwrap_or("???")
}

/// `4m ago`, `2h ago`, `3d ago` — the design's file-modified stamp.
pub fn elapsed_label(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

/// `mm:ss`, for the focus timer.
pub fn clock(remaining: std::time::Duration) -> String {
    let seconds = remaining.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i16, month: i8, day: i8) -> Date {
        Date::new(year, month, day).expect("valid date")
    }

    #[test]
    fn formats_match_the_design() {
        let due = date(2026, 9, 20);
        assert_eq!(short(due), "20 Sep");
        assert_eq!(iso(due), "2026-09-20");
        assert_eq!(day_tick(date(2026, 9, 8)), "08");
        assert_eq!(short_with_year(date(2026, 9, 21)), "21 Sep 2026");
    }

    #[test]
    fn relative_labels_read_naturally() {
        let today = date(2026, 9, 15);
        assert_eq!(relative(date(2026, 9, 20), today), "in 5 days");
        assert_eq!(relative(today, today), "today");
        assert_eq!(relative(date(2026, 9, 16), today), "tomorrow");
        assert_eq!(relative(date(2026, 9, 13), today), "2 days ago");
    }

    #[test]
    fn day_counts_are_pluralised() {
        assert_eq!(day_count(1), "1 day");
        assert_eq!(day_count(-1), "1 day");
        assert_eq!(day_count(0), "0 days");
        assert_eq!(day_count(19), "19 days");
    }

    #[test]
    fn day_arithmetic_crosses_months() {
        let start = date(2026, 9, 8);
        assert_eq!(days_between(start, date(2026, 9, 21)), 13);
        assert_eq!(add_days(start, 31), date(2026, 10, 9));
        assert_eq!(days_between(date(2026, 9, 20), date(2026, 9, 15)), -5);
    }

    #[test]
    fn clock_and_elapsed_read_as_the_design_writes_them() {
        use std::time::Duration;
        assert_eq!(clock(Duration::from_secs(17 * 60 + 42)), "17:42");
        assert_eq!(clock(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(elapsed_label(Duration::from_secs(240)), "4m ago");
    }
}

fn weekday_name(weekday: jiff::civil::Weekday) -> &'static str {
    use jiff::civil::Weekday::*;
    match weekday {
        Monday => "Monday",
        Tuesday => "Tuesday",
        Wednesday => "Wednesday",
        Thursday => "Thursday",
        Friday => "Friday",
        Saturday => "Saturday",
        Sunday => "Sunday",
    }
}

fn month_full(month: i8) -> &'static str {
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .get((month as usize).saturating_sub(1))
    .copied()
    .unwrap_or("")
}

#[cfg(test)]
mod long_tests {
    use super::*;

    #[test]
    fn the_day_line_spells_the_date_out() {
        let d = jiff::civil::Date::new(2026, 9, 16).expect("valid");
        assert_eq!(long(d), "Wednesday 16 September");
        // Every month and weekday resolves — no "???" leaking into the header.
        for month in 1..=12i8 {
            let d = jiff::civil::Date::new(2026, month, 1).expect("valid");
            assert!(!long(d).contains("  "), "empty component in {}", long(d));
        }
    }
}
