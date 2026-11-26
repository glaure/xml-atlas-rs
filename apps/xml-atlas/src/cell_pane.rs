//! The second pane beside the grid: what the clicked row or cell stands for
//! (`docs/spec/grid.md`, GRID-16). An experiment; View > Cell pane turns it off.
//!
//! The pane is worked out once per click ([`Grid::detail`]) and kept until the click, the
//! grid or the document's nodes change. Its table draws only the rows and columns on screen
//! (`crate::table`), and works each row out once.

use egui::{RichText, Ui};
use xml_atlas_engine::grid::{Column, DETAIL_CHARS, Detail, Grid};
use xml_atlas_engine::grid_cells::Cells;
use xml_atlas_engine::xml::{NodeId, NodeKind};
use xml_atlas_engine::{Document, NodesVersion};

use crate::app::Open;
use crate::lang::Lang;
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
    let lang = open.lang;
    let Some(key) = key(open) else {
        open.cell_pane = None;
        ui.label(RichText::new(lang.cell_empty()).weak());
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
    let mut copy = false;
    match &pane.detail {
        Detail::Absent => {
            ui.label(RichText::new(lang.cell_absent()).weak());
        }
        Detail::Value { name, value, cut } => {
            ui.label(RichText::new(name).strong());
            if *cut {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(cut_notice(lang)).weak());
                    copy = ui.button(lang.cell_copy_whole()).clicked();
                });
            }
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
                &[lang.grid_rows(grid.rows.len() as u64)],
            );
            if matches!(&pane.detail, Detail::Element(_)) {
                crate::attribute_table::show(ui, lang, grid);
            }
            let cells = &mut pane.cells;
            go = ui
                .scope(|ui| {
                    ui.style_mut().interaction.selectable_labels = false;
                    egui::ScrollArea::horizontal()
                        .id_salt(("cell-pane-across", grid.parent))
                        .auto_shrink(false)
                        .show(ui, |ui| table(ui, lang, &open.doc, grid, cells))
                        .inner
                })
                .inner;
        }
    }
    if copy
        && let (Some(grid), Some(column)) = (&open.grid, &pane.key.column)
        && let Some(whole) = grid.whole_value(&open.doc, pane.key.row, column)
    {
        ui.ctx().copy_text(whole);
    }
    // A double-click goes to the element: it becomes the selection.
    if let Some(id) = go {
        open.select(id);
    }
}

/// What the pane says above a value it cuts.
fn cut_notice(lang: Lang) -> String {
    let number = crate::diagram::grouped(lang, DETAIL_CHARS as u64);
    lang.cell_cut(&number)
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
fn table(
    ui: &mut Ui,
    lang: Lang,
    doc: &Document,
    grid: &Grid,
    cells: &mut Cells,
) -> Option<NodeId> {
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
                    1 => lang.grid_column_element().to_owned(),
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
        open.type_edit(
            Edit::new(xml_atlas_engine::StepName::Other("Type".into()))
                .replace(at..at, "<![CDATA[hi"),
        )
        .unwrap();
        let end = at + "<![CDATA[hi".len();
        open.type_edit(
            Edit::new(xml_atlas_engine::StepName::Other("Type".into())).replace(end..end, "]]>"),
        )
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

#[cfg(test)]
mod value_tests {
    use egui::{Event, PointerButton, Pos2, Rect, Shape, vec2};
    use xml_atlas_engine::{Edit, command};

    use super::*;

    /// One frame of the pane: its texts and where each is, and what was copied.
    fn frame(
        ctx: &egui::Context,
        open: &mut Open,
        events: Vec<Event>,
    ) -> (Vec<(String, Pos2)>, Vec<String>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(700.0, 400.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| show(ui, open));
        out.textures_delta.clear();
        let texts = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::Text(t) => Some((
                    t.galley.text().to_owned(),
                    t.visual_bounding_rect().center(),
                )),
                _ => None,
            })
            .collect();
        let copied = out
            .platform_output
            .commands
            .into_iter()
            .filter_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            })
            .collect();
        (texts, copied)
    }

    fn clicked_cell(value: &str) -> Open {
        let src = format!("\u{feff}<r>\r\n<o><Größe>{value}</Größe></o>\r\n</r>");
        let mut open = Open::new(
            Document::from_bytes(src.into_bytes()),
            egui::Id::unique("pane-value"),
        );
        let root = command::resolve(&open.doc, "/r").unwrap();
        open.select(root);
        let row = open.grid.as_ref().unwrap().rows[0];
        open.focus(row);
        open.grid_cell = Some(0);
        open
    }

    fn press_copy(ctx: &egui::Context, open: &mut Open) -> Vec<String> {
        for _ in 0..3 {
            frame(ctx, open, vec![]);
        }
        let (texts, _) = frame(ctx, open, vec![]);
        let pos = texts
            .iter()
            .find(|(t, _)| t == Lang::En.cell_copy_whole())
            .expect("a button")
            .1;
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(ctx, open, vec![Event::PointerMoved(pos)]);
        frame(ctx, open, vec![button(true)]);
        let mut copied = frame(ctx, open, vec![button(false)]).1;
        copied.extend(frame(ctx, open, vec![]).1);
        copied
    }

    /// GRID-16: a value over the limit is cut, says so, and Copy copies all of it, decoded,
    /// as it is after an edit.
    #[test]
    fn a_long_value_is_cut_and_copied_whole() {
        let body = "Größe ".repeat(DETAIL_CHARS / 5);
        let mut open = clicked_cell(&format!("{body}&amp;\r\nthe end"));
        let ctx = egui::Context::default();
        let (texts, copied) = frame(&ctx, &mut open, vec![]);
        assert_eq!(
            cut_notice(Lang::En),
            "Cut: the first 100,000 characters are shown."
        );
        assert!(
            texts.iter().any(|(t, _)| *t == cut_notice(Lang::En)),
            "no notice"
        );
        assert!(copied.is_empty(), "nothing is copied without a click");
        let longest = texts.iter().map(|(t, _)| t.chars().count()).max().unwrap();
        assert!(longest <= DETAIL_CHARS, "{longest} characters laid out");
        assert_eq!(press_copy(&ctx, &mut open), [format!("{body}&\nthe end")]);

        let root = open.selected.unwrap();
        let at = open
            .doc
            .src
            .windows(7)
            .position(|w| w == b"the end")
            .unwrap();
        open.type_edit(
            Edit::new(xml_atlas_engine::StepName::Other("Type".into()))
                .replace(at..at + 7, "a new end"),
        )
        .unwrap();
        open.select(root);
        let row = open.grid.as_ref().unwrap().rows[0];
        open.focus(row);
        open.grid_cell = Some(0);
        assert_eq!(press_copy(&ctx, &mut open), [format!("{body}&\na new end")]);
    }

    /// A value up to the limit is as before: whole, and without notice or button.
    #[test]
    fn a_value_up_to_the_limit_is_shown_whole() {
        let value = "ü".repeat(DETAIL_CHARS);
        let mut open = clicked_cell(&value);
        let (texts, _) = frame(&egui::Context::default(), &mut open, vec![]);
        assert!(texts.iter().any(|(t, _)| *t == value), "not whole");
        assert!(
            !texts
                .iter()
                .any(|(t, _)| *t == Lang::En.cell_copy_whole() || *t == cut_notice(Lang::En))
        );
    }
}
