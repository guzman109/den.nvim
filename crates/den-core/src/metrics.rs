//! Derived series for the burndown view.
//!
//! Everything here is computed from the vault plus [`crate::history`], which is
//! the only record of *when* a task closed — the Markdown format carries state
//! but no timestamps. Where history is missing, the series is short rather than
//! invented: `actual` simply has no points before [`Burndown::history_starts`].

use jiff::civil::Date;

use crate::date;
use crate::history::History;
use crate::vault::{Status, Vault};

/// The window the design draws: a fortnight with today inside it.
const WINDOW_DAYS: i64 = 14;

#[derive(Debug, Clone)]
pub struct Burndown {
    pub start: Date,
    pub end: Date,
    pub days: Vec<Date>,
    /// Tasks in scope today.
    pub scope: usize,
    pub remaining: usize,
    pub closed: usize,
    /// Remaining count per day; `None` for days we cannot speak to — either in
    /// the future, or before the ledger begins.
    pub actual: Vec<Option<usize>>,
    /// Tasks closed on each day of the window.
    pub closed_per_day: Vec<usize>,
    /// Closures per day across the observed part of the window.
    pub pace: f32,
    /// When the current pace would clear the remaining work.
    pub projected: Option<Date>,
    /// The first day the ledger knows anything about.
    pub history_starts: Option<Date>,
}

impl Burndown {
    pub fn compute(vault: &Vault, history: &History, today: Date) -> Burndown {
        let tasks: Vec<_> = vault.tasks().collect();
        let scope = tasks.len();
        let closed = tasks.iter().filter(|t| t.status() == Status::Done).count();
        let remaining = scope - closed;

        // End on the last thing that is actually due, when that is ahead of us;
        // otherwise just show the fortnight around today.
        let latest_due = tasks
            .iter()
            .filter(|task| task.status() != Status::Done)
            .filter_map(|task| task.due().and_then(date::parse))
            .max();
        let end = match latest_due {
            Some(due) if date::days_between(today, due) > 0 => due,
            _ => date::add_days(today, 6),
        };
        let start = date::add_days(end, -(WINDOW_DAYS - 1));

        let days: Vec<Date> = (0..WINDOW_DAYS)
            .map(|offset| date::add_days(start, offset))
            .collect();
        let history_starts = history.starts_on();

        let mut actual = Vec::with_capacity(days.len());
        let mut closed_per_day = Vec::with_capacity(days.len());
        for day in &days {
            let future = date::days_between(*day, today) < 0;
            let before_history = history_starts.is_some_and(|first| *day < first);

            if future || before_history || history_starts.is_none() {
                actual.push(None);
                closed_per_day.push(0);
                continue;
            }

            let mut open_then = 0;
            let mut closed_today = 0;
            for task in &tasks {
                let opened = history.opened_on(task);
                // A task we have never seen before this day was not in scope.
                if opened.is_some_and(|opened| opened > *day) {
                    continue;
                }
                match history.closed_on(task) {
                    Some(closed) if closed <= *day => {
                        // Only count it as work done on this day if we had
                        // previously seen it open. A task that was already
                        // finished the first time Den looked tells us nothing
                        // about pace, and counting it would inflate every
                        // projection on the first run.
                        let witnessed = opened.is_some_and(|opened| opened < closed);
                        if closed == *day && witnessed {
                            closed_today += 1;
                        }
                    }
                    _ => open_then += 1,
                }
            }
            actual.push(Some(open_then));
            closed_per_day.push(closed_today);
        }

        let observed_days = days
            .iter()
            .filter(|day| date::days_between(**day, today) >= 0)
            .filter(|day| history_starts.is_some_and(|first| **day >= first))
            .count();
        let closed_in_window: usize = closed_per_day.iter().sum();
        let pace = if observed_days == 0 {
            0.0
        } else {
            closed_in_window as f32 / observed_days as f32
        };

        let projected = (pace > 0.0 && remaining > 0)
            .then(|| date::add_days(today, (remaining as f32 / pace).ceil() as i64));

        Burndown {
            start,
            end,
            days,
            scope,
            remaining,
            closed,
            actual,
            closed_per_day,
            pace,
            projected,
            history_starts,
        }
    }

    /// The straight line from full scope at the start to zero at the end.
    pub fn ideal_at(&self, index: usize) -> f32 {
        if self.days.len() < 2 {
            return self.scope as f32;
        }
        let span = (self.days.len() - 1) as f32;
        self.scope as f32 * (1.0 - index as f32 / span)
    }

