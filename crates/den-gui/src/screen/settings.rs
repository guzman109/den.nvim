//! The settings screen.
//!
//! Everything here is a live preference rather than a stored document, so the
//! screen owns only which section is open and reports changes upward as
//! [`Action`]s. The root state stays the single source of truth for the palette
//! and the vault.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Background, Border, Length, Padding};

use crate::den::Link;
use crate::fonts;
use crate::theme::{Accent, Mode, Palette, Variant, radius};
use crate::widget::{Element, gap, muted, size, swatch};
use den_core::Vault;

pub struct Settings {
    section: Section,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Vault,
    Editor,
    About,
}

impl Section {
    const ALL: [Section; 4] = [
        Section::Appearance,
        Section::Vault,
        Section::Editor,
        Section::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Vault => "vault",
            Section::Editor => "neovim",
            Section::About => "about",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Open(Section),
    ChangeMode(Mode),
    ChangeVariant(Variant),
    ChangeAccent(Accent),
    ChangeFont(&'static str),
    Reload,
}

/// What the root has to act on. The screen never mutates shared state itself.
pub enum Action {
    None,
    ChangeMode(Mode),
    ChangeVariant(Variant),
    ChangeAccent(Accent),
    ChangeFont(&'static str),
    Reload,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::new()
    }
}

impl Settings {
    pub fn new() -> Self {
        Settings {
            section: Section::Appearance,
        }
    }

    pub fn title(&self) -> &'static str {
        "Settings"
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Open(section) => {
                self.section = section;
                Action::None
            }
            Message::ChangeMode(mode) => Action::ChangeMode(mode),
            Message::ChangeVariant(variant) => Action::ChangeVariant(variant),
            Message::ChangeAccent(accent) => Action::ChangeAccent(accent),
            Message::ChangeFont(name) => Action::ChangeFont(name),
            Message::Reload => Action::Reload,
        }
    }

