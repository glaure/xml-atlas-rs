//! Rows of the tree (`docs/spec/tree.md`, TREE-1).
//!
//! Under an element come first its attributes, one row each, then its text and child elements
//! in document order. Comments and processing instructions are not rows. Attributes are not
//! nodes in the index, so a row is either a node or "attribute `index` of `element`"; nothing
//! is stored for an attribute until its element is expanded.

use xml_atlas_xml::{NodeId, NodeKind, ROOT};

use crate::{Document, truncate};

/// Characters of text or attribute value shown in a row.
pub const VALUE_CHARS: usize = 60;

/// Characters of the first attribute's value in an element's hint (TREE-2).
pub const HINT_ATTRIBUTE_CHARS: usize = 40;

/// What the tree shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeOptions {
    /// Attribute rows under each element. The user can turn them off (TREE-1).
    pub attributes: bool,
}

impl Default for TreeOptions {
    fn default() -> Self {
        Self { attributes: true }
    }
}

/// One row of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    /// An element, or a text or CDATA node.
    Node(NodeId),
    /// The `index`th attribute of `element`, counted from zero.
    Attribute { element: NodeId, index: u32 },
}

/// The kind of a row, for its icon (TREE-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// An element with child elements (also when it has text between them).
    Parent,
    /// An element that holds only text or CDATA.
    TextElement,
    /// An element with no child elements and no text; it may have attributes.
    Empty,
    Attribute,
    /// A text or CDATA node.
    Text,
}

/// What a row says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Label {
    /// Element name, and the hint shown after it (TREE-2).
    Element { name: String, hint: Option<String> },
    /// Attribute name and value, shown as `name = "value"`.
    Attribute { name: String, value: String },
    /// Text or CDATA content, trimmed and cut at [`VALUE_CHARS`].
    Text(String),
}

impl Row {
    /// The element a row belongs to: itself, the attribute's element, or the text's parent.
    pub fn element(self, doc: &Document) -> NodeId {
        match self {
            Row::Attribute { element, .. } => element,
            Row::Node(id) if doc.index.node(id).kind == NodeKind::Element => id,
            Row::Node(id) => doc.index.node(id).parent,
        }
    }

    /// Nesting level: 0 for the root element.
    pub fn depth(self, doc: &Document) -> u16 {
        match self {
            Row::Node(id) => doc.index.depth(id).saturating_sub(1),
            Row::Attribute { element, .. } => doc.index.depth(element),
        }
    }
}

/// Rows directly under `id`: attributes first (when shown), then text and child elements.
pub fn children(doc: &Document, id: NodeId, options: TreeOptions) -> Vec<Row> {
    let idx = &doc.index;
    let mut rows = Vec::new();
    if options.attributes && id != ROOT {
        let count = idx.attributes(&doc.src, id).len() as u32;
        rows.extend((0..count).map(|index| Row::Attribute { element: id, index }));
    }
    rows.extend(
        idx.children(id)
            .filter(|&c| is_row(doc, c, id))
            .map(Row::Node),
    );
    rows
}

/// Whether a row can be expanded: an element with any rows under it.
pub fn has_children(doc: &Document, row: Row, options: TreeOptions) -> bool {
    let Row::Node(id) = row else { return false };
    let idx = &doc.index;
    if idx.node(id).kind != NodeKind::Element {
        return false;
    }
    idx.children(id).any(|c| is_row(doc, c, id))
        || (options.attributes && !idx.attributes(&doc.src, id).is_empty())
}

/// The rows to show, in order: every top-level row, and the rows under every expanded element.
/// `expanded` has one entry per node of the index.
pub fn visible_rows(doc: &Document, expanded: &[bool], options: TreeOptions, out: &mut Vec<Row>) {
    out.clear();
    let mut stack = children(doc, ROOT, options);
    stack.reverse();
    while let Some(row) = stack.pop() {
        out.push(row);
        if let Row::Node(id) = row
            && expanded[id as usize]
            && doc.index.node(id).kind == NodeKind::Element
        {
            let start = stack.len();
            stack.extend(children(doc, id, options));
            stack[start..].reverse();
        }
    }
}