    /// The largest value any series reaches, for scaling the chart.
    pub fn ceiling(&self) -> usize {
        self.actual
            .iter()
            .flatten()
            .copied()
            .max()
            .unwrap_or(self.scope)
            .max(self.scope)
            .max(1)
    }

    /// True when the ledger has nothing to draw a curve from yet.
    pub fn is_cold_start(&self) -> bool {
        self.actual.iter().flatten().count() < 2
    }

    pub fn label(&self) -> String {
        format!(
            "{} – {}",
            date::short(self.start),
            date::short_with_year(self.end)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{Entry, Kind};
    use std::path::PathBuf;

    fn day(y: i16, m: i8, d: i8) -> Date {
        Date::new(y, m, d).expect("valid")
    }

    fn vault_of(markdown: &str) -> Vault {
        let mut vault = Vault::default();
        vault.entries.push(Entry::parse(
            PathBuf::from("/tmp/p.md"),
            Kind::Projects,
            markdown,
            false,
        ));
        vault
    }

    #[test]
    fn cold_start_draws_no_invented_curve() {
        let vault = vault_of("# P\n- [ ] One @id(a)\n- [ ] Two @id(b)\n");
        let history = History::default();
        let burndown = Burndown::compute(&vault, &history, day(2026, 9, 15));

        assert_eq!(burndown.scope, 2);
        assert_eq!(burndown.remaining, 2);
        assert!(
            burndown.is_cold_start(),
            "no ledger means no history to plot"
        );
        assert!(burndown.actual.iter().all(Option::is_none));
        assert_eq!(burndown.pace, 0.0);
        assert_eq!(burndown.projected, None);
    }

    #[test]
    fn plots_remaining_from_the_ledger() {
        let mut history = History::default();
        let open = vault_of("# P\n- [ ] One @id(a)\n- [ ] Two @id(b)\n");
        history.observe(&open, day(2026, 9, 13));

        let half = vault_of("# P\n- [x] One @id(a)\n- [ ] Two @id(b)\n");
        history.observe(&half, day(2026, 9, 14));

        let burndown = Burndown::compute(&half, &history, day(2026, 9, 15));
        let index = |date: Date| {
            burndown
                .days
                .iter()
                .position(|d| *d == date)
                .expect("in window")
        };

        assert_eq!(burndown.actual[index(day(2026, 9, 13))], Some(2));
        assert_eq!(burndown.actual[index(day(2026, 9, 14))], Some(1));
        assert_eq!(burndown.actual[index(day(2026, 9, 15))], Some(1));
        assert_eq!(burndown.closed_per_day[index(day(2026, 9, 14))], 1);
        // Days before the ledger opened stay blank rather than showing zero.
        assert_eq!(burndown.actual[index(day(2026, 9, 12))], None);
    }

    #[test]
    fn projects_from_observed_pace() {
        let mut history = History::default();
        let open = vault_of("# P\n- [ ] a @id(a)\n- [ ] b @id(b)\n- [ ] c @id(c)\n");
        history.observe(&open, day(2026, 9, 14));
        let done_one = vault_of("# P\n- [x] a @id(a)\n- [ ] b @id(b)\n- [ ] c @id(c)\n");
        history.observe(&done_one, day(2026, 9, 15));

        let burndown = Burndown::compute(&done_one, &history, day(2026, 9, 15));
        // One closure over two observed days.
        assert!(
            (burndown.pace - 0.5).abs() < f32::EPSILON,
            "pace was {}",
            burndown.pace
        );
        // Two left at half a task per day is four more days.
        assert_eq!(burndown.projected, Some(day(2026, 9, 19)));
    }

    #[test]
    fn window_reaches_the_last_due_date() {
        let vault = vault_of("# P\n- [ ] Ship @id(a) @due(2026-09-21)\n");
        let burndown = Burndown::compute(&vault, &History::default(), day(2026, 9, 15));
        assert_eq!(burndown.end, day(2026, 9, 21));
        assert_eq!(burndown.start, day(2026, 9, 8));
        assert_eq!(burndown.days.len(), 14);
    }

    #[test]
    fn ideal_runs_from_scope_to_zero() {
        let vault = vault_of("# P\n- [ ] a @id(a)\n- [ ] b @id(b)\n");
        let burndown = Burndown::compute(&vault, &History::default(), day(2026, 9, 15));
        assert!((burndown.ideal_at(0) - 2.0).abs() < f32::EPSILON);
        assert!(burndown.ideal_at(burndown.days.len() - 1).abs() < f32::EPSILON);
    }
}
