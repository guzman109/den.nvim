//! The primitives the design repeats on every screen, plus the window chrome
//! and the two side panels.
//!
//! Building these once keeps the screens declarative and stops the same padding
//! and border values being retyped (and drifting) in eight places.

pub mod chrome;
pub mod sidebar;

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Length, Padding};

use crate::fonts::mono;
use crate::theme::{Palette, radius};

/// Defaults to the root message so screens that speak it can write
/// `Element<'a>`, while a screen with its own message type names it.
pub type Element<'a, Message = crate::den::Message> = iced::Element<'a, Message>;

/// Text sizes, named for the role the design gives them.
///
/// The old scale ran from 10 to 12 for nearly everything, which is why the app
/// had no hierarchy and read as uniformly small however it was zoomed. This one
/// exists for its contrast: the live task is more than twice the size of a row,
/// so what you are doing is legible from across the desk.
pub mod size {
    /// Dates, counts, file locations, key hints.
    pub const META: f32 = 11.0;
    /// Task rows, tree rows, control text.
    pub const ROW: f32 = 13.0;
    /// The lowercase section word: `now`, `next`, `cooling`.
    pub const SECTION: f32 = 15.0;
    /// The live task. The largest text in the app, by a wide margin.
    pub const LIVE: f32 = 30.0;

    // Older names, kept pointed at the new scale so the screens still standing
    // pick up the contrast until Phase 3 replaces them. Removed with them.
    pub const LABEL: f32 = META;
    pub const SMALL: f32 = META;
    pub const BODY: f32 = ROW;
    pub const TASK: f32 = ROW;
    pub const LEAD: f32 = SECTION;
    pub const HEADING: f32 = SECTION;
    pub const STAT: f32 = 23.0;
    /// Beside the 30pt live task this is a supporting number, not the subject —
    /// the design's 44 made the timer beat the task it was timing.
    pub const CLOCK: f32 = 15.0;
}

/// Empty space of a given size. `Space::new()` takes no arguments in iced 0.14,
/// and the two-axis form is what the layouts here want.
pub fn gap(width: impl Into<Length>, height: impl Into<Length>) -> Space {
    Space::new().width(width).height(height)
}

/// Space that eats the remaining room on the main axis.
pub fn filler() -> Space {
    gap(Length::Fill, Length::Shrink)
}

pub fn label<'a>(value: impl text::IntoFragment<'a>, palette: &Palette) -> iced::widget::Text<'a> {
    text(value)
        .size(size::LABEL)
        .font(mono())
        .color(palette.muted)
}

pub fn muted<'a>(value: impl text::IntoFragment<'a>, palette: &Palette) -> iced::widget::Text<'a> {
    text(value)
        .size(size::SMALL)
        .font(mono())
        .color(palette.muted)
}

pub fn body<'a>(value: impl text::IntoFragment<'a>, palette: &Palette) -> iced::widget::Text<'a> {
    text(value).size(size::TASK).font(mono()).color(palette.fg)
}

/// A hairline. The design draws these dashed at 55% opacity; iced has no dashed
/// border, so we match the weight with a translucent solid rule — at 1px the
/// difference is not visible, and a canvas per section header would not earn
/// its cost.
pub fn rule<'a, Message: 'a>(palette: &Palette) -> Element<'a, Message> {
    let color = palette.wash(palette.line2, 0.55);
    container(gap(Length::Fill, 1))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            ..container::Style::default()
        })
        .into()
}

/// A section name: an ordinary lowercase word, at a size you can read.
///
/// This replaces the tracked-out ALL-CAPS eyebrow with a rule trailing off to
/// the right. That pattern was on every panel in the app — `WORKSPACE`, `TAGS`,
/// `HEARTH`, `NOTE`, `VAULT`, `BURNDOWN` — and it was the most pervasive tell
/// that the interface had been assembled from defaults rather than designed.
/// Shouting a word carries no information that writing it does not.
pub fn heading<'a, Message: 'a>(
    name: impl text::IntoFragment<'a>,
    count: Option<usize>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut line = row![
        text(name)
            .size(size::SECTION)
            .font(mono())
            .color(palette.dim),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if let Some(count) = count {
        line = line.push(
            text(count.to_string())
                .size(size::META)
                .font(mono())
                .color(palette.line2),
        );
    }
    line.into()
}

/// How wide the lit surface is allowed to get.
///
/// Unconstrained it stretched the full window and the 30pt title floated in a
/// wide flat slab. Light pools; it does not tile. Capping the width is what
/// makes it read as a lit patch on a floor rather than as a banner.
pub const LIT_WIDTH: f32 = 720.0;

/// The lit surface: the one warm thing on the screen.
///
/// The pool of light from `assets/icons/ember/den-ember-small.svg`. A flat wash
/// over the whole card reads as a tan rectangle, not as light — the warmth has
/// to *fall off*. iced has no radial gradient, but it does have linear ones on
/// container backgrounds, and a diagonal running warm at the lower-left to
/// plain card at the upper-right carries the direction well enough that the
/// surface looks lit from somewhere.
///
/// Deliberately borderless. Every other surface in the old app had the same
/// 1px line and the same radius, so nothing read as more important than
/// anything else; this one is distinguished by being *lit*, not by being
/// outlined.
pub fn lit_surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    let (hot, warm, base) = palette.firelight_stops();

    container(content)
        .padding(Padding::from([28, 30]).bottom(22))
        .max_width(LIT_WIDTH)
        .style(move |_| container::Style {
            background: Some(Background::Gradient(
                // 45°: the gradient runs toward the north-east, which puts stop 0 —
                // the hot end — at the lower-left, where the icon's pool sits.
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_4))
                    .add_stop(0.0, hot)
                    .add_stop(0.45, warm)
                    .add_stop(1.0, base)
                    .into(),
            )),
            border: Border {
                radius: radius::CARD.into(),
                ..Border::default()
            },
            // On paper the pool cannot be brighter than the page, so the card is
            // lifted off the ground instead. Dark variants get no shadow: light
            // does not cast one on a floor it is lying on.
            shadow: palette.lift(),
            ..container::Style::default()
        })
        .into()
}

