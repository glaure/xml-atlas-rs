//! The table of an element's own attributes above a grid (`docs/spec/grid.md`, GRID-11), in
//! the grid and in the cell pane: only the rows in sight get cells (D5).
//!
//! It looks like an `egui::Grid` of two columns, which needs every row in every frame to know
//! how wide they are. Here the widest name and value are looked for once for each list of
//! attributes, and their widths are kept for the one shown.

use std::cmp::Ordering;
use std::ops::Range;
use std::sync::{Arc, Weak};

use egui::{
    Align, CollapsingHeader, FontId, Label, Layout, Rect, RichText, ScrollArea, TextStyle, Ui,
    UiBuilder, emath::GuiRounding as _, pos2, vec2,
};
use xml_atlas_engine::grid::Grid;

/// Rows shown before it scrolls.
const ROWS: f32 = 6.0;
/// Texts laid out to find a column's width: the widest by the advances of their characters,
/// which laying out can change a little.
const LAID_OUT: usize = 3;

type Attributes = Vec<(String, String)>;

/// The widths of the columns of one list of attributes.
#[derive(Clone)]
struct Widths {
    /// Of which list. Being weak it keeps no values, and being held its address is no other's.
    list: Weak<Attributes>,
    font: FontId,
    pixels_per_point: f32,
    name: f32,
    /// Not cut to the room there is.
    value: f32,
    height: f32,
    /// How often they were looked for, for tests: an idle view adds none.
    measured: u64,
}

/// The attribute table of `grid`, which folds away.
pub fn show(ui: &mut Ui, grid: &Grid) {
    let list = &grid.attributes;
    if list.is_empty() {
        ui.label(RichText::new("No attributes").weak());
        return;
    }
    let title = match list.len() {
        1 => "1 attribute".to_string(),
        n => format!("{n} attributes"),
    };
    // One slot for each view, whatever element it shows.
    let slot = ui.scope_id().with("grid-attribute-widths");
    CollapsingHeader::new(title)
        .id_salt(("grid-attributes", grid.parent))
        .default_open(true)
        .show(ui, |ui| {
            let row_h = ui.text_style_height(&TextStyle::Body) + 4.0;
            ScrollArea::vertical()
                .id_salt(("grid-attributes-scroll", grid.parent))
                .max_height(row_h * ROWS)
                .show_viewport(ui, |ui, viewport| table(ui, grid, slot, viewport));
        });
    ui.add_space(4.0);
}

