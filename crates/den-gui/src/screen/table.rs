//! Notes — every task in the vault, as one table.
//!
//! A table is the right shape here for a reason the other views did not have:
//! it is the only screen where you compare tasks *across* notes. Today answers
//! "what am I doing", the board answers "what state is everything in", and this
//! answers "show me all of it, lined up, so I can scan a column".
//!
//! Columns are the comparison. Every cell in a column is the same kind of fact,
//! left-aligned on a monospace grid, so the eye can run down one attribute
//! without reading the rows. That is what a table is for, and it is why this
//! replaced the old list view rather than sitting beside it.
//!
//! Filters and search apply here, so the table is also the answer to a query.

use iced::widget::{column, container, row, scrollable, text};
use iced::{Alignment, Length, Padding};

use den_core::vault::{Status, Task as VaultTask};

use crate::den::{Den, Message};
use crate::fonts::mono;
use crate::theme::{Palette, Temperature};
use crate::widget::{Element, chip, gap, line_text, size, status_box};

/// Column widths as portions, so the table holds its shape at any scale and any
/// window size. Fixed pixel columns broke the moment the zoom moved.
const TASK: u16 = 44;
const NOTE: u16 = 20;
const TAGS: u16 = 22;
const DUE: u16 = 14;

pub fn view(den: &Den) -> Element<'_> {
    let palette = &den.palette;
    let tasks = den.visible_tasks();

    if tasks.is_empty() {
        return empty(den);
    }

    let body = column(tasks.iter().map(|task| task_row(den, task)));

    container(
        column![
            header(palette),
            gap(1, 4),
            scrollable(body).height(Length::Fill),
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
    let filtered = den.tag_filter.is_some() || !den.query.is_empty();

    container(
        column![
            text(if filtered {
                "Nothing matches."
            } else {
                "No tasks yet."
            })
            .size(22.0)
            .font(mono())
            .color(palette.fg),
            gap(1, 10),
            text(if filtered {
                "Clear the filter with esc, or search for something else."
            } else {
                "Create a note with :DenProject in Neovim, or run Den with --demo."
            })
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

/// Column names, in the same lowercase as every other heading.
fn header(palette: &Palette) -> Element<'_> {
    let cell = |name: &'static str, portion: u16| {
        text(name)
            .size(size::META)
            .font(mono())
            .color(palette.line2)
            .width(Length::FillPortion(portion))
    };

    container(
        row![
            gap(26, 1),
            cell("task", TASK),
            cell("note", NOTE),
            cell("tags", TAGS),
            cell("due", DUE),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 10]).bottom(10))
    .into()
}

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

fn task_row<'a>(den: &'a Den, task: &'a VaultTask) -> Element<'a> {
    let palette = &den.palette;
    let done = task.status() == Status::Done;
    let temperature = Temperature::of(task.status(), false);

    let note = den
        .vault
        .entry(&task.file)
        .map(|entry| entry.title.clone())
        .unwrap_or_default();

    let mut tags = row![].spacing(9).align_y(Alignment::Center);
    for tag in task.tags() {
        tags = tags.push(chip(tag, palette.slot(den.vault.tags.slot(tag))));
    }

    let due: Element<'_> = match task.due().and_then(den_core::date::parse) {
        Some(date) => {
            let overdue = den_core::date::days_between(den.today, date) < 0;
            text(den_core::date::short(date))
                .size(size::META)
                .font(mono())
                .color(if overdue { palette.rose } else { palette.gold })
                .into()
        }
        // A dash, not an empty cell: the column stays readable as a column.
        None => text("—")
            .size(size::META)
            .font(mono())
            .color(palette.line2)
            .into(),
    };

    let location = crate::den::Location::of(task);
    let line = row![
        container(status_box(task.status(), temperature, palette)).width(Length::Fixed(26.0)),
        container(title_cell(
            den,
            &location,
            task,
            if done { palette.muted } else { palette.fg },
        ))
        .width(Length::FillPortion(TASK)),
        container(line_text(note, size::META, palette.muted)).width(Length::FillPortion(NOTE)),
        container(tags).width(Length::FillPortion(TAGS)).clip(true),
        container(due).width(Length::FillPortion(DUE)).clip(true),
    ]
    .spacing(16)
    .align_y(Alignment::Center);

    crate::widget::hover_row(
        container(line).padding(Padding::from([9, 10])).clip(true),
        Message::SelectTask(location.clone()),
        (den.selected.as_ref() == Some(&location)).then_some(palette.card_alt),
        palette,
    )
}
