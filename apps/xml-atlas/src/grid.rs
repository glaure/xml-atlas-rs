//! The grid: child elements of the selection as table rows, with their attributes and leaf
//! children as columns. Cells are decoded only for visible rows.

use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use xml_atlas_engine::Document;
use xml_atlas_engine::xml::{NodeId, NodeKind};

use crate::app::Open;
use crate::tree::truncate;

/// Rows sampled to discover the column set. Wide enough for real data, bounded for huge lists.
const COLUMN_SAMPLE: usize = 1000;

#[derive(Clone, PartialEq)]
enum Col {
    Attr(String),
    Child(String),
    Text,
}

pub struct GridModel {
    parent: NodeId,
    /// Shown above the table. Built once here: `Index::path` walks siblings, which is too
    /// slow to repeat every frame under a parent with hundreds of thousands of children.
    path: String,
    rows: Vec<NodeId>,
    cols: Vec<Col>,
}

impl GridModel {
    pub fn build(doc: &Document, parent: NodeId) -> Self {
        let idx = &doc.index;
        let src = &doc.src;
        let rows: Vec<NodeId> = idx
            .children(parent)
            .filter(|&c| idx.node(c).kind == NodeKind::Element)
            .collect();
        let mut cols = Vec::new();
        // A leaf parent has no rows: show the element itself as a single row.
        let sample: Vec<NodeId> = if rows.is_empty() {
            vec![parent]
        } else {
            rows.clone()
        };
        for &r in sample.iter().take(COLUMN_SAMPLE) {
            for (k, _) in idx.attributes(src, r) {
                push_unique(&mut cols, Col::Attr(k));
            }
            for c in idx.children(r) {
                match idx.node(c).kind {
                    NodeKind::Element if is_leaf(doc, c) => {
                        let name = String::from_utf8_lossy(idx.name(src, c)).into_owned();
                        push_unique(&mut cols, Col::Child(name));
                    }
                    NodeKind::Text | NodeKind::CData => push_unique(&mut cols, Col::Text),
                    _ => {}
                }
            }
        }
        let rows = if rows.is_empty() { vec![parent] } else { rows };
        Self {
            parent,
            path: idx.path(src, parent),
            rows,
            cols,
        }
    }

    fn cell(&self, doc: &Document, row: NodeId, col: &Col) -> String {
        let idx = &doc.index;
        let src = &doc.src;
        match col {
            Col::Attr(name) => idx
                .attributes(src, row)
                .into_iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v)
                .unwrap_or_default(),
            Col::Child(name) => idx
                .children(row)
                .find(|&c| {
                    idx.node(c).kind == NodeKind::Element && idx.name(src, c) == name.as_bytes()
                })
                .map(|c| leaf_text(doc, c))
                .unwrap_or_default(),
            Col::Text => leaf_text(doc, row),
        }
    }
}

fn push_unique(cols: &mut Vec<Col>, c: Col) {
    if !cols.contains(&c) {
        cols.push(c);
    }
}

fn is_leaf(doc: &Document, id: NodeId) -> bool {
    doc.index
        .children(id)
        .all(|c| doc.index.node(c).kind != NodeKind::Element)
}

fn leaf_text(doc: &Document, id: NodeId) -> String {
    let idx = &doc.index;
    let mut s = String::new();
    for c in idx.children(id) {
        if matches!(idx.node(c).kind, NodeKind::Text | NodeKind::CData) {
            s.push_str(&idx.text(&doc.src, c));
        }
    }
    truncate(&s, 200)
}

pub fn show(ui: &mut Ui, open: &mut Open) {
    let Some(grid) = &open.grid else { return };
    let doc = &open.doc;
    ui.horizontal(|ui| {
        ui.label(RichText::new(&grid.path).strong());
        ui.label(RichText::new(format!("{} rows", grid.rows.len())).weak());
    });
    let mut focus = None;
    let mut enter = None;
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    TableBuilder::new(ui)
        .id_salt(("grid", grid.parent))
        .striped(true)
        .resizable(true)
        .auto_shrink(false)
        .sense(egui::Sense::click())
        .column(Column::auto().at_least(40.0))
        .column(Column::initial(110.0).at_least(40.0).clip(true))
        .columns(
            Column::initial(140.0).at_least(40.0).clip(true),
            grid.cols.len(),
        )
        .header(row_h + 2.0, |mut h| {
            h.col(|ui| {
                ui.strong("#");
            });
            h.col(|ui| {
                ui.strong("element");
            });
            for c in &grid.cols {
                h.col(|ui| {
                    match c {
                        Col::Attr(n) => ui.strong(format!("@{n}")),
                        Col::Child(n) => ui.strong(n),
                        Col::Text => ui.strong("text()"),
                    };
                });
            }
        })
        .body(|body| {
            body.rows(row_h, grid.rows.len(), |mut row| {
                let i = row.index();
                let id = grid.rows[i];
                row.set_selected(open.focus == Some(id));
                row.col(|ui| {
                    ui.label(RichText::new((i + 1).to_string()).weak());
                });
                row.col(|ui| {
                    ui.label(String::from_utf8_lossy(doc.index.name(&doc.src, id)));
                });
                for c in &grid.cols {
                    row.col(|ui| {
                        ui.add(egui::Label::new(grid.cell(doc, id, c)).truncate());
                    });
                }
                let resp = row.response();
                if resp.clicked() {
                    focus = Some(id);
                }
                if resp.double_clicked() && id != grid.parent {
                    enter = Some(id);
                }
            });
        });
    if let Some(id) = enter {
        open.select(id);
    } else if let Some(id) = focus {
        open.focus(id);
    }
}