/// The lit surface with no fire in it.
///
/// Same shape and padding as [`lit_surface`], without the glow: nothing is
/// running, so nothing is warm. It still gets the card ground rather than
/// vanishing, because the space where the live task goes should stay visible —
/// an empty screen is an invitation, not an absence.
pub fn lit_surface_empty<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    container(content)
        .padding(Padding::from([28, 30]).bottom(22))
        .max_width(LIT_WIDTH)
        .style(move |_| container::Style {
            background: Some(Background::Color(palette.card)),
            border: Border {
                radius: radius::CARD.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A one-line label that yields to whatever follows it in the row.
///
/// `Wrapping::None` keeps a long task title on one line, but iced still
/// *measures* that text at its full width, so a long title overruns the tags
/// and the date beside it — which is exactly what it was doing. Clipping is
/// only effective on the element that carries the `Fill`, so the container has
/// to be the flexible one and the text inside it fixed.
pub fn line_text<'a, Message: 'a>(value: String, size: f32, color: Color) -> Element<'a, Message> {
    container(
        text(value)
            .size(size)
            .font(mono())
            .color(color)
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fill)
    .clip(true)
    .into()
}

/// The checkbox as the Markdown file writes it: `[ ]`, `[~]`, `[x]`.
///
/// `parse::Status::mark` already defines these, so the mark the user sees is
/// the mark on disk rather than a decorative arrow invented for the UI.
pub fn status_box<'a, Message: 'a>(
    status: den_core::vault::Status,
    temperature: crate::theme::Temperature,
    palette: &Palette,
) -> Element<'a, Message> {
    text(status.mark())
        .size(size::ROW)
        .font(mono())
        .color(match temperature {
            crate::theme::Temperature::Live | crate::theme::Temperature::Next => palette.firelight,
            crate::theme::Temperature::Cooling => palette.olive,
            crate::theme::Temperature::Cold => palette.muted,
        })
        .into()
}

/// A key hint, as a terminal would print it.
pub fn key_hint<'a, Message: 'a>(key: &'a str, palette: &Palette) -> Element<'a, Message> {
    let palette = *palette;
    container(text(key).size(10.0).font(mono()).color(palette.muted))
        .padding(Padding::from([3, 5]).bottom(2))
        .style(move |_| container::Style {
            border: Border {
                color: palette.line2,
                width: 1.0,
                radius: radius::MARK.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// `WORKSPACE ───────────` — the design's section divider.
pub fn section<'a, Message: 'a>(name: &'a str, palette: &Palette) -> Element<'a, Message> {
    section_with(name, None, palette)
}

/// The same, with a value tucked against the right edge.
pub fn section_with<'a, Message: 'a>(
    name: &'a str,
    trailing: Option<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let mut line = row![label(name, palette), rule(palette)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(trailing) = trailing {
        line = line.push(trailing);
    }
    line.into()
}

/// A `#tag` in its assigned hue.
pub fn chip<'a, Message: 'a>(name: &str, color: Color) -> Element<'a, Message> {
    text(format!("#{name}"))
        .size(size::SMALL)
        .font(mono())
        .color(color)
        .into()
}

/// The design's block-glyph progress meter: `██░░░░░░`.
pub fn meter_glyphs(fraction: f32, cells: usize) -> String {
    let filled = (fraction.clamp(0.0, 1.0) * cells as f32).round() as usize;
    let mut meter = String::with_capacity(cells * 3);
    meter.extend(std::iter::repeat_n('█', filled));
    meter.extend(std::iter::repeat_n('░', cells.saturating_sub(filled)));
    meter
}

pub fn meter<'a, Message: 'a>(fraction: f32, color: Color, cells: usize) -> Element<'a, Message> {
    text(meter_glyphs(fraction, cells))
        .size(size::SMALL)
        .font(mono())
        .color(color)
        .into()
}

/// A labelled meter row: `done  ██░░░░░░  25%`.
pub fn meter_row<'a, Message: 'a>(
    name: &'a str,
    fraction: f32,
    color: Color,
    trailing: String,
    palette: &Palette,
) -> Element<'a, Message> {
    row![
        muted(name, palette).width(Length::Fixed(38.0)),
        container(meter(fraction, color, 8)).width(Length::Fill),
        muted(trailing, palette),
    ]
    .spacing(7)
    .align_y(Alignment::Center)
    .into()
}

/// A raised surface: card ground, hairline border, soft corner.
pub fn card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    container(content)
        .padding(Padding::from([10, 11]))
        .style(move |_| surface(&palette))
        .into()
}

pub fn surface(palette: &Palette) -> container::Style {
    container::Style {
        background: Some(Background::Color(palette.card)),
        border: Border {
            color: palette.line,
            width: 1.0,
            radius: radius::CARD.into(),
        },
        ..container::Style::default()
    }
}

/// `OPEN / 3 / of 4 in this note` — the stat tiles across the top of home and
/// burndown.
pub fn stat_card<'a, Message: 'a>(
    key: &'a str,
    value: String,
    value_color: Color,
    sub: String,
    value_size: f32,
    palette: &Palette,
) -> Element<'a, Message> {
    card(
        column![
            label(key, palette),
            text(value).size(value_size).font(mono()).color(value_color),
            label(sub, palette),
        ]
        .spacing(5),
        palette,
    )
}

/// A row that lights up under the pointer.
///
/// The design's `style-hover` rules land on list rows and cards, not just
/// buttons, so hover has to come from a `button` with a custom style — a
/// `container` has no hovered state to key off.
/// A hoverable control that takes only the room it needs.
///
/// [`hover_row`] fills its parent, which is right for a list row and wrong for
/// a control sitting in a bar — there the fill stretched the capture block
/// across a third of the status line.
pub fn hover_cell<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    message: Message,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    button(content)
        .padding(0)
        .width(Length::Shrink)
        .on_press(message)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hovered.then_some(Background::Color(palette.card)),
                text_color: palette.fg,
                border: Border {
                    radius: radius::CONTROL.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
}

pub fn hover_row<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    message: Message,
    ground: Option<Color>,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    button(content)
        .padding(0)
        .width(Length::Fill)
        .on_press(message)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(match (hovered, ground) {
                    (true, _) => palette.card,
                    (false, Some(ground)) => ground,
                    (false, None) => Color::TRANSPARENT,
                })),
                text_color: palette.fg,
                border: Border {
                    radius: radius::MARK.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .into()
}

/// A bordered control: `#tags ▾`, `nvim ↗`, `new task`.
pub fn ghost_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    message: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    let mut control = button(content)
        .padding(Padding::from([0, 8]))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: None,
                text_color: if hovered { palette.fg } else { palette.dim },
                border: Border {
                    color: if hovered { palette.line2 } else { palette.line },
                    width: 1.0,
                    radius: radius::CONTROL.into(),
                },
                ..button::Style::default()
            }
        });
    if let Some(message) = message {
        control = control.on_press(message);
    }
    control.into()
}

