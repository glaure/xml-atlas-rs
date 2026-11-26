//! The examples of the themes in Settings > Appearance (`docs/spec/window.md`, WIN-12).
//!
//! Each is a small window painted with a [`Theme`]'s own colours, the text view's highlighter
//! and the tree's icons, not a stored picture: it cannot fall behind the theme, stays sharp
//! at any scale, and will show a theme the user wrote (D29) as well. Only the Settings window
//! draws them.

use crate::lang::Lang;
use egui::{
    Align2, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use xml_atlas_engine::tree::RowKind;

use crate::highlight::{State, line_job};
use crate::icons;
use crate::theme::Theme;

/// Size of one example.
const SIZE: Vec2 = vec2(144.0, 112.0);
/// Heights of the menu, the toolbar and the status bar in it.
const MENU: f32 = 13.0;
const TOOLBAR: f32 = 12.0;
const STATUS: f32 = 11.0;
/// Width of the tree, and height of the grid below the text.
const TREE: f32 = 56.0;
const GRID: f32 = 30.0;
const ROW: f32 = 10.0;
const FONT: f32 = 7.5;
const HATCH_STEP: f32 = 4.0;

/// What the tree shows: depth, name, icon, selected.
const TREE_ROWS: [(f32, &str, RowKind, bool); 5] = [
    (0.0, "orders", RowKind::Parent, false),
    (1.0, "order", RowKind::Parent, true),
    (2.0, "id", RowKind::Attribute, false),
    (2.0, "name", RowKind::TextElement, false),
    (1.0, "order", RowKind::Parent, false),
];
/// What the text shows; the first two lines are in the selected element.
const TEXT_LINES: [&str; 4] = [
    "<order id=\"7\">",
    " <name>Zoë</name>",
    "</order>",
    "<!-- next -->",
];
const SELECTED_LINES: usize = 3;

/// An example of one theme, or of two side by side for a choice that follows the system,
/// above the radio button of `value`. A click on either chooses it.
pub fn choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    lang: Lang,
    current: &mut T,
    value: T,
    label: &str,
    themes: &[&Theme],
) -> Response {
    ui.vertical(|ui| {
        // Not focusable: the radio button below is the choice for keys and screen readers.
        let (rect, image) = ui.allocate_exact_size(SIZE, Sense::CLICK);
        if ui.is_rect_visible(rect) {
            let width = rect.width() / themes.len() as f32;
            for (i, theme) in themes.iter().enumerate() {
                let left = rect.left() + width * i as f32;
                let part = Rect::from_min_size(pos2(left, rect.top()), vec2(width, rect.height()));
                paint(&ui.painter_at(part), lang, rect, theme);
            }
            let visuals = ui.visuals();
            let border = if *current == value {
                Stroke::new(2.0, visuals.selection.stroke.color)
            } else if image.hovered() {
                Stroke::new(1.0, visuals.text_color())
            } else {
                visuals.widgets.noninteractive.bg_stroke
            };
            ui.painter()
                .rect_stroke(rect, 2.0, border, StrokeKind::Outside);
        }
        if image.clicked() {
            *current = value;
        }
        ui.radio_value(current, value, label);
        image
    })
    .inner
}

