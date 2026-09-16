//! Today — the daily driver, and the answer to "what am I doing right now".
//!
//! This screen replaces home, list and table. Those three showed the same
//! dataset three ways, which is why none of them ever had enough to fill a
//! window: the emptiness was a content problem being treated as a layout
//! problem. One screen with a point of view has more to say than three
//! without one.
//!
//! The layout is the design's central idea. One surface is lit — the task in
//! the firelight — and everything else is bare rows on the den floor. The
//! hierarchy comes from that *difference*, not from repeating a card with a
//! border and a radius until the screen is full.
//!
//! Both columns run the whole height and are anchored top and bottom, so the
//! air in the middle reads as deliberate rather than as a layout that ran out
//! of things to say.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Length, Padding};

use den_core::vault::{Status, Task as VaultTask};

use crate::den::{Den, Message};
use crate::fonts::mono;
use crate::theme::{Palette, Temperature};
use crate::widget::{
    Element, chip, filler, gap, heading, key_hint, line_text, lit_surface, lit_surface_empty, size,
    status_box,
};

/// The width of the cooling / this-week column.
const ASIDE: f32 = 250.0;

pub fn view(den: &Den) -> Element<'_> {
    let lit = live(den);
    let lit_at = lit.map(|task| (task.file.clone(), task.line));

    let left = column![
        heading("now", None, &den.palette),
        gap(1, 14),
        now(den, lit),
        gap(1, 42),
        heading("next", Some(next(den, lit_at.as_ref()).len()), &den.palette),
        gap(1, 14),
        column(
            next(den, lit_at.as_ref())
                .into_iter()
                .map(|task| task_row(den, task, false)),
        ),
    ]
    .width(Length::Fill);

    let closed = cooling(den);
    let right = column![
        heading("cooling", Some(closed.len()), &den.palette),
        gap(1, 14),
        column(closed.into_iter().map(|task| task_row(den, task, true))),
        filler(),
        week(den),
    ]
    .width(Length::Fixed(ASIDE));

    container(row![left, right].spacing(40).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::from([0, 40]).bottom(24))
        .into()
}

/// The task in the light.
///
/// The focus timer's task wins when one is running — if you started a clock on
/// something, that is what you are doing, whatever the file ordering says.
/// Otherwise it is the first `@status(doing)` task by `@order`.
fn live(den: &Den) -> Option<&VaultTask> {
    if let Some(location) = &den.focus.task
        && let Some(task) = den.vault.task_at(&location.file, location.line)
    {
        return Some(task);
    }
    den.vault
        .tasks()
        .filter(|task| task.status() == Status::Doing)
        .min_by_key(|task| task.parsed.order.unwrap_or(u64::MAX))
}

/// What is queued.
///
/// Other `@status(doing)` tasks come first: den.nvim allows several at once, so
/// the ones that are not lit have to stay visible. They keep the warm `[~]`
/// mark without taking the light.
fn next<'a>(den: &'a Den, lit: Option<&(std::path::PathBuf, usize)>) -> Vec<&'a VaultTask> {
    let mut tasks: Vec<&VaultTask> = den
        .vault
        .tasks()
        .filter(|task| task.status() != Status::Done)
        .filter(|task| !lit.is_some_and(|(file, line)| task.file == *file && task.line == *line))
        .collect();

    tasks.sort_by_key(|task| {
        (
            // Started but unlit first, then the backlog.
            task.status() != Status::Doing,
            // A due date outranks file order: it is the only hard constraint.
            task.due().map(str::to_string),
            task.parsed.order.unwrap_or(u64::MAX),
        )
    });
    tasks
}

/// What closed today, newest first.
fn cooling(den: &Den) -> Vec<&VaultTask> {
    den.vault
        .tasks()
        .filter(|task| task.status() == Status::Done)
        .filter(|task| den.history.closed_on(task) == Some(den.today))
        .collect()
}

