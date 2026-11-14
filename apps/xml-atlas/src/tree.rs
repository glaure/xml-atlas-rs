//! The tree view: draws the rows from [`xml_atlas_engine::tree`], only those on screen.

use std::ops::Range;

use egui::{Align2, Color32, FontId, Sense, Shape, Ui, pos2, vec2};
use xml_atlas_engine::Document;
use xml_atlas_engine::tree::{self, Label, Row, TreeOptions};
use xml_atlas_engine::xml::{NONE, NodeId, NodeKind, ROOT};

use crate::app::Open;
use crate::highlight::Palette;
use crate::icons;

const INDENT: f32 = 14.0;

/// Rows kept above a row the tree jumps to, for context.
const CONTEXT_ROWS: usize = 5;

/// Keys the tree answers to (TREE-8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKey {
    Up,
    Down,
    /// Expands; on an expanded element, moves to its first row.
    Right,
    /// Collapses; on a collapsed or leaf row, moves to the parent element.
    Left,
    Home,
    End,
    PageUp,
    PageDown,
}

pub struct TreeState {
    pub options: TreeOptions,
    expanded: Vec<bool>,
    rows: Vec<Row>,
    /// The row marked as selected. Usually the selected element, but an attribute or text
    /// row when that was clicked.
    selected: Option<Row>,
    dirty: bool,
    /// Row to show at the top on the next frame.
    scroll_to: Option<usize>,
    /// Rows on screen in the last frame.
    visible: Range<usize>,
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
            visible: 0..0,
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
        self.scroll_to = self
            .rows
            .iter()
            .position(|&r| r == Row::Node(id))
            .map(|i| i.saturating_sub(CONTEXT_ROWS));
    }

    /// Scrolls just enough to show `row`, if it is off screen.
    pub fn keep_visible(&mut self, row: Row) {
        let Some(i) = self.rows.iter().position(|&r| r == row) else {
            return;
        };
        let height = self.visible.len().max(1);
        if i < self.visible.start {
            self.scroll_to = Some(i);
        } else if i + 1 >= self.visible.end {
            // The last row in `visible` may be cut off, so treat it as off screen.
            self.scroll_to = Some((i + 2).saturating_sub(height));
        }
    }

    /// Applies `key`: expands or collapses, or returns the row to select.
    pub fn key(&mut self, doc: &Document, key: TreeKey) -> Option<Row> {
        self.rebuild(doc);
        let last = self.rows.len().checked_sub(1)?;
        let Some(cur) = self
            .selected
            .and_then(|s| self.rows.iter().position(|&r| r == s))
        else {
            return self.rows.first().copied();
        };
        let page = self.visible.len().saturating_sub(1).max(1);
        let row = self.rows[cur];
        let target = match key {
            TreeKey::Up => cur.saturating_sub(1),
            TreeKey::Down => (cur + 1).min(last),
            TreeKey::Home => 0,
            TreeKey::End => last,
            TreeKey::PageUp => cur.saturating_sub(page),
            TreeKey::PageDown => (cur + page).min(last),
            TreeKey::Right => match row {
                Row::Node(id) if tree::has_children(doc, row, self.options) => {
                    if !self.expanded[id as usize] {
                        self.expanded[id as usize] = true;
                        self.dirty = true;
                        return None;
                    }
                    (cur + 1).min(last)
                }
                _ => return None,
            },
            TreeKey::Left => {
                if let Row::Node(id) = row
                    && self.expanded[id as usize]
                    && tree::has_children(doc, row, self.options)
                {
                    self.expanded[id as usize] = false;
                    self.dirty = true;
                    return None;
                }
                let parent = match row {
                    Row::Node(id) if doc.index.node(id).kind == NodeKind::Element => {
                        doc.index.node(id).parent
                    }
                    _ => row.element(doc),
                };
                if parent == ROOT || parent == NONE {
                    return None;
                }
                self.rows.iter().position(|&r| r == Row::Node(parent))?
            }
        };
        (target != cur).then(|| self.rows[target])
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
    if let Some(top) = open.tree.scroll_to.take() {
        area = area.vertical_scroll_offset(top as f32 * row_h);
    }
    let visuals = ui.visuals().clone();
    let palette = Palette::new(visuals.dark_mode);
    let mut toggle = None;
    let mut clicked = None;
    area.show_rows(ui, row_h, open.tree.rows.len(), |ui, range| {
        open.tree.visible = range.clone();
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
            icons::draw(
                painter,
                pos2(x + INDENT + 2.0 + icons::WIDTH / 2.0 - 2.0, rect.center().y),
                tree::kind(doc, row),
                &palette,
            );
            let left = pos2(x + INDENT + 2.0 + icons::WIDTH, rect.center().y);
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
    if toggle.is_some() || clicked.is_some() {
        open.keys = crate::app::Pane::Tree;
    }
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
pub fn select_row(open: &mut Open, row: Row) {
    open.select(row.element(&open.doc));
    match row {
        Row::Node(id) => open.focus(id),
        Row::Attribute { element, index } => open.mark_attribute(element, index),
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

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<r a="1"><x><y/></x><z>t</z></r>"#;

    fn setup() -> (Document, TreeState) {
        let doc = Document::from_bytes(DOC.as_bytes().to_vec());
        let mut t = TreeState::new(&doc);
        t.rebuild(&doc);
        (doc, t)
    }

    fn name(doc: &Document, row: Row) -> String {
        match tree::label(doc, row) {
            tree::Label::Element { name, .. } => name,
            tree::Label::Attribute { name, .. } => format!("@{name}"),
            tree::Label::Text(t) => format!("'{t}'"),
        }
    }

    /// Presses `key` and follows the selection, as the app does.
    fn press(doc: &Document, t: &mut TreeState, key: TreeKey) -> String {
        if let Some(row) = t.key(doc, key) {
            t.selected = Some(row);
        }
        t.rebuild(doc);
        t.selected.map(|r| name(doc, r)).unwrap_or_default()
    }

    #[test]
    fn arrows_walk_the_visible_rows() {
        let (doc, mut t) = setup();
        // Nothing selected yet: any key selects the first row.
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "r");
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "@a");
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "x");
        assert_eq!(press(&doc, &mut t, TreeKey::End), "z");
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "z");
        assert_eq!(press(&doc, &mut t, TreeKey::Home), "r");
        assert_eq!(press(&doc, &mut t, TreeKey::Up), "r");
    }

    #[test]
    fn right_expands_then_steps_in_and_left_collapses_then_steps_out() {
        let (doc, mut t) = setup();
        press(&doc, &mut t, TreeKey::Down);
        press(&doc, &mut t, TreeKey::Down);
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "x");
        // Right on collapsed `x` expands it and stays; again steps to `y`.
        assert_eq!(press(&doc, &mut t, TreeKey::Right), "x");
        assert_eq!(press(&doc, &mut t, TreeKey::Right), "y");
        // `y` has nothing under it: Right does nothing, Left goes to the parent.
        assert_eq!(press(&doc, &mut t, TreeKey::Right), "y");
        assert_eq!(press(&doc, &mut t, TreeKey::Left), "x");
        // Left on expanded `x` collapses it and stays; again goes to `r`.
        assert_eq!(press(&doc, &mut t, TreeKey::Left), "x");
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "z");
        assert_eq!(press(&doc, &mut t, TreeKey::Left), "r");
        // The root has no parent row.
        assert_eq!(press(&doc, &mut t, TreeKey::Left), "r");
    }

    #[test]
    fn left_from_attribute_and_text_rows_goes_to_their_element() {
        let (doc, mut t) = setup();
        press(&doc, &mut t, TreeKey::Down);
        assert_eq!(press(&doc, &mut t, TreeKey::Down), "@a");
        assert_eq!(press(&doc, &mut t, TreeKey::Left), "r");
        assert_eq!(press(&doc, &mut t, TreeKey::End), "z");
        press(&doc, &mut t, TreeKey::Right);
        assert_eq!(press(&doc, &mut t, TreeKey::Right), "'t'");
        assert_eq!(press(&doc, &mut t, TreeKey::Left), "z");
    }

    #[test]
    fn keep_visible_scrolls_only_when_needed() {
        let (doc, mut t) = setup();
        t.visible = 1..3;
        let rows = t.rows.clone();
        t.keep_visible(rows[1]);
        assert_eq!(t.scroll_to, None);
        t.keep_visible(rows[0]);
        assert_eq!(t.scroll_to, Some(0));
        t.scroll_to = None;
        // The last visible row may be cut off: scroll so the row is fully shown.
        t.keep_visible(rows[3]);
        assert_eq!(t.scroll_to, Some(3));
        assert_eq!(
            rows.len(),
            4,
            "{:?}",
            rows.iter().map(|&r| name(&doc, r)).collect::<Vec<_>>()
        );
    }
}
