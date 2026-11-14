//! Element tree: a flat list of visible rows, painted only for the rows on screen.

use egui::{Align2, FontId, Sense, Shape, Ui, pos2, vec2};
use xml_atlas_engine::Document;
use xml_atlas_engine::xml::{NodeId, NodeKind, ROOT};

use crate::app::Open;

const INDENT: f32 = 14.0;

pub struct TreeState {
    expanded: Vec<bool>,
    rows: Vec<NodeId>,
    dirty: bool,
    scroll_to: Option<usize>,
}

impl TreeState {
    pub fn new(doc: &Document) -> Self {
        let mut s = Self {
            expanded: vec![false; doc.index.len()],
            rows: Vec::new(),
            dirty: true,
            scroll_to: None,
        };
        if let Some(root) = doc.index.root_element() {
            s.expanded[root as usize] = true;
        }
        s
    }

    /// Expands every ancestor of `id` and scrolls to it.
    pub fn reveal(&mut self, doc: &Document, id: NodeId) {
        let mut p = doc.index.node(id).parent;
        while p != ROOT && p != xml_atlas_engine::xml::NONE {
            if !self.expanded[p as usize] {
                self.expanded[p as usize] = true;
                self.dirty = true;
            }
            p = doc.index.node(p).parent;
        }
        self.rebuild(doc);
        self.scroll_to = self.rows.iter().position(|&r| r == id);
    }

    fn rebuild(&mut self, doc: &Document) {
        if !self.dirty {
            return;
        }
        let idx = &doc.index;
        self.rows.clear();
        let mut stack: Vec<NodeId> = elements(doc, ROOT).collect();
        stack.reverse();
        while let Some(id) = stack.pop() {
            self.rows.push(id);
            if self.expanded[id as usize] {
                let start = stack.len();
                stack.extend(
                    idx.children(id)
                        .filter(|&c| idx.node(c).kind == NodeKind::Element),
                );
                stack[start..].reverse();
            }
        }
        self.dirty = false;
    }
}

fn elements(doc: &Document, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    doc.index
        .children(id)
        .filter(|&c| doc.index.node(c).kind == NodeKind::Element)
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
            let id = open.tree.rows[i];
            let idx = &open.doc.index;
            let src = &open.doc.src;
            let node = idx.node(id);
            let (rect, resp) = ui
                .allocate_exact_size(vec2(ui.available_width().max(300.0), row_h), Sense::click());
            let painter = ui.painter();
            if open.selected == Some(id) {
                painter.rect_filled(rect, 2.0, visuals.selection.bg_fill);
            } else if resp.hovered() {
                painter.rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
            }
            let x = rect.left() + (node.depth.saturating_sub(1)) as f32 * INDENT;
            let has_kids = elements(&open.doc, id).next().is_some();
            if has_kids {
                let c = pos2(x + INDENT * 0.5, rect.center().y);
                let s = 3.5;
                let pts = if open.tree.expanded[id as usize] {
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
                painter.add(Shape::convex_polygon(
                    pts,
                    visuals.weak_text_color(),
                    egui::Stroke::NONE,
                ));
            }
            let text_color = if open.selected == Some(id) {
                visuals.selection.stroke.color
            } else {
                visuals.text_color()
            };
            let name = String::from_utf8_lossy(idx.name(src, id));
            let name_rect = painter.text(
                pos2(x + INDENT + 2.0, rect.center().y),
                Align2::LEFT_CENTER,
                &name,
                font.clone(),
                text_color,
            );
            if let Some(preview) = preview(&open.doc, id) {
                painter.text(
                    pos2(name_rect.right() + 8.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    preview,
                    font.clone(),
                    visuals.weak_text_color(),
                );
            }
            if resp.clicked() {
                let on_arrow = resp
                    .interact_pointer_pos()
                    .is_some_and(|p| p.x < x + INDENT + 2.0);
                if has_kids && on_arrow {
                    toggle = Some(id);
                } else {
                    clicked = Some(id);
                }
            }
            if resp.double_clicked() && has_kids {
                toggle = Some(id);
            }
        }
    });
    if let Some(id) = toggle {
        let e = &mut open.tree.expanded[id as usize];
        *e = !*e;
        open.tree.dirty = true;
    }
    if let Some(id) = clicked {
        open.select(id);
        open.tree.scroll_to = None;
    }
}

/// Short hint after the name: the first attribute, or the text of a leaf element.
fn preview(doc: &Document, id: NodeId) -> Option<String> {
    let idx = &doc.index;
    let mut kids = idx.children(id);
    if let (Some(only), None) = (kids.next(), kids.next())
        && matches!(idx.node(only).kind, NodeKind::Text | NodeKind::CData)
    {
        return Some(truncate(&idx.text(&doc.src, only), 60));
    }
    let attrs = idx.attributes(&doc.src, id);
    attrs
        .first()
        .map(|(k, v)| format!("{k}=\"{}\"", truncate(v, 40)))
}

pub fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}
