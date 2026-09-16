//! The burndown view.
//!
//! Every series here comes from [`den_core::metrics::Burndown`], which draws only
//! what the ledger has actually observed. On a cold start that means an ideal
//! line and today's point, and a note saying so — not a fabricated curve.

use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{Column, column, container, row, scrollable, text};
use iced::{Alignment, Length, Padding, Point, Rectangle, Renderer, Theme};

use crate::den::{Den, Message};
use crate::fonts::mono;
use crate::theme::Palette;
use den_core::metrics::Burndown;

use crate::widget::{Element, card, gap, label, muted, section_with, size, stat_card};

pub fn view(den: &Den) -> Element<'_> {
    let burndown = Burndown::compute(&den.vault, &den.history, den.today);

    let body = column![
        heading(den, &burndown),
        stats(den, &burndown),
        chart_card(den, &burndown),
        row![
            velocity_card(den, &burndown).width(Length::Fill),
            projects_card(den, &burndown).width(Length::Fixed(392.0)),
        ]
        .spacing(12)
        .align_y(Alignment::Start),
    ]
    .spacing(14)
    .padding(Padding::from([14, 16]));

    scrollable(body).height(Length::Fill).into()
}

fn heading<'a>(den: &'a Den, burndown: &Burndown) -> Element<'a> {
    let palette = den.palette;
    row![
        text("Burndown")
            .size(size::HEADING)
            .font(mono())
            .color(palette.fg),
        muted(
            format!("{} · {}", den.vault.label(), burndown.label()),
            &palette
        ),
        gap(Length::Fill, 1),
    ]
    .spacing(10)
    .align_y(Alignment::End)
    .into()
}

fn stats<'a>(den: &'a Den, burndown: &Burndown) -> Element<'a> {
    let palette = den.palette;

    row![
        stat_card(
            "SCOPE",
            burndown.scope.to_string(),
            palette.fg,
            "tasks in the vault".to_string(),
            21.0,
            &palette,
        ),
        stat_card(
            "CLOSED",
            burndown.closed.to_string(),
            palette.olive,
            match burndown.history_starts {
                Some(start) => format!("since {}", den_core::date::short(start)),
                None => "all time".to_string(),
            },
            21.0,
            &palette,
        ),
        stat_card(
            "REMAINING",
            burndown.remaining.to_string(),
            palette.hero,
            {
                let counts = den.vault.counts();
                format!("{} backlog · {} doing", counts.backlog, counts.doing)
            },
            21.0,
            &palette,
        ),
        stat_card(
            "PACE",
            format!("{:.2}", burndown.pace),
            palette.gold,
            "closed per day".to_string(),
            21.0,
            &palette,
        ),
        stat_card(
            "PROJECTED",
            match burndown.projected {
                Some(date) => den_core::date::short(date),
                None => "—".to_string(),
            },
            palette.rose,
            match burndown.projected {
                Some(date) => {
                    let slip = den_core::date::days_between(burndown.end, date);
                    if slip > 0 {
                        format!("{} past the line", den_core::date::day_count(slip))
                    } else {
                        "inside the window".to_string()
                    }
                }
                None => "needs a closure first".to_string(),
            },
            21.0,
            &palette,
        ),
    ]
    .spacing(10)
    .into()
}

fn chart_card<'a>(den: &'a Den, burndown: &Burndown) -> Element<'a> {
    let palette = den.palette;

    let legend = row![
        text("— actual")
            .size(size::LABEL)
            .font(mono())
            .color(palette.hero),
        text("┄ ideal")
            .size(size::LABEL)
            .font(mono())
            .color(palette.line2),
        text("┄ projected")
            .size(size::LABEL)
            .font(mono())
            .color(palette.gold),
    ]
    .spacing(12);

    let ticks = row(burndown.days.iter().map(|day| {
        label(den_core::date::day_tick(*day), &palette)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .into()
    }))
    .spacing(0);

    let mut body = column![
        section_with("REMAINING TASKS", Some(legend.into()), &palette),
        Canvas::new(Chart {
            burndown: burndown.clone(),
            palette,
            today: den.today,
            compact: false,
        })
        .width(Length::Fill)
        .height(Length::Fixed(272.0)),
        container(ticks).padding(Padding::default().left(52).right(24)),
    ]
    .spacing(10);

    if burndown.is_cold_start() {
        body = body.push(muted(
            "Den records what it sees; the curve fills in from here. \
             The Markdown format has no closure timestamps to read back.",
            &palette,
        ));
    }

    card(body, &palette)
}

