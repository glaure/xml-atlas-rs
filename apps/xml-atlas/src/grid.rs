//! The grid view: draws an engine [`Grid`] (`docs/spec/grid.md`). Only the rows and columns
//! in sight get cells (`crate::table`, D42). Cells are decoded only for visible rows, and once while a row stays on screen: what a frame needs of a cell comes
//! from `grid_cells::Cells`, never from a walk over the document.

use crate::lang::Lang;
use egui::{Key, Rect, RichText, Stroke, StrokeKind, Ui, pos2, vec2};
use xml_atlas_engine::command;
use xml_atlas_engine::grid::{Cell, CellTarget, Column, Grid, Sort, SortBy};
use xml_atlas_engine::xml::NodeId;

use crate::app::{Note, Open, Pane, Shift, What};
use crate::keys;
use crate::table;

/// The table's columns before the grid's own: the row number and the element's name.
const LEAD: usize = 2;
/// The first widths of the name and of the grid's own columns, and the least of both.
const NAME_WIDTH: f32 = 110.0;
const COLUMN_WIDTH: f32 = 140.0;
const COLUMN_MIN: f32 = 40.0;

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
    /// Sort the rows, or put them back in document order (GRID-14).
    Sort(Option<Sort>),
    /// Hide a column (GRID-15).
    Hide(Column),
    /// Hide the columns that are empty in every row (GRID-15).
    HideEmpty,
    /// Show a hidden column again, or all of them.
    Show(Option<Column>),
}

/// What a header stands for.
enum Header<'a> {
    Number,
    Name,
    Column(&'a Column),
}

pub fn show(ui: &mut Ui, open: &mut Open, lang: Lang) {
    let Some(grid) = &open.grid else { return };
    let mut after = vec![format!("{} rows", grid.rows.len())];
    match grid.view().hidden.len() {
        0 => {}
        1 => after.push("1 column hidden".to_string()),
        n => after.push(format!("{n} columns hidden")),
    }
    let path = RichText::new(&grid.path).strong();
    if open.doc.stale() == Some(grid.parent) {
        path_line(ui, path.color(ui.visuals().error_fg_color), &after)
            .on_hover_text("The text of this element is being edited and is not well-formed; the grid shows it as it last was.");
    } else {
        path_line(ui, path, &after);
    }
    crate::attribute_table::show(ui, grid);
    // Cell labels must not take clicks for text selection: a click picks the row or cell.
    let scroll = open.grid_scroll_to.take();
    let reveal = std::mem::take(&mut open.grid_reveal_cell);
    let parent = grid.parent;
    let action = ui.scope(|ui| {
        ui.style_mut().interaction.selectable_labels = false;
        // Columns that do not fit scroll sideways; the header scrolls with them.
        egui::ScrollArea::horizontal()
            .id_salt(("grid-across", parent))
            .auto_shrink(false)
            .show(ui, |ui| table(ui, lang, open, scroll, reveal))
            .inner
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
        Action::Sort(sort) => {
            let Some(grid) = &mut open.grid else { return };
            grid.sort(&open.doc, sort);
            // The marked row stays in view.
            let focus = open.focus;
            open.grid_scroll_to = grid
                .rows
                .iter()
                .position(|&r| focus.is_some_and(|f| f == r || open.doc.index.node(f).parent == r));
        }
        Action::Hide(column) => {
            if let Some(grid) = &mut open.grid {
                grid.hide(&column);
            }
            unmark_hidden(open);
        }
        Action::HideEmpty => {
            let Some(grid) = &mut open.grid else { return };
            let n = grid.hide_empty(&open.doc);
            open.note = Some(Note::info(match n {
                0 => open.lang.status_no_empty_column().to_string(),
                n => open.lang.status_hid_columns(n as u64),
            }));
            unmark_hidden(open);
        }
        Action::Show(column) => {
            if let Some(grid) = &mut open.grid {
                match column {
                    Some(c) => grid.show(&c),
                    None => grid.show_all(),
                }
            }
        }
        Action::Close { save } => {
            let Some(editor) = open.grid_edit.take() else {
                return;
            };
            if save {
                open.command(
                    What::EditCell,
                    |doc| command::set_cell(doc, &editor.target, &editor.text),
                    Shift::None,
                );
            }
        }
    }
}

/// A marked cell whose column was hidden is no longer marked; its editor closes.
fn unmark_hidden(open: &mut Open) {
    let Some(grid) = &open.grid else { return };
    let hidden = |i: usize| grid.columns.get(i).is_none_or(|c| grid.is_hidden(c));
    if open.grid_cell.is_some_and(hidden) {
        open.grid_cell = None;
    }
    if open.grid_edit.as_ref().is_some_and(|e| hidden(e.column)) {
        open.grid_edit = None;
    }
}

/// Opens the editor on a cell, or says why the cell cannot be edited.
fn start_edit(open: &mut Open, row: NodeId, column: usize) {
    let Some(grid) = &open.grid else { return };
    let Some(c) = grid.columns.get(column) else {
        return;
    };
    if grid.too_long_to_edit(&open.doc, row, c) {
        open.note = Some(Note::refused(open.lang.status_too_long()));
        return;
    }
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
            let why = not_editable(open.lang, &grid.cell(&open.doc, row, c));
            open.note = Some(Note::refused(why));
        }
    }
}

