//! The ember palette, in the four variants Den ships.
//!
//! `assets/palette.lua` is the authoritative color source, shared with the
//! Emacs and Neovim themes. Its ramp is transcribed verbatim into [`Ramp`]
//! below; every semantic token the UI uses is *derived* from that ramp in
//! [`Palette::new`], with the mapping spelled out so it can be audited against
//! the Lua. Do not hardcode a color anywhere else in this crate.
//!
//! iced's built-in `Palette` carries six colors; this design uses twenty. So we
//! keep our own `Palette`, store it in app state, and capture it by value in
//! each widget's style closure. `Palette` is `Copy` to make that cheap.

use iced::Color;

const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

pub mod radius {
    //! Softer than the mockup, which sits at 2px nearly everywhere.
    pub const WINDOW: f32 = 10.0;
    pub const CARD: f32 = 6.0;
    pub const CONTROL: f32 = 5.0;
    pub const MARK: f32 = 3.0;
}

/// One variant's background ramp, transcribed from `assets/palette.lua`.
///
/// `base0` is the deepest surface and `base8` the lightest on dark variants;
/// the light variants run the other way, which is why the UI never addresses
/// `baseN` directly and goes through [`Palette`]'s semantic names instead.
#[derive(Debug, Clone, Copy)]
pub struct Ramp {
    pub bg: Color,
    pub bg_alt: Color,
    pub base0: Color,
    pub base1: Color,
    pub base2: Color,
    pub base3: Color,
    pub base4: Color,
    pub base5: Color,
    pub base6: Color,
    pub base7: Color,
    pub base8: Color,
    pub fg: Color,
    pub fg_alt: Color,
}

/// The eight accents. Tags and task stripes refer to a `Hue` rather than a
/// concrete color so they survive a theme switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hue {
    Coral,
    Orange,
    Gold,
    Olive,
    Sage,
    Steel,
    Rose,
    Mauve,
}

impl Hue {
    pub const ALL: [Hue; 8] = [
        Hue::Coral,
        Hue::Orange,
        Hue::Gold,
        Hue::Olive,
        Hue::Sage,
        Hue::Steel,
        Hue::Rose,
        Hue::Mauve,
    ];
}

/// `accents_dark` / `accents_light` from `palette.lua`, in [`Hue::ALL`] order.
const ACCENTS_DARK: [Color; 8] = [
    rgb(0xe08060), // coral  — hero
    rgb(0xc09058), // orange
    rgb(0xc8b468), // gold
    rgb(0x8a9868), // olive
    rgb(0x80a090), // sage
    rgb(0x7890a0), // steel
    rgb(0xb07878), // rose
    rgb(0x988090), // mauve
];

/// Deeper and more saturated, so accents hold contrast on the paper grounds.
const ACCENTS_LIGHT: [Color; 8] = [
    rgb(0xb84c30), // coral
    rgb(0x946030), // orange
    rgb(0x7a6820), // gold
    rgb(0x4a6830), // olive
    rgb(0x386858), // sage
    rgb(0x3a6080), // steel
    rgb(0x905050), // rose
    rgb(0x706070), // mauve
];

/// Which accent drives `--hero`, the app's primary color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Coral,
    Gold,
    Steel,
    Olive,
}

impl Accent {
    pub const ALL: [Accent; 4] = [Accent::Coral, Accent::Gold, Accent::Steel, Accent::Olive];

    pub fn as_str(self) -> &'static str {
        match self {
            Accent::Coral => "coral",
            Accent::Gold => "gold",
            Accent::Steel => "steel",
            Accent::Olive => "olive",
        }
    }

    pub fn parse(value: &str) -> Option<Accent> {
        Accent::ALL.into_iter().find(|a| a.as_str() == value)
    }

    fn hue(self) -> Hue {
        match self {
            Accent::Coral => Hue::Coral,
            Accent::Gold => Hue::Gold,
            Accent::Steel => Hue::Steel,
            Accent::Olive => Hue::Olive,
        }
    }
}

