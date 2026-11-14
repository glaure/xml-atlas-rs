//! Flat, span-based XML index.
//!
//! Instead of building a DOM with owned strings, [`index`] makes one pass over the source bytes
//! and records every node as a fixed-size [`Node`] in a single `Vec`. Names, attributes and text
//! are byte ranges into the source and are decoded only when a view asks for them. This keeps
//! opening large files close to the speed of the tokenizer itself and memory near
//! `48 bytes × node count`.
//!
//! Parsing never discards work: on malformed input the index holds every node read before the
//! error, and [`Index::error`] says where and why parsing stopped.

use std::borrow::Cow;
use std::ops::Range;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// Index of a node in [`Index::nodes`].
pub type NodeId = u32;

/// Marks a missing link (no parent, child or sibling).
pub const NONE: NodeId = NodeId::MAX;

/// The document node; always present at index 0.
pub const ROOT: NodeId = 0;

/// UTF-8 byte order mark. Common at the start of XML written on Windows.
pub const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Document,
    Element,
    Text,
    CData,
    Comment,
    ProcessingInstruction,
    Declaration,
    DocType,
}

/// One node. Offsets are byte positions in the source.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// Start of the node (the `<` for markup).
    pub start: u64,
    /// End of the node, exclusive. For elements this is after the end tag.
    pub end: u64,
    pub parent: NodeId,
    pub first_child: NodeId,
    pub next_sibling: NodeId,
    pub child_count: u32,
    /// Length of the start tag (`<a x="1">`) for elements; equals the span for other kinds.
    pub head_len: u32,
    /// Length of the element name, which starts at `start + 1`. Zero for other kinds.
    pub name_len: u32,
    pub depth: u16,
    pub kind: NodeKind,
}

impl Node {
    pub fn span(&self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    pub fn name_span(&self) -> Range<usize> {
        let s = self.start as usize + 1;
        s..s + self.name_len as usize
    }
}

/// Where and why parsing stopped.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{message} (at byte {offset})")]
pub struct ParseError {
    pub offset: u64,
    pub message: String,
}

/// Result of [`index`].
#[derive(Clone, Debug)]
pub struct Index {
    pub nodes: Vec<Node>,
    pub error: Option<ParseError>,
    pub element_count: u32,
    pub max_depth: u16,
}

/// Builds the index for `src`. Never fails; see [`Index::error`].
pub fn index(src: &[u8]) -> Index {
    Builder::new(src).run()
}

impl Index {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Children of `id` in document order.
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            index: self,
            next: self.node(id).first_child,
        }
    }

    /// The root element, if the document has one.
    pub fn root_element(&self) -> Option<NodeId> {
        self.children(ROOT)
            .find(|&c| self.node(c).kind == NodeKind::Element)
    }

    /// Element name as raw bytes. Empty for non-elements.
    pub fn name<'s>(&self, src: &'s [u8], id: NodeId) -> &'s [u8] {
        &src[self.node(id).name_span()]
    }

    /// Raw source of the node.
    pub fn raw<'s>(&self, src: &'s [u8], id: NodeId) -> &'s [u8] {
        &src[self.node(id).span()]
    }

    /// Attributes of an element as `(name, unescaped value)`. Decoded on demand, so only the
    /// rows a view shows pay for it.
    pub fn attributes(&self, src: &[u8], id: NodeId) -> Vec<(String, String)> {
        let n = self.node(id);
        if n.kind != NodeKind::Element {
            return Vec::new();
        }
        let head = &src[n.start as usize + 1..n.start as usize + n.head_len as usize];
        let head = head.strip_suffix(b">").unwrap_or(head);
        let head = head.strip_suffix(b"/").unwrap_or(head);
        let head = String::from_utf8_lossy(head);
        let tag = BytesStart::from_content(head, n.name_len as usize);
        tag.attributes()
            .with_checks(false)
            .flatten()
            .map(|a| {
                let key = a.key.as_ref().to_owned();
                (key, unescape(a.value.as_bytes()).into_owned())
            })
            .collect()
    }

    /// Text content of a text or CDATA node, unescaped.
    pub fn text<'s>(&self, src: &'s [u8], id: NodeId) -> Cow<'s, str> {
        let n = self.node(id);
        let raw = &src[n.span()];
        match n.kind {
            NodeKind::Text => unescape(raw),
            NodeKind::CData => String::from_utf8_lossy(&raw[9..raw.len() - 3]),
            _ => String::from_utf8_lossy(raw),
        }
    }

    /// Path from the root element, e.g. `/catalog/book[3]/title`.
    ///
    /// Walks the siblings before `id` at every level, so it costs time on wide parents.
    /// Call it when the selection changes, not once per frame.
    pub fn path(&self, src: &[u8], id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = id;
        while cur != ROOT && cur != NONE {
            let n = self.node(cur);
            if n.kind == NodeKind::Element {
                let name = self.name(src, cur);
                // Position among same-named siblings. Stops one sibling after `cur`: that is
                // enough to know whether an index is needed at all.
                let mut before = 0usize;
                let mut others = false;
                let mut passed = false;
                for s in self.children(n.parent) {
                    if s == cur {
                        passed = true;
                    } else if self.node(s).kind == NodeKind::Element && self.name(src, s) == name {
                        others = true;
                        if passed {
                            break;
                        }
                        before += 1;
                    }
                }
                let mut part = String::from_utf8_lossy(name).into_owned();
                if others {
                    part.push_str(&format!("[{}]", before + 1));
                }
                parts.push(part);
            }
            cur = n.parent;
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }
}