/// Why a cell cannot be edited.
fn not_editable(lang: Lang, cell: &Cell) -> &'static str {
    match cell {
        Cell::Element(_) | Cell::Repeated { .. } => lang.grid_why_elements(),
        Cell::Absent => lang.grid_why_absent(),
        Cell::Value(_) => lang.grid_why_mixed(),
    }
}

/// One line: the path of a grid's element, then `after`, weak. The path is cut to the width
/// the rest of the line leaves, and shown whole on hover: the path of a deep element (AUTOSAR
/// files nest a dozen levels) must not widen its pane, because an egui side panel is never
/// narrower than its content, and the border to the grid could no longer be dragged
/// (GRID-16). Returns the path's response.
pub(crate) fn path_line(ui: &mut Ui, path: RichText, after: &[String]) -> egui::Response {
    ui.horizontal(|ui| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let gap = ui.spacing().item_spacing.x;
        let rest: f32 = after
            .iter()
            .map(|t| {
                let galley = ui.fonts_mut(|f| {
                    f.layout_no_wrap(t.clone(), font.clone(), egui::Color32::PLACEHOLDER)
                });
                galley.size().x + gap
            })
            .sum();
        let room = (ui.available_width() - rest).max(0.0);
        let response = ui
            .scope(|ui| {
                ui.set_max_width(room);
                ui.add(egui::Label::new(path).truncate())
            })
            .inner;
        for t in after {
            ui.label(RichText::new(t).weak());
        }
        response
    })
    .inner
}

