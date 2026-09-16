//! The left panel: the app mark, three destinations, the vault, the tags.
//!
//! What used to be here and is not any more:
//!
//! - **The ALL-CAPS section bands.** `VIEW`, `WORKSPACE`, `TAGS`, `HEARTH` —
//!   four shouted labels with rules trailing off them, in a 214px column. They
//!   were the app's most pervasive generic tell and carried no information a
//!   lowercase word does not.
//! - **The HEARTH meters.** Two progress bars nobody acted on. What matters —
//!   what closed, what is open — is on Today where the work is.
//! - **The connection footer.** Moved to the status line, with the rest of the
//!   app's state, so there is one place to look.
//!
//! The active destination is marked with ground and full-strength text, never
//! with the accent: warmth is reserved for the live task, so navigation does
//! not get to spend it.

use iced::widget::{button, column, container, row, scrollable, svg, text};
use iced::{Alignment, Background, Border, Length, Padding};

use crate::den::{Den, Message};
use crate::fonts::mono;
use crate::screen::Kind;
use crate::widget::{Element, gap, hover_row, line_text, size};
use crate::{glyph, icon};

const WIDTH: f32 = 234.0;

pub fn view(den: &Den) -> Element<'_> {
    let palette = den.palette;

    let body = column![
        wordmark(den),
        nav(den),
        projects_heading(den),
        workspace(den),
        group("tags", &palette),
        tags(den),
        gap(1, 12),
    ];

    container(scrollable(body).height(Length::Fill))
        .width(WIDTH)
        .height(Length::Fill)
        .padding(Padding::default().top(16).bottom(12))
        .style(move |_| container::Style {
            background: Some(Background::Color(palette.panel)),
            ..container::Style::default()
        })
        .into()
}

/// The bear, in the current theme's variant, beside the name.
fn wordmark(den: &Den) -> Element<'_> {
    let palette = den.palette;
    container(
        row![
            container(
                svg(svg::Handle::from_memory(icon::small_svg(palette.variant)))
                    .width(26)
                    .height(26)
            )
            .clip(true),
            text("Den").size(14.0).font(mono()).color(palette.fg),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 14]).bottom(20))
    .into()
}

/// `projects`, plus a way out when one is selected.
///
/// A scope you cannot see and cannot leave is a trap, and the sidebar row that
/// set it is easy to lose among a hundred notes.
fn projects_heading(den: &Den) -> Element<'_> {
    let palette = den.palette;
    let mut line = row![
        text("projects")
            .size(size::META)
            .font(mono())
            .color(palette.line2)
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if den.project().is_some() {
        line = line.push(crate::widget::filler());
        line = line.push(crate::widget::hover_cell(
            container(
                text("all")
                    .size(size::META)
                    .font(mono())
                    .color(palette.firelight),
            )
            .padding(Padding::from([2, 6])),
            Message::ClearProject,
            &palette,
        ));
    }

    container(line)
        .padding(Padding::from([22, 14]).bottom(7))
        .into()
}

/// A quiet lowercase word. Not an eyebrow, not tracked, no rule after it.
fn group<'a>(name: &'a str, palette: &crate::theme::Palette) -> Element<'a> {
    container(
        text(name)
            .size(size::META)
            .font(mono())
            .color(palette.line2),
    )
    .padding(Padding::from([22, 14]).bottom(7))
    .into()
}

fn nav(den: &Den) -> Element<'_> {
    let palette = den.palette;
    let current = den.kind();
    // Counts follow the scope: a project's numbers, or the vault's.
    let scoped = den.scoped_tasks();
    let open = scoped
        .iter()
        .filter(|t| t.status() != den_core::vault::Status::Done)
        .count();

    let destinations = [
        (Kind::Today, glyph::DAILY, Some(open)),
        (Kind::Tasks, glyph::PROJECTS, Some(scoped.len())),
        (Kind::Roadmap, glyph::DUE, None),
    ];

    container(column(destinations.map(|(kind, mark, count)| {
        let active = current == kind;
        button(
            row![
                glyph::view(mark, 14.0, if active { palette.fg } else { palette.line2 }),
                text(kind.as_str())
                    .size(size::ROW)
                    .font(mono())
                    .color(if active { palette.fg } else { palette.muted })
                    .width(Length::Fill),
                text(count.map(|n| n.to_string()).unwrap_or_default())
                    .size(size::META)
                    .font(mono())
                    .color(if active { palette.muted } else { palette.line2 }),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding::from([8, 8]))
        .on_press(Message::Show(kind))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: (active || hovered).then_some(Background::Color(palette.card)),
                text_color: palette.fg,
                border: Border {
                    radius: crate::theme::radius::CONTROL.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
    })))
    .padding(Padding::from([0, 8]))
    .into()
}

/// The notes, each leading with its own mark.
///
/// A project that declares a `Stack:` shows it; everything else shows its kind.
/// That is what tells a project from a daily note at a glance — the complaint
/// that started this redesign.
fn workspace(den: &Den) -> Element<'_> {
    let palette = den.palette;

    let rows = den.vault.active().map(|entry| {
        let selected = den.project.as_ref() == Some(&entry.file);
        let open = entry.open_tasks().count();

        hover_row(
            container(
                row![
                    glyph::view(glyph::for_entry(entry), 12.0, palette.line2),
                    line_text(
                        entry.title.clone(),
                        size::ROW,
                        if selected { palette.fg } else { palette.dim },
                    ),
                    // Unsaved editor state, so a refused write is no surprise.
                    text(if entry.modified { "\u{25cf}" } else { "" })
                        .size(size::META)
                        .font(mono())
                        .color(palette.gold),
                    text(open.to_string())
                        .size(size::META)
                        .font(mono())
                        .color(palette.line2),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([7, 8]))
            .clip(true),
            Message::FocusEntry(entry.file.clone()),
            selected.then_some(palette.card_alt),
            &palette,
        )
    });

    container(column(rows))
        .padding(Padding::from([0, 8]))
        .into()
}

fn tags(den: &Den) -> Element<'_> {
    let palette = den.palette;
    let ranked = den.scoped_tags();

    if ranked.is_empty() {
        return container(
            text(if den.project().is_some() {
                "no tags in this project"
            } else {
                "no tags yet"
            })
            .size(size::META)
            .font(mono())
            .color(palette.line2),
        )
        .padding(Padding::from([0, 16]))
        .into();
    }

    container(column(ranked.into_iter().map(|(name, slot, count)| {
        let color = palette.slot(slot);
        let active = den.tag_filter.as_deref() == Some(name);

        hover_row(
            container(
                row![
                    crate::widget::swatch(color, 7.0),
                    line_text(
                        name.to_string(),
                        size::ROW,
                        if active { palette.fg } else { palette.dim },
                    ),
                    text(count.to_string())
                        .size(size::META)
                        .font(mono())
                        .color(palette.line2),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([7, 8]))
            .clip(true),
            Message::SetTagFilter((!active).then(|| name.to_string())),
            active.then_some(palette.card_alt),
            &palette,
        )
    })))
    .padding(Padding::from([0, 8]))
    .into()
}