/// The small inline chart the home view shows.
pub fn spark<'a>(den: &'a Den, burndown: &Burndown, width: f32, height: f32) -> Element<'a> {
    Canvas::new(Chart {
        burndown: burndown.clone(),
        palette: den.palette,
        today: den.today,
        compact: true,
    })
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .into()
}

fn velocity_card<'a>(den: &'a Den, burndown: &Burndown) -> Column<'a, Message> {
    let palette = den.palette;
    let peak = burndown
        .closed_per_day
        .iter()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);

    let bars = row(burndown
        .days
        .iter()
        .zip(&burndown.closed_per_day)
        .map(|(day, closed)| {
            let height = if *closed == 0 {
                2.0
            } else {
                6.0 + 84.0 * (*closed as f32 / peak as f32)
            };
            let color = if *closed == 0 {
                palette.line2
            } else {
                palette.olive
            };
            column![
                gap(Length::Fill, Length::Fill),
                container(gap(Length::Fill, height)).style(move |_| container::Style {
                    background: Some(iced::Background::Color(color)),
                    border: iced::Border {
                        radius: crate::theme::radius::MARK.into(),
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                }),
                text(den_core::date::day_tick(*day))
                    .size(9.5)
                    .font(mono())
                    .color(palette.muted)
                    .width(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Center),
            ]
            .spacing(5)
            .width(Length::Fill)
            .height(Length::Fixed(116.0))
            .into()
        }))
    .spacing(5);

    column![card(
        column![
            section_with(
                "CLOSED PER DAY",
                Some(label(format!("{:.2} / day", burndown.pace), &palette).into()),
                &palette,
            ),
            bars,
        ]
        .spacing(11),
        &palette,
    )]
}

fn projects_card<'a>(den: &'a Den, burndown: &Burndown) -> Column<'a, Message> {
    let palette = den.palette;

    let rows = column(den.vault.active().map(|entry| {
        let total = entry.tasks.len();
        let done = entry.done_tasks().count();
        let open = total - done;
        column![
            row![
                text(entry.title.clone())
                    .size(size::BODY)
                    .font(mono())
                    .color(palette.fg)
                    .width(Length::Fill),
                label(format!("{open} open · {done} closed"), &palette),
            ]
            .spacing(8)
            .align_y(Alignment::End),
            crate::widget::meter(
                if total == 0 {
                    0.0
                } else {
                    done as f32 / total as f32
                },
                palette.olive,
                24,
            ),
        ]
        .spacing(6)
        .into()
    }))
    .spacing(10);

    let summary = match burndown.projected {
        Some(date) => format!(
            "At the current pace the vault clears on {}, {} the {} line.",
            den_core::date::short(date),
            {
                let slip = den_core::date::days_between(burndown.end, date);
                if slip > 0 {
                    format!("{} after", den_core::date::day_count(slip))
                } else {
                    format!("{} before", den_core::date::day_count(slip))
                }
            },
            den_core::date::short(burndown.end),
        ),
        None => "No closures observed yet, so there is no pace to project from.".to_string(),
    };

    column![card(
        column![
            section_with("BY PROJECT", None, &palette),
            rows,
            muted(summary, &palette),
        ]
        .spacing(12),
        &palette,
    )]
}

// ── the chart itself ────────────────────────────────────────────────────────

struct Chart {
    burndown: Burndown,
    palette: Palette,
    today: jiff::civil::Date,
    /// The home view's inline version drops the axes and labels.
    compact: bool,
}

impl canvas::Program<Message> for Chart {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = &self.palette;
        let burndown = &self.burndown;

        let left = if self.compact { 4.0 } else { 52.0 };
        let right = bounds.width - if self.compact { 4.0 } else { 24.0 };
        let top = if self.compact { 8.0 } else { 32.0 };
        let bottom = bounds.height - if self.compact { 8.0 } else { 40.0 };
        if right <= left || bottom <= top {
            return vec![frame.into_geometry()];
        }