/// The lit surface, or the invitation when nothing is running.
fn now<'a>(den: &'a Den, task: Option<&'a VaultTask>) -> Element<'a> {
    let palette = &den.palette;
    let Some(task) = task else {
        // An empty state names the next action rather than drawing an empty box.
        return lit_surface_empty(
            column![
                text("Nothing lit.")
                    .size(22.0)
                    .font(mono())
                    .color(palette.fg),
                gap(1, 10),
                text("Start something from next, or press o to put down whatever is already on your mind.")
                    .size(size::SECTION)
                    .font(mono())
                    .color(palette.muted)
                    .width(Length::Fixed(430.0)),
            ],
            palette,
        );
    };

    let location = crate::den::Location::of(task);
    let entry = den
        .vault
        .entry(&task.file)
        .map(|entry| entry.title.clone())
        .unwrap_or_default();

    let mut meta = row![
        text(entry)
            .size(size::META)
            .font(mono())
            .color(palette.muted)
    ]
    .spacing(20)
    .align_y(Alignment::Center);

    if let Some(due) = task.due().and_then(den_core::date::parse) {
        // Overdue is the one thing on this screen allowed to be alarming.
        let overdue = den_core::date::days_between(den.today, due) < 0;
        meta = meta.push(
            text(den_core::date::relative(due, den.today))
                .size(size::META)
                .font(mono())
                .color(if overdue { palette.rose } else { palette.gold }),
        );
    }
    for tag in task.tags() {
        meta = meta.push(chip(tag, palette.slot(den.vault.tags.slot(tag))));
    }

    lit_surface(
        column![
            // The largest text in the app. You should be able to read what you
            // are doing without leaning in.
            text(task.title().to_string())
                .size(size::LIVE)
                .font(mono())
                .color(palette.fg),
            gap(1, 14),
            meta,
            gap(1, 26),
            actions(den, location),
        ],
        palette,
    )
}

fn actions(den: &Den, location: crate::den::Location) -> Element<'_> {
    let palette = &den.palette;

    let clock = row![
        text(den_core::date::clock(den.focus.remaining))
            .size(size::CLOCK)
            .font(mono())
            .color(palette.firelight),
    ]
    .align_y(Alignment::Center);

    row![
        clock,
        act(
            if den.focus.running { "pause" } else { "start" },
            "f",
            if den.focus.running {
                Message::FocusPause
            } else {
                Message::FocusStart
            },
            palette,
        ),
        act(
            "done",
            "space",
            Message::SetStatus(location.clone(), Status::Done),
            palette,
        ),
        filler(),
        act(
            "open in neovim",
            "enter",
            Message::OpenInNvim(location),
            palette,
        ),
    ]
    .spacing(20)
    .align_y(Alignment::Center)
    .into()
}

/// A text control with its key beside it.
///
/// Every keyboard action gets a visible control: keyboard-first must not mean
/// mouse-impossible, or the app is only usable by someone who has read the
/// keymap.
fn act<'a>(name: &'a str, key: &'a str, message: Message, palette: &Palette) -> Element<'a> {
    crate::widget::hover_row(
        row![
            text(name).size(12.0).font(mono()).color(palette.dim),
            key_hint(key, palette),
        ]
        .spacing(7)
        .align_y(Alignment::Center),
        message,
        None,
        palette,
    )
}

/// A bare row on the floor. No card, no border — the difference between this
/// and the lit surface above *is* the hierarchy.
/// The task title, or an input when this row is the one being edited.
///
/// Editing happens in place rather than in a dialog: the row keeps its position
/// in the list, so you can see what you are renaming relative to everything
/// around it.
fn title_cell<'a>(
    den: &'a Den,
    location: &crate::den::Location,
    task: &'a den_core::vault::Task,
    color: iced::Color,
) -> Element<'a> {
    let palette = &den.palette;
    match &den.editing {
        Some((editing, draft)) if editing == location => iced::widget::text_input("", draft)
            .id(iced::widget::Id::new(crate::den::EDIT_INPUT))
            .on_input(Message::EditChanged)
            .on_submit(Message::EditCommitted)
            .size(size::ROW)
            .font(mono())
            .padding(0)
            .width(Length::Fill)
            .style(move |_, _| iced::widget::text_input::Style {
                background: iced::Background::Color(iced::Color::TRANSPARENT),
                border: iced::Border::default(),
                icon: palette.muted,
                placeholder: palette.muted,
                value: palette.fg,
                selection: palette.wash(palette.firelight, 0.35),
            })
            .into(),
        _ => line_text(task.title().to_string(), size::ROW, color),
    }
}

