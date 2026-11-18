//! Row icons for the tree (`docs/spec/tree.md`, TREE-11).
//!
//! Drawn as small shapes rather than images, in the theme's syntax colours (D29), so
//! they follow the light and dark theme and stay sharp at any scale.

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use xml_atlas_engine::tree::RowKind;

use crate::theme::Syntax;

/// Width taken by an icon, including the gap to the name.
pub const WIDTH: f32 = 16.0;

/// Side of the square an icon is drawn in.
const SIZE: f32 = 10.0;

/// Draws the icon for `kind` centred on `center`.
pub fn draw(painter: &Painter, center: Pos2, kind: RowKind, syntax: &Syntax) {
    let half = SIZE / 2.0;
    let square = Rect::from_center_size(center, vec2(SIZE, SIZE));
    let line = |a: Pos2, b: Pos2, color: Color32| {
        painter.line_segment([a, b], Stroke::new(1.2, color));
    };
    match kind {
        RowKind::Parent => {
            // Two stacked squares: an element with elements inside.
            let back = square.translate(vec2(2.0, -2.0)).shrink(1.0);
            let front = square.translate(vec2(-1.0, 1.0)).shrink(1.0);
            painter.rect_stroke(
                back,
                CornerRadius::same(2),
                Stroke::new(1.2, syntax.tag),
                StrokeKind::Inside,
            );
            painter.rect_filled(front, CornerRadius::same(2), syntax.tag);
        }
        RowKind::TextElement => {
            // A square with lines of text in it.
            painter.rect_stroke(
                square,
                CornerRadius::same(2),
                Stroke::new(1.2, syntax.tag),
                StrokeKind::Inside,
            );
            line(
                pos2(center.x - 2.5, center.y - 1.5),
                pos2(center.x + 2.5, center.y - 1.5),
                syntax.text,
            );
            line(
                pos2(center.x - 2.5, center.y + 1.5),
                pos2(center.x + 1.0, center.y + 1.5),
                syntax.text,
            );
        }
        RowKind::Empty => {
            painter.rect_stroke(
                square.shrink(1.0),
                CornerRadius::same(2),
                Stroke::new(1.2, syntax.tag.gamma_multiply(0.7)),
                StrokeKind::Inside,
            );
        }
        RowKind::Attribute => {
            // A diamond in the attribute colour.
            let d = half - 1.0;
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(center.x, center.y - d),
                    pos2(center.x + d, center.y),
                    pos2(center.x, center.y + d),
                    pos2(center.x - d, center.y),
                ],
                syntax.attribute_name,
                Stroke::NONE,
            ));
        }
        RowKind::Text => {
            // Three lines of text, without a frame.
            let c = syntax.text.gamma_multiply(0.8);
            line(
                pos2(center.x - half, center.y - 3.0),
                pos2(center.x + half, center.y - 3.0),
                c,
            );
            line(
                pos2(center.x - half, center.y),
                pos2(center.x + half - 2.0, center.y),
                c,
            );
            line(
                pos2(center.x - half, center.y + 3.0),
                pos2(center.x + half - 4.0, center.y + 3.0),
                c,
            );
        }
    }
}
