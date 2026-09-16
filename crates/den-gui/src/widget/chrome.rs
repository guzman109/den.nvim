//! The window around whichever screen is showing: the day line, the status
//! line, the banner and the command palette.
//!
//! The toolbar band is gone. It held a search prompt and five outlined pills —
//! `#tags`, `due`, reload, settings, a panel toggle — at a size that was hard
//! to see and harder to hit, and it drew whether or not it had anything to say.
//! Search now lives in the day line and appears when it is being used; filters
//! appear there as chips only while set; reload is a file watch; settings is a
//! segment in the status line. Chrome that says nothing does not get drawn.

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Alignment, Background, Border, Length, Padding};

use crate::den::{Den, DueFilter, Message};
use crate::fonts::mono;
use crate::glyph;
use crate::screen::{Screen, TaskView, board, roadmap, table, today};
use crate::theme::radius;
use crate::widget::{Element, filler, gap, key_hint, muted, sidebar, size};

const STATUS_BAR: f32 = 34.0;

pub fn view(den: &Den) -> Element<'_> {
    let win = den.palette.win;

    let body = row![sidebar::view(den), main_area(den)].height(Length::Fill);
    let window = column![container(body).height(Length::Fill), status_bar(den)];

    let framed = container(window)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(win)),
            ..container::Style::default()
        });

    let mut layers = vec![framed.into()];
    if den.palette_open {
        layers.push(command_palette(den));
    }
    if let Some(ghost) = drag_ghost(den) {
        layers.push(ghost);
    }

    if layers.len() == 1 {
        layers.pop().expect("one layer")
    } else {
        stack(layers).into()
    }
}

/// The card being dragged, drawn under the cursor.
///
/// Without this a drag is invisible: the card stays where it was and only the
/// target column lights up, so there is nothing to tell you the app understood
/// the gesture. iced has no absolute positioning, so the ghost is a top-left
/// aligned layer pushed into place with padding — which is exactly what
/// absolute positioning is, spelled differently.
fn drag_ghost(den: &Den) -> Option<Element<'_>> {
    let location = den.drag.as_ref()?;
    let task = den.vault.task_at(&location.file, location.line)?;
    let palette = den.palette;

    let card = container(
        text(task.title().to_string())
            .size(size::ROW)
            .font(mono())
            .color(palette.fg)
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .padding(Padding::from([10, 13]))
    .max_width(260)
    .clip(true)
    .style(move |_| container::Style {
        background: Some(Background::Color(palette.card_alt)),
        border: Border {
            color: palette.firelight,
            width: 1.0,
            radius: radius::CARD.into(),
        },
        ..container::Style::default()
    });

    // Offset so the card hangs below-right of the pointer rather than under it,
    // which would put the cursor on top of the text it is carrying.
    Some(
        container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: (den.cursor.y + 12.0).max(0.0),
                left: (den.cursor.x + 12.0).max(0.0),
                right: 0.0,
                bottom: 0.0,
            })
            .into(),
    )
}

fn main_area(den: &Den) -> Element<'_> {
    let content: Element<'_> = match &den.screen {
        Screen::Today => today::view(den),
        Screen::Tasks => match den.task_view {
            TaskView::Board => board::view(den),
            TaskView::Table => table::view(den),
        },
        Screen::Roadmap => roadmap::view(den),
        Screen::Settings(settings) => settings
            .view(
                &den.palette,
                &den.vault,
                &den.link,
                den.root_label(),
                &den.keymap,
                den.mode,
            )
            .map(Message::Settings),
    };

    let mut area = column![day_line(den)];
    if let Some(notice) = &den.notice {
        area = area.push(container(banner(den, notice)).padding(Padding::from([0, 40]).bottom(10)));
    }
    area = area.push(content);

    container(area)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The header: what day it is, what is outstanding, and the way into search.
