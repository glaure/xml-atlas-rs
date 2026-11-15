//! The grid view: draws an engine [`Grid`] (`docs/spec/grid.md`). Cells are decoded only for
//! visible rows.

use egui::{Color32, Key, Rect, RichText, Stroke, StrokeKind, Ui, pos2, vec2};
use egui_extras::{Column as TableColumn, TableBuilder};
use xml_atlas_engine::command;
use xml_atlas_engine::grid::{Cell, CellTarget, Column, Grid};
use xml_atlas_engine::xml::NodeId;

use crate::app::{Note, Open, Pane, Shift};

/// Rows of the attribute table shown before it scrolls.
const ATTRIBUTE_ROWS: f32 = 6.0;

/// Spacing of the hatching that marks an absent value.
const HATCH_STEP: f32 = 6.0;

/// A cell being edited in place (GRID-10). Enter saves, Escape or a click elsewhere
/// cancels.
pub struct CellEditor {
    row: NodeId,
    column: usize,
    target: CellTarget,
    text: String,
    /// Keyboard focus has been asked for.
    focused: bool,
}

/// What a click or key in the grid asks for.
enum Action {
    /// Mark a row, and a cell of it, and highlight the node in the text; the grid stays
    /// (GRID-6). The node is the row, or the child element a nested cell stands for.
    Focus(NodeId, Option<usize>),
    /// Make a node the selection; the grid shows its children (GRID-7).
    Enter(NodeId),
    /// Start editing a cell (GRID-10).
    Edit { row: NodeId, column: usize },
    /// The editor closed: save its text, or not.
    Close { save: bool },
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
        run(open, action);
    }
}

fn run(open: &mut Open, action: Action) {
    match action {
        Action::Focus(id, column) => {
            open.focus(id);
            open.grid_cell = column;
        }
        Action::Enter(id) => open.select(id),
        Action::Edit { row, column } => start_edit(open, row, column),
        Action::Close { save } => {
            let Some(editor) = open.grid_edit.take() else {
                return;
            };
            if save {
                open.command(
                    "edit the cell",
                    |doc| command::set_cell(doc, &editor.target, &editor.text),
                    Shift::None,
                );
            }
        }
    }
}

/// Opens the editor on a cell, or says why the cell cannot be edited.
fn start_edit(open: &mut Open, row: NodeId, column: usize) {
    let Some(grid) = &open.grid else { return };
    let Some(c) = grid.columns.get(column) else {
        return;
    };
    match grid.edit_target(&open.doc, row, c) {
        Some((target, text)) => {
            open.grid_cell = Some(column);
            open.grid_edit = Some(CellEditor {
                row,
                column,
                target,
                text,
                focused: false,
            });
        }
        None => {
            open.note = Some(Note::refused(not_editable(&grid.cell(&open.doc, row, c))));
        }
    }
}

/// Why a cell cannot be edited.
fn not_editable(cell: &Cell) -> &'static str {
    match cell {
        Cell::Element(_) | Cell::Repeated { .. } => {
            "This cell stands for child elements: open it (Enter) to edit them"
        }
        Cell::Absent => "There is no such child element to edit",
        Cell::Value(_) => {
            "Only text can be edited here: the element also holds comments or elements"
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
fn table(ui: &mut Ui, open: &mut Open, scroll: Option<usize>) -> Option<Action> {
    let Open {
        grid,
        doc,
        focus,
        grid_cell,
        grid_edit,
        ..
    } = open;
    let grid = grid.as_ref()?;
    let doc = &*doc;
    let mut action = None;
    let mut editor_shown = false;
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
                let marked = focus.is_some_and(|f| f == id || doc.index.node(f).parent == id);
                row.set_selected(marked);
                row.col(|ui| {
                    ui.label(RichText::new((i + 1).to_string()).weak());
                });
                row.col(|ui| {
                    ui.label(String::from_utf8_lossy(doc.index.name(&doc.src, id)));
                });
                let mut cell_action = None;
                for (ci, c) in grid.columns.iter().enumerate() {
                    let editing = grid_edit.as_mut().filter(|e| e.row == id && e.column == ci);
                    if let Some(editor) = editing {
                        editor_shown = true;
                        row.col(|ui| {
                            if let Some(save) = cell_editor(ui, editor) {
                                cell_action = Some(Action::Close { save });
                            }
                        });
                        continue;
                    }
                    let cell = grid.cell(doc, id, c);
                    let nested = matches!(cell, Cell::Element(_) | Cell::Repeated { .. });
                    let (_, resp) = row.col(|ui| {
                        draw_cell(ui, &cell);
                        if marked && *grid_cell == Some(ci) {
                            outline(ui);
                        }
                    });
                    // A nested cell stands for its child element: a click highlights that
                    // child, a double-click opens it.
                    let child = nested.then(|| grid.cell_node(doc, id, c)).flatten();
                    let target = child.unwrap_or(id);
                    if resp.double_clicked() && child.is_some() {
                        cell_action = Some(Action::Enter(target));
                    } else if resp.clicked() || resp.secondary_clicked() {
                        cell_action = Some(Action::Focus(target, Some(ci)));
                    }
                    resp.context_menu(|ui| {
                        if let Some(child) = child {
                            if ui.button("Open    Enter").clicked() {
                                cell_action = Some(Action::Enter(child));
                            }
                        } else {
                            let editable = grid.edit_target(doc, id, c).is_some();
                            let button = ui
                                .add_enabled(editable, egui::Button::new("Edit    F2"))
                                .on_disabled_hover_text(not_editable(&cell));
                            if button.clicked() {
                                cell_action = Some(Action::Edit {
                                    row: id,
                                    column: ci,
                                });
                            }
                        }
                    });
                }
                let resp = row.response();
                // A double-click opens the row, also on a value cell (GRID-7).
                let open_row = resp.double_clicked()
                    && id != grid.parent
                    && !matches!(cell_action, Some(Action::Enter(_)));
                if open_row {
                    action = Some(Action::Enter(id));
                } else if cell_action.is_some() {
                    action = cell_action;
                } else if action.is_none() && resp.clicked() {
                    action = Some(Action::Focus(id, None));
                }
            });
        });
    // The edited row scrolled out of view: its editor is gone, so the edit is cancelled.
    if grid_edit.is_some() && !editor_shown && action.is_none() {
        action = Some(Action::Close { save: false });
    }
    action
}

