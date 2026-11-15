//! The grid: the child elements of one element as table rows (`docs/spec/grid.md`).
//!
//! [`Grid::build`] finds the rows and the columns once, when the selection changes. Cells are
//! decoded on demand by [`Grid::cell`], so a view pays only for the rows it shows.

use xml_atlas_xml::{Folded, NodeId, NodeKind};

use crate::command::{as_parsed, holds_only_text};
use crate::{Document, truncate};

/// Rows examined to find the columns. Wide enough for real data, bounded for huge lists
/// (`docs/spec/grid.md`, GRID-3).
pub const COLUMN_SAMPLE: usize = 1000;

/// Cell text is cut at this many characters (GRID-4).
pub const CELL_CHARS: usize = 200;

/// One column of the grid, in the order it was first met (GRID-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Column {
    /// An attribute of the row element.
    Attribute(String),
    /// A child element of the row element, by name.
    Child(String),
    /// The row element's own text.
    Text,
}

/// What a cell shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// The row has no such attribute, child or text. Drawn unlike an empty value (GRID-13).
    Absent,
    /// Decoded text, cut at [`CELL_CHARS`]. May be empty.
    Value(String),
    /// One child element with attributes or children of its own: shown by name (GRID-12).
    Element(String),
    /// Several child elements of the same name: shown by name and count (GRID-12).
    Repeated { name: String, count: usize },
}

/// What editing a cell changes (GRID-10); see [`crate::command::set_cell`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellTarget {
    /// An attribute of the row element; added when the row does not have it.
    Attribute { element: NodeId, name: String },
    /// The text of an element that holds only text, or nothing: the row's own, or that of
    /// the child a cell shows.
    Text(NodeId),
}

/// Rows and columns for the children of one element.
#[derive(Clone, Debug)]
pub struct Grid {
    /// The element whose children are the rows.
    pub parent: NodeId,
    /// Path of `parent`, e.g. `/orders/order[2]`. Built once: [`xml_atlas_xml::Index::path`]
    /// walks siblings and is too slow to repeat every frame.
    pub path: String,
    /// Attributes of `parent` itself, decoded (GRID-11).
    pub attributes: Vec<(String, String)>,
    /// Child elements of `parent` in document order (GRID-1); `parent` itself when it has
    /// none (GRID-5).
    pub rows: Vec<NodeId>,
    pub columns: Vec<Column>,
}

impl Grid {
    pub fn build(doc: &Document, parent: NodeId) -> Self {
        let idx = &doc.index;
        let src = &doc.src;
        let mut rows: Vec<NodeId> = idx
            .children(parent)
            .filter(|&c| idx.node(c).kind == NodeKind::Element)
            .collect();
        if rows.is_empty() {
            rows.push(parent);
        }
        let mut columns = Vec::new();
        for &row in rows.iter().take(COLUMN_SAMPLE) {
            for (name, _) in idx.attributes(src, row) {
                push_unique(&mut columns, Column::Attribute(name));
            }
            for c in idx.children(row) {
                match idx.node(c).kind {
                    NodeKind::Element => push_unique(&mut columns, Column::Child(name(doc, c))),
                    NodeKind::Text | NodeKind::CData => push_unique(&mut columns, Column::Text),
                    _ => {}
                }
            }
            if idx.node(row).folded != Folded::No {
                push_unique(&mut columns, Column::Text);
            }
        }
        Self {
            parent,
            path: idx.path(src, parent),
            attributes: idx.attributes(src, parent),
            rows,
            columns,
        }
    }

    /// The cell of `row` in `column`.
    pub fn cell(&self, doc: &Document, row: NodeId, column: &Column) -> Cell {
        let idx = &doc.index;
        let src = &doc.src;
        match column {
            Column::Attribute(attr) => idx
                .attributes(src, row)
                .into_iter()
                .find(|(k, _)| k == attr)
                .map_or(Cell::Absent, |(_, v)| Cell::Value(truncate(&v, CELL_CHARS))),
            Column::Child(child) => {
                let mut matching = idx.children(row).filter(|&c| {
                    idx.node(c).kind == NodeKind::Element && idx.name(src, c) == child.as_bytes()
                });
                let Some(first) = matching.next() else {
                    return Cell::Absent;
                };
                let more = matching.count();
                if more > 0 {
                    Cell::Repeated {
                        name: child.clone(),
                        count: more + 1,
                    }
                } else if has_structure(doc, first) {
                    Cell::Element(child.clone())
                } else {
                    Cell::Value(text(doc, first))
                }
            }
            Column::Text => {
                if idx.has_text(row) {
                    Cell::Value(text(doc, row))
                } else {
                    Cell::Absent
                }
            }
        }
    }