/// The rows of `grid`'s attributes that are in `viewport`, in room for all of them.
fn table(ui: &mut Ui, grid: &Grid, slot: egui::Id, viewport: Rect) {
    let list = &grid.attributes;
    let mut widths = widths(ui, list, slot);
    let spacing = ui.spacing().item_spacing;
    let least = ui.spacing().interact_size.x;
    let height = widths.height;
    let step = height + spacing.y;
    let area = ui.available_rect_before_wrap();
    let value_x = area.left() + widths.name + spacing.x;
    let room = (area.right() - value_x).max(least);
    let full = widths.name + spacing.x + widths.value.min(room);
    let id = ui.scope_id().with(("grid-attributes-table", grid.parent));

    let (mut name_seen, mut value_seen) = (0.0_f32, 0.0_f32);
    for i in in_sight(viewport, step, list.len()) {
        let top = area.top() + step * i as f32;
        if i % 2 == 1 {
            let stripe = Rect::from_min_size(pos2(area.left(), top), vec2(full, height))
                .expand2(vec2(2.0, spacing.y / 2.0));
            ui.painter()
                .rect_filled(stripe, 2.0, ui.visuals().faint_bg_color);
        }
        let (name, value) = &list[i];
        let mut cell = |column: usize, x: f32, width: f32, label: Label| {
            let rect = Rect::from_min_size(pos2(x, top), vec2(width, height)).round_ui();
            let mut cell = ui.new_child(
                UiBuilder::new()
                    // By the row, so that hover and selection stay with it while scrolling.
                    .scope_id(id.with((i, column)))
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            cell.add(label).rect.width()
        };
        let label = Label::new(RichText::new(name).strong()).extend();
        name_seen = name_seen.max(cell(0, area.left(), widths.name, label));
        let label = Label::new(value).truncate();
        value_seen = value_seen.max(cell(1, value_x, room, label));
    }
    let total = vec2(full, step * list.len() as f32 - spacing.y);
    ui.advance_cursor_after_rect(Rect::from_min_size(area.min, total));

    // Wider than measured, as with a font that was loaded since.
    if name_seen > widths.name + 0.5 || value_seen > widths.value + 0.5 {
        widths.name = widths.name.max(name_seen);
        widths.value = widths.value.max(value_seen);
        ui.data_mut(|d| d.insert_temp(slot, widths));
        ui.ctx().request_repaint();
    }
}

/// The rows to draw of `count` that are `step` apart: those in `viewport`, and one more.
fn in_sight(viewport: Rect, step: f32, count: usize) -> Range<usize> {
    let end = ((viewport.max.y / step).ceil() as usize + 1).min(count);
    let start = ((viewport.min.y / step).floor() as usize).min(end);
    start..end
}

/// The widths for `list`, looked for when it, the font or the scale is another than before.
fn widths(ui: &Ui, list: &Arc<Attributes>, slot: egui::Id) -> Widths {
    let font = TextStyle::Body.resolve(ui.style());
    let pixels_per_point = ui.ctx().pixels_per_point();
    let this = Arc::downgrade(list);
    let kept = ui.data_mut(|d| d.get_temp::<Widths>(slot));
    if let Some(kept) = &kept
        && kept.list.ptr_eq(&this)
        && kept.font == font
        && kept.pixels_per_point == pixels_per_point
    {
        return kept.clone();
    }
    let least = ui.spacing().interact_size;
    let name = widest(ui, &font, list.iter().map(|(name, _)| name.as_str()));
    let value = widest(ui, &font, list.iter().map(|(_, value)| value.as_str()));
    let found = Widths {
        list: this,
        name: name.x.max(least.x),
        value: value.x.max(least.x),
        height: name.y.max(value.y).max(least.y),
        font,
        pixels_per_point,
        measured: kept.map_or(0, |kept| kept.measured) + 1,
    };
    ui.data_mut(|d| d.insert_temp(slot, found.clone()));
    found
}

/// The size of the widest of `texts` as a label of one line shows it.
fn widest<'t>(ui: &Ui, font: &FontId, texts: impl Iterator<Item = &'t str>) -> egui::Vec2 {
    ui.fonts_mut(|fonts| {
        let mut ascii = [f32::NAN; 128];
        let mut top: Vec<(f32, &str, bool)> = Vec::with_capacity(LAID_OUT + 1);
        for text in texts {
            // A label of one line ends at the first break, with an ellipsis.
            let (line, cut) = text.split_once('\n').map_or((text, false), |l| (l.0, true));
            let mut width = 0.0;
            for c in line.chars() {
                width += match ascii.get_mut(c as usize) {
                    Some(known) => {
                        if known.is_nan() {
                            *known = fonts.glyph_width(font, c);
                        }
                        *known
                    }
                    None => fonts.glyph_width(font, c),
                };
            }
            if cut {
                width += fonts.glyph_width(font, '…');
            }
            if top.len() == LAID_OUT && top.last().is_some_and(|last| last.0 >= width) {
                continue;
            }
            top.push((width, line, cut));
            top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
            top.truncate(LAID_OUT);
        }
        top.into_iter()
            .fold(egui::Vec2::ZERO, |size, (_, line, cut)| {
                let mut line = line.to_owned();
                if cut {
                    line.push('…');
                }
                let galley = fonts.layout_no_wrap(line, font.clone(), egui::Color32::PLACEHOLDER);
                size.max(galley.size())
            })
    })
}

#[cfg(test)]
mod tests {
    use egui::{Event, RawInput, Shape};
    use xml_atlas_engine::{Document, Edit};

    use super::*;

    struct View {
        ctx: egui::Context,
        slot: Option<egui::Id>,
    }

