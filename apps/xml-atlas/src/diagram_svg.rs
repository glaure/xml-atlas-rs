//! The diagram as an SVG file (`docs/decisions.md`, D36; issue #124).
//!
//! The boxes as they are laid out and collapsed at the moment, written as text by this
//! file: no library, any size, sharp at any zoom. The picture is the diagram's own layout,
//! at 100 %, in the colours of the window; what is marked in the window (the selection,
//! Find's match, XPath's results) is not part of it.

use std::fmt::Write;

use egui::Color32;

use crate::diagram::{GAP_X, Layout, MARGIN, PAD, Placed, TOGGLE_RADIUS};

/// The colours of the picture.
pub struct Palette {
    pub background: Color32,
    pub fill: Color32,
    pub border: Color32,
    pub line: Color32,
    pub name: Color32,
    pub weak: Color32,
    pub attributes: Color32,
    pub text: Color32,
}

/// The font sizes the layout was made with, in pixels.
pub struct Sizes {
    pub name: f32,
    pub small: f32,
}

/// The whole of `layout` as an SVG document.
pub fn to_svg(layout: &Layout, palette: &Palette, sizes: &Sizes) -> String {
    let (w, h) = (
        layout.size.x + MARGIN * 2.0 - GAP_X,
        layout.size.y + MARGIN * 2.0,
    );
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="sans-serif">"#
    );
    let _ = writeln!(
        out,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        hex(palette.background)
    );
    let _ = writeln!(out, r#"<g transform="translate({MARGIN} {MARGIN})">"#);
    let line = hex(palette.line);
    let _ = writeln!(
        out,
        r#"<g stroke="{line}" stroke-width="1" fill="none" stroke-linecap="square">"#
    );
    for b in &layout.boxes {
        lines(&mut out, layout, b);
    }
    let _ = writeln!(out, "</g>");
    for b in &layout.boxes {
        draw_box(&mut out, b, palette, sizes);
    }
    let _ = writeln!(out, "</g>\n</svg>");
    out
}

/// From a box to a bar between the columns, and from the bar to each child.
fn lines(out: &mut String, layout: &Layout, b: &Placed) {
    let Some((&first, &last)) = b.kids.first().zip(b.kids.last()) else {
        return;
    };
    let (top, bottom) = (layout.boxes[first].rect, layout.boxes[last].rect);
    let bar_x = b.rect.right() + GAP_X * 0.6;
    let y = b.rect.center().y;
    let from = b.rect.right() + 2.0 * TOGGLE_RADIUS + 3.0;
    let _ = writeln!(
        out,
        r#"<path d="M{from:.1} {y:.1}H{bar_x:.1}M{bar_x:.1} {:.1}V{:.1}"/>"#,
        top.center().y,
        bottom.center().y
    );
    for &k in &b.kids {
        let r = layout.boxes[k].rect;
        let _ = writeln!(
            out,
            r#"<path d="M{bar_x:.1} {:.1}H{:.1}"/>"#,
            r.center().y,
            r.left()
        );
    }
}

fn draw_box(out: &mut String, b: &Placed, p: &Palette, sizes: &Sizes) {
    let r = b.rect;
    let _ = writeln!(
        out,
        r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{}" stroke="{}"/>"#,
        r.left(),
        r.top(),
        r.width(),
        r.height(),
        hex(p.fill),
        hex(p.border)
    );
    let x = r.left() + PAD.x;
    let [name_w, count_w, attributes_w, text_w] = b.widths;
    let y1 = r.top() + PAD.y + sizes.name * 0.95;
    piece(out, x, y1, sizes.name, name_w, p.name, &b.name);
    piece(
        out,
        x + name_w + 6.0,
        y1,
        sizes.small,
        count_w,
        p.weak,
        &b.count,
    );
    let y2 = r.top() + PAD.y + sizes.name * 1.2 + 1.0 + sizes.small * 0.95;
    let mut x2 = x;
    if !b.attributes.is_empty() {
        piece(
            out,
            x2,
            y2,
            sizes.small,
            attributes_w,
            p.attributes,
            &b.attributes,
        );
        x2 += attributes_w + 6.0;
    }
    if b.text {
        piece(out, x2, y2, sizes.small, text_w, p.text, "text");
    }
    if b.parent {
        let (cx, cy) = (r.right() + TOGGLE_RADIUS + 3.0, r.center().y);
        let arm = TOGGLE_RADIUS * 0.55;
        let mut d = format!("M{:.1} {cy:.1}H{:.1}", cx - arm, cx + arm);
        if b.collapsed {
            let _ = write!(d, "M{cx:.1} {:.1}V{:.1}", cy - arm, cy + arm);
        }
        let _ = writeln!(
            out,
            r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="{TOGGLE_RADIUS}" fill="{}" stroke="{}"/><path d="{d}" stroke="{}" stroke-width="1.2" fill="none"/>"#,
            hex(p.fill),
            hex(p.weak),
            hex(p.weak)
        );
    }
}

/// One piece of text at `x`, squeezed or stretched to `width`, the width the window measured.
fn piece(out: &mut String, x: f32, y: f32, size: f32, width: f32, color: Color32, text: &str) {
    let _ = writeln!(
        out,
        r#"<text x="{x:.1}" y="{y:.1}" font-size="{size}" fill="{}" textLength="{width:.1}" lengthAdjust="spacingAndGlyphs">{}</text>"#,
        hex(color),
        escape(text)
    );
}

/// `#rrggbb`: the colour without its transparency.
fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// `s` as XML text: the five markup characters escaped, control characters left out.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_characters_are_escaped_and_control_characters_dropped() {
        assert_eq!(escape("a<b>&\"c'\u{1}ö"), "a&lt;b&gt;&amp;&quot;c&apos;ö");
    }

    #[test]
    fn colours_are_six_digits() {
        assert_eq!(hex(Color32::from_rgb(1, 171, 255)), "#01abff");
    }
}
