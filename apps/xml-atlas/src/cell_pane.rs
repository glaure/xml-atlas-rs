//! The second pane beside the grid: what the clicked row or cell stands for
//! (`docs/spec/grid.md`, GRID-16). An experiment; View > Cell pane turns it off.
//!
//! The pane is worked out once per click ([`Grid::detail`]) and kept until the click, the
//! grid or the document changes. Its table draws only the rows on screen.

use egui::{RichText, Ui};
use egui_extras::{Column as TableColumn, TableBuilder};
use xml_atlas_engine::Document;
use xml_atlas_engine::grid::{Column, Detail, Grid};
use xml_atlas_engine::xml::{NodeId, NodeKind};

use crate::app::Open;

/// What the pane shows, and for which click.
pub struct CellPane {
    key: Key,
    detail: Detail,
}

/// What the pane was worked out for: the row, the column, the grid and the document revision.
#[derive(Clone, PartialEq)]
struct Key {
    row: NodeId,
    column: Option<Column>,
    parent: NodeId,
    revision: u64,
}

pub fn show(ui: &mut Ui, open: &mut Open) {
    let Some(key) = key(open) else {
        open.cell_pane = None;
        ui.label(RichText::new("Click a row or a cell in the grid to see it here.").weak());
        return;
    };
    let stale = open.cell_pane.as_ref().is_none_or(|p| p.key != key);
    if stale && let Some(grid) = &open.grid {
        let detail = grid.detail(&open.doc, key.row, key.column.as_ref());
        open.cell_pane = Some(CellPane { key, detail });
    }
    let Some(pane) = &open.cell_pane else { return };
    let mut go = None;
    match &pane.detail {
        Detail::Absent => {
            ui.label(RichText::new("Nothing here: the row has no such value.").weak());
        }
        Detail::Value { name, value } => {
            ui.label(RichText::new(name).strong());
            egui::ScrollArea::vertical()
                .id_salt("cell-pane-value")
                .auto_shrink(false)
                .show(ui, |ui| {
                    // Read only, but the text can be selected and copied.
                    ui.add(
                        egui::TextEdit::multiline(&mut value.as_str())
                            .desired_width(f32::INFINITY)
                            .code_editor(),
                    );
                });
        }
        Detail::Element(grid) | Detail::Elements(grid) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&grid.path).strong());
                ui.label(RichText::new(format!("{} rows", grid.rows.len())).weak());
            });
            if matches!(pane.detail, Detail::Element(_)) {
                crate::grid::attributes(ui, grid);
            }
            go = ui
                .scope(|ui| {
                    ui.style_mut().interaction.selectable_labels = false;
                    egui::ScrollArea::horizontal()
                        .id_salt(("cell-pane-across", grid.parent))
                        .auto_shrink(false)
                        .show(ui, |ui| table(ui, &open.doc, grid))
                        .inner
                })
                .inner;
        }
    }
    // A double-click goes to the element: it becomes the selection.
    if let Some(id) = go {
        open.select(id);
    }
}

/// The row and column marked in the grid; `None` when nothing in it was clicked.
fn key(open: &Open) -> Option<Key> {
    let grid = open.grid.as_ref()?;
    let focus = open.focus?;
    let idx = &open.doc.index;
    // The grid shows the selection itself as its one row (GRID-5).
    let single = grid.rows.len() == 1 && grid.rows[0] == grid.parent;
    let is_row = |id: NodeId| {
        idx.node(id).kind == NodeKind::Element
            && (idx.node(id).parent == grid.parent || (single && id == grid.parent))
    };
    // A click on a nested cell marks the child it stands for; its row is the parent.
    let row = if is_row(focus) {
        focus
    } else {
        Some(idx.node(focus).parent).filter(|&p| is_row(p))?
    };
    let column = open.grid_cell.and_then(|c| grid.columns.get(c)).cloned();
    Some(Key {
        row,
        column,
        parent: grid.parent,
        revision: open.doc.revision(),
    })
}

/// The rows of `grid`, read only. Returns the row double-clicked.
fn table(ui: &mut Ui, doc: &Document, grid: &Grid) -> Option<NodeId> {
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let mut go = None;
    TableBuilder::new(ui)
        .id_salt(("cell-pane-table", grid.parent))
        .striped(true)
        .resizable(true)
        .auto_shrink(false)
        .sense(egui::Sense::click())
        .column(TableColumn::auto().at_least(24.0))
        .column(TableColumn::initial(90.0).at_least(40.0).clip(true))
        .columns(
            TableColumn::initial(110.0).at_least(40.0).clip(true),
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
                    ui.strong(crate::grid::column_name(c));
                });
            }
        })
        .body(|body| {
            body.rows(row_h, grid.rows.len(), |mut row| {
                let i = row.index();
                let id = grid.rows[i];
                row.col(|ui| {
                    ui.label(RichText::new(grid.number(i).to_string()).weak());
                });
                row.col(|ui| {
                    ui.label(String::from_utf8_lossy(doc.index.name(&doc.src, id)));
                });
                for c in &grid.columns {
                    row.col(|ui| crate::grid::draw_cell(ui, &grid.cell(doc, id, c)));
                }
                if row.response().double_clicked() {
                    go = Some(id);
                }
            });
        });
    go
}
