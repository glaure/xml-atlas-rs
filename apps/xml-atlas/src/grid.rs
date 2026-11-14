//! The grid view: draws an engine [`Grid`] (`docs/spec/grid.md`). Cells are decoded only for
//! visible rows.

use egui::{Color32, Rect, RichText, Stroke, Ui, pos2};
use egui_extras::{Column as TableColumn, TableBuilder};
use xml_atlas_engine::grid::{Cell, Column, Grid};
use xml_atlas_engine::xml::NodeId;

use crate::app::{Open, Pane};

/// Rows of the attribute table shown before it scrolls.
const ATTRIBUTE_ROWS: f32 = 6.0;

/// Spacing of the hatching that marks an absent value.
const HATCH_STEP: f32 = 6.0;

/// What a click in the grid asks for.
enum Action {
    /// Highlight a node in the text; the grid stays (GRID-6).
    Focus(NodeId),
    /// Make a node the selection; the grid shows its children (GRID-7).
    Enter(NodeId),
}

pub fn show(ui: &mut Ui, open: &mut Open) {
    let Some(grid) = &open.grid else { return };
    ui.horizontal(|ui| {
        ui.label(RichText::new(&grid.path).strong());
        ui.label(RichText::new(format!("{} rows", grid.rows.len())).weak());
    });
    attributes(ui, grid);
    // Cell labels must not take clicks for text selection: a click picks the row or cell.
    let scroll = open.grid_scroll_to.take();
    let action = ui.scope(|ui| {
        ui.style_mut().interaction.selectable_labels = false;
        table(ui, open, scroll)
    });
    if let Some(action) = action.inner {
        open.keys = Pane::Grid;
        match action {
            Action::Focus(id) => open.focus(id),
            Action::Enter(id) => open.select(id),
        }
    }
}

/// The selected element's own attributes (GRID-11).
fn attributes(ui: &mut Ui, grid: &Grid) {
    if grid.attributes.is_empty() {
        ui.label(RichText::new("No attributes").weak());
        return;
    }
    let title = match grid.attributes.len() {
        1 => "1 attribute".to_string(),
        n => format!("{n} attributes"),
    };
    egui::CollapsingHeader::new(title)
        .id_salt(("grid-attributes", grid.parent))
        .default_open(true)
        .show(ui, |ui| {
            let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
            egui::ScrollArea::vertical()
                .id_salt(("grid-attributes-scroll", grid.parent))
                .max_height(row_h * ATTRIBUTE_ROWS)
                .show(ui, |ui| {
                    egui::Grid::new(("grid-attributes-table", grid.parent))
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for (name, value) in &grid.attributes {
                                ui.label(RichText::new(name).strong());
                                ui.add(egui::Label::new(value).truncate());
                                ui.end_row();
                            }
                        });
                });
        });
    ui.add_space(4.0);
}

/// The child table. Returns what a click asked for.
fn table(ui: &mut Ui, open: &Open, scroll: Option<usize>) -> Option<Action> {
    let grid = open.grid.as_ref()?;
    let doc = &open.doc;
    let mut action = None;
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let digits = grid.rows.len().to_string().len() as f32;
    let number_width = digits
        * ui.fonts_mut(|f| f.glyph_width(&egui::TextStyle::Body.resolve(ui.style()), '0'))
        + 16.0;
    let mut builder = TableBuilder::new(ui);
    if let Some(row) = scroll {
        builder = builder.scroll_to_row(row, None);
    }
    builder
        .id_salt(("grid", grid.parent))
        .striped(true)
        .resizable(true)
        .auto_shrink(false)
        .sense(egui::Sense::click())
        // Wide enough for the largest row number, so it never wraps.
        .column(TableColumn::initial(number_width).at_least(number_width))
        .column(TableColumn::initial(110.0).at_least(40.0).clip(true))
        .columns(
            TableColumn::initial(140.0).at_least(40.0).clip(true),
            grid.columns.len(),
        )
        .header(row_h + 2.0, |mut h| {
            h.col(|ui| {
                ui.strong("#");
            });
            h.col(|ui| {
                ui.strong("element");
            });
            for c in &grid.columns {
                h.col(|ui| {
                    match c {
                        Column::Attribute(n) => ui.strong(format!("@{n}")),
                        Column::Child(n) => ui.strong(n),
                        Column::Text => ui.strong("text()"),
                    };
                });
            }
        })
        .body(|body| {
            body.rows(row_h, grid.rows.len(), |mut row| {
                let i = row.index();
                let id = grid.rows[i];
                // Marked when the row, or a child shown in one of its cells, is the focus.
                row.set_selected(
                    open.focus
                        .is_some_and(|f| f == id || doc.index.node(f).parent == id),
                );
                row.col(|ui| {
                    ui.label(RichText::new((i + 1).to_string()).weak());
                });
                row.col(|ui| {
                    ui.label(String::from_utf8_lossy(doc.index.name(&doc.src, id)));
                });
                for c in &grid.columns {
                    let cell = grid.cell(doc, id, c);
                    let nested = matches!(cell, Cell::Element(_) | Cell::Repeated { .. });
                    let (_, resp) = row.col(|ui| draw_cell(ui, &cell));
                    // A nested cell stands for its child element: a click highlights that
                    // child, a double-click opens it.
                    if nested && let Some(child) = grid.cell_node(doc, id, c) {
                        if resp.double_clicked() {
                            action = Some(Action::Enter(child));
                        } else if resp.clicked() {
                            action = Some(Action::Focus(child));
                        }
                    }
                }
                let resp = row.response();
                if action.is_none() {
                    if resp.double_clicked() && id != grid.parent {
                        action = Some(Action::Enter(id));
                    } else if resp.clicked() {
                        action = Some(Action::Focus(id));
                    }
                }
            });
        });
    action
}