/// How warm a thing is allowed to be, and therefore what colour it gets.
///
/// The design's second rule is that temperature *is* state: warm means being
/// worked on, cooling means finished, cold means not started. Routing every
/// stateful colour through this enum is what keeps that a rule rather than a
/// habit — a screen cannot reach for `palette.hero` to make something look
/// nice without saying, in the type, that the thing is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temperature {
    /// The one task in the light. At most one per screen.
    Live,
    /// Queued, legible, unlit.
    Next,
    /// Finished and receding.
    Cooling,
    /// Not started, deliberately dim.
    Cold,
}

impl Temperature {
    /// The temperature of a task, from the status the file records.
    ///
    /// `lit` distinguishes the single task in the firelight from the other
    /// `@status(doing)` tasks — den.nvim allows several, and the extras must
    /// stay visible without stealing the light.
    pub fn of(status: den_core::vault::Status, lit: bool) -> Temperature {
        use den_core::vault::Status;
        match status {
            Status::Doing if lit => Temperature::Live,
            Status::Doing => Temperature::Next,
            Status::Backlog => Temperature::Cold,
            Status::Done => Temperature::Cooling,
        }
    }
}

/// Whether the theme follows the system, or is pinned.
///
/// Following the system needs *two* choices, not one, because Den has two dark
/// variants and two light ones. A single "auto" that picked for you would take
/// away the choice the palette exists to offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Auto,
    Dark,
    Light,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Auto, Mode::Dark, Mode::Light];

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }

    pub fn parse(value: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.as_str() == value)
    }

    /// Whether the app should be dark right now.
    ///
    /// A detection failure falls back to dark rather than erroring: the app has
    /// to draw something, and this vault's whole palette was designed dark
    /// first.
    pub fn is_dark(self) -> bool {
        match self {
            Mode::Dark => true,
            Mode::Light => false,
            Mode::Auto => !matches!(dark_light::detect(), Ok(dark_light::Mode::Light)),
        }
    }

    /// The variant to draw with, given the user's pick for each side.
    pub fn resolve(self, dark: Variant, light: Variant) -> Variant {
        if self.is_dark() { dark } else { light }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    #[default]
    Ember,
    EmberSoft,
    EmberLight,
    EmberLighter,
}