///
/// This replaces the toolbar. When nothing is filtered there is nothing here
/// but the date and a count, which is the point — the old bar drew five
/// controls whether or not any of them was doing anything.
fn day_line(den: &Den) -> Element<'_> {
    let palette = den.palette;
    let counts = den.vault.counts();
    let open = counts.total.saturating_sub(counts.done);

    let mut line = row![
        text(den_core::date::long(den.today))
            .size(size::ROW)
            .font(mono())
            .color(palette.dim),
        text(format!("{open} open"))
            .size(size::META)
            .font(mono())
            .color(palette.muted),
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    line = line.push(filler());

    // A filter is visible only while it is on, and carries its own way off.
    if let Some(tag) = &den.tag_filter {
        line = line.push(chip_filter(
            format!("#{tag}"),
            Message::SetTagFilter(None),
            &palette,
        ));
    }
    if den.due_filter != DueFilter::Any {
        line = line.push(chip_filter(
            den.due_filter.as_str().to_string(),
            Message::SetDueFilter(DueFilter::Any),
            &palette,
        ));
    }

    // The two readings of the tasks screen, switched in place. A destination
    // each would have split one dataset across two half-empty screens, which is
    // the mistake this redesign exists to undo.
    if den.kind() == crate::screen::Kind::Tasks {
        line = line.push(view_switch(den));
    }

    line = line.push(search(den));

    container(line)
        .width(Length::Fill)
        .padding(Padding::from([22, 40]))
        .into()
}