pub struct Children<'a> {
    index: &'a Index,
    next: NodeId,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.next == NONE {
            return None;
        }
        let id = self.next;
        self.next = self.index.node(id).next_sibling;
        Some(id)
    }
}

fn unescape(raw: &[u8]) -> Cow<'_, str> {
    match String::from_utf8_lossy(raw) {
        Cow::Borrowed(b) => quick_xml::escape::unescape(b).unwrap_or(Cow::Borrowed(b)),
        Cow::Owned(o) => match quick_xml::escape::unescape(&o) {
            Ok(Cow::Owned(u)) => Cow::Owned(u),
            _ => Cow::Owned(o),
        },
    }
}

/// One frame per open element: the element and its most recent child.
struct Open {
    id: NodeId,
    last_child: NodeId,
}

struct Builder<'s> {
    src: &'s [u8],
    /// Bytes skipped before parsing starts (the byte order mark). quick-xml drops the mark
    /// without counting it, so every position it reports is this much too low.
    base: u64,
    nodes: Vec<Node>,
    stack: Vec<Open>,
    element_count: u32,
    max_depth: u16,
}

impl<'s> Builder<'s> {
    fn new(src: &'s [u8]) -> Self {
        // Rough guess: one node per 40 bytes avoids most reallocations on typical files.
        let mut nodes = Vec::with_capacity(src.len() / 40 + 1);
        nodes.push(Node {
            start: 0,
            end: src.len() as u64,
            parent: NONE,
            first_child: NONE,
            next_sibling: NONE,
            child_count: 0,
            head_len: 0,
            name_len: 0,
            depth: 0,
            kind: NodeKind::Document,
        });
        let base = if src.starts_with(UTF8_BOM) {
            UTF8_BOM.len() as u64
        } else {
            0
        };
        Self {
            src,
            base,
            nodes,
            stack: vec![Open {
                id: ROOT,
                last_child: NONE,
            }],
            element_count: 0,
            max_depth: 0,
        }
    }

