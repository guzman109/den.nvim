//! The Review screen's numbers and the journal's day facts, from the fixture
//! vault and its timer log, in UTC so the day boundaries are fixed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use den_core::query::Scope;
use den_core::review::{DayCount, LineTimes};
use den_core::{TimerLog, Vault};
use jiff::Timestamp;
use jiff::civil::date;
use jiff::tz::TimeZone;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/vault")
}

fn now() -> Timestamp {
    "2026-09-24T17:00:00Z".parse().unwrap()
}

fn open() -> (Vault, TimerLog) {
    let vault = Vault::open(fixture()).unwrap();
    let log = TimerLog::load(&fixture(), "test").unwrap();
    (vault, log)
}

#[test]
fn a_week_of_time_and_two_weeks_of_finished_tasks() {
    let (vault, log) = open();
    let today = date(2026, 9, 24);
    let r = vault.review(&Scope::All, &log, today, now(), &TimeZone::UTC, None);

    assert_eq!(r.closed.len(), 14);
    assert_eq!(r.closed.first().unwrap().date, date(2026, 9, 11));
    let recent: Vec<(String, usize)> = r
        .closed
        .iter()
        .filter(|d| d.count > 0)
        .map(|d| (d.date.to_string(), d.count))
        .collect();
    assert_eq!(
        recent,
        vec![
            ("2026-09-22".to_string(), 1),
            ("2026-09-23".to_string(), 1),
            ("2026-09-24".to_string(), 2),
        ]
    );

    // Wednesday 70 minutes on the website; Thursday 72 minutes on haste plus
    // the session still running since 16:30.
    let week: Vec<i64> = r.week.iter().map(|d| d.seconds).collect();
    assert_eq!(week, vec![0, 0, 4200, 4320 + 1800, 0, 0, 0]);
    assert_eq!(r.week.first().unwrap().date, date(2026, 9, 21));
    assert_eq!(r.week_total, 10_320);
    let shares: Vec<(&str, i64)> = r
        .by_project
        .iter()
        .map(|s| (s.label.as_str(), s.seconds))
        .collect();
    assert_eq!(shares, vec![("haste", 6120), ("Personal website", 4200)]);
}

#[test]
fn progress_and_a_burndown_for_projects_with_an_end_date() {
    let (vault, log) = open();
    let today = date(2026, 9, 24);
    let r = vault.review(
        &Scope::Project("website".into()),
        &log,
        today,
        now(),
        &TimeZone::UTC,
        None,
    );
    assert_eq!(r.projects.len(), 1);
    let p = &r.projects[0];
    assert_eq!((p.done, p.open), (2, 5));
    assert_eq!(p.due, Some(date(2026, 10, 1)));

    assert_eq!(r.burndowns.len(), 1);
    let b = &r.burndowns[0];
    assert_eq!((b.start, b.due), (date(2026, 9, 1), date(2026, 10, 1)));
    assert_eq!(b.points.len(), 24);
    let at = |d| b.points.iter().find(|p| p.date == d).unwrap().count;
    assert_eq!(at(date(2026, 9, 1)), 7, "every task not dropped");
    assert_eq!(at(date(2026, 9, 10)), 6, "the domain was registered");
    assert_eq!(at(date(2026, 9, 22)), 5, "the font was picked");
    assert_eq!(at(today), 5);

    // Time only counts this project's files.
    assert_eq!(r.week_total, 4200);
    assert!(r.closed.iter().map(|d| d.count).sum::<usize>() == 1);
}

#[test]
fn history_tells_the_burndown_when_open_tasks_appeared() {
    let (vault, log) = open();
    let mut lines: LineTimes = HashMap::new();
    let mut website = HashMap::new();
    website.insert(
        "- [ ] Find the old logo files".to_string(),
        "2026-09-20T10:00:00Z".parse().unwrap(),
    );
    lines.insert("projects/website.md".to_string(), website);
    let r = vault.review(
        &Scope::Project("website".into()),
        &log,
        date(2026, 9, 24),
        now(),
        &TimeZone::UTC,
        Some(&lines),
    );
    let b = &r.burndowns[0];
    let at = |d| {
        b.points
            .iter()
            .find(|p: &&DayCount| p.date == d)
            .unwrap()
            .count
    };
    assert_eq!(at(date(2026, 9, 19)), 5);
    assert_eq!(at(date(2026, 9, 20)), 6);
}

#[test]
fn a_day_in_facts() {
    let (vault, log) = open();
    let facts = vault.day_facts(date(2026, 9, 24), &log, now(), &TimeZone::UTC);
    assert_eq!(facts.total_seconds, 6120);
    assert_eq!(facts.worked.len(), 1);
    assert_eq!(facts.worked[0].task, "Wire up the renderer");
    assert_eq!(facts.worked[0].project.as_ref().unwrap().name, "haste");
    let finished: Vec<&str> = facts.finished.iter().map(|f| f.title.as_str()).collect();
    assert_eq!(
        finished,
        vec!["Sketch the lua_module surface", "Clear the desk"]
    );
    assert_eq!((facts.walks, facts.walk_seconds), (1, 1500));

    let quiet = vault.day_facts(date(2026, 9, 20), &log, now(), &TimeZone::UTC);
    assert_eq!(quiet.total_seconds, 0);
    assert!(quiet.worked.is_empty() && quiet.finished.is_empty());
}

#[test]
fn a_task_ticked_without_a_date_leaves_the_burndown_today() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    std::fs::write(
        dir.path().join("projects/shed.md"),
        "---\ndue: 2026-10-10\ncreated: 2026-09-20\n---\n# Shed\n\n- [x] Buy paint\n- [ ] Paint\n",
    )
    .unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let log = TimerLog::load(dir.path(), "test").unwrap();
    let today = date(2026, 9, 24);
    let r = vault.review(&Scope::All, &log, today, now(), &TimeZone::UTC, None);
    let b = &r.burndowns[0];
    let at = |d| {
        b.points
            .iter()
            .find(|p: &&DayCount| p.date == d)
            .unwrap()
            .count
    };
    assert_eq!(at(date(2026, 9, 23)), 2);
    assert_eq!(at(today), 1);
}
