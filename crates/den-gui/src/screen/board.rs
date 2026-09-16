//! Board — three states, and the one place you can move work by hand.
//!
//! This is the screen that justifies a window. Everything else Den does,
//! Neovim does as well or better; dragging a card from backlog to doing is the
//! thing a terminal cannot give you.
//!
//! The three columns are deliberately drawn *differently* rather than as three
//! copies of one card kit: `doing` is a warm region holding warm cards,
//! `backlog` is cold and bare, `done` recedes. With the headings covered you
//! can still tell which column is which, which is the test the old board failed.
//!
//! iced has no drag-and-drop, so it is assembled from `mouse_area`: press on a
//! card starts a drag, moving over a column arms it as the target, release
//! commits through the same guarded write path every other mutation uses.

use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Background, Border, Length, Padding};

use den_core::vault::{Status, Task as VaultTask};

use crate::den::{Den, Location, Message};
use crate::fonts::mono;
use crate::theme::{Palette, Temperature, radius};
use crate::widget::{Element, chip, gap, heading, size};

pub fn view(den: &Den) -> Element<'_> {
    let columns = [Status::Backlog, Status::Doing, Status::Done].map(|status| lane(den, status));

    container(row(columns).spacing(22).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::from([0, 40]).bottom(24))
        .into()
}

fn lane(den: &Den, status: Status) -> Element<'_> {
    let palette = &den.palette;
    let tasks = den.tasks_with_status(status);
    let armed = den.drag.is_some() && den.drop_target == Some(status);

    let cards = column(tasks.iter().map(|task| card(den, task, status))).spacing(2);

    // The column is the drop target, so it has to be a region you can see and
    // aim at — a ground that runs the full height, not one that stops under the
    // last card. `doing` carries a whisper of the warmth its cards have.
    let ground = if armed {
        palette.wash(palette.firelight, 0.22)
    } else if status == Status::Doing {
        palette.glow_soft()
    } else {
        palette.wash(palette.line, 0.30)
    };

    let region = container(scrollable(cards).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(6)
        .style(move |_| container::Style {
            background: Some(Background::Color(ground)),
            border: Border {
                radius: radius::CARD.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

    // Arming happens on hover rather than on enter/exit so a drag that moves
    // fast across a column still registers.
    let droppable = mouse_area(region)
        .on_move(move |_| Message::DragOver(status))
        .on_release(Message::DragDropped);

    column![
        container(heading(name(status), Some(tasks.len()), palette))
            .padding(Padding::default().bottom(12)),
        droppable,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The column's own word, which is the status the file records — not Today's
/// temporal framing. The vocabulary must never lie about what is on disk.
fn name(status: Status) -> &'static str {
    match status {
        Status::Backlog => "backlog",
        Status::Doing => "doing",
        Status::Done => "done",
    }
}

/// The card as it appears under the cursor mid-drag.
///
/// The same element the column draws, not a stand-in: a drag that shows you a
/// different, smaller thing than the one you grabbed reads as a glitch. It is
/// drawn on its own opaque ground with a shadow so it clearly sits *above* the
/// board rather than in it.
pub fn ghost<'a>(den: &'a Den, task: &'a VaultTask) -> Element<'a> {
    let palette = den.palette;
    container(body(den, task, task.status()))
        .width(Length::Fixed(300.0))
        .padding(Padding::from([14, 14]))
        .style(move |_| container::Style {
            background: Some(Background::Color(palette.card_alt)),
            border: Border {
                color: palette.firelight,
                width: 1.0,
                radius: radius::CARD.into(),
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.35,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 6.0),
                blur_radius: 20.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// A card's contents: title, then tags and either a due date or its note.
fn body<'a>(den: &'a Den, task: &'a VaultTask, status: Status) -> Element<'a> {
    let palette = &den.palette;
    let hot = status == Status::Doing;
    let out = status == Status::Done;

    let note = den
        .vault
        .entry(&task.file)
        .map(|entry| entry.title.clone())
        .unwrap_or_default();

    let mut meta = row![].spacing(12).align_y(Alignment::Center);
    for tag in task.tags() {
        meta = meta.push(chip(tag, palette.slot(den.vault.tags.slot(tag))));
    }
    match task.due().and_then(den_core::date::parse) {
        Some(due) => {
            let overdue = den_core::date::days_between(den.today, due) < 0;
            meta = meta.push(
                text(den_core::date::relative(due, den.today))
                    .size(size::META)
                    .font(mono())
                    .color(if overdue { palette.rose } else { palette.gold }),
            );
        }
        None => {
            meta = meta.push(
                text(note)
                    .size(size::META)
                    .font(mono())
                    .color(if out { palette.line2 } else { palette.muted })
                    .wrapping(text::Wrapping::None),
            );
        }
    }

    column![
        text(task.title().to_string())
            .size(if hot { size::SECTION } else { size::ROW })
            .font(mono())
            .color(if out { palette.muted } else { palette.fg }),
        gap(1, 9),
        meta,
    ]
    .into()
}

fn card<'a>(den: &'a Den, task: &'a VaultTask, status: Status) -> Element<'a> {
    let palette = &den.palette;
    let location = Location::of(task);
    let dragging = den.drag.as_ref() == Some(&location);
    let hot = status == Status::Doing;
    let body = body(den, task, status);

    let surface: Element<'_> = if hot {
        // The same firelight as the live task, through the same stops, so the
        // board and Today agree about where the light comes from — and so light
        // variants brighten rather than darken.
        let palette = *palette;
        let (hot_stop, _, base) = palette.firelight_stops();
        container(body)
            .width(Length::Fill)
            .padding(Padding::from([14, 14]))
            .style(move |_| container::Style {
                background: Some(Background::Gradient(
                    iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_4))
                        .add_stop(0.0, hot_stop)
                        .add_stop(1.0, base)
                        .into(),
                )),
                border: Border {
                    radius: radius::CARD.into(),
                    ..Border::default()
                },
                shadow: palette.lift(),
                ..container::Style::default()
            })
            .into()
    } else {
        let palette = *palette;
        container(body)
            .width(Length::Fill)
            .padding(Padding::from([12, 13]))
            .style(move |_| container::Style {
                background: dragging.then_some(Background::Color(palette.card_alt)),
                border: Border {
                    radius: radius::CARD.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into()
    };

    mouse_area(surface)
        .on_press(Message::DragStarted(location.clone()))
        .on_release(Message::DragDropped)
        .on_right_press(Message::SelectTask(location))
        .into()
}

/// The status a task carries, as a temperature, for anything that needs to
/// colour by state rather than by column.
pub fn temperature(task: &VaultTask, lit: bool) -> Temperature {
    Temperature::of(task.status(), lit)
}

/// Kept for Notes, which draws the burndown as a panel.
pub fn column_accent(status: Status, palette: &Palette) -> iced::Color {
    match status {
        Status::Backlog => palette.muted,
        Status::Doing => palette.firelight,
        Status::Done => palette.olive,
    }
}