/// A filled control in the hero accent: `start focus`, the active view tab.
pub fn hero_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    message: Option<Message>,
    palette: &Palette,
) -> Element<'a, Message> {
    let palette = *palette;
    let mut control = button(content)
        .padding(Padding::from([0, 10]))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if hovered {
                    palette.wash(palette.hero, 0.85)
                } else {
                    palette.hero
                })),
                text_color: palette.on_accent,
                border: Border {
                    radius: radius::CONTROL.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        });
    if let Some(message) = message {
        control = control.on_press(message);
    }
    control.into()
}

/// A left accent stripe, as the board cards and list rows carry.
pub fn stripe<'a, Message: 'a>(color: Color, height: Length) -> Element<'a, Message> {
    container(gap(2, height))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 1.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A small filled square, used beside tag names.
pub fn swatch<'a, Message: 'a>(color: Color, side: f32) -> Element<'a, Message> {
    container(gap(side, side))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 1.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_fills_proportionally() {
        assert_eq!(meter_glyphs(0.0, 8), "░░░░░░░░");
        assert_eq!(meter_glyphs(0.25, 8), "██░░░░░░");
        assert_eq!(meter_glyphs(1.0, 8), "████████");
        // Out-of-range input is clamped rather than panicking on a bad ratio.
        assert_eq!(meter_glyphs(1.5, 8), "████████");
        assert_eq!(meter_glyphs(-1.0, 8), "░░░░░░░░");
    }

    #[test]
    fn meter_matches_the_designs_quarter_done_bar() {
        // The mockup draws 2 of 8 filled for "25%".
        assert_eq!(meter_glyphs(2.0 / 8.0, 8), "██░░░░░░");
    }
}