fn draw_cell(ui: &mut Ui, cell: &Cell) {
    match cell {
        Cell::Absent => hatch(ui, ui.max_rect()),
        Cell::Value(v) => {
            ui.add(egui::Label::new(v).truncate());
        }
        Cell::Element(name) => {
            ui.add(egui::Label::new(nested_text(ui, format!("<{name}>"))).truncate());
        }
        Cell::Repeated { name, count } => {
            ui.add(egui::Label::new(nested_text(ui, format!("<{name}> ×{count}"))).truncate());
        }
    }
}

/// Text for a cell that stands for child elements: in the link colour, so it reads as
/// something to open rather than a value.
fn nested_text(ui: &Ui, s: String) -> RichText {
    RichText::new(s).color(ui.visuals().hyperlink_color)
}

/// Diagonal lines over `rect`: the row has no such attribute, child or text (GRID-13).
fn hatch(ui: &Ui, rect: Rect) {
    let painter = ui.painter_at(rect);
    let color: Color32 = ui.visuals().weak_text_color().gamma_multiply(0.35);
    let stroke = Stroke::new(1.0, color);
    let h = rect.height();
    let mut x = rect.left() - h;
    while x < rect.right() {
        painter.line_segment([pos2(x, rect.bottom()), pos2(x + h, rect.top())], stroke);
        x += HATCH_STEP;
    }
}

/// Keys the grid answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// Opens the marked row, as a double-click does (GRID-7).
    Enter,
}

/// Rows moved by Page Up and Page Down.
const PAGE_ROWS: usize = 20;

/// Moves the marked row, or opens it.
pub fn key(open: &mut Open, key: GridKey) {
    let Some(grid) = &open.grid else { return };
    let Some(last) = grid.rows.len().checked_sub(1) else {
        return;
    };
    // The marked row: the focused one, or the one whose child is focused (a nested cell).
    let cur = open.focus.and_then(|f| {
        let parent = open.doc.index.node(f).parent;
        grid.rows.iter().position(|&r| r == f || r == parent)
    });
    if key == GridKey::Enter {
        if let Some(c) = cur
            && grid.rows[c] != grid.parent
        {
            open.select(grid.rows[c]);
        }
        return;
    }
    let target = match (cur, key) {
        (None, _) => 0,
        (Some(c), GridKey::Up) => c.saturating_sub(1),
        (Some(c), GridKey::Down) => (c + 1).min(last),
        (Some(_), GridKey::Home) => 0,
        (Some(_), GridKey::End) => last,
        (Some(c), GridKey::PageUp) => c.saturating_sub(PAGE_ROWS),
        (Some(c), GridKey::PageDown) => (c + PAGE_ROWS).min(last),
        (Some(_), GridKey::Enter) => unreachable!("handled above"),
    };
    let row = grid.rows[target];
    open.focus(row);
    open.grid_scroll_to = Some(target);
}
