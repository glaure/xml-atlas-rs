//! The tree view: draws the rows from [`xml_atlas_engine::tree`], only those on screen.

use egui::{Align2, Color32, FontId, Sense, Shape, Ui, pos2, vec2};
use xml_atlas_engine::Document;
use xml_atlas_engine::tree::{self, Label, Row, TreeOptions};
use xml_atlas_engine::xml::{NONE, NodeId, ROOT};

use crate::app::Open;

const INDENT: f32 = 14.0;

pub struct TreeState {
    pub options: TreeOptions,
    expanded: Vec<bool>,
    rows: Vec<Row>,
    /// The row marked as selected. Usually the selected element, but an attribute or text
    /// row when that was clicked.
    selected: Option<Row>,
    dirty: bool,
    scroll_to: Option<usize>,
}

impl TreeState {
    pub fn new(doc: &Document) -> Self {
        let mut s = Self {
            options: TreeOptions::default(),
            expanded: vec![false; doc.index.len()],
            rows: Vec::new(),
            selected: None,
            dirty: true,
            scroll_to: None,
        };
        if let Some(root) = doc.index.root_element() {
            s.expanded[root as usize] = true;
        }
        s
    }

    /// Expands every ancestor of `id`, marks it selected and scrolls to it.
    pub fn reveal(&mut self, doc: &Document, id: NodeId) {
        let mut p = doc.index.node(id).parent;
        while p != ROOT && p != NONE {
            if !self.expanded[p as usize] {
                self.expanded[p as usize] = true;
                self.dirty = true;
            }
            p = doc.index.node(p).parent;
        }
        self.selected = Some(Row::Node(id));
        self.rebuild(doc);
        self.scroll_to = self.rows.iter().position(|&r| r == Row::Node(id));
    }

    /// Shows or hides attribute rows.
    pub fn set_attributes(&mut self, show: bool) {
        if self.options.attributes != show {
            self.options.attributes = show;
            // A hidden attribute row hands its mark to its element.
            if let (false, Some(Row::Attribute { element, .. })) = (show, self.selected) {
                self.selected = Some(Row::Node(element));
            }
            self.dirty = true;
        }
    }

    fn rebuild(&mut self, doc: &Document) {
        if self.dirty {
            tree::visible_rows(doc, &self.expanded, self.options, &mut self.rows);
            self.dirty = false;
        }
    }
}

pub fn show(ui: &mut Ui, open: &mut Open) {
    open.tree.rebuild(&open.doc);
    ui.spacing_mut().item_spacing.y = 0.0;
    let font = FontId::proportional(13.0);
    let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 4.0;
    let mut area = egui::ScrollArea::both().id_salt("tree").auto_shrink(false);
    if let Some(i) = open.tree.scroll_to.take() {
        // Keep a few rows of context above the target.
        area = area.vertical_scroll_offset(((i as f32) - 5.0).max(0.0) * row_h);
    }
    let visuals = ui.visuals().clone();
    let mut toggle = None;
    let mut clicked = None;
    area.show_rows(ui, row_h, open.tree.rows.len(), |ui, range| {
        for i in range {
            let row = open.tree.rows[i];
            let doc = &open.doc;
            let (rect, resp) = ui
                .allocate_exact_size(vec2(ui.available_width().max(300.0), row_h), Sense::click());
            let painter = ui.painter();
            let selected = open.tree.selected == Some(row);
            if selected {
                painter.rect_filled(rect, 2.0, visuals.selection.bg_fill);
            } else if resp.hovered() {
                painter.rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
            }
            let x = rect.left() + row.depth(doc) as f32 * INDENT;
            let expandable = tree::has_children(doc, row, open.tree.options);
            if let (true, Row::Node(id)) = (expandable, row) {
                arrow(
                    painter,
                    pos2(x + INDENT * 0.5, rect.center().y),
                    open.tree.expanded[id as usize],
                    visuals.weak_text_color(),
                );
            }
            let strong = if selected {
                visuals.selection.stroke.color
            } else {
                visuals.text_color()
            };
            let weak = visuals.weak_text_color();
            let left = pos2(x + INDENT + 2.0, rect.center().y);
            let text = |pos, s: &str, color| {
                painter.text(pos, Align2::LEFT_CENTER, s, font.clone(), color)
            };
            match tree::label(doc, row) {
                Label::Element { name, hint } => {
                    let r = text(left, &name, strong);
                    if let Some(hint) = hint {
                        text(pos2(r.right() + 8.0, left.y), &hint, weak);
                    }
                }
                Label::Attribute { name, value } => {
                    let r = text(left, &name, strong);
                    text(
                        pos2(r.right() + 4.0, left.y),
                        &format!("= \"{value}\""),
                        weak,
                    );
                }
                Label::Text(s) => {
                    text(left, &s, weak);
                }
            }
            if resp.clicked() {
                let on_arrow = resp
                    .interact_pointer_pos()
                    .is_some_and(|p| p.x < x + INDENT + 2.0);
                match row {
                    Row::Node(id) if expandable && on_arrow => toggle = Some(id),
                    _ => clicked = Some(row),
                }
            }
            if let (true, true, Row::Node(id)) = (resp.double_clicked(), expandable, row) {
                toggle = Some(id);
            }
        }
    });
    if let Some(id) = toggle {
        let e = &mut open.tree.expanded[id as usize];
        *e = !*e;
        open.tree.dirty = true;
    }
    if let Some(row) = clicked {
        select_row(open, row);
    }
}

/// An element row selects the element. An attribute or text row selects its element for the
/// grid, highlights a text node in the text view, and stays marked in the tree.
fn select_row(open: &mut Open, row: Row) {
    open.select(row.element(&open.doc));
    if let Row::Node(id) = row {
        open.focus(id);
    }
    open.tree.selected = Some(row);
    open.tree.scroll_to = None;
}

fn arrow(painter: &egui::Painter, c: egui::Pos2, expanded: bool, color: Color32) {
    let s = 3.5;
    let pts = if expanded {
        vec![
            pos2(c.x - s, c.y - s * 0.6),
            pos2(c.x + s, c.y - s * 0.6),
            pos2(c.x, c.y + s * 0.8),
        ]
    } else {
        vec![
            pos2(c.x - s * 0.6, c.y - s),
            pos2(c.x + s * 0.8, c.y),
            pos2(c.x - s * 0.6, c.y + s),
        ]
    };
    painter.add(Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}