    fn run(mut self) -> Index {
        let base = self.base;
        let mut reader = Reader::from_reader(&self.src[base as usize..]);
        reader.config_mut().check_end_names = true;
        let error = loop {
            let start = base + reader.buffer_position();
            let event = match reader.read_event() {
                Ok(e) => e,
                Err(e) => {
                    break Some(ParseError {
                        offset: base + reader.error_position(),
                        message: e.to_string(),
                    });
                }
            };
            let end = base + reader.buffer_position();
            match event {
                Event::Start(e) => {
                    let id = self.add(NodeKind::Element, start, end, e.name().as_ref().len());
                    self.stack.push(Open {
                        id,
                        last_child: NONE,
                    });
                }
                Event::Empty(e) => {
                    self.add(NodeKind::Element, start, end, e.name().as_ref().len());
                }
                Event::End(_) => {
                    if self.stack.len() <= 1 {
                        break Some(ParseError {
                            offset: start,
                            message: "unexpected end tag".into(),
                        });
                    }
                    let open = self.stack.pop().expect("checked above");
                    self.nodes[open.id as usize].end = end;
                }
                Event::Text(t) => {
                    if !self.extend_text(start, end) && !t.bytes().all(|b| b.is_ascii_whitespace())
                    {
                        self.add(NodeKind::Text, start, end, 0);
                    }
                }
                Event::GeneralRef(_) => {
                    if !self.extend_text(start, end) {
                        self.add(NodeKind::Text, start, end, 0);
                    }
                }
                Event::CData(_) => {
                    self.add(NodeKind::CData, start, end, 0);
                }
                Event::Comment(_) => {
                    self.add(NodeKind::Comment, start, end, 0);
                }
                Event::PI(_) => {
                    self.add(NodeKind::ProcessingInstruction, start, end, 0);
                }
                Event::Decl(_) => {
                    self.add(NodeKind::Declaration, start, end, 0);
                }
                Event::DocType(_) => {
                    self.add(NodeKind::DocType, start, end, 0);
                }
                Event::Eof => {
                    if self.stack.len() > 1 {
                        let open = self.stack.last().expect("len > 1");
                        let name = String::from_utf8_lossy(
                            &self.src[self.nodes[open.id as usize].name_span()],
                        );
                        break Some(ParseError {
                            offset: end,
                            message: format!("unclosed element <{name}>"),
                        });
                    }
                    break None;
                }
            }
        };
        // Close anything left open at the error so spans stay valid.
        let stop = error.as_ref().map_or(self.src.len() as u64, |e| e.offset);
        for open in self.stack.drain(1..) {
            self.nodes[open.id as usize].end = stop;
        }
        // The capacity guess can overshoot by up to 2x after growth; give the slack back.
        self.nodes.shrink_to_fit();
        Index {
            nodes: self.nodes,
            error,
            element_count: self.element_count,
            max_depth: self.max_depth,
        }
    }

    fn add(&mut self, kind: NodeKind, start: u64, end: u64, name_len: usize) -> NodeId {
        let id = self.nodes.len() as NodeId;
        let depth = self.stack.len() as u16;
        let top = self.stack.last_mut().expect("root frame is never popped");
        let parent = top.id;
        if top.last_child == NONE {
            self.nodes[parent as usize].first_child = id;
        } else {
            self.nodes[top.last_child as usize].next_sibling = id;
        }
        top.last_child = id;
        self.nodes[parent as usize].child_count += 1;
        if kind == NodeKind::Element {
            self.element_count += 1;
            self.max_depth = self.max_depth.max(depth);
        }
        self.nodes.push(Node {
            start,
            end,
            parent,
            first_child: NONE,
            next_sibling: NONE,
            child_count: 0,
            head_len: (end - start) as u32,
            name_len: name_len as u32,
            depth,
            kind,
        });
        id
    }