    pub fn view<'a>(
        &'a self,
        palette: &'a Palette,
        vault: &'a Vault,
        link: &'a Link,
        root_label: String,
        keymap: &'a crate::keymap::Keymap,
        mode: Mode,
    ) -> Element<'a, Message> {
        let body = match self.section {
            Section::Appearance => self.appearance(palette, mode),
            Section::Vault => self.vault(palette, vault, root_label),
            Section::Editor => self.editor(palette, link),
            Section::About => self.about(palette, keymap),
        };

        row![
            self.nav(palette),
            container(scrollable(body).height(Length::Fill))
                .width(Length::Fill)
                .padding(Padding::from([16, 20])),
        ]
        .height(Length::Fill)
        .into()
    }

    fn nav<'a>(&'a self, palette: &'a Palette) -> Element<'a, Message> {
        let items = column(Section::ALL.map(|section| {
            let active = self.section == section;
            button(
                text(section.title())
                    .size(size::ROW)
                    .font(fonts::mono())
                    .color(if active { palette.fg } else { palette.dim }),
            )
            .width(Length::Fill)
            .padding(Padding::from([6, 12]))
            .on_press(Message::Open(section))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: (active || hovered).then_some(Background::Color(palette.card)),
                    text_color: palette.fg,
                    border: Border {
                        radius: radius::MARK.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .into()
        }))
        .spacing(2);

        container(items)
            .width(Length::Fixed(176.0))
            .height(Length::Fill)
            .padding(Padding::from([16, 10]))
            .style(move |_| container::Style {
                background: Some(Background::Color(palette.panel)),
                ..container::Style::default()
            })
            .into()
    }

    fn appearance<'a>(&'a self, palette: &'a Palette, mode: Mode) -> Element<'a, Message> {
        // Following the system needs two choices, because Den has two dark
        // variants and two light ones — so the mode picker sits above the
        // variant list rather than replacing it.
        let modes = row(Mode::ALL.map(|option| {
            let active = mode == option;
            button(
                text(match option {
                    Mode::Auto => "follow system",
                    Mode::Dark => "always dark",
                    Mode::Light => "always light",
                })
                .size(size::ROW)
                .font(fonts::mono())
                .color(if active { palette.fg } else { palette.muted }),
            )
            .padding(Padding::from([6, 12]))
            .on_press(Message::ChangeMode(option))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: (active || hovered).then_some(Background::Color(palette.card)),
                    text_color: palette.fg,
                    border: Border {
                        radius: radius::CONTROL.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .into()
        }))
        .spacing(6);

        let themes = column(Variant::ALL.map(|variant| {
            let active = palette.variant == variant;
            choice_row(
                palette,
                active,
                text(variant.as_str())
                    .size(size::ROW)
                    .font(fonts::mono())
                    .color(palette.fg)
                    .width(Length::Fill)
                    .into(),
                Some(
                    row(variant.swatches().map(|color| swatch(color, 13.0)))
                        .spacing(2)
                        .into(),
                ),
                Message::ChangeVariant(variant),
            )
        }))
        .spacing(2);

        let accents = column(Accent::ALL.map(|accent| {
            let active = palette.accent == accent;
            let color = Palette::new(palette.variant, accent).hero;
            choice_row(
                palette,
                active,
                text(accent.as_str())
                    .size(size::ROW)
                    .font(fonts::mono())
                    .color(palette.fg)
                    .width(Length::Fill)
                    .into(),
                Some(swatch(color, 13.0)),
                Message::ChangeAccent(accent),
            )
        }))
        .spacing(2);

        let current = fonts::current_name();
        let families = column(fonts::installed().iter().map(|family| {
            let active = *family == current;
            choice_row(
                palette,
                active,
                // Rendered in the face itself, so the list previews each one.
                text(*family)
                    .size(size::ROW)
                    .font(iced::Font::with_name(family))
                    .color(palette.fg)
                    .width(Length::Fill)
                    .into(),
                None,
                Message::ChangeFont(family),
            )
        }))
        .spacing(2);

        column![
            heading(
                palette,
                "theme",
                "Follow the system, or pin one. Den has two dark variants and two light \
                 ones, so following still leaves you the choice below."
            ),
            modes,
            gap(1, 14),
            themes,
            gap(1, 10),
            heading(
                palette,
                "accent",
                "Which hue the live task burns. Everything else keeps its meaning."
            ),
            accents,
            gap(1, 10),
            heading(
                palette,
                "font",
                "Den ships Roboto Mono, so it always has a face it can draw. Anything \
                 monospace installed here shows up too. Den never requests a family it \
                 cannot find — iced would silently fall back to a proportional one.",
            ),
            families,
        ]
        .spacing(8)
        .into()
    }

    fn vault<'a>(
        &'a self,
        palette: &'a Palette,
        vault: &'a Vault,
        root_label: String,
    ) -> Element<'a, Message> {
        let counts = vault.counts();

        column![
            heading(
                palette,
                "location",
                "Set with --vault; defaults to ~/Notes/den."
            ),
            field(palette, "root".into(), root_label),
            field(palette, "entries".into(), vault.entries.len().to_string()),
            field(
                palette,
                "tasks".into(),
                format!(
                    "{} · {} open · {} done",
                    counts.total,
                    counts.open(),
                    counts.done
                ),
            ),
            gap(1, 10),
            heading(palette, "folders", "den.nvim indexes exactly these four."),
            column(den_core::vault::Kind::ALL.map(|kind| {
                let present = vault.entries.iter().filter(|e| e.kind == kind).count();
                field(palette, kind.as_str().into(), format!("{present} entries"))
            }))
            .spacing(3),
            gap(1, 10),
            action_button(palette, "reload vault", Message::Reload),
        ]
        .spacing(8)
        .into()
    }

    fn editor<'a>(&'a self, palette: &'a Palette, link: &'a Link) -> Element<'a, Message> {
        let connected = link.is_connected();

        // Den writes the Markdown itself, so attaching is never what makes it
        // writable. What it adds is knowing which buffers are unsaved.
        let explanation = if connected {
            "Attached. Den can see which buffers have unsaved changes and refuses \
             to write those files, so an edit in progress is never clobbered."
        } else {
            "Standalone, which is a full mode — Den reads and writes the vault \
             itself. Attaching adds one thing: Den can see unsaved buffers and \
             refuse to write over them."
        };

        column![
            heading(palette, "neovim", explanation),
            field(palette, "status".into(), link.label()),
            field(
                palette,
                "writes".into(),
                if connected {
                    "guarded by buffer state"
                } else {
                    "guarded by file hash"
                }
                .to_string(),
            ),
            gap(1, 10),
            heading(
                palette,
                "attaching",
                "Optional. Start Neovim with a socket, then point Den at it."
            ),
            code(palette, "nvim --listen /tmp/den.sock"),
            code(palette, "den --nvim /tmp/den.sock"),
        ]
        .spacing(8)
        .into()
    }

    fn about<'a>(
        &'a self,
        palette: &'a Palette,
        keymap: &'a crate::keymap::Keymap,
    ) -> Element<'a, Message> {
        let bindings = column(
            keymap
                .listing()
                .into_iter()
                .map(|(keys, what)| field(palette, keys, what)),
        )
        .spacing(3);

        let source = match keymap.source() {
            Some(path) => format!("Overridden from {}", path.display()),
            None => match crate::keymap::Keymap::path() {
                Some(path) => format!("Create {} to rebind.", path.display()),
                None => "Rebinding needs a home directory.".to_string(),
            },
        };

        column![
            heading(
                palette,
                "den",
                "A desktop client for the den.nvim Markdown vault."
            ),
            field(
                palette,
                "version".into(),
                env!("CARGO_PKG_VERSION").to_string()
            ),
            field(
                palette,
                "tasks".into(),
                "plain Markdown checkboxes, owned by you".into()
            ),
            gap(1, 10),
            heading(
                palette,
                "keys",
                "Vim-shaped by default: `g` goes somewhere, `j`/`k` move, `/` searches.",
            ),
            bindings,
            muted(source, palette),
        ]
        .spacing(8)
        .into()
    }
}

