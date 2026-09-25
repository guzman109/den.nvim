//! The numbers behind the Review screen and the journal's day facts.
//!
//! Everything here is counted from what the vault already holds: `@done`
//! dates on tasks, project `created` and `due` fields, and the timer log.
//! Nothing is stored for the review's sake.

use std::collections::{BTreeMap, HashMap};

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};
use serde::Serialize;

use crate::parse::{State, Task};
use crate::query::{Pace, ProjectRef, Scope};
use crate::timer::TimerLog;
use crate::vault::Vault;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayCount {
    pub date: Date,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DaySeconds {
    pub date: Date,
    pub seconds: i64,
}

/// Time spent on one project (or on files that belong to none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Share {
    pub project: Option<ProjectRef>,
    pub label: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    pub project: ProjectRef,
    pub done: usize,
    /// Open and doing.
    pub open: usize,
    pub due: Option<Date>,
    pub pace: Option<Pace>,
}

/// Tasks left, day by day, for a project with an end date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Burndown {
    pub project: ProjectRef,
    pub start: Date,
    pub due: Date,
    /// From `start` to today (or `due`, if that came first).
    pub points: Vec<DayCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Review {
    pub today: Date,
    /// Tasks finished per day, the last 14 days ending today.
    pub closed: Vec<DayCount>,
    /// Time worked per day, Monday to Sunday of this week.
    pub week: Vec<DaySeconds>,
    pub week_total: i64,
    /// This week's time by project, most first.
    pub by_project: Vec<Share>,
    pub projects: Vec<Progress>,
    pub burndowns: Vec<Burndown>,
}

/// One task worked on during a day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Worked {
    pub file: String,
    pub task: String,
    pub seconds: i64,
    pub project: Option<ProjectRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finished {
    pub path: String,
    pub title: String,
    pub project: Option<ProjectRef>,
}

/// What a day held, for the journal page. Drawn beside the page, never
/// written into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayFacts {
    pub date: Date,
    /// Most time first.
    pub worked: Vec<Worked>,
    pub finished: Vec<Finished>,
    pub total_seconds: i64,
    pub walks: usize,
    pub walk_seconds: i64,
}

/// When each line of each file was first committed (from git history), for
/// telling when an open task appeared. Optional everywhere.
pub type LineTimes = HashMap<String, HashMap<String, Timestamp>>;

/// `[start, end)` of a calendar day in a time zone.
pub fn day_bounds(date: Date, tz: &TimeZone) -> (Timestamp, Timestamp) {
    let start = date
        .to_zoned(tz.clone())
        .and_then(|z| z.start_of_day())
        .map(|z| z.timestamp())
        .unwrap_or(Timestamp::UNIX_EPOCH);
    let end = date
        .tomorrow()
        .ok()
        .and_then(|d| d.to_zoned(tz.clone()).ok())
        .and_then(|z| z.start_of_day().ok())
        .map(|z| z.timestamp())
        .unwrap_or(start);
    (start, end)
}

fn days(from: Date, to: Date) -> Vec<Date> {
    let mut out = Vec::new();
    let mut d = from;
    while d <= to {
        out.push(d);
        match d.tomorrow() {
            Ok(next) => d = next,
            Err(_) => break,
        }
    }
    out
}

fn minus_days(date: Date, n: i64) -> Date {
    date.checked_sub(Span::new().days(n)).unwrap_or(date)
}

impl Vault {
    /// The project a timer-log file belongs to.
    fn project_ref_of(&self, path: &str) -> Option<ProjectRef> {
        self.doc(path)
            .and_then(|d| self.project_of(d))
            .map(ProjectRef::of)
    }

    fn in_scope(&self, scope: &Scope, path: &str) -> bool {
        match scope {
            Scope::All => true,
            Scope::Project(name) => self.project_ref_of(path).is_some_and(|p| &p.name == name),
        }
    }