    /// Text arrives split around entity references (`a &amp; b` is three events). Merge them
    /// into the previous text node when they are contiguous.
    fn extend_text(&mut self, start: u64, end: u64) -> bool {
        let last = self.stack.last().expect("root frame").last_child;
        if last == NONE {
            return false;
        }
        let n = &mut self.nodes[last as usize];
        if n.kind == NodeKind::Text && n.end == start {
            n.end = end;
            n.head_len = (end - n.start) as u32;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &[u8] = br#"<?xml version="1.0"?>
<!-- catalog -->
<catalog>
  <book id="b1" lang="en &amp; de">
    <title>Rust &amp; XML</title>
    <price>12.50</price>
  </book>
  <book id="b2"/>
  <note><![CDATA[a < b]]></note>
</catalog>
"#;

    #[test]
    fn builds_tree() {
        let idx = index(DOC);
        assert!(idx.error.is_none(), "{:?}", idx.error);
        let root = idx.root_element().unwrap();
        assert_eq!(idx.name(DOC, root), b"catalog");
        let kids: Vec<_> = idx
            .children(root)
            .map(|c| idx.name(DOC, c).to_vec())
            .collect();
        assert_eq!(
            kids,
            vec![b"book".to_vec(), b"book".to_vec(), b"note".to_vec()]
        );
        assert_eq!(idx.element_count, 6);
        assert_eq!(idx.max_depth, 3);
        // The element span covers its end tag.
        assert!(idx.raw(DOC, root).ends_with(b"</catalog>"));
    }

    #[test]
    fn merges_text_around_entities() {
        let idx = index(DOC);
        let book = idx.children(idx.root_element().unwrap()).next().unwrap();
        let title = idx.children(book).next().unwrap();
        let text: Vec<_> = idx.children(title).collect();
        assert_eq!(text.len(), 1);
        assert_eq!(idx.text(DOC, text[0]), "Rust & XML");
    }

    #[test]
    fn decodes_attributes_lazily() {
        let idx = index(DOC);
        let book = idx.children(idx.root_element().unwrap()).next().unwrap();
        let attrs = idx.attributes(DOC, book);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].0, "id");
        assert_eq!(attrs[1].1, "en & de");
        let empty = idx.children(idx.root_element().unwrap()).nth(1).unwrap();
        assert_eq!(idx.attributes(DOC, empty)[0].1, "b2");
    }

    #[test]
    fn cdata_and_paths() {
        let idx = index(DOC);
        let root = idx.root_element().unwrap();
        let note = idx.children(root).nth(2).unwrap();
        let cdata = idx.children(note).next().unwrap();
        assert_eq!(idx.node(cdata).kind, NodeKind::CData);
        assert_eq!(idx.text(DOC, cdata), "a < b");
        let second_book = idx.children(root).nth(1).unwrap();
        assert_eq!(idx.path(DOC, second_book), "/catalog/book[2]");
        assert_eq!(idx.path(DOC, note), "/catalog/note");
    }

    #[test]
    fn keeps_partial_tree_on_error() {
        let src = b"<a><b>one</b><c>two</a>";
        let idx = index(src);
        let err = idx.error.as_ref().expect("mismatched end tag");
        assert!(err.offset > 0);
        let a = idx.root_element().unwrap();
        assert_eq!(idx.children(a).count(), 2);
        // Open elements are closed at the error position.
        assert!(idx.node(a).end <= src.len() as u64);
    }

    #[test]
    fn offsets_account_for_byte_order_mark() {
        let src = b"\xEF\xBB\xBF<a><b id=\"1\">x</b></a>";
        let idx = index(src);
        assert!(idx.error.is_none(), "{:?}", idx.error);
        let a = idx.root_element().unwrap();
        assert_eq!(idx.name(src, a), b"a");
        assert_eq!(idx.raw(src, a), b"<a><b id=\"1\">x</b></a>");
        let b = idx.children(a).next().unwrap();
        assert_eq!(
            idx.attributes(src, b),
            vec![("id".to_string(), "1".to_string())]
        );
        assert_eq!(idx.text(src, idx.children(b).next().unwrap()), "x");
    }

    #[test]
    fn error_offset_accounts_for_byte_order_mark() {
        let plain = index(b"<a><b></a>").error.unwrap().offset;
        let marked = index(b"\xEF\xBB\xBF<a><b></a>").error.unwrap().offset;
        assert_eq!(marked, plain + 3);
    }

    #[test]
    fn path_indexes_only_repeated_names() {
        let src = b"<r><x/><y/><x/><x/></r>";
        let idx = index(src);
        let kids: Vec<_> = idx.children(idx.root_element().unwrap()).collect();
        assert_eq!(idx.path(src, kids[0]), "/r/x[1]");
        assert_eq!(idx.path(src, kids[1]), "/r/y");
        assert_eq!(idx.path(src, kids[2]), "/r/x[2]");
        assert_eq!(idx.path(src, kids[3]), "/r/x[3]");
    }

    #[test]
    fn reports_unclosed_element() {
        let idx = index(b"<a><b></b>");
        assert!(idx.error.unwrap().message.contains("<a>"));
    }
}
