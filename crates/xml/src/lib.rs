//! Flat, span-based XML index.
//!
//! Instead of building a DOM with owned strings, [`index`] makes one pass over the source bytes
//! and records every node as a fixed-size [`Node`] in a single `Vec`. Names, attributes and text
//! are byte ranges into the source and are decoded only when a view asks for them. This keeps
//! opening large files close to the speed of the tokenizer itself and memory near
//! `32 bytes × node count`.
//!
//! Parsing never discards work: on malformed input the index holds every node read before the
//! error, and [`Index::error`] says where and why parsing stopped.

use std::borrow::Cow;
use std::ops::Range;

use quick_xml::Reader;
use quick_xml::encoding::EncodingError;
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

/// Text or CDATA kept inside its element instead of in a node of its own (D11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Folded {
    #[default]
    No,
    /// The element's whole content is one text node.
    Text,
    /// The element's whole content is one CDATA section.
    CData,
}

/// Longest element name the index can hold, in bytes.
pub const MAX_NAME_LEN: usize = u16::MAX as usize;

/// One node. Offsets are byte positions in the source.
///
/// Nodes are stored in document order, so a node's subtree is the run of nodes right after
/// it: the first child of `n` is `n + 1`, and the next sibling follows the subtree. Links,
/// child counts and depth are derived from that (`docs/decisions.md`, D11).
///
/// An element whose whole content is one text or CDATA section (`<price>12.50</price>`) has no
/// child node for it: [`Node::folded`] says so, and [`Index::text`] reads it. On typical
/// documents that is a third of all nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// Start of the node (the `<` for markup).
    pub start: u64,
    /// End of the node, exclusive. For elements this is after the end tag.
    pub end: u64,
    pub parent: NodeId,
    /// Number of nodes in the subtree below this one (children, their children, ...).
    pub descendants: u32,
    /// Length of the start tag (`<a x="1">`) for elements; equals the span for other kinds.
    pub head_len: u32,
    /// Length of the element name, which starts at `start + 1`. Zero for other kinds.
    pub name_len: u16,
    pub kind: NodeKind,
    /// The element's content, when it is one text or CDATA section without a node.
    pub folded: Folded,
}

// The index is the memory limit (D11): keep the node at 32 bytes.
const _: () = assert!(std::mem::size_of::<Node>() == 32);

impl Node {
    pub fn has_children(&self) -> bool {
        self.descendants > 0
    }

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
    /// Deepest element nesting; the root element is 1. After [`Index::update`] it can be
    /// higher than the document now has (an upper bound), until the next full index.
    pub max_depth: u16,
}

/// Builds the index for `src`. Never fails; see [`Index::error`].
pub fn index(src: &[u8]) -> Index {
    Builder::new(src).run()
}

impl Index {
    /// Updates the index after `removed` bytes at `at` were replaced by `inserted` bytes;
    /// `src` is the source after the change. Only the nodes around the change are indexed
    /// again and spliced in (`docs/decisions.md`, D9), so the cost follows them plus one pass
    /// to shift the nodes after them:
    ///
    /// - when the change lies between the children of the innermost element around it, only
    ///   the children it touches (moving, deleting or adding elements under a large parent);
    /// - otherwise that whole element (its start tag, its text).
    ///
    /// Returns false, with the index unchanged, when that is not possible: the document
    /// already had an error, no element contains the change with a byte to spare on both
    /// sides, or the new bytes do not parse on their own. The caller then indexes the whole
    /// document.
    pub fn update(&mut self, src: &[u8], at: usize, removed: usize, inserted: usize) -> bool {
        if self.error.is_some() {
            return false;
        }
        let (at, old_end) = (at as u64, (at + removed) as u64);
        let delta = inserted as i64 - removed as i64;
        let Some(e) = self.innermost_around(at, old_end) else {
            return false;
        };
        if let Some(done) = self.update_children(src, e, at, old_end, delta) {
            return done;
        }
        let old = self.nodes[e as usize];
        let Some(new_end) = old.end.checked_add_signed(delta) else {
            return false;
        };
        let Some(bytes) = src.get(old.start as usize..new_end as usize) else {
            return false;
        };
        let frag = index(bytes);
        let whole = frag.nodes.get(1).is_some_and(|n| {
            n.kind == NodeKind::Element
                && n.start == 0
                && n.end == bytes.len() as u64
                && n.descendants as usize + 2 == frag.nodes.len()
        });
        if frag.error.is_some() || !whole {
            return false;
        }
        let after = e as usize + 1 + old.descendants as usize;
        self.splice_nodes(old.parent, e as usize..after, old.start, &frag, delta);
        true
    }

