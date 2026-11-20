//! Toolbar icons (`docs/spec/window.md`, WIN-5).
//!
//! Drawn as shapes, as the tree's icons are (TREE-11), rather than taken from a font: the
//! symbols the toolbar used are missing from the fonts egui finds on Windows, which showed
//! them as boxes. Each icon is drawn in the button's text colour, so it follows the theme and
//! is greyed out with a disabled button.

use egui::{
    Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};

/// Width and height of a toolbar button.
const BUTTON: f32 = 24.0;
/// Width and height of the icon inside it.
const ICON: f32 = 14.0;
/// Width of the icons' lines.
const LINE: f32 = 1.3;
/// Corner rounding of the hover background.
const HOVER_ROUNDING: f32 = 3.0;

/// What a toolbar button does, and so which icon it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    New,
    Open,
    Save,
    Cut,
    Copy,
    Paste,
    Undo,
    Redo,
    Find,
    /// Edit > XPath… (XP-1).
    XPath,
    DrillUp,
    DrillDown,
    Format,
    /// View > Diagram (D36).
    Diagram,
}

impl Tool {
    /// Every tool, in toolbar order.
    #[cfg(test)]
    pub const ALL: [Tool; 14] = [
        Tool::New,
        Tool::Open,
        Tool::Save,
        Tool::Cut,
        Tool::Copy,
        Tool::Paste,
        Tool::Undo,
        Tool::Redo,
        Tool::Find,
        Tool::XPath,
        Tool::DrillUp,
        Tool::DrillDown,
        Tool::Format,
        Tool::Diagram,
    ];
}

/// A toolbar button with `tool`'s icon and `tip` on hover, greyed out unless `enabled`.
/// Returns whether it was clicked.
pub fn button(ui: &mut Ui, enabled: bool, tool: Tool, tip: &str) -> bool {
    ui.add_enabled(enabled, |ui: &mut Ui| icon_button(ui, tool))
        .on_hover_text(tip)
        .on_disabled_hover_text(tip)
        .clicked()
}