/// The window in miniature, as the default layout has it (WIN-1).
fn paint(painter: &Painter, lang: Lang, window: Rect, theme: &Theme) {
    let v = &theme.visuals;
    let font = FontId::proportional(FONT);
    let label = |x: f32, y: f32, text: &str, color| {
        painter.text(pos2(x, y), Align2::LEFT_CENTER, text, font.clone(), color)
    };
    let separator = v.widgets.noninteractive.bg_stroke;

    painter.rect_filled(window, 0.0, v.panel_fill);
    let top = window.top() + MENU + TOOLBAR;
    let bottom = window.bottom() - STATUS;
    let split = window.left() + TREE;
    let views = Rect::from_min_max(pos2(window.left(), top), pos2(window.right(), bottom));
    painter.rect_filled(views, 0.0, theme.view_background);

    // Menu, toolbar and status bar.
    let y = window.top() + MENU / 2.0;
    let mut x = window.left() + 4.0;
    for name in [lang.menu_file(), lang.menu_edit(), lang.menu_view()] {
        // As many names as the little window has room for: German ones are longer.
        let font = font.clone();
        let width = painter
            .layout_no_wrap(name.to_owned(), font, v.text_color())
            .size()
            .x;
        if x + width > window.right() - 2.0 {
            break;
        }
        x = label(x, y, name, v.text_color()).right() + 5.0;
    }
    for i in 0..4 {
        let at = pos2(
            window.left() + 4.0 + 12.0 * i as f32,
            window.top() + MENU + 1.0,
        );
        let button = Rect::from_min_size(at, vec2(9.0, 9.0));
        painter.rect_filled(button, 1.0, v.widgets.inactive.weak_bg_fill);
    }
    let y = bottom + STATUS / 2.0;
    label(window.left() + 4.0, y, "orders.xml", v.weak_text_color());

    // Tree.
    let tree = Rect::from_min_max(views.min, pos2(split, bottom));
    let in_tree = painter.with_clip_rect(tree.intersect(painter.clip_rect()));
    for (i, (depth, name, kind, selected)) in TREE_ROWS.into_iter().enumerate() {
        let row_top = top + 2.0 + ROW * i as f32;
        let row = Rect::from_min_size(pos2(tree.left(), row_top), vec2(TREE, ROW));
        let color = if selected {
            in_tree.rect_filled(row, 0.0, v.selection.bg_fill);
            v.selection.stroke.color
        } else {
            v.text_color()
        };
        let x = tree.left() + 8.0 + 5.0 * depth;
        icons::draw(&in_tree, pos2(x, row.center().y), kind, &theme.syntax);
        in_tree.text(
            pos2(x + 9.0, row.center().y),
            Align2::LEFT_CENTER,
            name,
            font.clone(),
            color,
        );
    }

    // Text, the selected element behind its lines.
    let mono = FontId::monospace(FONT);
    let mut state = State::default();
    for (i, line) in TEXT_LINES.iter().enumerate() {
        let at = pos2(split + 4.0, top + 2.0 + ROW * i as f32);
        if i < SELECTED_LINES {
            let row = Rect::from_min_max(
                pos2(split + 2.0, at.y),
                pos2(window.right() - 2.0, at.y + ROW),
            );
            painter.rect_filled(row, 0.0, theme.selected_element);
        }
        let (job, end) = line_job(line, &mono, &theme.syntax, state);
        state = end;
        let galley = painter.layout_job(job);
        painter.galley(at, galley, theme.syntax.text);
    }

    // Grid, with a striped row and a hatched cell for an absent value (GRID-13).
    let grid_top = bottom - GRID;
    let column = split + (window.right() - split) / 2.0;
    let striped = Rect::from_min_max(
        pos2(split, grid_top + ROW),
        pos2(window.right(), grid_top + 2.0 * ROW),
    );
    painter.rect_filled(striped, 0.0, v.faint_bg_color);
    for (row, (id, name)) in [("@id", "name"), ("7", "Zoë"), ("8", "")]
        .into_iter()
        .enumerate()
    {
        let y = grid_top + ROW * (row as f32 + 0.5);
        let color = if row == 0 {
            v.strong_text_color()
        } else {
            v.text_color()
        };
        label(split + 4.0, y, id, color);
        label(column + 4.0, y, name, color);
    }
    let absent = Rect::from_min_max(
        pos2(column, grid_top + 2.0 * ROW),
        pos2(window.right(), bottom),
    );
    let hatched = painter.with_clip_rect(absent.intersect(painter.clip_rect()));
    let mut x = absent.left() - absent.height();
    while x < absent.right() {
        hatched.line_segment(
            [
                pos2(x, absent.bottom()),
                pos2(x + absent.height(), absent.top()),
            ],
            Stroke::new(1.0, theme.hatch),
        );
        x += HATCH_STEP;
    }

    painter.hline(window.x_range(), top, separator);
    painter.hline(window.x_range(), bottom, separator);
    painter.hline(split..=window.right(), grid_top, separator);
    painter.vline(split, top..=bottom, separator);
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Context, Event, Modifiers, PointerButton, RawInput, Shape};

    use super::*;

    /// One frame with the example of `themes`: where it is, and what was painted as (clip,
    /// shape).
    fn frame(
        ctx: &Context,
        events: Vec<Event>,
        current: &mut u8,
        themes: &[&Theme],
    ) -> (Rect, Vec<(Rect, Shape)>) {
        let input = RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut rect = Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            rect = choice(ui, crate::lang::Lang::En, current, 1, "Größe", themes).rect;
        });
        out.textures_delta.clear();
        let shapes = out
            .shapes
            .into_iter()
            .map(|s| (s.clip_rect, s.shape))
            .collect();
        (rect, shapes)
    }

    /// Every colour used by `shapes` for a fill or for text.
    fn colours(shapes: &[(Rect, Shape)]) -> Vec<Color32> {
        let mut found = Vec::new();
        for (_, shape) in shapes {
            match shape {
                Shape::Rect(r) => found.push(r.fill),
                Shape::Text(t) => {
                    found.push(t.fallback_color);
                    found.extend(t.galley.job.sections.iter().map(|s| s.format.color));
                }
                _ => {}
            }
        }
        found
    }

    /// Each example is drawn in its theme's colours, not in those of the window it is in
    /// (WIN-12).
    #[test]
    fn an_example_has_its_themes_colours() {
        for theme in [Theme::light(), Theme::light_classic(), Theme::dark()] {
            let ctx = Context::default();
            let (_, shapes) = frame(&ctx, Vec::new(), &mut 0, &[&theme]);
            let found = colours(&shapes);
            let s = &theme.syntax;
            for (what, colour) in [
                ("chrome", theme.visuals.panel_fill),
                ("views", theme.view_background),
                ("selection", theme.visuals.selection.bg_fill),
                ("selected element", theme.selected_element),
                ("tag", s.tag),
                ("attribute name", s.attribute_name),
                ("attribute value", s.attribute_value),
                ("comment", s.comment),
            ] {
                assert!(found.contains(&colour), "{what} is missing");
            }
        }
    }

    /// Follow system shows light on the left and dark on the right.
    #[test]
    fn two_themes_share_one_example_side_by_side() {
        let (light, dark) = (Theme::light(), Theme::dark());
        let ctx = Context::default();
        let (rect, shapes) = frame(&ctx, Vec::new(), &mut 0, &[&light, &dark]);
        let middle = rect.center().x;
        let clips = |fill: Color32| -> Vec<Rect> {
            shapes
                .iter()
                .filter(|(_, s)| matches!(s, Shape::Rect(r) if r.fill == fill && r.rect == rect))
                .map(|(clip, _)| *clip)
                .collect()
        };
        let (left, right) = (
            clips(light.visuals.panel_fill),
            clips(dark.visuals.panel_fill),
        );
        assert_eq!((left.len(), right.len()), (1, 1));
        assert!((left[0].right() - middle).abs() <= 1.0);
        assert!((right[0].left() - middle).abs() <= 1.0);
    }

    /// A click on the picture chooses, as one on the radio button does.
    #[test]
    fn a_click_on_the_example_chooses_it() {
        let ctx = Context::default();
        let theme = Theme::dark();
        let mut current = 0;
        let (rect, _) = frame(&ctx, Vec::new(), &mut current, &[&theme]);
        assert_eq!(current, 0);
        let pos = rect.center();
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(
            &ctx,
            vec![Event::PointerMoved(pos), button(true)],
            &mut current,
            &[&theme],
        );
        frame(&ctx, vec![button(false)], &mut current, &[&theme]);
        assert_eq!(current, 1);
    }
}