    /// The node a cell stands for: the attribute's element, or the first matching child.
    /// `None` when the cell is [`Cell::Absent`].
    pub fn cell_node(&self, doc: &Document, row: NodeId, column: &Column) -> Option<NodeId> {
        match column {
            Column::Child(child) => doc.index.children(row).find(|&c| {
                doc.index.node(c).kind == NodeKind::Element
                    && doc.index.name(&doc.src, c) == child.as_bytes()
            }),
            Column::Attribute(_) | Column::Text => {
                (self.cell(doc, row, column) != Cell::Absent).then_some(row)
            }
        }
    }

    /// What editing the cell of `row` in `column` would change, and its whole current value
    /// (not cut at [`CELL_CHARS`]). `None` when the cell cannot be edited: it stands for child
    /// elements (GRID-12), the child is missing, or the element holds more than text.
    pub fn edit_target(
        &self,
        doc: &Document,
        row: NodeId,
        column: &Column,
    ) -> Option<(CellTarget, String)> {
        let idx = &doc.index;
        let src = &doc.src;
        let text = |id| {
            holds_only_text(doc, id)
                .then(|| (CellTarget::Text(id), as_parsed(&idx.element_text(src, id))))
        };
        match column {
            Column::Attribute(name) => {
                let value = idx
                    .attributes(src, row)
                    .into_iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v)
                    .unwrap_or_default();
                let target = CellTarget::Attribute {
                    element: row,
                    name: name.clone(),
                };
                Some((target, value))
            }
            Column::Text => text(row),
            Column::Child(_) => match self.cell(doc, row, column) {
                Cell::Value(_) => text(self.cell_node(doc, row, column)?),
                _ => None,
            },
        }
    }
}

fn push_unique(columns: &mut Vec<Column>, c: Column) {
    if !columns.contains(&c) {
        columns.push(c);
    }
}

fn name(doc: &Document, id: NodeId) -> String {
    String::from_utf8_lossy(doc.index.name(&doc.src, id)).into_owned()
}

/// Whether an element has attributes or child elements, so that its text alone would hide
/// something.
fn has_structure(doc: &Document, id: NodeId) -> bool {
    let idx = &doc.index;
    idx.children(id)
        .any(|c| idx.node(c).kind == NodeKind::Element)
        || !idx.attributes(&doc.src, id).is_empty()
}