/// Draws `tool`'s icon centred on `center`, `color` for its lines and fills.
pub fn draw(painter: &Painter, center: Pos2, tool: Tool, color: Color32) {
    let r = Rect::from_center_size(center, vec2(ICON, ICON));
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    let stroke = Stroke::new(LINE, color);
    let line = |points: Vec<Pos2>| {
        painter.add(Shape::line(points, stroke));
    };
    let closed = |points: Vec<Pos2>| {
        painter.add(Shape::closed_line(points, stroke));
    };
    let fill = |points: Vec<Pos2>| {
        painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
    };
    match tool {
        Tool::New => {
            // A sheet with its top right corner folded.
            let fold = 4.0;
            closed(vec![
                pos2(l + 2.0, t),
                pos2(rt - 2.0 - fold, t),
                pos2(rt - 2.0, t + fold),
                pos2(rt - 2.0, b),
                pos2(l + 2.0, b),
            ]);
            line(vec![
                pos2(rt - 2.0 - fold, t),
                pos2(rt - 2.0 - fold, t + fold),
                pos2(rt - 2.0, t + fold),
            ]);
        }
        Tool::Open => {
            // A folder: its tab, its back, and its front opened down and to the right.
            closed(vec![
                pos2(l, b - 1.0),
                pos2(l, t + 2.0),
                pos2(l + 5.0, t + 2.0),
                pos2(l + 6.5, t + 4.0),
                pos2(rt - 2.0, t + 4.0),
                pos2(rt - 2.0, t + 6.0),
            ]);
            closed(vec![
                pos2(l, b - 1.0),
                pos2(l + 3.0, t + 6.0),
                pos2(rt, t + 6.0),
                pos2(rt - 3.0, b - 1.0),
            ]);
        }
        Tool::Save => {
            // A floppy disk: its outline with a cut corner, the shutter and the label.
            closed(vec![
                pos2(l + 1.0, t + 1.0),
                pos2(rt - 3.0, t + 1.0),
                pos2(rt - 1.0, t + 3.0),
                pos2(rt - 1.0, b - 1.0),
                pos2(l + 1.0, b - 1.0),
            ]);
            painter.rect_filled(
                Rect::from_min_max(pos2(l + 4.0, t + 1.0), pos2(rt - 5.0, t + 5.0)),
                0.0,
                color,
            );
            painter.rect_stroke(
                Rect::from_min_max(pos2(l + 3.5, b - 6.0), pos2(rt - 3.5, b - 1.0)),
                0.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        Tool::Cut => {
            // Scissors: two crossed blades and two finger rings.
            let ring = 2.3;
            let left = pos2(l + 3.0, b - 3.0);
            let right = pos2(rt - 3.0, b - 3.0);
            painter.circle_stroke(left, ring, stroke);
            painter.circle_stroke(right, ring, stroke);
            line(vec![pos2(left.x + 1.5, left.y - 1.8), pos2(rt - 3.0, t)]);
            line(vec![pos2(right.x - 1.5, right.y - 1.8), pos2(l + 3.0, t)]);
        }
        Tool::Copy => {
            // Two sheets, the back one showing above and to the right of the front one.
            let shift = 4.0;
            line(vec![
                pos2(l + 1.0 + shift, t + shift),
                pos2(l + 1.0 + shift, t),
                pos2(rt - 1.0, t),
                pos2(rt - 1.0, b - shift),
                pos2(rt - 1.0 - shift, b - shift),
            ]);
            painter.rect_stroke(
                Rect::from_min_max(pos2(l + 1.0, t + shift), pos2(rt - 1.0 - shift, b)),
                0.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        Tool::Paste => {
            // A clipboard: the board, its clip and lines of text on it.
            closed(vec![
                pos2(l + 4.0, t + 2.0),
                pos2(l + 1.0, t + 2.0),
                pos2(l + 1.0, b),
                pos2(rt - 1.0, b),
                pos2(rt - 1.0, t + 2.0),
                pos2(rt - 4.0, t + 2.0),
            ]);
            painter.rect_filled(
                Rect::from_min_max(pos2(l + 4.0, t), pos2(rt - 4.0, t + 3.5)),
                1.0,
                color,
            );
            line(vec![pos2(l + 4.0, t + 7.0), pos2(rt - 4.0, t + 7.0)]);
            line(vec![pos2(l + 4.0, t + 10.0), pos2(rt - 5.5, t + 10.0)]);
        }
        Tool::Undo | Tool::Redo => {
            // A hook: an arrow pointing back, turning round on the far side.
            let flip = tool == Tool::Redo;
            let x = |x: f32| if flip { l + rt - x } else { x };
            let top = t + 4.0;
            let bottom = b - 1.0;
            let mid_y = (top + bottom) / 2.0;
            let radius = (bottom - top) / 2.0;
            let turn_x = rt - 1.0 - radius;
            let mut points = vec![pos2(x(l + 4.0), top), pos2(x(turn_x), top)];
            for i in 1..=8 {
                // From the top round the far side to the bottom.
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 8.0;
                points.push(pos2(x(turn_x + radius * a.cos()), mid_y + radius * a.sin()));
            }
            points.push(pos2(x(l + 5.0), bottom));
            line(points);
            fill(arrow_head(x(l), top, x(l + 4.5), 3.2));
        }
        Tool::Find => {
            // A magnifying glass.
            let lens = pos2(l + 5.5, t + 5.5);
            painter.circle_stroke(lens, 4.5, stroke);
            painter.line_segment(
                [pos2(l + 9.0, t + 9.0), pos2(rt - 0.5, b - 0.5)],
                Stroke::new(2.2, color),
            );
        }
        Tool::XPath => {
            // A small tree, its last node filled: nodes selected by a path.
            let node = |x: f32, y: f32, filled: bool| {
                let n = Rect::from_min_size(pos2(x, y), vec2(4.5, 4.5));
                if filled {
                    painter.rect_filled(n, 0.5, color);
                } else {
                    painter.rect_stroke(n, 0.5, stroke, StrokeKind::Inside);
                }
            };
            node(l + 0.5, t + 0.5, false);
            node(l + 8.5, t + 4.75, false);
            node(l + 8.5, b - 5.0, true);
            line(vec![
                pos2(l + 2.75, t + 5.0),
                pos2(l + 2.75, b - 2.75),
                pos2(l + 8.5, b - 2.75),
            ]);
            line(vec![pos2(l + 2.75, t + 7.0), pos2(l + 8.5, t + 7.0)]);
        }
        Tool::DrillUp | Tool::DrillDown => {
            // A broad arrow up or down.
            let up = tool == Tool::DrillUp;
            let y = |y: f32| if up { y } else { t + b - y };
            let c = r.center().x;
            fill(vec![
                pos2(c, y(t + 1.0)),
                pos2(rt - 1.0, y(t + 7.5)),
                pos2(l + 1.0, y(t + 7.5)),
            ]);
            painter.rect_filled(
                Rect::from_x_y_ranges(c - 2.0..=c + 2.0, {
                    let (a, z) = (y(t + 7.0), y(b - 1.0));
                    a.min(z)..=a.max(z)
                }),
                0.0,
                color,
            );
        }
        Tool::Format => {
            // Lines of text, indented as Format lays out elements.
            let rows = [(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (8.0, 0.0), (0.0, 5.0)];
            for (i, (indent, short)) in rows.into_iter().enumerate() {
                let yy = t + 1.0 + i as f32 * 3.0;
                line(vec![pos2(l + 1.0 + indent, yy), pos2(rt - 1.0 - short, yy)]);
            }
        }
        Tool::Diagram => {
            // A box joined to two boxes right of it, as the diagram lays out a path's
            // children.
            let node = |x: f32, y: f32| {
                let n = Rect::from_min_size(pos2(x, y), vec2(5.0, 4.0));
                painter.rect_stroke(n, 0.5, stroke, StrokeKind::Inside);
            };
            let mid = r.center().y;
            node(l + 0.5, mid - 2.0);
            node(rt - 5.5, t + 1.0);
            node(rt - 5.5, b - 5.0);
            let bar = l + 7.5;
            line(vec![pos2(l + 5.5, mid), pos2(bar, mid)]);
            line(vec![pos2(bar, t + 3.0), pos2(bar, b - 3.0)]);
            line(vec![pos2(bar, t + 3.0), pos2(rt - 5.5, t + 3.0)]);
            line(vec![pos2(bar, b - 3.0), pos2(rt - 5.5, b - 3.0)]);
        }
    }
}

/// The button itself: a frameless square that shows a background under the pointer.
fn icon_button(ui: &mut Ui, tool: Tool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&resp);
        if resp.hovered() || resp.has_focus() {
            ui.painter()
                .rect_filled(rect, HOVER_ROUNDING, visuals.weak_bg_fill);
        }
        draw(ui.painter(), rect.center(), tool, visuals.fg_stroke.color);
    }
    resp
}

/// A filled arrow head with its tip at (`tip_x`, `y`), pointing away from `base_x`, `half`
/// high on each side.
fn arrow_head(tip_x: f32, y: f32, base_x: f32, half: f32) -> Vec<Pos2> {
    vec![
        pos2(tip_x, y),
        pos2(base_x, y - half),
        pos2(base_x, y + half),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon is drawn from shapes, without any text, and stays inside its square.
    #[test]
    fn icons_are_shapes_inside_their_square() {
        let ctx = egui::Context::default();
        for tool in Tool::ALL {
            let center = pos2(100.0, 100.0);
            let mut shapes = Vec::new();
            let mut out = ctx.run_ui(Default::default(), |ui| {
                draw(ui.painter(), center, tool, Color32::BLACK);
            });
            out.textures_delta.clear();
            for clipped in out.shapes {
                shapes.push(clipped.shape);
            }
            assert!(!shapes.is_empty(), "{tool:?} draws nothing");
            let square = Rect::from_center_size(center, vec2(ICON, ICON)).expand(LINE);
            for shape in &shapes {
                assert!(
                    !matches!(shape, Shape::Text(_)),
                    "{tool:?} draws text: {shape:?}"
                );
                let bounds = shape.visual_bounding_rect();
                assert!(
                    square.contains_rect(bounds),
                    "{tool:?} leaves its square: {bounds:?}"
                );
            }
        }
    }

    /// No two tools look the same.
    #[test]
    fn every_tool_has_its_own_icon() {
        let ctx = egui::Context::default();
        let mut drawn: Vec<(Tool, String)> = Vec::new();
        for tool in Tool::ALL {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                draw(ui.painter(), pos2(50.0, 50.0), tool, Color32::BLACK);
            });
            out.textures_delta.clear();
            let shapes: Vec<_> = out.shapes.into_iter().map(|c| c.shape).collect();
            let text = format!("{shapes:?}");
            if let Some((other, _)) = drawn.iter().find(|(_, t)| *t == text) {
                panic!("{tool:?} looks like {other:?}");
            }
            drawn.push((tool, text));
        }
    }
}