    /// The children of element `e` that the change at `at..old_end` touches, indexed again:
    /// everything from the end of the last child before the change to the start of the first
    /// child after it. `None` when this does not apply and the whole element must be read;
    /// `Some(false)` when the new bytes do not parse.
    fn update_children(
        &mut self,
        src: &[u8],
        e: NodeId,
        at: u64,
        old_end: u64,
        delta: i64,
    ) -> Option<bool> {
        let n = self.nodes[e as usize];
        let head_end = n.start + n.head_len as u64;
        if !n.has_children() || at < head_end {
            return None;
        }
        let mut before = None;
        let mut first_after = None;
        for c in self.children(e) {
            let k = self.node(c);
            if k.end <= at {
                before = Some(c);
            } else if k.start >= old_end {
                first_after = Some(c);
                break;
            }
        }
        // Every child touched: reading the element is as cheap, and it folds text (D11).
        if before.is_none() && first_after.is_none() {
            return None;
        }
        // Text next to the change could merge with new text, and a text or CDATA section left
        // alone would be folded into the element (D11); reading the element decides both.
        let is_text = |id: Option<NodeId>| {
            id.is_some_and(|c| matches!(self.node(c).kind, NodeKind::Text | NodeKind::CData))
        };
        if is_text(before) || is_text(first_after) {
            return None;
        }
        let end_id = e as usize + 1 + n.descendants as usize;
        let lo = before.map_or(e as usize + 1, |b| {
            b as usize + 1 + self.node(b).descendants as usize
        });
        let hi = first_after.map_or(end_id, |c| c as usize);
        let from = before.map_or(head_end, |b| self.node(b).end);
        // Up to the next child, or to the element's end tag. The end tag is found in the new
        // source; it must lie wholly after the inserted bytes, or the change touched it.
        let (to, new_to) = match first_after {
            Some(c) => {
                let t = self.node(c).start;
                (t, t.checked_add_signed(delta)?)
            }
            None => {
                let new_end = n.end.checked_add_signed(delta)?;
                let span = src.get(n.start as usize..new_end as usize)?;
                let new_to = n.start + memchr::memrchr(b'<', span)? as u64;
                if new_to < at + (old_end - at).wrapping_add_signed(delta) {
                    return None;
                }
                (new_to.checked_add_signed(-delta)?, new_to)
            }
        };
        if old_end > to {
            return None;
        }
        let bytes = src.get(from as usize..new_to as usize)?;
        let frag = index(bytes);
        if frag.error.is_some() {
            return Some(false);
        }
        self.splice_nodes(e, lo..hi, from, &frag, delta);
        Some(true)
    }

    /// Replaces nodes `old` (whole subtrees, children of `parent`) with the nodes of `frag`,
    /// whose top-level nodes become children of `parent` and whose offsets start at `base`.
    /// Shifts everything after by `delta` bytes and adjusts `parent` and its ancestors.
    fn splice_nodes(
        &mut self,
        parent: NodeId,
        old: Range<usize>,
        base: u64,
        frag: &Index,
        delta: i64,
    ) {
        let old_elements = self.nodes[old.clone()]
            .iter()
            .filter(|n| n.kind == NodeKind::Element)
            .count() as u32;
        let new_count = frag.nodes.len() - 1;
        let dn = new_count as i64 - old.len() as i64;
        let depth = if parent == NONE {
            0
        } else {
            self.depth(parent)
        };
        // Nodes after move by `delta` bytes and `dn` ids; parents before the splice stay.
        for n in &mut self.nodes[old.end..] {
            n.start = n.start.wrapping_add_signed(delta);
            n.end = n.end.wrapping_add_signed(delta);
            if n.parent != NONE && n.parent as usize >= old.end {
                n.parent = (n.parent as i64 + dn) as NodeId;
            }
        }
        // The ancestors end later or earlier and hold more or fewer nodes.
        let mut p = parent;
        while p != NONE {
            let n = &mut self.nodes[p as usize];
            n.end = n.end.wrapping_add_signed(delta);
            n.descendants = (n.descendants as i64 + dn) as u32;
            p = n.parent;
        }
        let first = old.start as NodeId;
        let spliced = frag.nodes[1..].iter().map(|n| Node {
            start: n.start + base,
            end: n.end + base,
            parent: if n.parent == ROOT {
                parent
            } else {
                first + n.parent - 1
            },
            ..*n
        });
        self.nodes.splice(old, spliced);
        self.element_count = self.element_count - old_elements + frag.element_count;
        self.max_depth = self.max_depth.max(depth + frag.max_depth);
    }

