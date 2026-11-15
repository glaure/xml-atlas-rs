//! Rows of the tree (`docs/spec/tree.md`, TREE-1).
//!
//! Under an element come first its attributes, one row each, then its text and child elements
//! in document order. Comments and processing instructions are not rows. Attributes are not
//! nodes in the index, so a row is either a node or "attribute `index` of `element`"; nothing
//! is stored for an attribute until its element is expanded.

use xml_atlas_xml::{Folded, NodeId, NodeKind, NodeSplice, ROOT};

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
    /// The text or CDATA that the index keeps inside `element` (D11). Shown like a text node.
    FoldedText(NodeId),
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
            Row::Attribute { element, .. } | Row::FoldedText(element) => element,
            Row::Node(id) if doc.index.node(id).kind == NodeKind::Element => id,
            Row::Node(id) => doc.index.node(id).parent,
        }
    }

    /// The node a row is filed under. Rows in tree order never go down in it: an element's
    /// attribute and text rows follow its own, and its children have higher ids.
    pub fn id(self) -> NodeId {
        match self {
            Row::Node(id) | Row::FoldedText(id) | Row::Attribute { element: id, .. } => id,
        }
    }

    /// The same row after a change replaced nodes ([`NodeSplice`]); `None` when its node
    /// was replaced.
    pub fn follow(self, ns: &NodeSplice) -> Option<Row> {
        let id = ns.map(self.id())?;
        Some(match self {
            Row::Node(_) => Row::Node(id),
            Row::FoldedText(_) => Row::FoldedText(id),
            Row::Attribute { index, .. } => Row::Attribute { element: id, index },
        })
    }

    /// Nesting level: 0 for the root element.
    pub fn depth(self, doc: &Document) -> u16 {
        match self {
            Row::Node(id) => doc.index.depth(id).saturating_sub(1),
            Row::Attribute { element, .. } | Row::FoldedText(element) => doc.index.depth(element),
        }
    }
}

/// Rows directly under `id`: attributes first (when shown), then text and child elements.
pub fn children(doc: &Document, id: NodeId, options: TreeOptions) -> Vec<Row> {
    let idx = &doc.index;
    let mut rows = own_rows(doc, id, options);
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
    idx.node(id).folded != Folded::No
        || idx.children(id).any(|c| is_row(doc, c, id))
        || (options.attributes && !idx.attributes(&doc.src, id).is_empty())
}

/// The rows to show, in order: every top-level row, and the rows under every expanded element.
/// `expanded` has one entry per node of the index.
pub fn visible_rows(doc: &Document, expanded: &[bool], options: TreeOptions, out: &mut Vec<Row>) {
    out.clear();
    push_rows(doc, children(doc, ROOT, options), expanded, options, out);
}

/// Follows a change that replaced nodes ([`NodeSplice`], D19) in the tree's state, instead
/// of building it again: `expanded` (one entry per node) and `rows` (as [`visible_rows`]
/// made them before the change) become what they are for the changed document. Only the
/// rows of the replaced nodes are made again, and the parent's own attribute and text rows
/// (its start tag may have changed); the rows after them only change their ids. So the cost
/// follows what changed, not how many rows the parent has. A replaced node keeps its
/// expansion when as many nodes came back as went, else only the first of them keeps it.
pub fn follow(
    doc: &Document,
    ns: &NodeSplice,
    expanded: &mut Vec<bool>,
    options: TreeOptions,
    rows: &mut Vec<Row>,
) {
    let at = ns.at as usize;
    if ns.removed != ns.inserted {
        let first = ns.removed > 0 && expanded[at];
        expanded.splice(
            at..at + ns.removed as usize,
            std::iter::repeat_n(false, ns.inserted as usize),
        );
        if ns.inserted > 0 {
            expanded[at] = first;
        }
    }
    debug_assert_eq!(expanded.len(), doc.index.len());
    let parent = ns.parent;
    // The rows of the replaced nodes: ids from `at` up to the first id after them.
    let lo = rows.partition_point(|r| r.id() < ns.at);
    let hi = rows.partition_point(|r| r.id() < ns.at + ns.removed);
    for r in &mut rows[hi..] {
        *r = r.follow(ns).expect("rows after the replaced nodes stay");
    }
    // Shown when the parent is shown and expanded (the document node always is).
    let own = rows.partition_point(|r| r.id() < parent);
    let shown = parent == ROOT || rows.get(own) == Some(&Row::Node(parent));
    if !shown || (parent != ROOT && !expanded[parent as usize]) {
        debug_assert_eq!(lo, hi, "nothing under a hidden parent is shown");
        return;
    }
    let top: Vec<Row> = ns
        .new_ids()
        .filter(|&id| doc.index.node(id).parent == parent && is_row(doc, id, parent))
        .map(Row::Node)
        .collect();
    let mut made = Vec::new();
    push_rows(doc, top, expanded, options, &mut made);
    rows.splice(lo..hi, made);
    if parent != ROOT {
        let first = own + 1;
        let old = rows[first..]
            .iter()
            .take_while(|r| r.id() == parent)
            .count();
        rows.splice(first..first + old, own_rows(doc, parent, options));
    }
}