/// The kind of `row`.
pub fn kind(doc: &Document, row: Row) -> RowKind {
    let Row::Node(id) = row else {
        return RowKind::Attribute;
    };
    let idx = &doc.index;
    if idx.node(id).kind != NodeKind::Element {
        return RowKind::Text;
    }
    let mut text = false;
    for c in idx.children(id) {
        match idx.node(c).kind {
            NodeKind::Element => return RowKind::Parent,
            NodeKind::Text | NodeKind::CData => text = true,
            _ => {}
        }
    }
    if text {
        RowKind::TextElement
    } else {
        RowKind::Empty
    }
}

/// What `row` shows.
pub fn label(doc: &Document, row: Row) -> Label {
    let idx = &doc.index;
    let src = &doc.src;
    match row {
        Row::Attribute { element, index } => {
            let (name, value) = idx
                .attributes(src, element)
                .into_iter()
                .nth(index as usize)
                .unwrap_or_default();
            Label::Attribute {
                name,
                value: truncate(&value, VALUE_CHARS),
            }
        }
        Row::Node(id) if idx.node(id).kind == NodeKind::Element => Label::Element {
            name: String::from_utf8_lossy(idx.name(src, id)).into_owned(),
            hint: hint(doc, id),
        },
        Row::Node(id) => Label::Text(truncate(&idx.text(src, id), VALUE_CHARS)),
    }
}

/// Elements always; text and CDATA only under an element (text outside the root element is
/// not content).
fn is_row(doc: &Document, id: NodeId, parent: NodeId) -> bool {
    match doc.index.node(id).kind {
        NodeKind::Element => true,
        NodeKind::Text | NodeKind::CData => parent != ROOT,
        _ => false,
    }
}