impl Variant {
    pub const ALL: [Variant; 4] = [
        Variant::Ember,
        Variant::EmberSoft,
        Variant::EmberLight,
        Variant::EmberLighter,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Ember => "ember",
            Variant::EmberSoft => "ember-soft",
            Variant::EmberLight => "ember-light",
            Variant::EmberLighter => "ember-lighter",
        }
    }

    pub fn parse(value: &str) -> Option<Variant> {
        Variant::ALL.into_iter().find(|v| v.as_str() == value)
    }

    /// `type = "dark"` in `palette.lua`.
    pub fn is_dark(self) -> bool {
        matches!(self, Variant::Ember | Variant::EmberSoft)
    }

    /// The variant's ramp, verbatim from `assets/palette.lua`.
    pub fn ramp(self) -> Ramp {
        match self {
            Variant::Ember => Ramp {
                bg: rgb(0x1c1b19),
                bg_alt: rgb(0x242320),
                base0: rgb(0x151412),
                base1: rgb(0x1c1b19),
                base2: rgb(0x252422),
                base3: rgb(0x2e2d2a),
                base4: rgb(0x3e3c38),
                base5: rgb(0x585550),
                base6: rgb(0x706c61),
                base7: rgb(0x908a7e),
                base8: rgb(0xb8b0a0),
                fg: rgb(0xd8d0c0),
                fg_alt: rgb(0xb0a898),
            },
            Variant::EmberSoft => Ramp {
                bg: rgb(0x242320),
                bg_alt: rgb(0x2a2927),
                base0: rgb(0x1c1b19),
                base1: rgb(0x222120),
                base2: rgb(0x2c2b28),
                base3: rgb(0x353430),
                base4: rgb(0x444240),
                base5: rgb(0x585550),
                base6: rgb(0x706c61),
                base7: rgb(0x908a7e),
                base8: rgb(0xb8b0a0),
                fg: rgb(0xd8d0c0),
                fg_alt: rgb(0xb0a898),
            },
            Variant::EmberLight => Ramp {
                bg: rgb(0xe6dac4),
                bg_alt: rgb(0xddd0b8),
                base0: rgb(0xf0e8d8),
                base1: rgb(0xe6dac4),
                base2: rgb(0xd8ccb0),
                base3: rgb(0xcec2a8),
                base4: rgb(0xb8ac96),
                base5: rgb(0x989080),
                base6: rgb(0x787060),
                base7: rgb(0x605848),
                base8: rgb(0x484030),
                fg: rgb(0x282418),
                fg_alt: rgb(0x585040),
            },
            Variant::EmberLighter => Ramp {
                bg: rgb(0xe8e4de),
                bg_alt: rgb(0xdfd9d4),
                base0: rgb(0xf2efec),
                base1: rgb(0xe8e4de),
                base2: rgb(0xdfd9d4),
                base3: rgb(0xd0ccc6),
                base4: rgb(0xc8c2b8),
                base5: rgb(0xa09484),
                base6: rgb(0x807868),
                base7: rgb(0x585040),
                base8: rgb(0x3a3428),
                fg: rgb(0x3a3428),
                fg_alt: rgb(0x585040),
            },
        }
    }

    /// The five chips the theme picker draws beside each variant name:
    /// window ground, a raised surface, then coral, gold and olive.
    pub fn swatches(self) -> [Color; 5] {
        let palette = Palette::new(self, Accent::Coral);
        [
            palette.win,
            palette.card_alt,
            palette.coral,
            palette.gold,
            palette.olive,
        ]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub variant: Variant,
    pub accent: Accent,
    /// The raw ramp, for the rare surface that wants a step the semantic names
    /// do not cover.
    pub ramp: Ramp,

    pub win: Color,
    pub panel: Color,
    pub card: Color,
    pub card_alt: Color,
    pub line: Color,
    pub line2: Color,
    pub muted: Color,
    pub dim: Color,
    pub fg: Color,
    pub on_accent: Color,
    /// `--hero`: the accent currently driving primary surfaces.
    pub hero: Color,
    /// The pool of light on the den floor, taken from the icon's
    /// `ellipse fill="#c09058"`. The only colour in the app allowed to be
    /// decorative, and only under the live task.
    pub firelight: Color,

    pub coral: Color,
    pub orange: Color,
    pub gold: Color,
    pub olive: Color,
    pub sage: Color,
    pub steel: Color,
    pub rose: Color,
    pub mauve: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Palette::new(Variant::default(), Accent::default())
    }
}

impl Palette {
    pub fn new(variant: Variant, accent: Accent) -> Palette {
        let ramp = variant.ramp();
        let accents = if variant.is_dark() {
            ACCENTS_DARK
        } else {
            ACCENTS_LIGHT
        };

        // Semantic tokens, derived from the ramp. Dark variants stack upward
        // from base0; light variants put the lightest step (base0) on top and
        // recede into the parchment, so the two run in opposite directions.
        let (win, panel, card, card_alt, on_accent) = if variant.is_dark() {
            (ramp.base0, ramp.bg, ramp.base2, ramp.base3, ramp.bg)
        } else {
            (ramp.base2, ramp.bg, ramp.base0, ramp.bg_alt, ramp.base0)
        };
        let (line, line2, muted) = if variant.is_dark() {
            (ramp.base3, ramp.base4, ramp.base7)
        } else {
            (ramp.base4, ramp.base5, ramp.base6)
        };

        let mut palette = Palette {
            variant,
            accent,
            ramp,
            win,
            panel,
            card,
            card_alt,
            line,
            line2,
            muted,
            dim: ramp.fg_alt,
            fg: ramp.fg,
            on_accent,
            hero: accents[0],
            firelight: accents[1],
            coral: accents[0],
            orange: accents[1],
            gold: accents[2],
            olive: accents[3],
            sage: accents[4],
            steel: accents[5],
            rose: accents[6],
            mauve: accents[7],
        };
        palette.hero = palette.hue(accent.hue());
        palette
    }