    /// The innermost element whose span contains `at..end` with at least one byte of the
    /// element before and after it, so that its start and end stay where they were.
    fn innermost_around(&self, at: u64, end: u64) -> Option<NodeId> {
        let mut cur = ROOT;
        'descend: loop {
            for c in self.children(cur) {
                let n = self.node(c);
                if n.start >= at {
                    break;
                }
                if n.kind == NodeKind::Element && end < n.end {
                    cur = c;
                    continue 'descend;
                }
            }
            break;
        }
        (cur != ROOT).then_some(cur)
    }

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
        let end = id as u64 + 1 + self.node(id).descendants as u64;
        Children {
            index: self,
            next: id as u64 + 1,
            end,
        }
    }

    /// Number of children of `id`. Walks them.
    pub fn child_count(&self, id: NodeId) -> usize {
        self.children(id).count()
    }

    /// The sibling after `id`, if any.
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.node(id).parent;
        if parent == NONE {
            return None;
        }
        let next = id as u64 + 1 + self.node(id).descendants as u64;
        let end = parent as u64 + 1 + self.node(parent).descendants as u64;
        (next < end).then_some(next as NodeId)
    }

    /// Number of ancestors of `id`: 0 for the document, 1 for the root element and other
    /// top-level nodes. Walks up the tree.
    pub fn depth(&self, id: NodeId) -> u16 {
        let mut depth = 0;
        let mut cur = self.node(id).parent;
        while cur != NONE {
            depth += 1;
            cur = self.node(cur).parent;
        }
        depth
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

    /// Byte range of each attribute of an element, from the first byte of its name to the
    /// closing quote of its value, in the order of [`Index::attributes`]. Lets a view
    /// highlight one attribute in the source.
    pub fn attribute_spans(&self, src: &[u8], id: NodeId) -> Vec<Range<usize>> {
        let n = self.node(id);
        if n.kind != NodeKind::Element {
            return Vec::new();
        }
        let start = n.start as usize;
        let mut end = start + n.head_len as usize;
        // Drop the closing `>` or `/>`.
        end = end.saturating_sub(1);
        if end > start && src[end - 1] == b'/' {
            end -= 1;
        }
        let ws = |b: u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
        let mut spans = Vec::new();
        let mut i = start + 1 + n.name_len as usize;
        loop {
            while i < end && ws(src[i]) {
                i += 1;
            }
            let name = i;
            while i < end && !ws(src[i]) && src[i] != b'=' {
                i += 1;
            }
            while i < end && ws(src[i]) {
                i += 1;
            }
            if i >= end || src[i] != b'=' || i == name {
                break;
            }
            i += 1;
            while i < end && ws(src[i]) {
                i += 1;
            }
            let Some(&quote) = src
                .get(i)
                .filter(|&&q| i < end && (q == b'"' || q == b'\''))
            else {
                break;
            };
            i += 1;
            while i < end && src[i] != quote {
                i += 1;
            }
            if i >= end {
                break;
            }
            i += 1;
            spans.push(name..i);
        }
        spans
    }

    /// Text content of a text or CDATA node, or the folded text of an element, unescaped.
    pub fn text<'s>(&self, src: &'s [u8], id: NodeId) -> Cow<'s, str> {
        let n = self.node(id);
        let (kind, span) = match (n.kind, n.folded, self.folded_span(src, id)) {
            (NodeKind::Element, Folded::Text, Some(span)) => (NodeKind::Text, span),
            (NodeKind::Element, Folded::CData, Some(span)) => (NodeKind::CData, span),
            (kind, _, _) => (kind, n.span()),
        };
        let raw = &src[span];
        match kind {
            NodeKind::Text => unescape(raw),
            NodeKind::CData => String::from_utf8_lossy(&raw[9..raw.len() - 3]),
            _ => String::from_utf8_lossy(raw),
        }
    }

    /// Byte range of an element's folded text or CDATA section: everything between the start
    /// tag and the end tag. `None` when nothing is folded.
    pub fn folded_span(&self, src: &[u8], id: NodeId) -> Option<Range<usize>> {
        let n = self.node(id);
        if n.folded == Folded::No {
            return None;
        }
        let start = n.start as usize + n.head_len as usize;
        // Text holds no `<`, and a CDATA section ends before the end tag: the last `<` of the
        // element starts its end tag.
        let end = memchr::memrchr(b'<', &src[start..n.end as usize])? + start;
        Some(start..end)
    }

    /// Whether element `id` holds text or CDATA, folded or as children.
    pub fn has_text(&self, id: NodeId) -> bool {
        self.node(id).folded != Folded::No
            || self
                .children(id)
                .any(|c| matches!(self.node(c).kind, NodeKind::Text | NodeKind::CData))
    }

    /// Whether the only content of element `id` is one text or CDATA section.
    pub fn is_text_only(&self, id: NodeId) -> bool {
        let mut kids = self.children(id);
        match (kids.next(), kids.next()) {
            (None, _) => self.node(id).folded != Folded::No,
            (Some(only), None) => {
                matches!(self.node(only).kind, NodeKind::Text | NodeKind::CData)
            }
            _ => false,
        }
    }

    /// The text of element `id`: its folded text, or its text and CDATA children joined.
    pub fn element_text<'s>(&self, src: &'s [u8], id: NodeId) -> Cow<'s, str> {
        if self.node(id).folded != Folded::No {
            return self.text(src, id);
        }
        let mut out = Cow::Borrowed("");
        for c in self.children(id) {
            if matches!(self.node(c).kind, NodeKind::Text | NodeKind::CData) {
                let t = self.text(src, c);
                if out.is_empty() {
                    out = t;
                } else {
                    out.to_mut().push_str(&t);
                }
            }
        }
        out
    }

    /// Where element `id` sits: its position among the element children of its parent, for
    /// each level from the top. Comments, text and whitespace do not count, so the position
    /// survives Format and Remove comments; [`Index::element_at_position`] finds the element
    /// again in a new index. Walks the siblings before `id` at every level.
    pub fn element_position(&self, id: NodeId) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = id;
        while cur != ROOT && cur != NONE {
            let n = self.node(cur);
            if n.kind != NodeKind::Element {
                return Vec::new();
            }
            let before = self
                .children(n.parent)
                .take_while(|&c| c != cur)
                .filter(|&c| self.node(c).kind == NodeKind::Element)
                .count();
            out.push(before as u32);
            cur = n.parent;
        }
        out.reverse();
        out
    }

    /// The element at `position` (see [`Index::element_position`]), if there is one.
    pub fn element_at_position(&self, position: &[u32]) -> Option<NodeId> {
        if position.is_empty() {
            return None;
        }
        let mut cur = ROOT;
        for &p in position {
            cur = self
                .children(cur)
                .filter(|&c| self.node(c).kind == NodeKind::Element)
                .nth(p as usize)?;
        }
        Some(cur)
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
    /// The next child, or `end`. `u64` so that the end of the last subtree cannot overflow.
    next: u64,
    /// One past the parent's subtree.
    end: u64,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.next >= self.end {
            return None;
        }
        let id = self.next as NodeId;
        self.next += 1 + self.index.node(id).descendants as u64;
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

struct Builder<'s> {
    src: &'s [u8],
    /// Bytes skipped before parsing starts (the byte order mark). quick-xml drops the mark
    /// without counting it, so every position it reports is this much too low.
    base: u64,
    nodes: Vec<Node>,
    /// The open elements, innermost last; the document at the bottom.
    stack: Vec<NodeId>,
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
            descendants: 0,
            head_len: 0,
            name_len: 0,
            kind: NodeKind::Document,
            folded: Folded::No,
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
            stack: vec![ROOT],
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
                Err(e) => break Some(parse_error(e, start, base + reader.error_position())),
            };
            let end = base + reader.buffer_position();
            match event {
                Event::Start(e) => {
                    let name_len = e.name().as_ref().len();
                    if name_len > MAX_NAME_LEN {
                        break Some(name_too_long(start));
                    }
                    let id = self.add(NodeKind::Element, start, end, name_len);
                    self.stack.push(id);
                }
                Event::Empty(e) => {
                    let name_len = e.name().as_ref().len();
                    if name_len > MAX_NAME_LEN {
                        break Some(name_too_long(start));
                    }
                    self.add(NodeKind::Element, start, end, name_len);
                }
                Event::End(_) => {
                    if self.stack.len() <= 1 {
                        break Some(ParseError {
                            offset: start,
                            message: "unexpected end tag".into(),
                        });
                    }
                    let open = self.stack.pop().expect("checked above");
                    self.fold(open, start);
                    self.close(open, end);
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
                        let open = *self.stack.last().expect("len > 1");
                        let name = String::from_utf8_lossy(
                            &self.src[self.nodes[open as usize].name_span()],
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
        while self.stack.len() > 1 {
            let open = self.stack.pop().expect("len > 1");
            self.close(open, stop);
        }
        self.close(ROOT, self.src.len() as u64);
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
        let parent = *self.stack.last().expect("root frame is never popped");
        if kind == NodeKind::Element {
            self.element_count += 1;
            self.max_depth = self.max_depth.max(depth);
        }
        self.nodes.push(Node {
            start,
            end,
            parent,
            descendants: 0,
            head_len: (end - start) as u32,
            name_len: name_len as u16,
            kind,
            folded: Folded::No,
        });
        id
    }

    /// Folds the content of element `id` into it when that is one text or CDATA node filling
    /// everything up to the end tag at `end_tag`. Runs before [`Builder::close`].
    fn fold(&mut self, id: NodeId, end_tag: u64) {
        if self.nodes.len() != id as usize + 2 {
            return;
        }
        let e = self.nodes[id as usize];
        let t = self.nodes[id as usize + 1];
        let folded = match t.kind {
            NodeKind::Text => Folded::Text,
            NodeKind::CData => Folded::CData,
            _ => return,
        };
        if t.start == e.start + e.head_len as u64 && t.end == end_tag {
            self.nodes.pop();
            self.nodes[id as usize].folded = folded;
        }
    }

    /// Ends element `id` at byte `end`; its subtree is everything added since.
    fn close(&mut self, id: NodeId, end: u64) {
        let descendants = (self.nodes.len() - 1 - id as usize) as u32;
        let n = &mut self.nodes[id as usize];
        n.end = end;
        n.descendants = descendants;
    }

    /// Text arrives split around entity references (`a &amp; b` is three events). Merge them
    /// into the previous text node when they are contiguous. A text node has no children, so
    /// when the newest node is text that ends where this piece starts, it is the previous
    /// sibling.
    fn extend_text(&mut self, start: u64, end: u64) -> bool {
        let Some(n) = self.nodes.last_mut() else {
            return false;
        };
        if n.kind == NodeKind::Text && n.end == start {
            n.end = end;
            n.head_len = (end - n.start) as u32;
            return true;
        }
        false
    }
}

#[cold]
#[inline(never)]
fn name_too_long(offset: u64) -> ParseError {
    ParseError {
        offset,
        message: format!("element name longer than {MAX_NAME_LEN} bytes"),
    }
}

/// The error to report for `e`, which happened in the event that starts at byte `start`.
/// `position` is quick-xml's error position. Kept out of the parse loop: it runs once.
#[cold]
#[inline(never)]
fn parse_error(e: quick_xml::Error, start: u64, position: u64) -> ParseError {
    // quick-xml does not set the error position for bytes that are not UTF-8, but the error
    // says how far into the failing event they were valid.
    let offset = match &e {
        quick_xml::Error::Encoding(EncodingError::Utf8(u)) => start + u.valid_up_to() as u64,
        _ => position,
    };
    ParseError {
        offset,
        message: e.to_string(),
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

    /// Bytes that are not UTF-8 are reported where they are, not at the start of the file.
    #[test]
    fn invalid_utf8_error_points_at_the_bad_byte() {
        let src = b"<?xml version=\"1.0\"?><r>Gr\xF6\xDFe</r>";
        let err = index(src).error.expect("an error");
        assert_eq!(
            err.offset as usize,
            src.iter().position(|&b| b == 0xF6).unwrap()
        );
        let src = b"<r>\n<n a=\"\xC4pfel\"/></r>";
        let err = index(src).error.expect("an error");
        assert_eq!(
            err.offset as usize,
            src.iter().position(|&b| b == 0xC4).unwrap()
        );
    }

    #[test]
    fn attribute_spans_cover_name_to_closing_quote() {
        let src = "<r><a x=\"1\"  y = 'zwei'\n   z=\"ä>b\"/><b/><c k=\"v\">t</c></r>".as_bytes();
        let idx = index(src);
        let a = idx.children(idx.root_element().unwrap()).next().unwrap();
        let spans: Vec<&str> = idx
            .attribute_spans(src, a)
            .into_iter()
            .map(|r| std::str::from_utf8(&src[r]).unwrap())
            .collect();
        assert_eq!(spans, ["x=\"1\"", "y = 'zwei'", "z=\"ä>b\""]);
        assert_eq!(spans.len(), idx.attributes(src, a).len());

        let mut kids = idx.children(idx.root_element().unwrap()).skip(1);
        let b = kids.next().unwrap();
        assert!(idx.attribute_spans(src, b).is_empty());
        let c = kids.next().unwrap();
        let k = idx.attribute_spans(src, c);
        assert_eq!(&src[k[0].clone()], b"k=\"v\"");
        // Folded text does not disturb the attributes.
        assert_eq!(idx.node(c).folded, Folded::Text);
        assert_eq!(idx.text(src, c), "t");
    }

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
        assert_eq!(idx.children(title).count(), 0, "folded");
        assert_eq!(idx.text(DOC, title), "Rust & XML");
        // Not folded: mixed content keeps its nodes, merged around the entity.
        let src = b"<p>a &amp; b<br/>c</p>";
        let idx = index(src);
        let p = idx.root_element().unwrap();
        let kids: Vec<_> = idx.children(p).collect();
        assert_eq!(kids.len(), 3);
        assert_eq!(idx.text(src, kids[0]), "a & b");
        assert_eq!(idx.element_text(src, p), "a & bc");
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
        assert_eq!(idx.node(note).folded, Folded::CData);
        assert_eq!(idx.text(DOC, note), "a < b");
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
        assert_eq!(idx.text(src, b), "x");
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

    /// Children, siblings and depth derived from subtree sizes agree with the parent links,
    /// also for a tree cut short by an error.
    fn check_links(src: &[u8]) {
        let idx = index(src);
        let mut kids = vec![Vec::new(); idx.len()];
        for id in 1..idx.len() as NodeId {
            kids[idx.node(id).parent as usize].push(id);
        }
        for id in 0..idx.len() as NodeId {
            let k = &kids[id as usize];
            assert_eq!(idx.children(id).collect::<Vec<_>>(), *k, "children of {id}");
            assert_eq!(idx.child_count(id), k.len());
            assert_eq!(idx.node(id).has_children(), !k.is_empty());
            for w in k.windows(2) {
                assert_eq!(idx.next_sibling(w[0]), Some(w[1]));
            }
            if let Some(&last) = k.last() {
                assert_eq!(idx.next_sibling(last), None);
            }
            let p = idx.node(id).parent;
            if p != NONE {
                assert_eq!(idx.depth(id), idx.depth(p) + 1);
            }
        }
        assert_eq!(idx.depth(ROOT), 0);
        assert_eq!(idx.next_sibling(ROOT), None);
    }

    #[test]
    fn links_follow_from_subtree_sizes() {
        check_links(DOC);
        check_links(b"<a><b>one</b><c>two<d/></a>");
        check_links(b"<a><b><c>x");
        check_links(b"<?xml version=\"1.0\"?><!--c--><r/><!--after-->");
        check_links(b"");
        let deep = format!("{}{}", "<x>".repeat(500), "</x>".repeat(500));
        check_links(deep.as_bytes());
        let idx = index(deep.as_bytes());
        assert_eq!(idx.depth(idx.len() as NodeId - 1), 500);
    }

    /// The real-world files of the engine's corpus: byte order marks, UTF-16, errors, deep
    /// nesting, one huge line.
    #[test]
    fn links_in_the_corpus() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/corpus");
        let mut n = 0;
        for entry in std::fs::read_dir(dir).expect("corpus") {
            let path = entry.expect("entry").path();
            check_links(&std::fs::read(&path).expect("read"));
            n += 1;
        }
        assert!(n > 5, "corpus not found");
    }

    #[test]
    fn refuses_names_the_index_cannot_hold() {
        let ok = format!("<{}/>", "n".repeat(MAX_NAME_LEN));
        let idx = index(ok.as_bytes());
        assert!(idx.error.is_none());
        assert_eq!(idx.name(ok.as_bytes(), 1).len(), MAX_NAME_LEN);
        let long = format!("<r><{0}>x</{0}></r>", "n".repeat(MAX_NAME_LEN + 1));
        let idx = index(long.as_bytes());
        let err = idx.error.as_ref().expect("too long");
        assert_eq!(err.offset, 3);
        assert!(err.message.contains("longer than"));
        // The tree up to the error is kept.
        assert_eq!(idx.name(long.as_bytes(), idx.root_element().unwrap()), b"r");
    }

    /// Only content that is exactly one text or CDATA section is folded; the text reads the
    /// same either way.
    #[test]
    fn folds_only_whole_text_content() {
        let src = "<r><a>Grüße &lt;3</a><b> <![CDATA[x</y>]]> </b><c><![CDATA[]]></c>\
                   <d>t<!--c--></d><e/><f></f><g>  </g><h>x<i/></h></r>"
            .as_bytes();
        let idx = index(src);
        assert!(idx.error.is_none(), "{:?}", idx.error);
        let kids: Vec<_> = idx.children(idx.root_element().unwrap()).collect();
        let folded: Vec<_> = kids.iter().map(|&k| idx.node(k).folded).collect();
        use Folded::*;
        assert_eq!(folded, [Text, No, CData, No, No, No, No, No]);
        assert_eq!(idx.text(src, kids[0]), "Grüße <3");
        assert_eq!(
            &src[idx.folded_span(src, kids[0]).unwrap()],
            "Grüße &lt;3".as_bytes()
        );
        assert_eq!(idx.element_text(src, kids[1]), "x</y>");
        assert_eq!(idx.text(src, kids[2]), "");
        assert_eq!(idx.element_text(src, kids[3]), "t");
        let only: Vec<_> = kids.iter().map(|&k| idx.is_text_only(k)).collect();
        assert_eq!(only, [true, true, true, false, false, false, false, false]);
        let text: Vec<_> = kids.iter().map(|&k| idx.has_text(k)).collect();
        assert_eq!(text, [true, true, true, true, false, false, false, true]);
        assert_eq!(idx.folded_span(src, kids[1]), None);
    }

    /// An element cut short by an error keeps its text as a node.
    #[test]
    fn does_not_fold_at_an_error() {
        let src = b"<r><a>text";
        let idx = index(src);
        let a = idx.children(idx.root_element().unwrap()).next().unwrap();
        assert_eq!(idx.node(a).folded, Folded::No);
        assert_eq!(idx.element_text(src, a), "text");
    }

    #[test]
    fn element_positions_ignore_comments_and_text() {
        let a = index(DOC);
        let b = index(b"<catalog><book/><!-- x --><book/>text<note/></catalog>");
        let note_a = a.children(a.root_element().unwrap()).nth(2).unwrap();
        let pos = a.element_position(note_a);
        assert_eq!(pos, [0, 2]);
        let note_b = b.element_at_position(&pos).unwrap();
        assert_eq!(
            b.name(
                b"<catalog><book/><!-- x --><book/>text<note/></catalog>",
                note_b
            ),
            b"note"
        );
        assert_eq!(a.element_position(a.root_element().unwrap()), [0]);
        assert_eq!(b.element_at_position(&[0, 3]), None);
        assert_eq!(b.element_at_position(&[]), None);
        assert_eq!(a.element_position(ROOT), Vec::<u32>::new());
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

    const SNIPPETS: &[&str] = &[
        "<x/>",
        "<y a=\"1\">t</y>",
        "text",
        "&amp;",
        "</",
        "<",
        "\"",
        "\r\n",
        "<!--c-->",
        "<![CDATA[z]]>",
        "é",
        " ",
        "\n",
        "<b>",
        "</b>",
        "=",
        "q",
    ];

    /// Applies random single changes to `src` and checks that whenever [`Index::update`]
    /// accepts one, the result equals a full index of the new source. Returns how many it
    /// accepted.
    fn check_updates(mut src: Vec<u8>, seed: u64, rounds: usize) -> usize {
        let mut rng = Lcg(seed);
        let mut idx = index(&src);
        let mut accepted = 0;
        for _ in 0..rounds {
            let at = rng.below(src.len() + 1);
            let removed = rng.below(7).min(src.len() - at);
            let inserted: &[u8] = if rng.below(3) == 0 {
                b""
            } else {
                SNIPPETS[rng.below(SNIPPETS.len())].as_bytes()
            };
            let old: Vec<u8> = src
                .splice(at..at + removed, inserted.iter().copied())
                .collect();
            let full = index(&src);
            if idx.update(&src, at, removed, inserted.len()) {
                accepted += 1;
                assert_eq!(
                    idx.nodes, full.nodes,
                    "seed {seed}: at {at} -{removed} +{inserted:?}"
                );
                assert_eq!(idx.element_count, full.element_count);
                assert!(idx.error.is_none() && full.error.is_none());
                assert!(idx.max_depth >= full.max_depth);
            } else if full.error.is_some() && idx.error.is_none() {
                // Keep editing a well-formed document: take back a change that broke it.
                src.splice(at..at + inserted.len(), old);
            } else {
                idx = full;
            }
        }
        accepted
    }

    #[test]
    fn updates_match_a_full_index() {
        let mut accepted = 0;
        let mut total = 0;
        for seed in 0..20 {
            accepted += check_updates(DOC.to_vec(), seed, 200);
            total += 200;
        }
        // Many siblings under one parent, some with text or CDATA between them: edits between
        // children take the shorter path.
        let siblings = b"<r>\n  <a x=\"1\">t</a>\n  <b/>text<c><![CDATA[d]]></c>\n  <![CDATA[e]]><f/>\n  <!--g-->\n  <h><i/></h>\n</r>\n";
        for seed in 0..20 {
            accepted += check_updates(siblings.to_vec(), 100 + seed, 200);
            total += 200;
        }
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/corpus");
        for (i, entry) in std::fs::read_dir(dir).expect("corpus").enumerate() {
            let src = std::fs::read(entry.expect("entry").path()).expect("read");
            accepted += check_updates(src, i as u64, 200);
            total += 200;
        }
        // Most random edits break the document; enough must take the fast path to test it.
        assert!(
            accepted * 10 > total,
            "only {accepted} of {total} updates were local"
        );
    }

    #[test]
    fn update_handles_typical_edits_locally() {
        let src = DOC.to_vec();
        let edits: &[(&[u8], &[u8])] = &[
            (b"12.50", b"13"),              // folded text
            (b"<price>", b"<cost>"),        // start tag only: not well-formed alone
            (b"lang=\"en &amp; de\"", b""), // an attribute
            (b"<book id=\"b2\"/>", b""),    // a whole element
            (b"a < b", b"a <<< b"),         // inside CDATA
        ];
        let mut local = Vec::new();
        for (from, to) in edits {
            let at = src.windows(from.len()).position(|w| w == *from).unwrap();
            let mut new = src.clone();
            new.splice(at..at + from.len(), to.iter().copied());
            let mut idx = index(&src);
            let ok = idx.update(&new, at, from.len(), to.len());
            if ok {
                assert_eq!(idx.nodes, index(&new).nodes);
            }
            local.push(ok);
        }
        assert_eq!(local, [true, false, true, true, true]);
        // An index with an error always needs the full pass.
        let mut broken = index(b"<a><b></a>");
        assert!(!broken.update(b"<a><c></a>", 4, 1, 1));
    }

    #[test]
    fn reports_unclosed_element() {
        let idx = index(b"<a><b></b>");
        assert!(idx.error.unwrap().message.contains("<a>"));
    }
}