/// The child table. Returns what a click asked for.
/// `reveal`: scroll the marked cell into view sideways.
fn table(
    ui: &mut Ui,
    lang: Lang,
    open: &mut Open,
    scroll: Option<usize>,
    reveal: bool,
) -> Option<Action> {
    let Open {
        grid,
        grid_cells,
        doc,
        focus,
        grid_cell,
        grid_edit,
        ..
    } = open;
    let grid = grid.as_ref()?;
    let doc = &*doc;
    let mut action = None;
    // Columns shown, by index into `grid.columns` (GRID-15).
    let shown: Vec<usize> = (0..grid.columns.len())
        .filter(|&i| !grid.is_hidden(&grid.columns[i]))
        .collect();
    let mut editor_shown = false;
    // Where the marked cell was drawn, to scroll it into view sideways.
    let mut reveal_rect = None;
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let digits = grid.rows.len().to_string().len() as f32;
    let number_width = digits
        * ui.fonts_mut(|f| f.glyph_width(&egui::TextStyle::Body.resolve(ui.style()), '0'))
        + 16.0;
    grid_cells.begin(doc, grid);
    let mut columns = vec![
        // Wide enough for the largest row number, so it never wraps.
        table::Column::new(number_width, number_width),
        table::Column::new(NAME_WIDTH, COLUMN_MIN),
    ];
    columns.resize(
        LEAD + shown.len(),
        table::Column::new(COLUMN_WIDTH, COLUMN_MIN),
    );
    // Drawn also out of sight: the edited cell's editor lives on, and the marked cell has to
    // be drawn to be scrolled to.
    let place = |ci: usize| shown.iter().position(|&i| i == ci).map(|at| LEAD + at);
    let edited = grid_edit.as_ref().and_then(|e| place(e.column));
    let revealed = grid_cell.filter(|_| reveal).and_then(place);
    let body = table::Table::new(ui, ("grid", grid.parent), columns)
        .scroll_to_row(scroll)
        .keep_column(edited)
        .keep_column(revealed)
        .header(row_h + 2.0, |row| {
            while let Some(column) = row.next_column() {
                let header = match column {
                    0 => Header::Number,
                    1 => Header::Name,
                    _ => Header::Column(&grid.columns[shown[column - LEAD]]),
                };
                let resp = row.col(|ui| {
                    ui.strong(header_text(grid, &header));
                });
                if let Some(a) = header_action(lang, grid, &header, &resp) {
                    action = Some(a);
                }
            }
        });
    body.rows(row_h, grid.rows.len(), |row| {
                let i = row.index();
                let id = grid.rows[i];
                // Marked when the row, or a child shown in one of its cells, is the focus.
                let marked = focus.is_some_and(|f| f == id || doc.index.node(f).parent == id);
                row.set_selected(marked);
                let mut cell_action = None;
                let cells = grid_cells.row(doc, id);
                while let Some(column) = row.next_column() {
                    if column == 0 {
                        row.col(|ui| {
                            // A bookmark (TREE-15): a bar at the row's left edge.
                            if crate::bookmarks::is_marked(doc, id) {
                                let r = ui.max_rect();
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(
                                        r.left_top(),
                                        egui::vec2(3.0, r.height()),
                                    ),
                                    1.0,
                                    crate::bookmarks::color(ui.visuals()),
                                );
                            }
                            ui.label(RichText::new(grid.number(i).to_string()).weak());
                            if reveal && marked && grid_cell.is_none() {
                                reveal_rect = Some(ui.max_rect());
                            }
                        });
                        continue;
                    }
                    if column == 1 {
                        row.col(|ui| {
                            let name = String::from_utf8_lossy(doc.index.name(&doc.src, id));
                            // Typing broke this element's text: its last good state (TEXT-10).
                            if doc.stale() == Some(id) {
                                ui.label(RichText::new(name).color(ui.visuals().error_fg_color))
                                    .on_hover_text("The text of this element is being edited and is not well-formed; this row shows it as it last was.");
                            } else {
                                ui.label(name);
                            }
                        });
                        continue;
                    }
                    let ci = shown[column - LEAD];
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
                    let kept = &cells[ci];
                    let cell = &kept.cell;
                    let nested = matches!(cell, Cell::Element(_) | Cell::Repeated { .. });
                    let resp = row.col(|ui| {
                        draw_cell(ui, cell);
                        if marked && *grid_cell == Some(ci) {
                            outline(ui);
                            if reveal {
                                reveal_rect = Some(ui.max_rect());
                            }
                        }
                    });
                    // A nested cell stands for its child element: a click highlights that
                    // child, a double-click opens it.
                    let child = kept.node.filter(|_| nested);
                    let target = child.unwrap_or(id);
                    if resp.double_clicked() && child.is_some() {
                        cell_action = Some(Action::Enter(target));
                    } else if resp.clicked() || resp.secondary_clicked() {
                        cell_action = Some(Action::Focus(target, Some(ci)));
                    }
                    resp.context_menu(|ui| {
                        if let Some(child) = child {
                            let button = egui::Button::new(lang.grid_open())
                                .shortcut_text(keys::text(ui.ctx(), lang, &keys::ENTER));
                            if ui.add(button).clicked() {
                                cell_action = Some(Action::Enter(child));
                            }
                        } else {
                            let button = ui
                                .add_enabled(
                                    kept.editable,
                                    egui::Button::new(lang.grid_edit())
                                        .shortcut_text(keys::text(ui.ctx(), lang, &keys::EDIT)),
                                )
                                .on_disabled_hover_text(not_editable(lang, cell));
                            if button.clicked() {
                                cell_action = Some(Action::Edit {
                                    row: id,
                                    column: ci,
                                });
                            }
                        }
                    });
                }
                let Some(resp) = row.response() else { return };
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
    grid_cells.end();
    // Asked for here, outside the table: the table's own scroll area, which scrolls only up
    // and down, would swallow it.
    if let Some(rect) = reveal_rect {
        ui.scroll_to_rect(rect, None);
    }
    // The edited row scrolled out of view: its editor is gone, so the edit is cancelled.
    if grid_edit.is_some() && !editor_shown && action.is_none() {
        action = Some(Action::Close { save: false });
    }
    action
}

/// A header's text, with an arrow when the rows are sorted by it (GRID-14).
fn header_text(grid: &Grid, header: &Header) -> String {
    let (text, by) = match header {
        Header::Number => ("#".to_string(), None),
        Header::Name => ("element".to_string(), Some(SortBy::Name)),
        Header::Column(c) => (column_name(c), Some(SortBy::Column((*c).clone()))),
    };
    match &grid.view().sort {
        Some(sort) if Some(&sort.by) == by.as_ref() => {
            format!("{text} {}", if sort.descending { "⏷" } else { "⏶" })
        }
        _ => text,
    }
}

pub(crate) fn column_name(column: &Column) -> String {
    match column {
        Column::Attribute(n) => format!("@{n}"),
        Column::Child(n) => n.clone(),
        Column::Text => "text()".to_string(),
    }
}

/// A click on a header sorts by it, again reverses; on `#` it restores document order
/// (GRID-14). Its menu sorts, hides and shows columns (GRID-15).
fn header_action(
    lang: Lang,
    grid: &Grid,
    header: &Header,
    resp: &egui::Response,
) -> Option<Action> {
    let by = match header {
        Header::Number => None,
        Header::Name => Some(SortBy::Name),
        Header::Column(c) => Some(SortBy::Column((*c).clone())),
    };
    let current = grid.view().sort.as_ref();
    let mut action = None;
    if resp.clicked() {
        action = Some(Action::Sort(by.clone().map(|by| {
            let descending = current.is_some_and(|s| s.by == by && !s.descending);
            Sort { by, descending }
        })));
    }
    resp.context_menu(|ui| {
        if let Some(by) = &by {
            for (label, descending) in [
                (lang.grid_sort_ascending(), false),
                (lang.grid_sort_descending(), true),
            ] {
                if ui.button(label).clicked() {
                    action = Some(Action::Sort(Some(Sort {
                        by: by.clone(),
                        descending,
                    })));
                }
            }
        }
        let sorted = current.is_some();
        if ui
            .add_enabled(sorted, egui::Button::new(lang.grid_document_order()))
            .clicked()
        {
            action = Some(Action::Sort(None));
        }
        ui.separator();
        if let Header::Column(c) = header
            && ui.button(lang.grid_hide_column()).clicked()
        {
            action = Some(Action::Hide((*c).clone()));
        }
        if ui.button(lang.grid_hide_empty()).clicked() {
            action = Some(Action::HideEmpty);
        }
        let hidden = &grid.view().hidden;
        ui.add_enabled_ui(!hidden.is_empty(), |ui| {
            ui.menu_button(lang.grid_show_column(), |ui| {
                for c in hidden {
                    if ui.button(column_name(c)).clicked() {
                        action = Some(Action::Show(Some(c.clone())));
                    }
                }
                ui.separator();
                if ui.button(lang.grid_show_all()).clicked() {
                    action = Some(Action::Show(None));
                }
            });
        });
    });
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

pub(crate) fn draw_cell(ui: &mut Ui, cell: &Cell) {
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

/// Diagonal lines over `rect`: the row has no such attribute, child or text (GRID-13). Only
/// those in sight are drawn: labels leave themselves out when out of sight, shapes do not,
/// and a grid can have a hundred columns beside the few on screen.
fn hatch(ui: &Ui, rect: Rect) {
    let painter = ui.painter_at(rect);
    let seen = painter.clip_rect();
    if !seen.is_positive() {
        return;
    }
    let stroke = Stroke::new(1.0, crate::theme::current(ui).hatch);
    let h = rect.height();
    let start = rect.left() - h;
    // The first line that reaches into sight, in step with those left out.
    let mut step = ((seen.left() - h - start) / HATCH_STEP).floor().max(0.0);
    loop {
        let x = start + step * HATCH_STEP;
        if x >= rect.right() || x > seen.right() + stroke.width {
            break;
        }
        painter.line_segment([pos2(x, rect.bottom()), pos2(x + h, rect.top())], stroke);
        step += 1.0;
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
    let shown = |i: usize| !grid.is_hidden(&grid.columns[i]);
    match (key, cur, open.grid_cell) {
        // Hidden columns are skipped (GRID-15).
        (GridKey::Left, Some(_), cell) => {
            open.grid_cell = cell.and_then(|c| (0..c).rev().find(|&i| shown(i)));
            open.grid_reveal_cell = true;
            return;
        }
        (GridKey::Right, Some(_), cell) => {
            let from = cell.map_or(0, |c| c + 1);
            if let Some(next) = (from..columns).find(|&i| shown(i)) {
                open.grid_cell = Some(next);
                open.grid_reveal_cell = true;
            }
            return;
        }
        (GridKey::Left | GridKey::Right, None, _) => return,
        (GridKey::Edit, Some(c), Some(column)) => {
            return start_edit(open, grid.rows[c], column);
        }
        (GridKey::Edit, ..) => {
            open.note = Some(Note::refused(open.lang.status_mark_cell()));
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

#[cfg(test)]
mod tests {
    use xml_atlas_engine::grid::EDIT_CHARS;
    use xml_atlas_engine::{Document, command};

    use super::*;

    /// GRID-10: F2, Enter and the menu's Edit do not open the editor on a value over the
    /// limit; they say why and change nothing. At the limit they open it with all of it.
    #[test]
    fn a_long_value_is_not_edited_in_place() {
        let edit = |open: &mut Open, row, way| match way {
            0 => key(open, GridKey::Edit),
            1 => key(open, GridKey::Enter),
            // What the menu's Edit does.
            _ => run(open, Action::Edit { row, column: 0 }),
        };
        for (chars, opens) in [(EDIT_CHARS, true), (EDIT_CHARS + 1, false)] {
            let value = "ü".repeat(chars);
            for src in [
                format!("\u{feff}<r>\r\n<o><Größe>{value}</Größe></o>\r\n</r>"),
                format!("\u{feff}<r>\r\n<o Größe=\"{value}\"/>\r\n</r>"),
            ] {
                for way in 0..3 {
                    let doc = Document::from_bytes(src.clone().into_bytes());
                    let mut open = Open::new(doc, egui::Id::unique("grid-long"));
                    let root = command::resolve(&open.doc, "/r").unwrap();
                    open.select(root);
                    let row = open.grid.as_ref().unwrap().rows[0];
                    open.focus(row);
                    open.grid_cell = Some(0);
                    edit(&mut open, row, way);
                    let at = format!("{chars} characters, way {way}");
                    match (&open.grid_edit, &open.note) {
                        (Some(editor), None) if opens => assert_eq!(editor.text, value, "{at}"),
                        (None, Some(note)) if !opens => {
                            assert!(note.refused, "{at}");
                            assert_eq!(note.text, Lang::En.status_too_long(), "{at}");
                        }
                        _ => panic!("editor or note wrong at {at}"),
                    }
                    assert_eq!(open.focus, Some(row), "{at}");
                    assert_eq!(open.grid_cell, Some(0), "{at}");
                    assert!(*open.doc.src == src.as_bytes(), "changed at {at}");
                }
            }
        }
    }

    /// AGENTS.md, rule 2: frames in which nothing changes work out no cell, in the grid or in
    /// the cell pane beside it.
    #[test]
    fn idle_frames_work_out_no_cells() {
        let many = "<v/>".repeat(1000);
        let src = format!("<r><o><Größe>1</Größe>{many}</o><o>{many}</o></r>");
        let doc = Document::from_bytes(src.into_bytes());
        let mut open = Open::new(doc, egui::Id::unique("grid-idle"));
        let root = command::resolve(&open.doc, "/r").unwrap();
        open.select(root);
        let first = open.grid.as_ref().unwrap().rows[0];
        open.focus(first);
        let ctx = egui::Context::default();
        let draw = |open: &mut Open| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 600.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::Panel::right("pane")
                    .default_size(400.0)
                    .show(ui, |ui| crate::cell_pane::show(ui, open));
                egui::CentralPanel::default().show(ui, |ui| show(ui, open, crate::lang::Lang::En));
            });
            out.textures_delta.clear();
        };
        let worked_out = |open: &Open| {
            let pane = open.cell_pane.as_ref().unwrap();
            (open.grid_cells.worked_out(), pane.worked_out())
        };
        // A table learns its size in its first frames.
        for _ in 0..5 {
            draw(&mut open);
        }
        // So that a cache made anew each frame, with the same count each time, differs.
        open.cell_pane.as_mut().unwrap().count_one_more(&open.doc);
        let before = worked_out(&open);
        assert!(before.0 >= 2 && before.1 >= 1, "drawn: {before:?}");
        for _ in 0..10 {
            draw(&mut open);
        }
        assert_eq!(worked_out(&open), before);
    }

    /// AGENTS.md, rule 2: hatching costs what is on screen, not a line for every step of
    /// every absent cell in the columns scrolled out of view (GRID-13).
    #[test]
    fn only_hatching_on_screen_is_drawn() {
        // A row for each column: all but the diagonal is absent.
        let rows: String = (0..60).map(|i| format!("<o a{i}=\"1\"/>")).collect();
        let doc = Document::from_bytes(format!("<r>{rows}</r>").into_bytes());
        let mut open = Open::new(doc, egui::Id::unique("grid-hatch"));
        let root = open.doc.index.root_element().unwrap();
        open.select(root);
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 400.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| show(ui, &mut open, crate::lang::Lang::En));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        let (mut seen, mut unseen) = (0, 0);
        for s in &shapes {
            // Hatching is diagonal; the table's column borders are not.
            if let egui::Shape::LineSegment { points, .. } = &s.shape
                && points[0].x != points[1].x
                && points[0].y != points[1].y
            {
                let clip = s.clip_rect;
                // A step more on either side is the price of not measuring each line.
                let near = clip.expand(HATCH_STEP + 2.0);
                if clip.is_positive() && near.intersects(s.shape.visual_bounding_rect()) {
                    seen += 1;
                } else {
                    unseen += 1;
                }
            }
        }
        assert!(seen > 50, "the hatching on screen is still drawn: {seen}");
        assert_eq!(
            unseen, 0,
            "lines drawn out of sight, beside {seen} in sight"
        );
    }

    /// GRID-20: the marked cell comes into view from columns that are not drawn, and goes
    /// back to the row's start.
    #[test]
    fn a_marked_cell_out_of_sight_is_scrolled_to() {
        let attributes: String = (0..120).map(|i| format!(" a{i}=\"v{i}\"")).collect();
        let doc = Document::from_bytes(format!("<r><o{attributes}/></r>").into_bytes());
        let mut open = Open::new(doc, egui::Id::unique("grid-reveal"));
        let root = open.doc.index.root_element().unwrap();
        open.select(root);
        let row = open.grid.as_ref().unwrap().rows[0];
        open.focus(row);
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 400.0));
        let mut time = 0.0;
        for (cell, text) in [(Some(117), "v117"), (Some(3), "v3"), (None, "#")] {
            open.grid_cell = cell;
            open.grid_reveal_cell = true;
            let mut in_sight = false;
            // Scrolling there is animated.
            for _ in 0..60 {
                time += 0.1;
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| show(ui, &mut open, crate::lang::Lang::En));
                out.textures_delta.clear();
                in_sight = out.shapes.iter().any(|s| match &s.shape {
                    egui::Shape::Text(t) => {
                        let rect = t.galley.rect.translate(t.pos.to_vec2());
                        t.galley.text() == text
                            && s.clip_rect.intersect(screen).contains(rect.center())
                    }
                    _ => false,
                });
            }
            assert!(in_sight, "{cell:?}");
        }
    }
}