    /// The colour for an engine tag slot.
    ///
    /// `den-core` assigns each tag a stable slot and knows nothing about
    /// colour; choosing what a slot looks like is the interface's job.
    ///
    /// Coral and orange are deliberately **not** in the rotation. They are the
    /// hero and the firelight, and a `#tag` sitting on a cold row must never
    /// compete with the live task for attention. That leaves six hues, so two
    /// of the engine's eight slots collide — a far smaller cost than breaking
    /// the one rule the rest of the design rests on.
    pub fn slot(&self, slot: u8) -> Color {
        const TAG_HUES: [Hue; 6] = [
            Hue::Gold,
            Hue::Olive,
            Hue::Sage,
            Hue::Steel,
            Hue::Rose,
            Hue::Mauve,
        ];
        self.hue(TAG_HUES[usize::from(slot) % TAG_HUES.len()])
    }

    /// The three stops of the firelight pool: hot, warm, and the plain ground.
    ///
    /// Dark and light need opposite treatments and the first version only had
    /// one. Blending the card toward the accent *darkens* it on a paper ground,
    /// so the lit surface came out murkier than the page around it — the exact
    /// inverse of "this is the thing in the light". On light variants the pool
    /// therefore brightens toward warm white and leans on a shadow to lift the
    /// card, which is how paper actually shows light.
    pub fn firelight_stops(&self) -> (Color, Color, Color) {
        if self.variant.is_dark() {
            (
                self.blend(self.card, self.firelight, 0.30),
                self.blend(self.card, self.firelight, 0.10),
                self.card,
            )
        } else {
            // Toward a warm white, not toward the accent.
            let glow = Color {
                r: 1.0,
                g: 0.96,
                b: 0.88,
                a: 1.0,
            };
            (
                self.blend(self.card, glow, 0.85),
                self.blend(self.card, glow, 0.45),
                self.card,
            )
        }
    }

    /// The shadow that lifts a lit surface off a paper ground.
    ///
    /// Zero on dark variants: a pool of light does not cast a shadow on the
    /// floor it is cast onto.
    pub fn lift(&self) -> iced::Shadow {
        if self.variant.is_dark() {
            iced::Shadow::default()
        } else {
            iced::Shadow {
                color: Color {
                    a: 0.16,
                    ..self.ramp.base8
                },
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 14.0,
            }
        }
    }

    /// The firelight wash beneath the live task.
    ///
    /// Held here rather than written as a literal at each call site so the pool
    /// has one strength across the app, and so the light variants — where the
    /// same alpha over a paper ground reads far stronger — can differ.
    pub fn glow(&self) -> Color {
        self.wash(
            self.firelight,
            if self.variant.is_dark() { 0.17 } else { 0.13 },
        )
    }

    /// The faint spill of that light past the edge of the lit surface.
    pub fn glow_soft(&self) -> Color {
        self.wash(
            self.firelight,
            if self.variant.is_dark() { 0.09 } else { 0.06 },
        )
    }

    /// The colour a piece of state is allowed to be.
    pub fn temperature(&self, temperature: Temperature) -> Color {
        match temperature {
            Temperature::Live => self.hero,
            Temperature::Next => self.fg,
            Temperature::Cooling => self.olive,
            Temperature::Cold => self.muted,
        }
    }

    pub fn hue(&self, hue: Hue) -> Color {
        let index = Hue::ALL
            .iter()
            .position(|h| *h == hue)
            .expect("Hue::ALL is exhaustive");
        [
            self.coral,
            self.orange,
            self.gold,
            self.olive,
            self.sage,
            self.steel,
            self.rose,
            self.mauve,
        ][index]
    }

    /// `base` mixed with `amount` of `tint`, both opaque.
    ///
    /// Gradient stops interpolate between colours rather than compositing over
    /// what is behind them, so a translucent overlay is no use there — the warm
    /// end of the firelight has to be a real colour.
    pub fn blend(&self, base: Color, tint: Color, amount: f32) -> Color {
        let amount = amount.clamp(0.0, 1.0);
        Color {
            r: base.r + (tint.r - base.r) * amount,
            g: base.g + (tint.g - base.g) * amount,
            b: base.b + (tint.b - base.b) * amount,
            a: 1.0,
        }
    }

    /// A translucent wash of `color`, for hover grounds and chart fills.
    pub fn wash(&self, color: Color, alpha: f32) -> Color {
        Color { a: alpha, ..color }
    }