    impl View {
        /// One frame of the table: its texts in sight, and where the widest stripe ends.
        fn frame(&mut self, grid: &Grid, events: Vec<Event>) -> (Vec<String>, f32) {
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 500.0));
            let input = RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut out = self.ctx.run_ui(input, |ui| {
                self.slot = Some(ui.scope_id().with("grid-attribute-widths"));
                show(ui, grid);
            });
            out.textures_delta.clear();
            let mut texts = Vec::new();
            let mut stripe = 0.0_f32;
            for s in &out.shapes {
                match &s.shape {
                    Shape::Text(t) if s.clip_rect.intersects(t.visual_bounding_rect()) => {
                        texts.push(t.galley.text().to_owned());
                    }
                    Shape::Rect(r) if r.fill == self.ctx.global_style().visuals.faint_bg_color => {
                        stripe = stripe.max(r.rect.right());
                    }
                    _ => {}
                }
            }
            (texts, stripe)
        }

        fn widgets(&self) -> usize {
            self.ctx
                .viewport(|v| v.prev_pass.widgets.layers().map(|(_, w)| w.len()).sum())
        }

        fn measured(&self) -> u64 {
            let slot = self.slot.expect("drawn");
            self.ctx
                .data_mut(|d| d.get_temp::<Widths>(slot))
                .expect("measured")
                .measured
        }
    }

    fn grid(doc: &Document) -> Grid {
        Grid::start(doc, doc.index.root_element().unwrap())
    }

    fn many(last: &str) -> Document {
        let attributes: String = (0..4999).map(|i| format!(" a{i}=\"Größe {i}\"")).collect();
        let src = format!("\u{feff}<r{attributes} {last}>\r\n</r>");
        Document::from_bytes(src.into_bytes())
    }

    /// GRID-11 and AGENTS.md, rule 2: only the rows in sight get cells, and a frame in which
    /// nothing changed looks at no other.
    #[test]
    fn only_rows_in_sight_get_cells() {
        let doc = many("z=\"1\"");
        let grid = grid(&doc);
        let ctx = egui::Context::default();
        let mut view = View { ctx, slot: None };
        for _ in 0..4 {
            view.frame(&grid, vec![]);
        }
        let (texts, _) = view.frame(&grid, vec![]);
        assert!(texts.iter().any(|t| t == "Größe 0"), "{texts:?}");
        assert!(view.widgets() < 100, "{} widgets", view.widgets());
        assert_eq!(view.measured(), 1);
    }

    /// Scrolling reaches the last attribute, with room for all of them above it.
    #[test]
    fn the_last_attribute_is_reached() {
        let doc = many("größe=\"ganz unten\"");
        let grid = grid(&doc);
        let ctx = egui::Context::default();
        let mut view = View { ctx, slot: None };
        view.frame(&grid, vec![]);
        view.frame(&grid, vec![Event::PointerMoved(pos2(40.0, 60.0))]);
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -1.0e7),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Default::default(),
        };
        view.frame(&grid, vec![wheel]);
        let mut texts = Vec::new();
        for _ in 0..120 {
            texts = view.frame(&grid, vec![]).0;
        }
        assert!(texts.iter().any(|t| t == "ganz unten"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "größe"), "{texts:?}");
        assert!(view.widgets() < 100, "{} widgets", view.widgets());
    }

    /// The columns are as wide as the widest of all rows, also of one out of sight, so they
    /// stay still while scrolling; after an edit they are looked for again.
    #[test]
    fn a_row_out_of_sight_sets_the_width() {
        let end = |last: &str| {
            let doc = many(last);
            let ctx = egui::Context::default();
            View { ctx, slot: None }.frame(&grid(&doc), vec![]).1
        };
        let short = end("z=\"1\"");
        assert!(end("z=\"a value that is wider than others\"") > short + 40.0);
        assert!(end("a-name-that-is-wider-than-others=\"1\"") > short + 40.0);
        // Not by the number of characters.
        assert!(end("z=\"WWWWWWWWWW\"") > short + 20.0);

        let mut doc = many("z=\"1\"");
        let ctx = egui::Context::default();
        let mut view = View { ctx, slot: None };
        let before = view.frame(&grid(&doc), vec![]).1;
        let at = doc.src.len() - "\">\r\n</r>".len();
        doc.apply(Edit::new("Type").replace(at..at, " and now much wider than before"))
            .unwrap();
        assert!(view.frame(&grid(&doc), vec![]).1 > before + 40.0);
        assert_eq!(view.measured(), 2);
    }
}