// ── pieces ──────────────────────────────────────────────────────────────────

fn heading<'a>(palette: &'a Palette, title: &'a str, blurb: &'a str) -> Element<'a, Message> {
    column![
        crate::widget::heading(title, None, palette),
        text(blurb)
            .size(size::SMALL)
            .font(fonts::mono())
            .color(palette.muted)
            .width(Length::Fill),
    ]
    .spacing(5)
    .into()
}

fn field<'a>(palette: &'a Palette, name: String, value: String) -> Element<'a, Message> {
    row![
        muted(name, palette).width(Length::Fixed(120.0)),
        text(value)
            .size(size::SMALL)
            .font(fonts::mono())
            .color(palette.dim)
            .width(Length::Fill),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn code<'a>(palette: &'a Palette, line: &'a str) -> Element<'a, Message> {
    container(
        text(line)
            .size(size::SMALL)
            .font(fonts::mono())
            .color(palette.fg),
    )
    .width(Length::Fill)
    .padding(Padding::from([7, 10]))
    .style(move |_| container::Style {
        background: Some(Background::Color(palette.win)),
        border: Border {
            color: palette.line,
            width: 1.0,
            radius: radius::CONTROL.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// A selectable row with a radio mark, used by every list on this screen.
fn choice_row<'a>(
    palette: &'a Palette,
    active: bool,
    body: Element<'a, Message>,
    trailing: Option<Element<'a, Message>>,
    message: Message,
) -> Element<'a, Message> {
    let mut content = row![
        text(if active { "(•)" } else { "( )" })
            .size(size::SMALL)
            .font(fonts::mono())
            // Not the hero: that belongs to the live task alone.
            .color(if active {
                palette.firelight
            } else {
                palette.muted
            }),
        body,
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    if let Some(trailing) = trailing {
        content = content.push(trailing);
    }

    button(content)
        .width(Length::Fill)
        .padding(Padding::from([6, 10]))
        .on_press(message)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: (active || hovered).then_some(Background::Color(palette.card)),
                text_color: palette.fg,
                border: Border {
                    color: if active {
                        palette.hero
                    } else {
                        iced::Color::TRANSPARENT
                    },
                    width: if active { 1.0 } else { 0.0 },
                    radius: radius::CONTROL.into(),
                },
                ..button::Style::default()
            }
        })
        .into()
}

fn action_button<'a>(
    palette: &'a Palette,
    name: &'a str,
    message: Message,
) -> Element<'a, Message> {
    button(text(name).size(size::BODY).font(fonts::mono()))
        .padding(Padding::from([6, 12]))
        .on_press(message)
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
        })
        .into()
}