fn task_row<'a>(den: &'a Den, task: &'a VaultTask, closed: bool) -> Element<'a> {
    let palette = &den.palette;
    let temperature = Temperature::of(task.status(), false);

    let trailing = if closed {
        match den.history.closed_on(task) {
            Some(date) if date == den.today => "today".to_string(),
            Some(date) => den_core::date::short(date),
            None => String::new(),
        }
    } else if task.status() == Status::Doing {
        "started".to_string()
    } else {
        den.vault
            .entry(&task.file)
            .map(|entry| entry.title.clone())
            .unwrap_or_default()
    };

    let location = crate::den::Location::of(task);
    let mut line = row![
        status_box(task.status(), temperature, palette),
        title_cell(
            den,
            &location,
            task,
            if closed { palette.muted } else { palette.fg },
        ),
    ]
    .spacing(13)
    .align_y(Alignment::Center);

    for tag in task.tags() {
        line = line.push(chip(tag, palette.slot(den.vault.tags.slot(tag))));
    }
    line = line.push(
        text(trailing)
            .size(size::META)
            .font(mono())
            .color(palette.muted),
    );

    crate::widget::hover_row(
        container(line).padding(Padding::from([10, 10])).clip(true),
        Message::SelectTask(location),
        None,
        palette,
    )
}

/// The week, as a block sparkline.
///
/// Block glyphs rather than a canvas: it is the terminal's own way of drawing a
/// small series, it costs one text node, and it survives any scale factor.
fn week(den: &Den) -> Element<'_> {
    let palette = &den.palette;
    let counts = den.vault.counts();
    let projects = den.vault.active().count();
    let days = closures_this_week(den);
    let closed: usize = days.iter().sum();

    column![
        heading("this week", None, palette),
        gap(1, 14),
        text(sparkline(&days))
            .size(16.0)
            .font(mono())
            .color(palette.olive),
        gap(1, 12),
        text(format!(
            "{} closed since Monday\n{} open across {projects} projects",
            closed,
            counts.total.saturating_sub(counts.done),
        ))
        .size(size::META)
        .font(mono())
        .color(palette.muted),
    ]
    .into()
}

/// How many tasks closed on each of the last seven days, oldest first.
fn closures_this_week(den: &Den) -> Vec<usize> {
    (0..7)
        .map(|back| den_core::date::add_days(den.today, back - 6))
        .map(|day| {
            den.vault
                .tasks()
                .filter(|task| den.history.closed_on(task) == Some(day))
                .count()
        })
        .collect()
}

/// `▁▃▂▅▃▁▂` — one block per day, scaled to the busiest.
pub fn sparkline(days: &[usize]) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let peak = days.iter().copied().max().unwrap_or(0);
    if peak == 0 {
        return BLOCKS[0].to_string().repeat(days.len());
    }
    days.iter()
        .map(|count| {
            let step = (count * (BLOCKS.len() - 1)) / peak;
            BLOCKS[step]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sparkline_scales_to_its_busiest_day() {
        assert_eq!(sparkline(&[0, 0, 0]), "▁▁▁");
        assert_eq!(sparkline(&[0, 4]), "▁█");
        // The peak always reaches the top, whatever the absolute numbers.
        assert_eq!(sparkline(&[1, 2]).chars().last(), Some('█'));
        assert_eq!(sparkline(&[50, 100]).chars().last(), Some('█'));
    }

    #[test]
    fn a_sparkline_has_one_mark_per_day() {
        for len in 1..14 {
            let days: Vec<usize> = (0..len).collect();
            assert_eq!(sparkline(&days).chars().count(), len);
        }
    }
}
