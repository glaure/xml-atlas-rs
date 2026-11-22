//! The second pane beside the grid: what the clicked row or cell stands for
//! (`docs/spec/grid.md`, GRID-16). An experiment; View > Cell pane turns it off.
//!
//! The pane is worked out once per click ([`Grid::detail`]) and kept until the click, the
//! grid or the document's nodes change. Its table draws only the rows and columns on screen
//! (`crate::table`), and works each row out once.

use egui::{RichText, Ui};
use xml_atlas_engine::grid::{Column, Detail, Grid};
use xml_atlas_engine::grid_cells::Cells;
use xml_atlas_engine::xml::{NodeId, NodeKind};
use xml_atlas_engine::{Document, NodesVersion};

use crate::app::Open;
use crate::table;

/// The table's columns before the element's own: the row number and the name.
const LEAD: usize = 2;
/// The first widths of the name and of the element's own columns, and the least of both.
const NAME_WIDTH: f32 = 90.0;
const COLUMN_WIDTH: f32 = 110.0;
const COLUMN_MIN: f32 = 40.0;

/// What the pane shows, and for which click.
pub struct CellPane {
    key: Key,
    detail: Detail,
    cells: Cells,
}

/// What the pane was worked out for: the row, the column, the grid and the state of the
/// document's nodes.
#[derive(Clone, PartialEq)]
struct Key {
    row: NodeId,
    column: Option<Column>,
    parent: NodeId,
    nodes: NodesVersion,
}

impl CellPane {
    /// How often a row was worked out, for tests.
    #[cfg(test)]
    pub(crate) fn worked_out(&self) -> u64 {
        self.cells.worked_out()
    }

    /// Works out one more row, so that cells made anew in a frame count less, for tests.
    #[cfg(test)]
    pub(crate) fn count_one_more(&mut self, doc: &Document) {
        self.cells.row(doc, self.key.parent);
    }
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
        open.cell_pane = Some(CellPane {
            key,
            detail,
            cells: Cells::default(),
        });
    }
    let Some(pane) = &mut open.cell_pane else {
        return;
    };
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
            crate::grid::path_line(
                ui,
                RichText::new(&grid.path).strong(),
                &[format!("{} rows", grid.rows.len())],
            );
            if matches!(&pane.detail, Detail::Element(_)) {
                crate::attribute_table::show(ui, grid);
            }
            let cells = &mut pane.cells;
            go = ui
                .scope(|ui| {
                    ui.style_mut().interaction.selectable_labels = false;
                    egui::ScrollArea::horizontal()
                        .id_salt(("cell-pane-across", grid.parent))
                        .auto_shrink(false)
                        .show(ui, |ui| table(ui, &open.doc, grid, cells))
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
        nodes: open.doc.nodes_version(),
    })
}

/// The rows of `grid`, read only. Returns the row double-clicked.
fn table(ui: &mut Ui, doc: &Document, grid: &Grid, cells: &mut Cells) -> Option<NodeId> {
    cells.begin(doc, grid);
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let mut go = None;
    let mut columns = vec![
        table::Column::auto(24.0),
        table::Column::new(NAME_WIDTH, COLUMN_MIN),
    ];
    columns.resize(
        LEAD + grid.columns.len(),
        table::Column::new(COLUMN_WIDTH, COLUMN_MIN),
    );
    let body = table::Table::new(ui, ("cell-pane-table", grid.parent), columns).header(
        row_h + 2.0,
        |row| {
            while let Some(column) = row.next_column() {
                let title = match column {
                    0 => "#".to_owned(),
                    1 => "element".to_owned(),
                    _ => crate::grid::column_name(&grid.columns[column - LEAD]),
                };
                row.col(|ui| {
                    ui.strong(title);
                });
            }
        },
    );
    body.rows(row_h, grid.rows.len(), |row| {
        let i = row.index();
        let id = grid.rows[i];
        let shown = cells.row(doc, id);
        while let Some(column) = row.next_column() {
            row.col(|ui| match column {
                0 => {
                    ui.label(RichText::new(grid.number(i).to_string()).weak());
                }
                1 => {
                    ui.label(String::from_utf8_lossy(doc.index.name(&doc.src, id)));
                }
                _ => crate::grid::draw_cell(ui, &shown[column - LEAD].cell),
            });
        }
        if row.response().is_some_and(|r| r.double_clicked()) {
            go = Some(id);
        }
    });
    cells.end();
    go
}

#[cfg(test)]
mod reparse_tests {
    use xml_atlas_engine::{Edit, command};

    use super::*;

    /// Reading a document again after typing (TEXT-22) replaces its nodes without a new
    /// revision: the pane follows it.
    #[test]
    fn the_pane_follows_the_document_read_again() {
        let src = "\u{feff}<r>\r\n<Größe></Größe>\r\n</r>";
        let doc = Document::from_bytes(src.as_bytes().to_vec());
        let mut open = Open::new(doc, egui::Id::unique("pane-reparse"));
        let leaf = command::resolve(&open.doc, "/r/Größe").unwrap();
        open.select(leaf);
        let ctx = egui::Context::default();
        let draw = |open: &mut Open| {
            let mut out = ctx.run_ui(Default::default(), |ui| show(ui, open));
            out.textures_delta.clear();
        };

        let at = src.find("</Größe>").unwrap();
        open.type_edit(Edit::new("Type").replace(at..at, "<![CDATA[hi"))
            .unwrap();
        let end = at + "<![CDATA[hi".len();
        open.type_edit(Edit::new("Type").replace(end..end, "]]>"))
            .unwrap();
        assert!(open.doc.stale().is_some(), "waits to be read again");
        draw(&mut open);
        open.settle();
        assert!(open.doc.stale().is_none());
        draw(&mut open);

        let Some(CellPane {
            detail: Detail::Element(shown),
            ..
        }) = &open.cell_pane
        else {
            panic!("the pane shows the element");
        };
        assert_eq!(shown.columns, open.grid.as_ref().unwrap().columns);
    }
}