    /// The Review screen's numbers.
    pub fn review(
        &self,
        scope: &Scope,
        log: &TimerLog,
        today: Date,
        now: Timestamp,
        tz: &TimeZone,
        lines: Option<&LineTimes>,
    ) -> Review {
        let docs = self.history_docs(scope);

        let from = minus_days(today, 13);
        let mut per_day: BTreeMap<Date, usize> =
            days(from, today).into_iter().map(|d| (d, 0)).collect();
        for task in docs.iter().flat_map(|d| d.parsed.tasks.iter()) {
            if task.state == State::Done
                && let Some(done) = task.done
                && let Some(n) = per_day.get_mut(&done)
            {
                *n += 1;
            }
        }
        let closed = per_day
            .into_iter()
            .map(|(date, count)| DayCount { date, count })
            .collect();

        let monday = minus_days(today, i64::from(today.weekday().to_monday_zero_offset()));
        let sessions = log.sessions(now);
        let mut week = Vec::new();
        for date in days(monday, minus_days(monday, -6)) {
            let (start, end) = day_bounds(date, tz);
            let seconds = sessions
                .iter()
                .filter(|s| self.in_scope(scope, &s.file))
                .map(|s| s.seconds_within(start, end))
                .sum();
            week.push(DaySeconds { date, seconds });
        }
        let week_total = week.iter().map(|d| d.seconds).sum();

        let (week_start, _) = day_bounds(monday, tz);
        let (_, week_end) = day_bounds(minus_days(monday, -6), tz);
        let mut shares: BTreeMap<Option<String>, Share> = BTreeMap::new();
        for s in sessions.iter().filter(|s| self.in_scope(scope, &s.file)) {
            let seconds = s.seconds_within(week_start, week_end);
            if seconds == 0 {
                continue;
            }
            let project = self.project_ref_of(&s.file);
            let key = project.as_ref().map(|p| p.name.clone());
            let share = shares.entry(key).or_insert_with(|| Share {
                label: project
                    .as_ref()
                    .map_or_else(|| "No project".to_string(), |p| p.title.clone()),
                project,
                seconds: 0,
            });
            share.seconds += seconds;
        }
        let mut by_project: Vec<Share> = shares.into_values().collect();
        by_project.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.label.cmp(&b.label)));

        let mut projects = Vec::new();
        let mut burndowns = Vec::new();
        for p in self.scoped_projects(scope) {
            let mut project_docs = vec![p.doc];
            project_docs.extend(self.notes_of(p.name()));
            let tasks: Vec<(&str, &Task)> = project_docs
                .iter()
                .flat_map(|d| d.parsed.tasks.iter().map(move |t| (d.path.as_str(), t)))
                .collect();
            let done = tasks.iter().filter(|(_, t)| t.state == State::Done).count();
            let open = tasks.iter().filter(|(_, t)| !t.state.is_closed()).count();
            projects.push(Progress {
                project: ProjectRef::of(p),
                done,
                open,
                due: p.due(),
                pace: self.pace(p, today),
            });

            let Some(due) = p.due() else { continue };
            let earliest_done = tasks.iter().filter_map(|(_, t)| t.done).min();
            let start = p
                .created()
                .or(earliest_done)
                .unwrap_or_else(|| minus_days(today, 14))
                .max(minus_days(due, 120))
                .min(today);
            if start > due {
                continue;
            }
            let first_seen = |path: &str, task: &Task| -> Option<Date> {
                let line = self.doc(path)?.buf.lines.get(task.line)?;
                let at = lines?.get(path)?.get(line)?;
                Some(at.to_zoned(tz.clone()).date())
            };
            let points = days(start, today.min(due))
                .into_iter()
                .map(|date| {
                    let count = tasks
                        .iter()
                        .filter(|(_, t)| t.state != State::Dropped)
                        .filter(|(_, t)| {
                            // Ticked by hand with no @done date: done as of today.
                            !(t.state == State::Done && t.done.unwrap_or(today) <= date)
                        })
                        .filter(|(path, t)| {
                            // An open task counts from the day it was first
                            // committed, when history knows it.
                            t.state == State::Done
                                || first_seen(path, t).is_none_or(|seen| seen <= date)
                        })
                        .count();
                    DayCount { date, count }
                })
                .collect();
            burndowns.push(Burndown {
                project: ProjectRef::of(p),
                start,
                due,
                points,
            });
        }

        Review {
            today,
            closed,
            week,
            week_total,
            by_project,
            projects,
            burndowns,
        }
    }

    /// What happened on `date`: time per task, tasks finished, walks.
    pub fn day_facts(&self, date: Date, log: &TimerLog, now: Timestamp, tz: &TimeZone) -> DayFacts {
        let (start, end) = day_bounds(date, tz);
        let mut worked: BTreeMap<(String, String), i64> = BTreeMap::new();
        for s in log.sessions(now) {
            let seconds = s.seconds_within(start, end);
            if seconds > 0 {
                *worked.entry((s.file, s.task)).or_insert(0) += seconds;
            }
        }
        let mut worked: Vec<Worked> = worked
            .into_iter()
            .map(|((file, task), seconds)| Worked {
                project: self.project_ref_of(&file),
                file,
                task,
                seconds,
            })
            .collect();
        worked.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.task.cmp(&b.task)));
        let total_seconds = worked.iter().map(|w| w.seconds).sum();

        let mut finished = Vec::new();
        for doc in self.history_docs(&Scope::All) {
            for task in &doc.parsed.tasks {
                if task.state == State::Done && task.done == Some(date) {
                    finished.push(Finished {
                        path: doc.path.clone(),
                        title: task.title.clone(),
                        project: self.project_of(doc).map(ProjectRef::of),
                    });
                }
            }
        }

        let walks: Vec<i64> = log
            .walks(now)
            .into_iter()
            .map(|(a, b)| (b.min(end).as_second() - a.max(start).as_second()).max(0))
            .filter(|s| *s > 0)
            .collect();
        DayFacts {
            date,
            worked,
            finished,
            total_seconds,
            walks: walks.len(),
            walk_seconds: walks.iter().sum(),
        }
    }
}
