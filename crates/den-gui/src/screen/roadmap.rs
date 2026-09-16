//! Roadmap — what is coming, by week, per project.
//!
//! This is the clearest case in the app for a GUI. Neovim is better than Den at
//! nearly everything involving one file, but "which projects have work landing
//! in the next two months, and where do they collide" is a two-dimensional
//! question, and a terminal buffer is one-dimensional. Laying projects down one
//! axis and time across the other is the thing a window is actually for.
//!
//! It is built entirely from `@due()`, which den.nvim already writes — no new
//! file format, no milestones to maintain. A project with no dated work still
//! appears, with its undated count, because a roadmap that silently drops
//! projects is worse than no roadmap.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Length, Padding};

use den_core::vault::{Entry, Status, Task as VaultTask};
use jiff::civil::Date;

use crate::den::{Den, Message};
use crate::fonts::mono;
use crate::glyph;
use crate::theme::Palette;
use crate::widget::{Element, gap, line_text, size};

/// How far ahead the roadmap looks. Eight weeks is about as far as a due date
/// on a personal vault means anything; past that it is a wish, not a plan.
const WEEKS: usize = 8;
/// The project-name gutter.
const GUTTER: f32 = 210.0;

pub fn view(den: &Den) -> Element<'_> {
    let palette = &den.palette;
    let start = week_start(den.today);
    let projects: Vec<&Entry> = den
        .vault
        .active()
        .filter(|entry| !entry.tasks.is_empty())
        .collect();

    if projects.is_empty() {
        return empty(den);
    }

    let rows = column(projects.iter().map(|entry| lane(den, entry, start)));

    container(
        column![
            scale(den, start),
            gap(1, 6),
            scrollable(rows).height(Length::Fill),
            gap(1, 16),
            legend(palette),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(Padding::from([0, 40]).bottom(24))
    .into()
}

fn empty(den: &Den) -> Element<'_> {
    let palette = &den.palette;
    container(
        column![
            text("Nothing scheduled.")
                .size(22.0)
                .font(mono())
                .color(palette.fg),
            gap(1, 10),
            text("Add @due(2026-09-30) to a task in Neovim and it appears here.")
                .size(size::SECTION)
                .font(mono())
                .color(palette.muted),
        ]
        .width(Length::Shrink),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// The Monday on or before `date`.
///
/// Weeks are the unit because a personal roadmap is planned in weeks; days
/// would give eight times the columns and no more information.
pub fn week_start(date: Date) -> Date {
    let back = i64::from(date.weekday().to_monday_zero_offset());
    den_core::date::add_days(date, -back)
}

/// Which column a date falls in, or `None` when it is off the board.
///
/// Anything overdue clamps to column 0 rather than disappearing — an overdue
/// task is the most important thing a roadmap can tell you, so it must not fall
/// off the left edge.
pub fn column_of(due: Date, start: Date, weeks: usize) -> Option<usize> {
    let days = den_core::date::days_between(start, due);
    if days < 0 {
        return Some(0);
    }
    let index = (days / 7) as usize;
    (index < weeks).then_some(index)
}

/// The week ruler across the top.
fn scale(den: &Den, start: Date) -> Element<'_> {
    let palette = &den.palette;

    let mut ticks = row![gap(GUTTER, 1)].spacing(4).align_y(Alignment::Center);
    for week in 0..WEEKS {
        let monday = den_core::date::add_days(start, (week * 7) as i64);
        let current = week == 0;
        ticks = ticks.push(
            container(
                text(den_core::date::short(monday))
                    .size(size::META)
                    .font(mono())
                    .color(if current { palette.dim } else { palette.line2 }),
            )
            .width(Length::FillPortion(1))
            .padding(Padding::from([0, 6])),
        );
    }

    container(ticks)
        .padding(Padding::default().bottom(10))
        .into()
}

/// One project, with its dated work spread across the weeks.
fn lane<'a>(den: &'a Den, entry: &'a Entry, start: Date) -> Element<'a> {
    let palette = &den.palette;

    // Bucket the entry's open tasks by week.
    let mut weeks: Vec<Vec<&VaultTask>> = vec![Vec::new(); WEEKS];
    let mut undated = 0usize;
    for task in entry.tasks.iter().filter(|t| t.status() != Status::Done) {
        match task.due().and_then(den_core::date::parse) {
            Some(due) => match column_of(due, start, WEEKS) {
                Some(index) => weeks[index].push(task),
                None => undated += 1,
            },
            None => undated += 1,
        }
    }

    let name = row![
        glyph::view(glyph::for_entry(entry), 12.0, palette.line2),
        line_text(entry.title.clone(), size::ROW, palette.dim),
        text(if undated > 0 {
            format!("+{undated}")
        } else {
            String::new()
        })
        .size(size::META)
        .font(mono())
        .color(palette.line2),
    ]
    .spacing(9)
    .align_y(Alignment::Center);

    let mut cells = row![container(name).width(Length::Fixed(GUTTER)).clip(true)]
        .spacing(4)
        .align_y(Alignment::Center);

    for (index, tasks) in weeks.iter().enumerate() {
        cells = cells.push(cell(den, tasks, index == 0));
    }

    container(cells)
        .padding(Padding::from([5, 0]))
        .width(Length::Fill)
        .into()
}

/// One project-week. Empty weeks are a faint ground so the grid stays legible.
fn cell<'a>(den: &'a Den, tasks: &[&'a VaultTask], this_week: bool) -> Element<'a> {
    let palette = &den.palette;

    if tasks.is_empty() {
        let ground = palette.wash(palette.line, if this_week { 0.55 } else { 0.28 });
        return container(gap(Length::Fill, 30))
            .width(Length::FillPortion(1))
            .style(move |_| container::Style {
                background: Some(Background::Color(ground)),
                border: Border {
                    radius: crate::theme::radius::MARK.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into();
    }

    // Overdue dominates the cell's colour: it is the one thing here you cannot
    // afford to skim past.
    let overdue = tasks.iter().any(|task| {
        task.due()
            .and_then(den_core::date::parse)
            .is_some_and(|due| den_core::date::days_between(den.today, due) < 0)
    });
    let accent = if overdue {
        palette.rose
    } else if this_week {
        palette.firelight
    } else {
        palette.steel
    };

    let label = if tasks.len() == 1 {
        tasks[0].title().to_string()
    } else {
        format!("{} tasks", tasks.len())
    };
    let ground = palette.wash(accent, 0.18);

    crate::widget::hover_row(
        container(
            container(line_text(label, size::META, accent))
                .padding(Padding::from([7, 8]))
                .width(Length::Fill)
                .clip(true),
        )
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(ground)),
            border: Border {
                radius: crate::theme::radius::MARK.into(),
                ..Border::default()
            },
            ..container::Style::default()
        }),
        Message::SelectTask(crate::den::Location::of(tasks[0])),
        None,
        palette,
    )
}

/// What the three colours mean, because a chart that needs a manual is a bad
/// chart and one that needs nothing at all is rare.
fn legend(palette: &Palette) -> Element<'_> {
    let key = |color: iced::Color, name: &'static str| {
        row![
            crate::widget::swatch(color, 7.0),
            text(name)
                .size(size::META)
                .font(mono())
                .color(palette.muted),
        ]
        .spacing(7)
        .align_y(Alignment::Center)
    };

    row![
        key(palette.rose, "overdue"),
        key(palette.firelight, "this week"),
        key(palette.steel, "scheduled"),
    ]
    .spacing(20)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i16, m: i8, d: i8) -> Date {
        Date::new(y, m, d).expect("valid date")
    }

    #[test]
    fn weeks_start_on_monday() {
        // 16 Sep 2026 is a Wednesday; its week began on the 14th.
        assert_eq!(week_start(date(2026, 9, 16)), date(2026, 9, 14));
        // A Monday is its own week start.
        assert_eq!(week_start(date(2026, 9, 14)), date(2026, 9, 14));
        // A Sunday belongs to the week that began six days earlier.
        assert_eq!(week_start(date(2026, 9, 20)), date(2026, 9, 14));
    }

    #[test]
    fn dates_land_in_the_right_column() {
        let start = date(2026, 9, 14);
        assert_eq!(column_of(date(2026, 9, 14), start, 8), Some(0));
        assert_eq!(column_of(date(2026, 9, 20), start, 8), Some(0));
        assert_eq!(column_of(date(2026, 9, 21), start, 8), Some(1));
        assert_eq!(column_of(date(2026, 11, 1), start, 8), Some(6));
    }

    #[test]
    fn overdue_clamps_left_rather_than_vanishing() {
        let start = date(2026, 9, 14);
        // The most important thing a roadmap says must not fall off the edge.
        assert_eq!(column_of(date(2026, 8, 1), start, 8), Some(0));
        assert_eq!(column_of(date(2026, 9, 13), start, 8), Some(0));
    }

    #[test]
    fn dates_past_the_horizon_are_dropped() {
        let start = date(2026, 9, 14);
        assert_eq!(column_of(date(2027, 3, 1), start, 8), None);
        // The last column is inclusive, the one after it is not.
        assert_eq!(column_of(date(2026, 11, 8), start, 8), Some(7));
        assert_eq!(column_of(date(2026, 11, 9), start, 8), None);
    }
}