/// The in-place editor. `Some(true)` when Enter saves it, `Some(false)` when Escape or a
/// click elsewhere closes it.
fn cell_editor(ui: &mut Ui, editor: &mut CellEditor) -> Option<bool> {
    let resp = ui.add(
        egui::TextEdit::singleline(&mut editor.text)
            .id_salt("grid-cell-editor")
            .desired_width(f32::INFINITY)
            .margin(vec2(2.0, 0.0)),
    );
    if !editor.focused {
        resp.request_focus();
        editor.focused = true;
    }
    resp.lost_focus()
        .then(|| ui.input(|i| i.key_pressed(Key::Enter)))
}

/// A frame around the marked cell.
fn outline(ui: &Ui) {
    let stroke = Stroke::new(1.5, ui.visuals().selection.stroke.color);
    ui.painter()
        .rect_stroke(ui.max_rect().shrink(1.0), 1.0, stroke, StrokeKind::Inside);
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
    /// Marks the cell to the left or right; left of the first column the whole row.
    Left,
    Right,
    /// Edits the marked cell (GRID-10); on a cell that stands for child elements, or with no
    /// cell marked, opens the row or child, as a double-click does (GRID-7).
    Enter,
    /// Edits the marked cell (GRID-10).
    Edit,
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
    let columns = grid.columns.len();
    match (key, cur, open.grid_cell) {
        (GridKey::Left, Some(_), cell) => {
            open.grid_cell = cell.and_then(|c| c.checked_sub(1));
            return;
        }
        (GridKey::Right, Some(_), cell) => {
            let next = cell.map_or(0, |c| c + 1);
            if next < columns {
                open.grid_cell = Some(next);
            }
            return;
        }
        (GridKey::Left | GridKey::Right, None, _) => return,
        (GridKey::Edit, Some(c), Some(column)) => {
            return start_edit(open, grid.rows[c], column);
        }
        (GridKey::Edit, ..) => {
            open.note = Some(Note::refused(
                "Mark a cell first: click it, or use the arrow keys",
            ));
            return;
        }
        (GridKey::Enter, Some(c), cell) => {
            let row = grid.rows[c];
            let column = cell.and_then(|i| grid.columns.get(i).map(|col| (i, col)));
            match column {
                Some((i, col)) => match grid.cell(&open.doc, row, col) {
                    Cell::Element(_) | Cell::Repeated { .. } => {
                        if let Some(child) = grid.cell_node(&open.doc, row, col) {
                            open.select(child);
                        }
                    }
                    _ => start_edit(open, row, i),
                },
                None if row != grid.parent => open.select(row),
                None => {}
            }
            return;
        }
        (GridKey::Enter, None, _) => return,
        _ => {}
    }
    let target = match (cur, key) {
        (None, _) => 0,
        (Some(c), GridKey::Up) => c.saturating_sub(1),
        (Some(c), GridKey::Down) => (c + 1).min(last),
        (Some(_), GridKey::Home) => 0,
        (Some(_), GridKey::End) => last,
        (Some(c), GridKey::PageUp) => c.saturating_sub(PAGE_ROWS),
        (Some(c), GridKey::PageDown) => (c + PAGE_ROWS).min(last),
        (Some(_), GridKey::Left | GridKey::Right | GridKey::Enter | GridKey::Edit) => {
            unreachable!("handled above")
        }
    };
    let row = grid.rows[target];
    // The marked column stays, as in a spreadsheet.
    open.focus(row);
    open.grid_scroll_to = Some(target);
}