/// Short hint after an element's name: the text of an element that holds only text, otherwise
/// its first attribute (TREE-2).
fn hint(doc: &Document, id: NodeId) -> Option<String> {
    let idx = &doc.index;
    let mut kids = idx.children(id);
    if let (Some(only), None) = (kids.next(), kids.next())
        && matches!(idx.node(only).kind, NodeKind::Text | NodeKind::CData)
    {
        return Some(truncate(&idx.text(&doc.src, only), VALUE_CHARS));
    }
    idx.attributes(&doc.src, id)
        .into_iter()
        .next()
        .map(|(k, v)| format!("{k}=\"{}\"", truncate(&v, HINT_ATTRIBUTE_CHARS)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOP: &str = r#"<?xml version="1.0"?>
<!-- top -->
<shop name="Zentrale" city="Graz">
  <!-- inner -->
  <owner id="7">Günther &amp; Söhne</owner>
  <mixed>Hello <b>big</b> world</mixed>
  <script><![CDATA[a < b]]></script>
  <?pi x?>
  <empty/>
</shop>"#;

    fn doc() -> Document {
        Document::from_bytes(SHOP.as_bytes().to_vec())
    }

    fn element(doc: &Document, name: &str) -> NodeId {
        (0..doc.index.len() as NodeId)
            .find(|&id| {
                doc.index.node(id).kind == NodeKind::Element
                    && doc.index.name(&doc.src, id) == name.as_bytes()
            })
            .unwrap()
    }

    fn labels(doc: &Document, rows: &[Row]) -> Vec<Label> {
        rows.iter().map(|&r| label(doc, r)).collect()
    }

    fn el(name: &str, hint: Option<&str>) -> Label {
        Label::Element {
            name: name.into(),
            hint: hint.map(Into::into),
        }
    }

    fn attr(name: &str, value: &str) -> Label {
        Label::Attribute {
            name: name.into(),
            value: value.into(),
        }
    }

    fn text(s: &str) -> Label {
        Label::Text(s.into())
    }

    /// TREE-1: attributes first, then text and elements in document order; no comments or
    /// processing instructions.
    #[test]
    fn rows_under_an_element() {
        let d = doc();
        let shop = element(&d, "shop");
        assert_eq!(
            labels(&d, &children(&d, shop, TreeOptions::default())),
            [
                attr("name", "Zentrale"),
                attr("city", "Graz"),
                el("owner", Some("Günther & Söhne")),
                el("mixed", None),
                el("script", Some("a < b")),
                el("empty", None),
            ]
        );
        assert_eq!(
            labels(
                &d,
                &children(&d, element(&d, "owner"), TreeOptions::default())
            ),
            [attr("id", "7"), text("Günther & Söhne")]
        );
        assert_eq!(
            labels(
                &d,
                &children(&d, element(&d, "mixed"), TreeOptions::default())
            ),
            [text("Hello"), el("b", Some("big")), text("world")]
        );
    }

    /// The top level holds only the root element: no comments, declaration or whitespace.
    #[test]
    fn top_level_is_the_root_element() {
        let d = doc();
        assert_eq!(
            children(&d, ROOT, TreeOptions::default()),
            [Row::Node(element(&d, "shop"))]
        );
    }

    #[test]
    fn attribute_rows_can_be_hidden() {
        let d = doc();
        let off = TreeOptions { attributes: false };
        assert_eq!(
            labels(&d, &children(&d, element(&d, "shop"), off))[..2],
            [el("owner", Some("Günther & Söhne")), el("mixed", None)]
        );
        // `<empty/>` has nothing under it either way; `owner` still has its text.
        assert!(!has_children(&d, Row::Node(element(&d, "empty")), off));
        assert!(has_children(&d, Row::Node(element(&d, "owner")), off));
    }

    #[test]
    fn expandable_rows() {
        let d = doc();
        let on = TreeOptions::default();
        assert!(has_children(&d, Row::Node(element(&d, "shop")), on));
        assert!(!has_children(&d, Row::Node(element(&d, "empty")), on));
        let owner_rows = children(&d, element(&d, "owner"), on);
        assert!(owner_rows.iter().all(|&r| !has_children(&d, r, on)));
    }

    #[test]
    fn visible_rows_follow_expansion() {
        let d = doc();
        let on = TreeOptions::default();
        let mut expanded = vec![false; d.index.len()];
        let mut rows = Vec::new();

        visible_rows(&d, &expanded, on, &mut rows);
        assert_eq!(labels(&d, &rows), [el("shop", Some("name=\"Zentrale\""))]);

        expanded[element(&d, "shop") as usize] = true;
        expanded[element(&d, "owner") as usize] = true;
        visible_rows(&d, &expanded, on, &mut rows);
        assert_eq!(rows.len(), 1 + 2 + 4 + 2);
        assert_eq!(
            labels(&d, &rows[3..6]),
            [
                el("owner", Some("Günther & Söhne")),
                attr("id", "7"),
                text("Günther & Söhne")
            ]
        );
        let depths: Vec<u16> = rows.iter().map(|r| r.depth(&d)).collect();
        assert_eq!(depths, [0, 1, 1, 1, 2, 2, 1, 1, 1]);
    }

    #[test]
    fn row_kinds() {
        let d = doc();
        let shop = element(&d, "shop");
        let kinds: Vec<RowKind> = children(&d, shop, TreeOptions::default())
            .into_iter()
            .map(|r| kind(&d, r))
            .collect();
        // name, city, owner (text), mixed (text and <b>), script (CDATA), empty
        assert_eq!(
            kinds,
            [
                RowKind::Attribute,
                RowKind::Attribute,
                RowKind::TextElement,
                RowKind::Parent,
                RowKind::TextElement,
                RowKind::Empty,
            ]
        );
        assert_eq!(kind(&d, Row::Node(shop)), RowKind::Parent);
        let owner_text = children(&d, element(&d, "owner"), TreeOptions::default())[1];
        assert_eq!(kind(&d, owner_text), RowKind::Text);
        // Attributes alone do not make an element a parent.
        let e = Document::from_bytes(b"<r><x a=\"1\"/></r>".to_vec());
        assert_eq!(kind(&e, Row::Node(element(&e, "x"))), RowKind::Empty);
    }

    #[test]
    fn row_element() {
        let d = doc();
        let owner = element(&d, "owner");
        let rows = children(&d, owner, TreeOptions::default());
        assert!(rows.iter().all(|r| r.element(&d) == owner));
        assert_eq!(Row::Node(owner).element(&d), owner);
    }
}