/// The rows an element has of its own, before its children: its attributes (when shown) and
/// its folded text.
fn own_rows(doc: &Document, id: NodeId, options: TreeOptions) -> Vec<Row> {
    let idx = &doc.index;
    let mut rows = Vec::new();
    if id == ROOT {
        return rows;
    }
    if options.attributes {
        let count = idx.attributes(&doc.src, id).len() as u32;
        rows.extend((0..count).map(|index| Row::Attribute { element: id, index }));
    }
    if idx.node(id).folded != Folded::No {
        rows.push(Row::FoldedText(id));
    }
    rows
}

/// Appends `top` and, depth first, the rows under every expanded element among them.
fn push_rows(
    doc: &Document,
    mut top: Vec<Row>,
    expanded: &[bool],
    options: TreeOptions,
    out: &mut Vec<Row>,
) {
    top.reverse();
    let mut stack = top;
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
    let id = match row {
        Row::Attribute { .. } => return RowKind::Attribute,
        Row::FoldedText(_) => return RowKind::Text,
        Row::Node(id) => id,
    };
    let idx = &doc.index;
    if idx.node(id).kind != NodeKind::Element {
        return RowKind::Text;
    }
    if idx
        .children(id)
        .any(|c| idx.node(c).kind == NodeKind::Element)
    {
        return RowKind::Parent;
    }
    if idx.has_text(id) {
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
        Row::Node(id) | Row::FoldedText(id) => {
            Label::Text(truncate(&idx.text(src, id), VALUE_CHARS))
        }
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
    if idx.is_text_only(id) {
        return Some(truncate(&idx.element_text(&doc.src, id), VALUE_CHARS));
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
        // The text of a text-only element is kept in the element (D11); its row says so.
        assert_eq!(rows[1], Row::FoldedText(owner));
        let script = element(&d, "script");
        assert_eq!(
            children(&d, script, TreeOptions::default()),
            [Row::FoldedText(script)]
        );
        // Mixed content keeps its text nodes.
        let mixed = children(&d, element(&d, "mixed"), TreeOptions::default());
        assert!(matches!(mixed[0], Row::Node(_)));
    }

    /// Small generator for edits; no dependency needed.
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n.max(1)
        }
    }

    const TYPED: &[&str] = &[
        "x", "ü", "<", ">", "\"", "<a/>", "</", " b=\"1\"", "\r\n", "&amp;",
    ];

    /// Random typed changes, kept local by the index (D9) or stale (D19): following them
    /// gives the rows a full rebuild gives, and every node that stays keeps its expansion.
    fn check_follow(src: Vec<u8>, seed: u64, rounds: usize) {
        let mut rng = Lcg(seed);
        let mut doc = Document::from_bytes(src);
        for _ in 0..rounds {
            let options = TreeOptions {
                attributes: rng.below(4) != 0,
            };
            let mut expanded = vec![false; doc.index.len()];
            for e in expanded.iter_mut() {
                *e = rng.below(3) != 0;
            }
            let mut rows = Vec::new();
            visible_rows(&doc, &expanded, options, &mut rows);
            let before = expanded.clone();

            let len = doc.src.len();
            let at = crate::text::snap(&doc.src, rng.below(len + 1));
            let sel = if rng.below(3) == 0 {
                let to = crate::text::snap(&doc.src, (at + rng.below(8)).min(len));
                crate::text::Selection {
                    anchor: at,
                    head: to,
                }
            } else {
                crate::text::Selection::cursor(at)
            };
            let typed = TYPED[rng.below(TYPED.len())];
            let change = if rng.below(4) == 0 {
                crate::text::backspace(&doc, sel, false)
            } else {
                crate::text::insert(&doc, sel, typed)
            };
            let Some(change) = change else { continue };
            doc.apply_typed(change.edit).unwrap();
            let Some(ns) = doc.node_splice() else {
                panic!("seed {seed}: a typed change is always local");
            };
            follow(&doc, &ns, &mut expanded, options, &mut rows);
            let mut fresh = Vec::new();
            visible_rows(&doc, &expanded, options, &mut fresh);
            assert_eq!(rows, fresh, "seed {seed}: {ns:?}");
            for (old, &e) in before.iter().enumerate() {
                if let Some(new) = ns.map(old as NodeId) {
                    assert_eq!(expanded[new as usize], e, "seed {seed}: node {old}");
                }
            }
            // Read again now and then, as the editor does after a pause.
            if rng.below(5) == 0 {
                doc.reparse();
            }
        }
    }

    #[test]
    fn following_a_change_equals_a_rebuild() {
        let sample = "\u{FEFF}<?xml version=\"1.0\"?>\r\n<r a=\"1\">\r\n  <b c=\"2\">Grüße</b>\r\n  <!--x-->\r\n  <d><e/><e f=\"3\">t</e>text<![CDATA[<]]></d>\r\n</r>\r\n";
        for seed in 0..40 {
            check_follow(sample.as_bytes().to_vec(), seed, 60);
        }
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus");
        for (i, entry) in std::fs::read_dir(dir).expect("corpus").enumerate() {
            let src = std::fs::read(entry.expect("entry").path()).expect("read");
            check_follow(src, 1000 + i as u64, 40);
        }
    }
}
