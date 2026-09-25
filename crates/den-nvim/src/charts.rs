//! Charts and focus rings as PNG images, for kitty's image protocol.
//!
//! Drawn as SVG and turned into pixels with resvg. Shapes only: labels stay
//! ordinary terminal text around the image, so no fonts are loaded and the
//! text matches the rest of the editor.

use resvg::{tiny_skia, usvg};
use serde::Deserialize;

/// Colours (`#rrggbb`, from the colour scheme) and the image size in pixels.
#[derive(Debug, Clone, Deserialize)]
pub struct Style {
    pub width: u32,
    pub height: u32,
    pub fg: String,
    pub muted: String,
    pub accent: String,
    #[serde(default)]
    pub extra: Option<String>,
}

fn colour(c: &str) -> String {
    // Only `#` and hex digits reach the SVG.
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        c.to_string()
    } else {
        "#888888".to_string()
    }
}

/// Renders SVG to PNG bytes.
pub fn png(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut pixmap =
        tiny_skia::Pixmap::new(width.max(1), height.max(1)).ok_or("the image size is zero")?;
    let size = tree.size();
    let transform = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

fn open(style: &Style) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#,
        w = style.width,
        h = style.height
    )
}

/// Vertical bars, one per value.
pub fn bars(values: &[f64], style: &Style) -> String {
    let (w, h) = (style.width as f64, style.height as f64);
    let (fg, muted) = (colour(&style.fg), colour(&style.muted));
    let n = values.len().max(1) as f64;
    let slot = w / n;
    let bar = slot * 0.62;
    let max = values.iter().cloned().fold(0.0_f64, f64::max);
    let base = h - 2.0;
    let mut svg = open(style);
    svg.push_str(&format!(
        r#"<rect x="0" y="{base}" width="{w}" height="1.5" fill="{muted}" opacity="0.5"/>"#
    ));
    for (i, v) in values.iter().enumerate() {
        let x = i as f64 * slot + (slot - bar) / 2.0;
        if *v <= 0.0 || max <= 0.0 {
            svg.push_str(&format!(
                r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="2" fill="{muted}"/>"#,
                cx = x + bar / 2.0,
                cy = base - 4.0
            ));
            continue;
        }
        let bh = (v / max * (base - 6.0)).max(3.0);
        svg.push_str(&format!(
            r#"<rect x="{x:.1}" y="{y:.1}" width="{bar:.1}" height="{bh:.1}" rx="3" fill="{fg}"/>"#,
            y = base - bh
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// Tasks left per day against the straight line to zero on the end date.
pub fn burndown(points: &[f64], days: usize, style: &Style) -> String {
    let (w, h) = (style.width as f64, style.height as f64);
    let (fg, muted, accent) = (
        colour(&style.fg),
        colour(&style.muted),
        colour(&style.accent),
    );
    let days = days.max(2) as f64;
    let total = points.first().copied().unwrap_or(0.0);
    let max = points.iter().cloned().fold(total, f64::max).max(1.0);
    let pad = 6.0;
    let x = |i: f64| pad + i / (days - 1.0) * (w - 2.0 * pad);
    let y = |v: f64| h - pad - v / max * (h - 2.0 * pad);
    let mut svg = open(style);
    svg.push_str(&format!(
        r#"<line x1="{x0:.1}" y1="{b:.1}" x2="{x1:.1}" y2="{b:.1}" stroke="{muted}" stroke-width="1.5" opacity="0.5"/>"#,
        x0 = x(0.0),
        x1 = x(days - 1.0),
        b = y(0.0)
    ));
    svg.push_str(&format!(
        r#"<line x1="{x0:.1}" y1="{y0:.1}" x2="{x1:.1}" y2="{y1:.1}" stroke="{muted}" stroke-width="2" stroke-dasharray="6 6"/>"#,
        x0 = x(0.0),
        y0 = y(total),
        x1 = x(days - 1.0),
        y1 = y(0.0)
    ));
    if !points.is_empty() {
        let mut path = format!("M {:.1} {:.1}", x(0.0), y(0.0));
        let mut line = String::new();
        for (i, v) in points.iter().enumerate() {
            let (px, py) = (x(i as f64), y(*v));
            path.push_str(&format!(" L {px:.1} {py:.1}"));
            line.push_str(&format!("{}{px:.1},{py:.1}", if i == 0 { "" } else { " " }));
        }
        let last = (points.len() - 1) as f64;
        path.push_str(&format!(" L {:.1} {:.1} Z", x(last), y(0.0)));
        svg.push_str(&format!(r#"<path d="{path}" fill="{fg}" opacity="0.18"/>"#));
        svg.push_str(&format!(
            r#"<polyline points="{line}" fill="none" stroke="{fg}" stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>"#
        ));
        let (tx, ty) = (x(last), y(points[points.len() - 1]));
        svg.push_str(&format!(
            r#"<circle cx="{tx:.1}" cy="{ty:.1}" r="5" fill="{accent}"/>"#
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// Concentric rings: this session outside, today in the middle, steps
/// inside (left empty when there is no step data).
pub fn rings(fractions: [Option<f64>; 3], style: &Style) -> String {
    let size = style.width.min(style.height) as f64;
    let c = size / 2.0;
    let stroke = size * 0.085;
    let colours = [
        colour(&style.accent),
        colour(&style.fg),
        colour(style.extra.as_deref().unwrap_or(&style.fg)),
    ];
    let muted = colour(&style.muted);
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">"#
    );
    for (i, f) in fractions.iter().enumerate() {
        let r = c - stroke * (0.6 + i as f64 * 1.35);
        let length = 2.0 * std::f64::consts::PI * r;
        svg.push_str(&format!(
            r#"<circle cx="{c}" cy="{c}" r="{r:.2}" fill="none" stroke="{muted}" stroke-width="{stroke:.2}" opacity="0.25"/>"#
        ));
        let Some(f) = f else { continue };
        let f = f.clamp(0.0, 1.0);
        if f <= 0.0 {
            continue;
        }
        svg.push_str(&format!(
            r#"<circle cx="{c}" cy="{c}" r="{r:.2}" fill="none" stroke="{col}" stroke-width="{stroke:.2}" stroke-linecap="round" stroke-dasharray="{on:.2} {length:.2}" transform="rotate(-90 {c} {c})"/>"#,
            col = colours[i],
            on = f * length
        ));
    }
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style {
            width: 440,
            height: 100,
            fg: "#d4be98".into(),
            muted: "#5a524c".into(),
            accent: "#e0a060".into(),
            extra: None,
        }
    }

    fn is_png(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x89, b'P', b'N', b'G'])
    }

    #[test]
    fn every_chart_renders() {
        let s = style();
        let closed = bars(&[0.0, 1.0, 0.0, 3.0, 2.0], &s);
        assert!(is_png(&png(&closed, s.width, s.height).unwrap()));
        let down = burndown(&[7.0, 7.0, 6.0, 6.0, 5.0], 31, &s);
        assert!(is_png(&png(&down, s.width, s.height).unwrap()));
        let r = rings([Some(0.4), Some(1.3), None], &s);
        assert!(is_png(&png(&r, 100, 100).unwrap()));
    }

    #[test]
    fn only_hex_colours_reach_the_svg() {
        assert_eq!(colour("#a1B2c3"), "#a1B2c3");
        assert_eq!(colour("red\"/><script"), "#888888");
    }
}