/// `board | table`, as one control.
fn view_switch(den: &Den) -> Element<'_> {
    let palette = den.palette;

    let options = TaskView::ALL.map(|view| {
        let active = den.task_view == view;
        button(
            text(view.as_str())
                .size(size::META)
                .font(mono())
                .color(if active { palette.fg } else { palette.muted }),
        )
        .padding(Padding::from([4, 10]))
        .on_press(Message::SetTaskView(view))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: (active || hovered).then_some(Background::Color(palette.card_alt)),
                text_color: palette.fg,
                border: Border {
                    radius: radius::MARK.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
    });

    container(row(options).spacing(2))
        .padding(2)
        .style(move |_| container::Style {
            background: Some(Background::Color(palette.card)),
            border: Border {
                radius: radius::CONTROL.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

fn chip_filter<'a>(label: String, clear: Message, palette: &crate::theme::Palette) -> Element<'a> {
    let palette = *palette;
    button(
        row![
            text(label).size(size::META).font(mono()),
            glyph::view(glyph::PLUS, 10.0, palette.line2),
        ]
        .spacing(7)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([3, 8]))
    .on_press(clear)
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if hovered {
                palette.card_alt
            } else {
                palette.card
            })),
            text_color: palette.dim,
            border: Border {
                radius: radius::MARK.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .into()
}

fn search(den: &Den) -> Element<'_> {
    let palette = den.palette;

    if den.search_active {
        return row![
            glyph::view(glyph::SEARCH, 13.0, palette.muted),
            text_input("find task", &den.query)
                .id(iced::widget::Id::new(crate::den::SEARCH_INPUT))
                .on_input(Message::QueryChanged)
                .size(size::ROW)
                .font(mono())
                .padding(0)
                .width(Length::Fixed(240.0))
                .style(move |_, _| text_input::Style {
                    background: Background::Color(iced::Color::TRANSPARENT),
                    border: Border::default(),
                    icon: palette.muted,
                    placeholder: palette.muted,
                    value: palette.fg,
                    selection: palette.wash(palette.firelight, 0.35),
                }),
            key_hint("esc", &palette),
        ]
        .spacing(9)
        .align_y(Alignment::Center)
        .into();
    }

    crate::widget::hover_cell(
        container(
            row![
                glyph::view(glyph::SEARCH, 13.0, palette.muted),
                text("search").size(12.0).font(mono()).color(palette.muted),
                key_hint("/", &palette),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([4, 8])),
        Message::SearchFocused,
        &palette,
    )
}

fn banner<'a>(den: &'a Den, notice: &'a crate::den::Notice) -> Element<'a> {
    let palette = den.palette;
    let accent = if notice.is_error {
        palette.rose
    } else {
        palette.olive
    };
    container(
        row![
            text(if notice.is_error { "!" } else { "✓" })
                .size(size::SMALL)
                .font(mono())
                .color(accent),
            text(notice.text.clone())
                .size(size::SMALL)
                .font(mono())
                .color(palette.fg)
                .width(Length::Fill),
            button(text("×").size(size::BODY).font(mono()).color(palette.muted))
                .padding(0)
                .on_press(Message::DismissNotice)
                .style(|_, _| button::Style::default()),
        ]
        .spacing(9)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding::from([7, 12]))
    .style(move |_| container::Style {
        background: Some(Background::Color(palette.wash(accent, 0.12))),
        border: Border {
            color: palette.wash(accent, 0.4),
            width: 1.0,
            radius: radius::CARD.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// The status line: capture on the left, the app's state on the right.
///
/// Segmented the way a vim statusline is, because that is the vocabulary this
/// app's users already read. What it does *not* say is as deliberate as what it
/// does: running without Neovim is `standalone`, in the neutral colour, not a
/// warning — Den writes the Markdown itself, so an unattached Den is fully
/// capable. The only genuinely read-only state is a vault that refuses writes,
/// and that gets the lock and the rose.
fn status_bar(den: &Den) -> Element<'_> {
    let palette = den.palette;
    let read_only = den.is_read_only();

    let capture: Element<'_> = if read_only {
        row![
            glyph::view(glyph::LOCK, 12.0, palette.line2),
            text("capture unavailable")
                .size(12.0)
                .font(mono())
                .color(palette.line2),
        ]
        .spacing(9)
        .align_y(Alignment::Center)
        .into()
    } else if let Some(draft) = &den.capture {
        // Open: the prompt becomes the input, in place, so the eye does not
        // have to move and a modal never covers the work.
        container(
            row![
                glyph::view(glyph::PLUS, 12.0, palette.firelight),
                text_input("what's on your mind?", draft)
                    .id(iced::widget::Id::new(crate::den::CAPTURE_INPUT))
                    .on_input(Message::CaptureChanged)
                    .on_submit(Message::CaptureCommitted)
                    .size(12.0)
                    .font(mono())
                    .padding(0)
                    .width(Length::Fixed(420.0))
                    .style(move |_, _| text_input::Style {
                        background: Background::Color(iced::Color::TRANSPARENT),
                        border: Border::default(),
                        icon: palette.muted,
                        placeholder: palette.muted,
                        value: palette.fg,
                        selection: palette.wash(palette.firelight, 0.35),
                    }),
                key_hint("enter", &palette),
                key_hint("esc", &palette),
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([5, 10]))
        .style(move |_| container::Style {
            background: Some(Background::Color(palette.glow())),
            border: Border {
                radius: radius::CONTROL.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
    } else {
        // A warm ground, not a solid accent: the capture block anchors the line
        // the way vim's mode block does without competing with the lit task.
        crate::widget::hover_cell(
            container(
                row![
                    glyph::view(glyph::PLUS, 12.0, palette.firelight),
                    text("capture a thought")
                        .size(12.0)
                        .font(mono())
                        .color(palette.dim),
                    key_hint("o", &palette),
                ]
                .spacing(9)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([5, 10]))
            .style(move |_| container::Style {
                background: Some(Background::Color(palette.glow_soft())),
                border: Border {
                    radius: radius::CONTROL.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
            Message::CaptureOpened,
            &palette,
        )
    };

    let (link_mark, link_text, link_color) = match &den.link {
        crate::den::Link::Attached => (glyph::NVIM, "nvim".to_string(), palette.olive),
        crate::den::Link::Standalone => (glyph::NVIM, "standalone".to_string(), palette.muted),
        crate::den::Link::Unreachable => {
            (glyph::NVIM, "nvim unreachable".to_string(), palette.rose)
        }
    };

    let mut right = row![].spacing(14).align_y(Alignment::Center);
    if read_only {
        right = right.push(segment(
            glyph::LOCK,
            "read-only · vault not writable".to_string(),
            palette.rose,
            None,
            &palette,
        ));
        right = right.push(divider(&palette));
    }
    right = right.push(segment(
        glyph::VAULT,
        den.root_label(),
        palette.muted,
        None,
        &palette,
    ));
    right = right.push(divider(&palette));
    right = right.push(segment(link_mark, link_text, link_color, None, &palette));
    right = right.push(divider(&palette));
    right = right.push(segment(
        glyph::CLOCK,
        den_core::date::clock(den.focus.remaining),
        if den.focus.running {
            palette.firelight
        } else {
            palette.muted
        },
        Some(if den.focus.running {
            Message::FocusPause
        } else {
            Message::FocusStart
        }),
        &palette,
    ));
    right = right.push(divider(&palette));
    // A control, not a readout: the variant name told you something you could
    // already see, and the one thing you might want from that corner is the way
    // into the settings that change it.
    right = right.push(segment(
        glyph::SETTINGS,
        "settings".to_string(),
        if den.kind() == crate::screen::Kind::Settings {
            palette.fg
        } else {
            palette.muted
        },
        Some(Message::Show(crate::screen::Kind::Settings)),
        &palette,
    ));

    container(
        row![capture, filler(), right]
            .spacing(14)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(STATUS_BAR)
    .padding(Padding::from([0, 14]))
    .style(move |_| container::Style {
        background: Some(Background::Color(palette.panel)),
        ..container::Style::default()
    })
    .into()
}

fn segment<'a>(
    mark: &'static str,
    label: String,
    color: iced::Color,
    message: Option<Message>,
    palette: &crate::theme::Palette,
) -> Element<'a> {
    let content = row![
        glyph::view(mark, 12.0, color),
        text(label).size(size::META).font(mono()).color(color),
    ]
    .spacing(7)
    .align_y(Alignment::Center);

    match message {
        Some(message) => crate::widget::hover_cell(
            container(content).padding(Padding::from([3, 5])),
            message,
            palette,
        ),
        None => container(content).padding(Padding::from([3, 5])).into(),
    }
}

fn divider<'a>(palette: &crate::theme::Palette) -> Element<'a> {
    let color = palette.wash(palette.line2, 0.55);
    container(gap(1, 13))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            ..container::Style::default()
        })
        .into()
}

fn command_palette(den: &Den) -> Element<'_> {
    let palette = den.palette;
    let commands = den.commands();

    let rows = column(
        commands
            .iter()
            .enumerate()
            .take(12)
            .map(|(index, command)| {
                button(
                    row![
                        text(command.label.clone())
                            .size(size::TASK)
                            .font(mono())
                            .width(Length::Fill),
                        muted(command.hint.clone(), &palette),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding(Padding::from([6, 10]))
                .on_press(Message::RunCommand(index))
                .style(move |_, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: hovered.then_some(Background::Color(palette.card_alt)),
                        text_color: palette.fg,
                        border: Border {
                            radius: radius::MARK.into(),
                            ..Border::default()
                        },
                        ..button::Style::default()
                    }
                })
                .into()
            }),
    )
    .spacing(2);

    let sheet = container(
        column![
            text_input("Type a command…", &den.palette_query)
                .on_input(Message::PaletteQuery)
                .size(size::TASK)
                .font(mono())
                .padding(Padding::from([8, 10]))
                .style(move |_, _| text_input::Style {
                    background: Background::Color(palette.win),
                    border: Border {
                        color: palette.line,
                        width: 1.0,
                        radius: radius::CONTROL.into()
                    },
                    icon: palette.muted,
                    placeholder: palette.muted,
                    value: palette.fg,
                    selection: palette.wash(palette.hero, 0.35),
                }),
            scrollable(rows).height(Length::Shrink),
        ]
        .spacing(8),
    )
    .width(520)
    .padding(10)
    .style(move |_| container::Style {
        background: Some(Background::Color(palette.panel)),
        border: Border {
            color: palette.line2,
            width: 1.0,
            radius: radius::CARD.into(),
        },
        ..container::Style::default()
    });

    // A click anywhere off the sheet closes it.
    mouse_area(
        container(container(sheet).align_top(Length::Fill))
            .center_x(Length::Fill)
            .height(Length::Fill)
            .padding(Padding::from([90, 0]))
            .style(move |_| container::Style {
                background: Some(Background::Color(palette.wash(palette.ramp.base0, 0.55))),
                ..container::Style::default()
            }),
    )
    .on_press(Message::TogglePalette)
    .into()
}