/// The text and CDATA children of an element, decoded, joined and cut at [`CELL_CHARS`].
fn text(doc: &Document, id: NodeId) -> String {
    truncate(&doc.index.element_text(&doc.src, id), CELL_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERS: &str = r#"<orders>
  <order id="1" status="open">
    <customer>Müller</customer>
    <total currency="EUR">12.50</total>
    <items><item sku="A1"/><item sku="B2"/></items>
  </order>
  <order id="2">
    <customer></customer>
    <note>first</note>
    <note>second</note>
  </order>
  <order id="3" status="closed" extra="late">
    <customer>Ø &amp; Co</customer>
    <gift/>
  </order>
</orders>"#;

    fn doc(src: &str) -> Document {
        Document::from_bytes(src.as_bytes().to_vec())
    }

    fn grid_of(doc: &Document, element: &str) -> Grid {
        let id = (0..doc.index.len() as NodeId)
            .find(|&id| {
                doc.index.node(id).kind == NodeKind::Element
                    && doc.index.name(&doc.src, id) == element.as_bytes()
            })
            .unwrap();
        Grid::build(doc, id)
    }

    fn cells(doc: &Document, grid: &Grid, column: &Column) -> Vec<Cell> {
        grid.rows
            .iter()
            .map(|&r| grid.cell(doc, r, column))
            .collect()
    }

    fn value(s: &str) -> Cell {
        Cell::Value(s.into())
    }

    fn attr(s: &str) -> Column {
        Column::Attribute(s.into())
    }

    fn child(s: &str) -> Column {
        Column::Child(s.into())
    }

    #[test]
    fn columns_in_order_of_first_appearance() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        assert_eq!(g.rows.len(), 3);
        assert_eq!(
            g.columns,
            [
                attr("id"),
                attr("status"),
                child("customer"),
                child("total"),
                child("items"),
                child("note"),
                attr("extra"),
                child("gift"),
            ]
        );
    }

    /// GRID-12: a child with structure shows its name, a repeated child its name and count.
    #[test]
    fn nested_and_repeated_children() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        assert_eq!(
            cells(&d, &g, &child("total")),
            [Cell::Element("total".into()), Cell::Absent, Cell::Absent]
        );
        assert_eq!(
            cells(&d, &g, &child("items")),
            [Cell::Element("items".into()), Cell::Absent, Cell::Absent]
        );
        assert_eq!(
            cells(&d, &g, &child("note")),
            [
                Cell::Absent,
                Cell::Repeated {
                    name: "note".into(),
                    count: 2
                },
                Cell::Absent
            ]
        );
    }

    /// GRID-13: an absent attribute or child differs from an empty one.
    #[test]
    fn absent_differs_from_empty() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        assert_eq!(
            cells(&d, &g, &attr("status")),
            [value("open"), Cell::Absent, value("closed")]
        );
        assert_eq!(
            cells(&d, &g, &child("customer")),
            [value("Müller"), value(""), value("Ø & Co")]
        );
        // `<gift/>` has no text, attributes or children: an empty value, not absent.
        assert_eq!(
            cells(&d, &g, &child("gift")),
            [Cell::Absent, Cell::Absent, value("")]
        );
    }

    /// GRID-11: the selected element's own attributes, decoded.
    #[test]
    fn attributes_of_the_selected_element() {
        let d = doc(r#"<shop name="Zentrale" city="Gr&#xE4;z"><a/></shop>"#);
        let g = grid_of(&d, "shop");
        assert_eq!(
            g.attributes,
            [
                ("name".to_string(), "Zentrale".to_string()),
                ("city".to_string(), "Gräz".to_string())
            ]
        );
        assert_eq!(grid_of(&d, "a").attributes, []);
    }

    /// GRID-5: an element without child elements is its own single row.
    #[test]
    fn leaf_element_is_its_own_row() {
        let d = doc(r#"<r><price currency="EUR">12.50</price></r>"#);
        let g = grid_of(&d, "price");
        assert_eq!(g.rows, [g.parent]);
        assert_eq!(g.columns, [attr("currency"), Column::Text]);
        assert_eq!(g.cell(&d, g.parent, &Column::Text), value("12.50"));
    }

    #[test]
    fn text_column_and_mixed_content() {
        let d = doc("<r><p>Hello <b>big</b> world</p><p><b>only</b></p></r>");
        let g = grid_of(&d, "r");
        assert_eq!(g.columns, [Column::Text, child("b")]);
        assert_eq!(
            cells(&d, &g, &Column::Text),
            [value("Hello  world"), Cell::Absent]
        );
        assert_eq!(cells(&d, &g, &child("b")), [value("big"), value("only")]);
    }

    #[test]
    fn long_values_are_cut() {
        let long = "ä".repeat(CELL_CHARS + 50);
        let d = doc(&format!("<r><a v=\"{long}\"/></r>"));
        let g = grid_of(&d, "r");
        let Cell::Value(v) = g.cell(&d, g.rows[0], &attr("v")) else {
            panic!()
        };
        assert_eq!(v.chars().count(), CELL_CHARS + 1);
        assert!(v.ends_with('…'));
    }

    #[test]
    fn cell_node_points_at_the_child() {
        let d = doc(ORDERS);
        let g = grid_of(&d, "orders");
        let first = g.rows[0];
        let total = g.cell_node(&d, first, &child("total")).unwrap();
        assert_eq!(d.index.name(&d.src, total), b"total");
        assert_eq!(g.cell_node(&d, first, &attr("id")), Some(first));
        assert_eq!(g.cell_node(&d, g.rows[1], &child("total")), None);
        assert_eq!(g.cell_node(&d, g.rows[1], &attr("status")), None);
    }

    #[test]
    fn edit_targets_name_what_a_cell_changes_with_its_whole_value() {
        let long = "ä".repeat(CELL_CHARS + 50);
        let src = format!(
            "\u{feff}<r>\r\n  <a id=\"1\" t=\"x &amp; y\"><c>{long}</c><m>p<b/>q</m><n><!-- c --></n><e k=\"v\"/></a>\r\n  <a/>\r\n</r>\r\n"
        );
        let doc = doc(&src);
        let grid = grid_of(&doc, "r");
        let (a1, a2) = (grid.rows[0], grid.rows[1]);
        let child = |row, name: &str| {
            grid.cell_node(&doc, row, &Column::Child(name.into()))
                .unwrap()
        };
        let target = |row, c: Column| grid.edit_target(&doc, row, &c);
        assert_eq!(
            target(a1, Column::Attribute("t".into())),
            Some((
                CellTarget::Attribute {
                    element: a1,
                    name: "t".into()
                },
                "x & y".into()
            ))
        );
        // A missing attribute can be added: its value starts empty.
        assert_eq!(
            target(a2, Column::Attribute("t".into())),
            Some((
                CellTarget::Attribute {
                    element: a2,
                    name: "t".into()
                },
                String::new()
            ))
        );
        // The whole text, not the 200 characters the cell shows.
        assert_eq!(
            target(a1, Column::Child("c".into())),
            Some((CellTarget::Text(child(a1, "c")), long))
        );
        // Mixed content, a comment, a child with attributes, a missing child: not editable.
        assert_eq!(target(a1, Column::Child("m".into())), None);
        assert_eq!(target(a1, Column::Child("n".into())), None);
        assert_eq!(target(a1, Column::Child("e".into())), None);
        assert_eq!(target(a2, Column::Child("c".into())), None);
        // An empty row element can take text; one with children cannot.
        let grid_a = grid_of(&doc, "c");
        assert_eq!(grid_a.rows, [child(a1, "c")]);
        assert_eq!(
            Grid::build(&doc, a2).edit_target(&doc, a2, &Column::Text),
            Some((CellTarget::Text(a2), String::new()))
        );
        assert_eq!(target(a1, Column::Text), None);
    }
}
