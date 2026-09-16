//! The Den app icon, in the four theme variants.
//!
//! `assets/icons/` is generated artwork; `assets/icons/README.txt` describes
//! the set. Detail drops with size, so the small SVG master is the right one
//! for in-app marks and the 256px raster is the right one for the window icon.

use crate::theme::Variant;

macro_rules! variant_assets {
    ($($variant:ident => $dir:literal, $stem:literal;)*) => {
        /// The simplified SVG master, correct from 16px up.
        pub fn small_svg(variant: Variant) -> &'static [u8] {
            match variant {
                $(Variant::$variant => include_bytes!(
                    concat!("../../../assets/icons/", $dir, "/", $stem, "-small.svg")
                ),)*
            }
        }

        /// The full-detail SVG master, for 64px and up.
        pub fn svg(variant: Variant) -> &'static [u8] {
            match variant {
                $(Variant::$variant => include_bytes!(
                    concat!("../../../assets/icons/", $dir, "/", $stem, ".svg")
                ),)*
            }
        }

        /// The 256px raster, used for the window icon.
        pub fn png_256(variant: Variant) -> &'static [u8] {
            match variant {
                $(Variant::$variant => include_bytes!(
                    concat!("../../../assets/icons/", $dir, "/", $stem, "-256.png")
                ),)*
            }
        }
    };
}

variant_assets! {
    Ember        => "ember",         "den-ember";
    EmberSoft    => "ember-soft",    "den-ember-soft";
    EmberLight   => "ember-light",   "den-ember-light";
    EmberLighter => "ember-lighter", "den-ember-lighter";
}

/// The default (`ember`) variant at each rendered size, for packaging. The set
/// is fixed by `assets/icons/README.txt`.
pub fn ember_png(size: u32) -> Option<&'static [u8]> {
    Some(match size {
        16 => include_bytes!("../../../assets/icons/ember/den-ember-16.png"),
        24 => include_bytes!("../../../assets/icons/ember/den-ember-24.png"),
        32 => include_bytes!("../../../assets/icons/ember/den-ember-32.png"),
        48 => include_bytes!("../../../assets/icons/ember/den-ember-48.png"),
        64 => include_bytes!("../../../assets/icons/ember/den-ember-64.png"),
        128 => include_bytes!("../../../assets/icons/ember/den-ember-128.png"),
        256 => include_bytes!("../../../assets/icons/ember/den-ember-256.png"),
        512 => include_bytes!("../../../assets/icons/ember/den-ember-512.png"),
        1024 => include_bytes!("../../../assets/icons/ember/den-ember-1024.png"),
        _ => return None,
    })
}

/// The size the window icon is rasterised at.
///
/// Large enough that macOS can downsample it for the Dock without softening
/// the bear's edges.
const ICON_PX: u32 = 512;

/// Renders the variant's icon from its **SVG** for `window::Settings::icon`.
///
/// The PNGs are a fixed-resolution export of the same artwork; rasterising the
/// vector means the icon is sharp at whatever size the platform asks for, and
/// there is one source of truth for the mark rather than two that can drift.
/// `resvg` is already in the tree — iced's `svg` feature pulls it — so this
/// costs no new dependency.
///
/// A malformed icon is not worth failing a launch over: the window simply keeps
/// the platform default.
pub fn window_icon(variant: Variant) -> Option<iced::window::Icon> {
    let tree = resvg::usvg::Tree::from_data(svg(variant), &resvg::usvg::Options::default()).ok()?;

    let size = tree.size();
    let scale = ICON_PX as f32 / size.width().max(size.height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new(ICON_PX, ICON_PX)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // resvg hands back premultiplied alpha; `from_rgba` wants it straight, and
    // skipping the conversion darkens every partially transparent edge pixel.
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let a = pixel.alpha();
            let straighten = |c: u8| {
                if a == 0 {
                    0
                } else {
                    ((c as u16 * 255) / a as u16).min(255) as u8
                }
            };
            [
                straighten(pixel.red()),
                straighten(pixel.green()),
                straighten(pixel.blue()),
                a,
            ]
        })
        .collect();

    iced::window::icon::from_rgba(rgba, ICON_PX, ICON_PX).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_artwork() {
        for variant in Variant::ALL {
            assert!(!small_svg(variant).is_empty(), "{}", variant.as_str());
            assert!(!svg(variant).is_empty(), "{}", variant.as_str());
            assert!(
                png_256(variant).starts_with(b"\x89PNG"),
                "{}",
                variant.as_str()
            );
        }
    }

    #[test]
    fn window_icons_decode() {
        for variant in Variant::ALL {
            assert!(window_icon(variant).is_some(), "{}", variant.as_str());
        }
    }
}