    /// An `iced::Theme` built from the nearest equivalents, so the internals we
    /// do not style by hand (text-input selection, scrollbars, the caret) land
    /// in the right family rather than defaulting to iced's stock blue.
    pub fn iced_theme(&self) -> iced::Theme {
        iced::Theme::custom(
            format!("{}-{}", self.variant.as_str(), self.accent.as_str()),
            iced::theme::Palette {
                background: self.win,
                text: self.fg,
                primary: self.hero,
                success: self.olive,
                warning: self.gold,
                danger: self.rose,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design's CSS custom properties are the contract for the UI; the ramp
    /// is the contract with the other Den themes. Pin the derivation so a change
    /// to either side has to be deliberate.
    #[test]
    fn semantic_tokens_match_the_design() {
        let expected = [
            (
                Variant::Ember,
                [
                    0x151412, 0x1c1b19, 0x252422, 0x2e2d2a, 0x2e2d2a, 0x3e3c38, 0x908a7e, 0xb0a898,
                    0xd8d0c0, 0x1c1b19,
                ],
            ),
            (
                Variant::EmberSoft,
                [
                    0x1c1b19, 0x242320, 0x2c2b28, 0x353430, 0x353430, 0x444240, 0x908a7e, 0xb0a898,
                    0xd8d0c0, 0x242320,
                ],
            ),
            (
                Variant::EmberLight,
                [
                    0xd8ccb0, 0xe6dac4, 0xf0e8d8, 0xddd0b8, 0xb8ac96, 0x989080, 0x787060, 0x585040,
                    0x282418, 0xf0e8d8,
                ],
            ),
            (
                Variant::EmberLighter,
                [
                    0xdfd9d4, 0xe8e4de, 0xf2efec, 0xdfd9d4, 0xc8c2b8, 0xa09484, 0x807868, 0x585040,
                    0x3a3428, 0xf2efec,
                ],
            ),
        ];
        for (variant, hexes) in expected {
            let p = Palette::new(variant, Accent::Coral);
            let actual = [
                p.win,
                p.panel,
                p.card,
                p.card_alt,
                p.line,
                p.line2,
                p.muted,
                p.dim,
                p.fg,
                p.on_accent,
            ];
            for (color, hex) in actual.into_iter().zip(hexes) {
                assert_eq!(color, rgb(hex), "{} token mismatch", variant.as_str());
            }
        }
    }

    #[test]
    fn accent_selects_the_hero() {
        for variant in Variant::ALL {
            let palette = Palette::new(variant, Accent::Steel);
            assert_eq!(palette.hero, palette.steel);
        }
    }
}

#[cfg(test)]
mod temperature_tests {
    use super::*;
    use den_core::vault::Status;

    #[test]
    fn only_the_lit_task_is_warm() {
        assert_eq!(Temperature::of(Status::Doing, true), Temperature::Live);
        // A second @status(doing) task stays legible but unlit.
        assert_eq!(Temperature::of(Status::Doing, false), Temperature::Next);
        assert_eq!(Temperature::of(Status::Backlog, false), Temperature::Cold);
        assert_eq!(Temperature::of(Status::Done, false), Temperature::Cooling);
    }

    #[test]
    fn a_tag_can_never_wear_the_hero_or_the_firelight() {
        for variant in Variant::ALL {
            for accent in Accent::ALL {
                let palette = Palette::new(variant, accent);
                for slot in 0..=32u8 {
                    let colour = palette.slot(slot);
                    assert_ne!(colour, palette.coral, "slot {slot} took the hero");
                    assert_ne!(colour, palette.orange, "slot {slot} took the firelight");
                }
            }
        }
    }

    #[test]
    fn the_glow_is_warm_and_translucent() {
        for variant in Variant::ALL {
            let palette = Palette::new(variant, Accent::Coral);
            assert_eq!(palette.firelight, palette.orange);
            let glow = palette.glow();
            assert!(glow.a > 0.0 && glow.a < 1.0, "the pool must be a wash");
            assert!(palette.glow_soft().a < glow.a, "the spill is fainter");
        }
    }
}
