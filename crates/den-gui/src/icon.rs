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

/// Decodes the variant's 256px icon for `window::Settings::icon`.
///
/// A missing or malformed icon is not worth failing a launch over — the window
/// simply keeps the platform default.
pub fn window_icon(variant: Variant) -> Option<iced::window::Icon> {
    let decoder = png::Decoder::new(png_256(variant));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 0xff])
            .collect(),
        _ => return None,
    };

    iced::window::icon::from_rgba(rgba, info.width, info.height).ok()
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