        let ceiling = burndown.ceiling() as f32;
        let span = (burndown.days.len().saturating_sub(1)).max(1) as f32;
        let x_at = |index: usize| left + (right - left) * (index as f32 / span);
        let y_at = |value: f32| bottom - (bottom - top) * (value / ceiling).clamp(0.0, 1.0);

        // Gridlines, one per whole task up to five bands.
        let bands = ceiling.min(4.0).max(1.0) as usize;
        for band in 0..=bands {
            let value = ceiling * band as f32 / bands as f32;
            let y = y_at(value);
            let color = if band == 0 {
                palette.line2
            } else {
                palette.line
            };
            frame.stroke(
                &Path::line(Point::new(left, y), Point::new(right, y)),
                Stroke::default().with_color(color).with_width(1.0),
            );
            if !self.compact {
                frame.fill_text(Text {
                    content: format!("{}", value.round() as u32),
                    position: Point::new(left - 12.0, y - 5.0),
                    color: palette.muted,
                    size: 10.0.into(),
                    font: mono(),
                    align_x: iced::alignment::Horizontal::Right.into(),
                    ..Text::default()
                });
            }
        }

        // The ideal line: full scope down to zero across the window.
        dashed(
            &mut frame,
            Point::new(x_at(0), y_at(burndown.ideal_at(0))),
            Point::new(x_at(burndown.days.len() - 1), y_at(0.0)),
            palette.line2,
            1.5,
        );

        // Today's marker.
        if let Some(index) = burndown.days.iter().position(|day| *day == self.today) {
            let x = x_at(index);
            dashed(
                &mut frame,
                Point::new(x, top - 6.0),
                Point::new(x, bottom),
                palette.line2,
                1.0,
            );
            if !self.compact {
                frame.fill_text(Text {
                    content: "today".to_string(),
                    position: Point::new(x + 8.0, top - 12.0),
                    color: palette.muted,
                    size: 10.0.into(),
                    font: mono(),
                    ..Text::default()
                });
            }
        }

        // The observed remaining line.
        let observed: Vec<(usize, usize)> = burndown
            .actual
            .iter()
            .enumerate()
            .filter_map(|(index, value)| value.map(|value| (index, value)))
            .collect();

        if observed.len() >= 2 {
            let path = Path::new(|builder| {
                builder.move_to(Point::new(x_at(observed[0].0), y_at(observed[0].1 as f32)));
                for (index, value) in &observed[1..] {
                    builder.line_to(Point::new(x_at(*index), y_at(*value as f32)));
                }
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(palette.hero)
                    .with_width(if self.compact { 2.0 } else { 2.5 }),
            );
        }

        for (index, value) in &observed {
            frame.fill(
                &Path::circle(Point::new(x_at(*index), y_at(*value as f32)), 3.0),
                palette.hero,
            );
        }

        // The projection, from the last observed point to the clearing date.
        if let (Some((last_index, last_value)), Some(projected)) =
            (observed.last().copied(), burndown.projected)
        {
            let offset = den_core::date::days_between(burndown.start, projected);
            let end_x = if offset as f32 > span {
                right
            } else {
                x_at(offset.max(0) as usize)
            };
            dashed(
                &mut frame,
                Point::new(x_at(last_index), y_at(last_value as f32)),
                Point::new(end_x, y_at(0.0)),
                palette.gold,
                1.5,
            );
        }

        vec![frame.into_geometry()]
    }
}

/// iced's `Stroke` has no dash pattern, so dashes are drawn as segments.
fn dashed(frame: &mut Frame, from: Point, to: Point, color: iced::Color, width: f32) {
    const DASH: f32 = 5.0;
    const GAP: f32 = 4.0;

    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f32::EPSILON {
        return;
    }
    let (ux, uy) = (dx / length, dy / length);

    let mut travelled = 0.0;
    while travelled < length {
        let end = (travelled + DASH).min(length);
        frame.stroke(
            &Path::line(
                Point::new(from.x + ux * travelled, from.y + uy * travelled),
                Point::new(from.x + ux * end, from.y + uy * end),
            ),
            Stroke::default().with_color(color).with_width(width),
        );
        travelled = end + GAP;
    }
}
