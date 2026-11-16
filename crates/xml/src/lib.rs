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
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use quick_xml::Reader;
use quick_xml::encoding::EncodingError;
use quick_xml::events::{BytesStart, Event};

mod scan;

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

/// Errors kept per document. Parsing goes on after the last one (OPEN-10); a file of noise
/// does not fill memory with messages.
pub const MAX_ERRORS: usize = 1000;

/// A parent with at least this many nodes below it keeps a list of its child elements
/// ([`Index::child_elements`], D24): walking its children reads one far-apart node each.
pub const WIDE_DESCENDANTS: u32 = 10_000;

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

/// Which node ids the last local change replaced (`Index::update`, `Index::keep_stale`):
/// the `removed` ids from `at` on, whole subtrees under `parent`, became `inserted` new ones.
/// Ids before `at` stay; ids after the replaced ones move by `inserted - removed`. Views use
/// it to follow a change without building their state again (D19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeSplice {
    pub parent: NodeId,
    pub at: NodeId,
    pub removed: u32,
    pub inserted: u32,
}

impl NodeSplice {
    /// Where node `id` of before the change is now; `None` when it was replaced.
    pub fn map(&self, id: NodeId) -> Option<NodeId> {
        if id < self.at {
            Some(id)
        } else if id - self.at < self.removed {
            None
        } else {
            Some(id - self.removed + self.inserted)
        }
    }

    /// The ids the replaced ones became.
    pub fn new_ids(&self) -> Range<NodeId> {
        self.at..self.at + self.inserted
    }
}

/// The child elements of a wide parent, in document order (D24).
#[derive(Debug, PartialEq, Eq)]
pub struct ChildElements {
    pub ids: Vec<NodeId>,
    /// Whether every child element has the same name.
    pub one_name: bool,
    /// Whether any child is text or CDATA (mixed content).
    pub text: bool,
}

/// [`ChildElements`] by parent, made on demand and dropped whenever the nodes change. A
/// clone starts empty.
#[derive(Debug, Default)]
struct WideChildren(Mutex<HashMap<NodeId, Arc<ChildElements>>>);

impl Clone for WideChildren {
    fn clone(&self) -> Self {
        Self::default()
    }
}

/// Result of [`index`].
#[derive(Clone, Debug)]
pub struct Index {
    pub nodes: Vec<Node>,
    /// The first error. Parsing recovers after it (OPEN-10), so nodes after it are there too.
    pub error: Option<ParseError>,
    /// The errors after the first, in document order, up to [`MAX_ERRORS`] in all.
    pub more_errors: Vec<ParseError>,
    pub element_count: u32,
    /// Deepest element nesting; the root element is 1. After [`Index::update`] it can be
    /// higher than the document now has (an upper bound), until the next full index.
    pub max_depth: u16,
    /// What the last local change ([`Index::update`], [`Index::keep_stale`]) replaced;
    /// `None` for a fresh index. Callers clear it before a change to tell whether that
    /// change was local.
    pub last_splice: Option<NodeSplice>,
    wide: WideChildren,
}

/// Builds the index for `src`. Never fails; see [`Index::error`].
///
/// A well-formed UTF-8 document is read by the byte scanner of D14; anything it is not sure
/// of, including every error, by quick-xml from the start.
pub fn index(src: &[u8]) -> Index {
    let mut fast = Builder::new(src);
    if fast.scan() {
        return fast.finish(Vec::new());
    }
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
        self.wide = WideChildren::default();
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

    /// Keeps the nodes after a change that [`Index::update`] could not take in, because the
    /// text around it is not well-formed (`docs/decisions.md`, D19; TEXT-10). `src` is the
    /// source after `removed` bytes at `at` became `inserted` bytes.
    ///
    /// Nodes after the change move with it, and the elements around it end that much later
    /// or earlier. Nodes whose own bytes the change touched are dropped, so that every node
    /// left reads bytes that are its own. The change lies inside the element it returns: the
    /// innermost one whose `<` and end tag it left alone ([`ROOT`] when there is none). That
    /// element keeps its children the change did not touch; its start tag is measured again
    /// when the change was there (and so is any start tag around it the change reaches).
    /// `stale`, an element an earlier change left stale, is
    /// merged in: the result is the innermost element around both.
    ///
    /// Nodes are not read again, so tree and grid show the last well-formed state of the
    /// stale element until the whole document is indexed again.
    pub fn keep_stale(
        &mut self,
        src: &[u8],
        at: usize,
        removed: usize,
        inserted: usize,
        stale: Option<NodeId>,
    ) -> NodeId {
        self.wide = WideChildren::default();
        let (at, old_end) = (at as u64, (at + removed) as u64);
        let new_change_end = at + inserted as u64;
        let delta = inserted as i64 - removed as i64;
        // Bytes from `old_end` on are unchanged, from `new_change_end` on in `src`.
        let tail_intact = |n: &Node| {
            let Some(new_end) = n.end.checked_add_signed(delta) else {
                return false;
            };
            let Some(tail) = src.get(new_change_end as usize..new_end as usize) else {
                return false;
            };
            if n.head_len as u64 == n.end - n.start {
                // `<a .../>`: its `/>` is intact.
                tail.len() >= 2
            } else {
                // The end tag's `<` is the last of the element; it is intact when it lies in
                // the unchanged tail.
                memchr::memrchr(b'<', tail).is_some()
            }
        };
        let mut s = ROOT;
        'descend: loop {
            for c in self.children(s) {
                let n = self.node(c);
                if n.start >= at {
                    break;
                }
                if n.kind == NodeKind::Element && old_end < n.end && tail_intact(n) {
                    s = c;
                    continue 'descend;
                }
            }
            break;
        }
        // The children of `s` the change touched are contiguous; drop them.
        let mut first = None;
        let mut past = None;
        for c in self.children(s) {
            let n = self.node(c);
            let next = c as usize + 1 + n.descendants as usize;
            if n.start >= old_end {
                past.get_or_insert(c as usize);
                break;
            }
            if n.end > at {
                first.get_or_insert(c as usize);
                past = Some(next);
            }
        }
        let subtree_end = s as usize + 1 + self.node(s).descendants as usize;
        let range = match (first, past) {
            (Some(f), Some(p)) => f..p,
            (None, Some(p)) => p..p,
            _ => subtree_end..subtree_end,
        };
        let dropped = range.len();
        let stale = stale.and_then(|p| match p as usize {
            p if p < range.start => Some(p as NodeId),
            p if p >= range.end => Some((p - dropped) as NodeId),
            _ => None,
        });
        self.splice_nodes(s, range, 0, &index(b""), delta);
        // A start tag the change lies in is measured again: the innermost element's, and an
        // outer one's that an earlier measuring stretched over its children.
        let mut a = s;
        while a != ROOT && a != NONE {
            if at < self.node(a).start + self.node(a).head_len as u64 {
                self.measure_head(src, a);
            }
            a = self.node(a).parent;
        }
        for e in self.error.iter_mut().chain(self.more_errors.iter_mut()) {
            if e.offset >= old_end {
                e.offset = e.offset.wrapping_add_signed(delta);
            } else if e.offset > at {
                e.offset = at;
            }
        }
        match stale {
            Some(p) => self.common_ancestor(p, s),
            None => s,
        }
    }

    /// The innermost node that holds both `a` and `b`, or is one of them.
    pub fn common_ancestor(&self, a: NodeId, b: NodeId) -> NodeId {
        let (mut a, mut b) = (a, b);
        let (mut da, mut db) = (self.depth(a), self.depth(b));
        while da > db {
            a = self.node(a).parent;
            da -= 1;
        }
        while db > da {
            b = self.node(b).parent;
            db -= 1;
        }
        while a != b {
            a = self.node(a).parent;
            b = self.node(b).parent;
        }
        a
    }

    /// Measures the start tag of element `id` again after its bytes changed: up to the first
    /// `>` outside quotes, or to its end tag when there is none. Keeps every accessor inside
    /// the element's bytes.
    fn measure_head(&mut self, src: &[u8], id: NodeId) {
        let n = self.nodes[id as usize];
        let (start, end) = (n.start as usize, n.end as usize);
        let self_closing = n.head_len as u64 == n.end - n.start;
        let limit = if self_closing {
            end
        } else {
            // The end tag is intact (see `keep_stale`): its `<` is the element's last.
            memchr::memrchr(b'<', &src[start + 1..end]).map_or(end, |i| start + 1 + i)
        };
        let head = if self_closing {
            end - start
        } else {
            let mut quote = None;
            let found = src[start + 1..limit].iter().position(|&b| match quote {
                Some(q) => {
                    if b == q {
                        quote = None;
                    }
                    false
                }
                None if b == b'"' || b == b'\'' => {
                    quote = Some(b);
                    false
                }
                None => b == b'>',
            });
            found.map_or(limit - start, |i| i + 2)
        };
        let name = src[start + 1..start + head]
            .iter()
            .take_while(|&&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>'))
            .count();
        let node = &mut self.nodes[id as usize];
        node.head_len = head as u32;
        node.name_len = name.min(u16::MAX as usize) as u16;
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
        self.last_splice = Some(NodeSplice {
            parent,
            at: first,
            removed: old.len() as u32,
            inserted: new_count as u32,
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

    /// The innermost node whose span holds byte `at`; [`ROOT`] when none does. A binary
    /// search, as nodes start in id order, then a walk up: for a view that needs it every
    /// frame (TEXT-3).
    pub fn innermost_at(&self, at: u64) -> NodeId {
        let k = self.nodes[1..].partition_point(|n| n.start <= at);
        let mut id = k as NodeId;
        while id != ROOT && id != NONE && self.node(id).end <= at {
            id = self.node(id).parent;
        }
        if id == NONE { ROOT } else { id }
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

    /// The child elements of `parent` when it has at least [`WIDE_DESCENDANTS`] nodes below
    /// it, else `None` (D24). The first call walks the children; later calls reuse the list
    /// until the nodes change ([`Index::update`], [`Index::keep_stale`]).
    pub fn child_elements(&self, src: &[u8], parent: NodeId) -> Option<Arc<ChildElements>> {
        if self.node(parent).descendants < WIDE_DESCENDANTS {
            return None;
        }
        let mut lists = self.wide.0.lock().unwrap_or_else(PoisonError::into_inner);
        let list = lists.entry(parent).or_insert_with(|| {
            let mut ids = Vec::new();
            let mut text = false;
            for c in self.children(parent) {
                match self.node(c).kind {
                    NodeKind::Element => ids.push(c),
                    NodeKind::Text | NodeKind::CData => text = true,
                    _ => {}
                }
            }
            let first = ids.first().map(|&c| self.name(src, c));
            let one_name = ids.iter().all(|&c| Some(self.name(src, c)) == first);
            Arc::new(ChildElements {
                ids,
                one_name,
                text,
            })
        });
        Some(Arc::clone(list))
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
        self.attribute_iter(src, id).collect()
    }

    /// The name of each attribute of an element, raw, in the order of [`Index::attributes`].
    /// Allocates nothing: for scanning many elements, as the grid's columns do (GRID-3).
    pub fn attribute_names<'s>(
        &self,
        src: &'s [u8],
        id: NodeId,
    ) -> impl Iterator<Item = &'s [u8]> + use<'s> {
        let ws = |b: &u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
        self.attribute_iter(src, id).map(move |r| {
            let a = &src[r];
            let name = a.split(|&b| b == b'=').next().unwrap_or(a);
            let end = name.iter().rposition(|b| !ws(b)).map_or(0, |i| i + 1);
            &name[..end]
        })
    }

    fn attribute_iter<'s>(&self, src: &'s [u8], id: NodeId) -> AttributeSpans<'s> {
        let n = self.node(id);
        if n.kind != NodeKind::Element {
            return AttributeSpans { src, at: 0, end: 0 };
        }
        let start = n.start as usize;
        let mut end = start + n.head_len as usize;
        // Drop the closing `>` or `/>`.
        end = end.saturating_sub(1);
        if end > start && src[end - 1] == b'/' {
            end -= 1;
        }
        AttributeSpans {
            src,
            at: start + 1 + n.name_len as usize,
            end,
        }
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
            // Inside `<![CDATA[` and `]]>`; all of it while a typed change has broken them.
            NodeKind::CData => {
                String::from_utf8_lossy(raw.get(9..raw.len().saturating_sub(3)).unwrap_or(raw))
            }
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
        // element starts its end tag. `get`: a start tag measured again while the text is
        // broken (`Index::keep_stale`) may reach the end.
        let end = memchr::memrchr(b'<', src.get(start..n.end as usize)?)? + start;
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
    /// Walks the siblings before `id` at every level, so it costs time on wide parents,
    /// except under a parent whose child elements all have one name (D24).
    /// Call it when the selection changes, not once per frame.
    pub fn path(&self, src: &[u8], id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = id;
        while cur != ROOT && cur != NONE {
            let n = self.node(cur);
            if n.kind == NodeKind::Element {
                let name = self.name(src, cur);
                let (before, others) = match self.child_elements(src, n.parent) {
                    // Ids increase in document order.
                    Some(list) if list.one_name => {
                        (list.ids.partition_point(|&s| s < cur), list.ids.len() > 1)
                    }
                    _ => self.same_name_before(src, cur, name),
                };
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

    /// The number of sibling elements before `id` named `name`, and whether it has any
    /// sibling of that name at all. Walks the siblings, and stops one after `id`: that is
    /// enough to know whether a path needs a position.
    fn same_name_before(&self, src: &[u8], id: NodeId, name: &[u8]) -> (usize, bool) {
        let mut before = 0usize;
        let mut others = false;
        let mut passed = false;
        for s in self.children(self.node(id).parent) {
            if s == id {
                passed = true;
            } else if self.node(s).kind == NodeKind::Element && self.name(src, s) == name {
                others = true;
                if passed {
                    break;
                }
                before += 1;
            }
        }
        (before, others)
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

/// Walks the attributes of a start tag in `at..end` of the source; see
/// [`Index::attribute_spans`]. Stops at the first one that is not `name = "value"`.
struct AttributeSpans<'s> {
    src: &'s [u8],
    at: usize,
    end: usize,
}

impl Iterator for AttributeSpans<'_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        let src = self.src;
        let ws = |b: u8| matches!(b, b' ' | b'\t' | b'\r' | b'\n');
        let end = self.end;
        let mut i = self.at;
        // Ends the walk whatever is returned.
        self.at = end;
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
            return None;
        }
        i += 1;
        while i < end && ws(src[i]) {
            i += 1;
        }
        let &quote = src
            .get(i)
            .filter(|&&q| i < end && (q == b'"' || q == b'\''))?;
        i += 1;
        while i < end && src[i] != quote {
            i += 1;
        }
        if i >= end {
            return None;
        }
        i += 1;
        self.at = i;
        Some(name..i)
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
    /// The names of the open elements one after another, and where each starts in it, in the
    /// order of `stack` without the document. Matching an end tag here stays in cache;
    /// reading the name from the node and the source misses it (OPEN-10).
    names: Vec<u8>,
    name_starts: Vec<usize>,
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
            names: Vec::new(),
            name_starts: Vec::new(),
            element_count: 0,
            max_depth: 0,
        }
    }

    /// Indexes the source. After an error it goes on as far as it can (OPEN-10, D14): an end
    /// tag closes the elements left open inside the element it matches, an end tag that
    /// matches none is skipped, a byte that is not UTF-8 is skipped up to the next `<`, and
    /// elements open at the end of the file end there. Each of these is an error.
    fn run(mut self) -> Index {
        let mut errors = Vec::new();
        let mut base = self.base;
        let mut reader = Self::reader(&self.src[base as usize..]);
        loop {
            let start = base + reader.buffer_position();
            let event = match reader.read_event() {
                Ok(e) => e,
                Err(e) => {
                    let err = parse_error(e, start, base + reader.error_position());
                    let utf8 = err.not_utf8;
                    let at = err.offset;
                    errors.push(err.error);
                    if !utf8 {
                        // Malformed markup: the reader has skipped it and goes on, or it ran
                        // to the end of the file.
                        continue;
                    }
                    if self.src.starts_with(b"\xFF\xFE") || self.src.starts_with(b"\xFE\xFF") {
                        // UTF-16 cannot be read yet (OPEN-8): every byte after would fail too.
                        break;
                    }
                    // Go on from the next `<` after the byte that is not UTF-8.
                    let from = (at + 1).min(self.src.len() as u64) as usize;
                    let Some(next) = memchr::memchr(b'<', &self.src[from..]) else {
                        break;
                    };
                    base = (from + next) as u64;
                    reader = Self::reader(&self.src[base as usize..]);
                    continue;
                }
            };
            let end = base + reader.buffer_position();
            match event {
                Event::Start(e) => {
                    let name_len = e.name().as_ref().len();
                    if name_len > MAX_NAME_LEN {
                        errors.push(name_too_long(start));
                        continue;
                    }
                    let id = self.add(NodeKind::Element, start, end, name_len);
                    self.stack.push(id);
                    self.name_starts.push(self.names.len());
                    self.names.extend_from_slice(e.name().as_ref().as_bytes());
                }
                Event::Empty(e) => {
                    let name_len = e.name().as_ref().len();
                    if name_len > MAX_NAME_LEN {
                        errors.push(name_too_long(start));
                        continue;
                    }
                    self.add(NodeKind::Element, start, end, name_len);
                }
                Event::End(e) => {
                    let name = e.name();
                    let name = name.as_ref().as_bytes();
                    let Some(depth) = self.open_named(name) else {
                        errors.push(unexpected_end(start, name));
                        continue;
                    };
                    while self.stack.len() > depth + 1 {
                        let open = self.pop();
                        errors.push(self.not_closed(open));
                        self.fold(open, start);
                        self.close(open, start);
                    }
                    let open = self.pop();
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
                Event::Eof => break,
            }
        }
        // Elements still open end with the file.
        let len = self.src.len() as u64;
        while self.stack.len() > 1 {
            let open = self.pop();
            errors.push(self.not_closed(open));
            self.close(open, len);
        }
        self.finish(errors)
    }

    /// Closes the document node and makes the index, with `errors` in any order.
    fn finish(mut self, mut errors: Vec<ParseError>) -> Index {
        let len = self.src.len() as u64;
        self.close(ROOT, len);
        // The capacity guess can overshoot by up to 2x after growth; give the slack back.
        self.nodes.shrink_to_fit();
        // Unclosed elements are reported where they start, before what was read after them.
        errors.sort_by_key(|e| e.offset);
        errors.truncate(MAX_ERRORS);
        let mut errors = errors.into_iter();
        Index {
            nodes: self.nodes,
            error: errors.next(),
            more_errors: errors.collect(),
            element_count: self.element_count,
            max_depth: self.max_depth,
            last_splice: None,
            wide: WideChildren::default(),
        }
    }

    /// A reader over `src`, which checks nothing that [`Builder::run`] checks itself.
    fn reader(src: &'s [u8]) -> Reader<&'s [u8]> {
        let mut reader = Reader::from_reader(src);
        let config = reader.config_mut();
        config.check_end_names = false;
        config.allow_unmatched_ends = true;
        reader
    }

    /// Where in `stack` the innermost open element named `name` is; `None` when no open
    /// element has that name. Nearly always the innermost one.
    fn open_named(&self, name: &[u8]) -> Option<usize> {
        let mut end = self.names.len();
        for (i, &from) in self.name_starts.iter().enumerate().rev() {
            if &self.names[from..end] == name {
                return Some(i + 1);
            }
            end = from;
        }
        None
    }

    /// Closes the innermost open element in `stack` and `names`; returns it.
    fn pop(&mut self) -> NodeId {
        let from = self.name_starts.pop().expect("an open element");
        self.names.truncate(from);
        self.stack.pop().expect("an open element")
    }

    /// The error for element `id`, open when it should have been closed.
    fn not_closed(&self, id: NodeId) -> ParseError {
        let n = &self.nodes[id as usize];
        let name = String::from_utf8_lossy(&self.src[n.name_span()]);
        ParseError {
            offset: n.start,
            message: format!("element <{name}> is not closed"),
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

/// An end tag that matches no open element.
#[cold]
#[inline(never)]
fn unexpected_end(offset: u64, name: &[u8]) -> ParseError {
    ParseError {
        offset,
        message: format!(
            "end tag </{}> matches no open element",
            String::from_utf8_lossy(name)
        ),
    }
}

/// A parse error, and whether its cause is a byte that is not UTF-8.
struct Failed {
    error: ParseError,
    not_utf8: bool,
    offset: u64,
}

/// The error to report for `e`, which happened in the event that starts at byte `start`.
/// `position` is quick-xml's error position. Kept out of the parse loop: it runs rarely.
#[cold]
#[inline(never)]
fn parse_error(e: quick_xml::Error, start: u64, position: u64) -> Failed {
    // quick-xml does not set the error position for bytes that are not UTF-8, but the error
    // says how far into the failing event they were valid.
    let (offset, not_utf8) = match &e {
        quick_xml::Error::Encoding(EncodingError::Utf8(u)) => {
            (start + u.valid_up_to() as u64, true)
        }
        _ => (position, false),
    };
    Failed {
        error: ParseError {
            offset,
            message: e.to_string(),
        },
        not_utf8,
        offset,
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

    /// The element named `name`, the first in document order.
    fn named(idx: &Index, src: &[u8], name: &str) -> NodeId {
        (1..idx.len() as NodeId)
            .find(|&id| {
                idx.node(id).kind == NodeKind::Element && idx.name(src, id) == name.as_bytes()
            })
            .unwrap_or_else(|| panic!("no <{name}>"))
    }

    /// All errors, first to last.
    fn errors(idx: &Index) -> Vec<(u64, String)> {
        idx.error
            .iter()
            .chain(&idx.more_errors)
            .map(|e| (e.offset, e.message.clone()))
            .collect()
    }

    /// OPEN-10: an end tag closes the elements left open inside the element it matches, at
    /// its `<`; each of them is an error where it starts. What follows is read as usual.
    #[test]
    fn end_tag_closes_what_was_left_open() {
        let src = "\u{feff}<r>\r\n<ä><ö>x\r\n<ü/></ä>\r\n<c>Grüße</c>\r\n</r>\r\n".as_bytes();
        let idx = index(src);
        let at = |s: &str| {
            src.windows(s.len())
                .position(|w| w == s.as_bytes())
                .unwrap() as u64
        };
        let (r, a, o, u, c) = (
            named(&idx, src, "r"),
            named(&idx, src, "ä"),
            named(&idx, src, "ö"),
            named(&idx, src, "ü"),
            named(&idx, src, "c"),
        );
        assert_eq!(errors(&idx).len(), 1);
        assert_eq!(idx.error.as_ref().unwrap().offset, at("<ö>"));
        assert!(idx.error.as_ref().unwrap().message.contains("<ö>"));
        assert_eq!(idx.node(o).end, at("</ä>"));
        assert_eq!(idx.node(u).parent, o);
        assert_eq!(idx.node(a).end, at("</ä>") + "</ä>".len() as u64);
        assert_eq!(idx.node(c).parent, r);
        assert_eq!(idx.text(src, c), "Grüße");
        assert_eq!(idx.node(r).end, at("</r>") + 4);
        assert_eq!(idx.node(r).descendants as usize, idx.len() - 2);
    }

    /// An end tag that matches no open element is an error and is skipped.
    #[test]
    fn stray_end_tag_is_skipped() {
        let src = b"<r><a/></x><b/></r>";
        let idx = index(src);
        assert_eq!(errors(&idx).len(), 1);
        let e = idx.error.as_ref().unwrap();
        assert_eq!(e.offset, 7);
        assert!(e.message.contains("</x>"), "{e}");
        assert_eq!(
            idx.node(named(&idx, src, "b")).parent,
            named(&idx, src, "r")
        );
        assert_eq!(idx.node(named(&idx, src, "r")).end, src.len() as u64);
    }

    /// Elements open at the end of the file end with it: one error each, outermost first.
    #[test]
    fn open_elements_end_with_the_file() {
        let src = "<r>\r\n<a>Maß".as_bytes();
        let idx = index(src);
        let len = src.len() as u64;
        let (r, a) = (named(&idx, src, "r"), named(&idx, src, "a"));
        assert_eq!((idx.node(r).end, idx.node(a).end), (len, len));
        let errs = errors(&idx);
        assert_eq!(errs.len(), 2);
        assert_eq!((errs[0].0, errs[1].0), (0, 5));
        assert!(
            errs[0].1.contains("<r>") && errs[1].1.contains("<a>"),
            "{errs:?}"
        );
    }

    /// A byte that is not UTF-8 is an error; reading goes on from the next `<`, also after a
    /// byte order mark.
    #[test]
    fn reading_goes_on_after_a_byte_that_is_not_utf8() {
        let src = b"\xEF\xBB\xBF<r>\r\n<a>Gr\xFC\xDFe</a>\r\n<b>\xC3\xBC</b>\r\n</r>\r\n";
        let idx = index(src);
        let errs = errors(&idx);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].0, 13, "at the first byte that is not UTF-8");
        let (r, a, b) = (
            named(&idx, src, "r"),
            named(&idx, src, "a"),
            named(&idx, src, "b"),
        );
        assert_eq!(idx.node(a).end, 20, "closed by its own end tag");
        assert_eq!(idx.node(b).parent, r);
        assert_eq!(idx.text(src, b), "ü");
        assert_eq!(idx.node(r).end, src.len() as u64 - 2);
    }

    /// Markup that is never closed runs to the end of the file: nothing after it is read,
    /// what came before stays, and it is an error.
    #[test]
    fn unclosed_markup_runs_to_the_end() {
        for src in [&b"<r><a/><b x=\"1></b><c/></r>"[..], b"<r><a/><!-- x </r>"] {
            let idx = index(src);
            named(&idx, src, "a");
            assert!(
                !(1..idx.len() as NodeId).any(|id| idx.name(src, id) == b"c"),
                "{}",
                String::from_utf8_lossy(src)
            );
            assert!(idx.error.is_some());
            assert_eq!(idx.node(named(&idx, src, "r")).end, src.len() as u64);
        }
    }

    /// No more than [`MAX_ERRORS`] errors are kept; reading goes on to the end.
    #[test]
    fn errors_are_capped() {
        let mut src = String::from("<r>");
        for _ in 0..MAX_ERRORS + 500 {
            src.push_str("</x>");
        }
        src.push_str("<last/></r>");
        let idx = index(src.as_bytes());
        assert_eq!(errors(&idx).len(), MAX_ERRORS);
        named(&idx, src.as_bytes(), "last");
    }

    #[test]
    fn innermost_at_finds_the_node_holding_a_byte() {
        let src = "\u{feff}<r>\r\n<!-- ä\r\n -->\r\n<a x=\"1\r\n2\">t</a>\r\n</r>\r\n".as_bytes();
        let idx = index(src);
        let at = |s: &str| {
            src.windows(s.len())
                .position(|w| w == s.as_bytes())
                .unwrap() as u64
        };
        let kind = |at: u64| idx.node(idx.innermost_at(at)).kind;
        assert_eq!(idx.innermost_at(0), ROOT);
        assert_eq!(kind(at("<r>")), NodeKind::Element);
        assert_eq!(kind(at(" -->")), NodeKind::Comment);
        assert_eq!(kind(at("2\"")), NodeKind::Element);
        assert_eq!(idx.name(src, idx.innermost_at(at("2\""))), b"a");
        assert_eq!(idx.name(src, idx.innermost_at(at("</r>"))), b"r");
        assert_eq!(idx.innermost_at(src.len() as u64 - 1), ROOT);
        assert_eq!(idx.innermost_at(u64::MAX), ROOT);
        // Every byte, against a walk down from the root.
        for b in 0..src.len() as u64 {
            let mut cur = ROOT;
            'down: loop {
                for c in idx.children(cur) {
                    let n = idx.node(c);
                    if n.start <= b && b < n.end {
                        cur = c;
                        continue 'down;
                    }
                }
                break;
            }
            assert_eq!(idx.innermost_at(b), cur, "byte {b}");
        }
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

    /// A root with `n` children, each named by `name(i)`, separated by text.
    fn wide(n: usize, name: impl Fn(usize) -> &'static str) -> Vec<u8> {
        let mut src = b"<r>".to_vec();
        for i in 0..n {
            src.extend_from_slice(format!("\n<{}/>", name(i)).as_bytes());
        }
        src.extend_from_slice(b"\n</r>");
        src
    }

    /// The path a walk over every sibling gives, as before D24.
    fn walked_path(idx: &Index, src: &[u8], id: NodeId) -> String {
        let name = idx.name(src, id);
        let (before, others) = idx.same_name_before(src, id, name);
        let name = String::from_utf8_lossy(name);
        if others {
            format!("/r/{name}[{}]", before + 1)
        } else {
            format!("/r/{name}")
        }
    }

    #[test]
    fn wide_parent_with_one_name_lists_its_children() {
        let n = WIDE_DESCENDANTS as usize;
        let src = wide(n, |_| "a");
        let idx = index(&src);
        let root = idx.root_element().unwrap();
        let list = idx.child_elements(&src, root).expect("wide");
        assert!(list.one_name);
        assert!(!list.text, "whitespace between the children is not text");
        let kids: Vec<NodeId> = idx
            .children(root)
            .filter(|&c| idx.node(c).kind == NodeKind::Element)
            .collect();
        assert_eq!(list.ids, kids);
        for &k in [kids[0], kids[1], kids[n / 2], kids[n - 1]].iter() {
            assert_eq!(idx.path(&src, k), walked_path(&idx, &src, k));
        }
        assert_eq!(idx.path(&src, kids[n - 1]), format!("/r/a[{n}]"));
        // A narrow parent has no list.
        assert!(idx.child_elements(&src, kids[0]).is_none());
        assert!(
            index(b"<r><a/></r>")
                .child_elements(b"<r><a/></r>", 1)
                .is_none()
        );
    }

    #[test]
    fn wide_parent_with_mixed_names_walks_for_the_path() {
        let n = WIDE_DESCENDANTS as usize;
        let src = wide(n, |i| if i % 3 == 0 { "b" } else { "a" });
        let idx = index(&src);
        let root = idx.root_element().unwrap();
        assert!(!idx.child_elements(&src, root).expect("wide").one_name);
        let kids: Vec<NodeId> = idx
            .children(root)
            .filter(|&c| idx.name(&src, c) != b"")
            .collect();
        for &k in [kids[0], kids[1], kids[2], kids[n - 1]].iter() {
            assert_eq!(idx.path(&src, k), walked_path(&idx, &src, k));
        }
        assert_eq!(idx.path(&src, kids[3]), "/r/b[2]");
    }

    #[test]
    fn wide_parent_with_text_says_so() {
        let n = WIDE_DESCENDANTS as usize;
        for between in ["Grüße", "<![CDATA[<]]>"] {
            let mut src = wide(n, |_| "a");
            let at = src.len() - b"</r>".len();
            src.splice(at..at, between.bytes());
            let idx = index(&src);
            let list = idx.child_elements(&src, 1).expect("wide");
            assert!(list.text, "{between}");
            assert_eq!(list.ids.len(), n);
        }
    }

    #[test]
    fn wide_children_follow_an_update() {
        let n = WIDE_DESCENDANTS as usize;
        let src = wide(n, |_| "a");
        let mut idx = index(&src);
        let root = idx.root_element().unwrap();
        assert_eq!(idx.child_elements(&src, root).unwrap().ids.len(), n);
        // One more child before the end tag.
        let at = src.len() - b"\n</r>".len();
        let mut new = src.clone();
        new.splice(at..at, b"\n<a/>".iter().copied());
        assert!(idx.update(&new, at, 0, 5));
        let list = idx.child_elements(&new, root).unwrap();
        assert_eq!(list.ids.len(), n + 1);
        let last = *list.ids.last().unwrap();
        assert_eq!(idx.path(&new, last), format!("/r/a[{}]", n + 1));
        // A name that differs is seen too.
        let mut renamed = new.clone();
        let b = renamed.len() - b"/>\n</r>".len() - 1;
        renamed[b] = b'b';
        assert!(idx.update(&renamed, b, 1, 1));
        assert!(!idx.child_elements(&renamed, root).unwrap().one_name);
        assert_eq!(idx.path(&renamed, last), "/r/b");
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

    /// The scanner's index of `src` (D14), or `None` where it leaves `src` to quick-xml;
    /// when it reads it, the index equals quick-xml's.
    fn scanned_like_quick_xml(src: &[u8]) -> Option<Index> {
        let mut fast = Builder::new(src);
        if !fast.scan() {
            return None;
        }
        let fast = fast.finish(Vec::new());
        let slow = Builder::new(src).run();
        let show = String::from_utf8_lossy(&src[..src.len().min(200)]);
        assert!(
            slow.error.is_none(),
            "scanned, but quick-xml finds an error: {show}"
        );
        assert_eq!(fast.nodes, slow.nodes, "{show}");
        assert_eq!(fast.element_count, slow.element_count, "{show}");
        assert_eq!(fast.max_depth, slow.max_depth, "{show}");
        Some(fast)
    }

    #[test]
    fn scanner_reads_what_quick_xml_reads() {
        let cases: &[&[u8]] = &[
            DOC,
            b"<r/>",
            b"\xEF\xBB\xBF<r>\r\n  <a x=\"1\">t</a>\r\n</r>\r\n",
            b"<?xml version=\"1.0\"?>\n<!-- c -->\n<?pi data?>\n<r/>\n<!-- after -->\n",
            b"<!DOCTYPE r SYSTEM \"r.dtd\">\n<r/>",
            b"<r>  &amp;  </r>",
            b"<r>  x &amp; y  </r>",
            b"<r>&#x41;&#66;</r>",
            b"<r>a<b/>c<![CDATA[ d ]]>e</r>",
            b"<r><![CDATA[x]]></r>",
            b"<r> <![CDATA[x]]> </r>",
            b"<r><a x='>' y=\"'>'\">gt</a></r>",
            b"<r><a/><a /><a x=\"/\"/></r>",
            b"<r>\n  <a>1</a >\n</r >",
            b"<r>caf\xC3\xA9 \xE2\x82\xAC</r>",
            b"<r>\t\n </r>",
            b"text before <r/> text after",
            b"<r><x:y xmlns:x=\"u\"/></r>",
        ];
        for &src in cases {
            assert!(
                scanned_like_quick_xml(src).is_some(),
                "not scanned: {:?}",
                String::from_utf8_lossy(src)
            );
        }
        // Errors and what the scanner leaves to quick-xml.
        let left: &[&[u8]] = &[
            b"<r><a></r>",
            b"<r>",
            b"<r></a>",
            b"<r>a & b</r>",
            b"<r>\xFF</r>",
            b"<!DOCTYPE r [<!ENTITY e \"x\">]><r>&e;</r>",
            b"<r><</r>",
            b"<r><!-- open </r>",
            b"< r/>",
            b"<?XML version=\"1.0\"?><r/>",
        ];
        for &src in left {
            assert!(
                scanned_like_quick_xml(src).is_none(),
                "scanned: {:?}",
                String::from_utf8_lossy(src)
            );
            assert_eq!(index(src).nodes, Builder::new(src).run().nodes);
        }
    }

    #[test]
    fn scanner_reads_the_corpus_like_quick_xml() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/corpus");
        let mut scanned = Vec::new();
        for entry in std::fs::read_dir(dir).expect("corpus") {
            let path = entry.expect("entry").path();
            let src = std::fs::read(&path).expect("read");
            if scanned_like_quick_xml(&src).is_some() {
                scanned.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        scanned.sort();
        // Every well-formed UTF-8 file; not UTF-16, Latin-1, Windows-1252, the broken one,
        // or the DOCTYPE with an internal subset.
        assert_eq!(
            scanned,
            [
                "namespaces.xml",
                "one_line.xml",
                "tabs_and_mixed_content.xml",
                "utf8_bom_crlf.xml",
                "utf8_no_declaration.xml",
            ]
        );
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

    /// Every node [`NodeSplice::map`] keeps is the same node: same kind, its start moved
    /// with the change when it lay after it.
    fn check_splice(before: &[Node], idx: &Index, at: usize, removed: usize, inserted: usize) {
        let ns = idx.last_splice.expect("a local change records its splice");
        assert_eq!(
            before.len() - ns.removed as usize + ns.inserted as usize,
            idx.len(),
            "{ns:?}"
        );
        let delta = inserted as i64 - removed as i64;
        // The parent and its ancestors hold the change: their starts stay.
        let mut around = vec![false; idx.len()];
        let mut a = ns.parent;
        while a != NONE {
            around[a as usize] = true;
            a = idx.node(a).parent;
        }
        for (old, n) in before.iter().enumerate() {
            let Some(new) = ns.map(old as NodeId) else {
                continue;
            };
            let m = idx.node(new);
            let start = if !around[new as usize] && n.start >= (at + removed) as u64 {
                n.start.wrapping_add_signed(delta)
            } else {
                n.start
            };
            assert_eq!(
                (m.start, m.kind),
                (start, n.kind),
                "node {old} -> {new}, {ns:?}"
            );
        }
    }

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
            let before = idx.nodes.clone();
            if idx.update(&src, at, removed, inserted.len()) {
                accepted += 1;
                check_splice(&before, &idx, at, removed, inserted.len());
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

    /// Every node is inside its parent and after its previous sibling, and inside the source;
    /// a start tag is inside its element.
    fn check_structure(idx: &Index, src: &[u8]) {
        assert_eq!(idx.node(ROOT).end, src.len() as u64);
        for id in 1..idx.len() as NodeId {
            let n = idx.node(id);
            let p = idx.node(n.parent);
            assert!(n.parent < id);
            assert!(
                id as usize + n.descendants as usize <= n.parent as usize + p.descendants as usize
            );
            assert!(
                p.start <= n.start && n.end <= p.end,
                "node {id} outside its parent"
            );
            assert!(
                n.start + n.head_len as u64 <= n.end,
                "start tag of {id} too long"
            );
            assert!(n.name_len as u32 <= n.head_len);
        }
        for id in 0..idx.len() as NodeId {
            let kids: Vec<_> = idx.children(id).collect();
            for w in kids.windows(2) {
                assert!(idx.node(w[0]).end <= idx.node(w[1]).start);
            }
        }
    }

    /// Reads everything a view reads, to show that nothing panics.
    fn read_all(idx: &Index, src: &[u8]) {
        for id in 1..idx.len() as NodeId {
            let _ = idx.name(src, id);
            let _ = idx.raw(src, id);
            let _ = idx.attributes(src, id);
            let _ = idx.attribute_spans(src, id);
            let _ = idx.folded_span(src, id);
            let _ = idx.text(src, id);
            if idx.node(id).kind == NodeKind::Element {
                let _ = idx.element_text(src, id);
            }
        }
    }

    /// Random typed changes kept with [`Index::keep_stale`]: the index stays consistent, every
    /// node but the elements around each change lies clear of it, so it reads its own bytes,
    /// and nothing a view reads panics. When the
    /// source is well-formed again, the document is indexed afresh, as the editor does.
    fn check_stale(mut src: Vec<u8>, seed: u64, rounds: usize) {
        let mut rng = Lcg(seed);
        let mut idx = index(&src);
        let mut stale = None;
        for _ in 0..rounds {
            let at = rng.below(src.len() + 1);
            let removed = rng.below(7).min(src.len() - at);
            let inserted: &[u8] = if rng.below(3) == 0 {
                b""
            } else {
                SNIPPETS[rng.below(SNIPPETS.len())].as_bytes()
            };
            src.splice(at..at + removed, inserted.iter().copied());
            let before = idx.nodes.clone();
            let s = idx.keep_stale(&src, at, removed, inserted.len(), stale);
            check_splice(&before, &idx, at, removed, inserted.len());
            let what = format!("seed {seed}: at {at} -{removed} +{inserted:?}");
            check_structure(&idx, &src);
            // The nodes the change lies in, outermost first: elements whose `<` it left alone.
            // The stale element holds the innermost. (A start tag whose `>` was deleted runs
            // on to its end tag until the next full index; that is safe, only odd.)
            let (from, to) = (at as u64, (at + inserted.len()) as u64);
            let mut around = Vec::new();
            for id in 1..idx.len() as NodeId {
                let n = idx.node(id);
                if n.end <= from || n.start >= to {
                    continue;
                }
                assert!(
                    n.kind == NodeKind::Element && n.start < from && to <= n.end,
                    "{what}: node {id} {n:?} overlaps the change"
                );
                if let Some(&outer) = around.last() {
                    assert_eq!(n.parent, outer, "{what}: not a chain");
                }
                around.push(id);
            }
            if let Some(&inner) = around.last() {
                assert_eq!(
                    idx.common_ancestor(s, inner),
                    s,
                    "{what}: stale is not around it"
                );
            }
            read_all(&idx, &src);
            stale = Some(s);
            let full = index(&src);
            if full.error.is_none() {
                idx = full;
                stale = None;
            }
        }
    }

    #[test]
    fn stale_nodes_read_their_own_bytes() {
        for seed in 0..20 {
            check_stale(DOC.to_vec(), seed, 200);
        }
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/corpus");
        for (i, entry) in std::fs::read_dir(dir).expect("corpus").enumerate() {
            let src = std::fs::read(entry.expect("entry").path()).expect("read");
            check_stale(src, i as u64, 100);
        }
    }

    /// What tree and grid keep while the text is broken (TEXT-10).
    #[test]
    fn stale_element_keeps_its_last_state() {
        let src = "<r>\r\n <a x=\"1\"><b>Grüße</b><c/></a>\r\n <d/></r>";
        let edit = |s: &str, at: &str, removed: usize, ins: &str| {
            let i = s.find(at).unwrap();
            let mut new = s.as_bytes().to_vec();
            new.splice(i..i + removed, ins.bytes());
            (i, new)
        };
        let names = |idx: &Index, src: &[u8]| -> Vec<String> {
            (1..idx.len() as NodeId)
                .filter(|&id| idx.node(id).kind == NodeKind::Element)
                .map(|id| String::from_utf8_lossy(idx.name(src, id)).into_owned())
                .collect()
        };
        let all = ["r", "a", "b", "c", "d"];

        // `<` typed in the text of `b`: every node stays, `b` is stale, its text is what it
        // is now.
        let mut idx = index(src.as_bytes());
        let (at, new) = edit(src, "üße", 0, "<");
        let s = idx.keep_stale(&new, at, 0, 1, None);
        assert_eq!(idx.name(&new, s), b"b");
        assert_eq!(names(&idx, &new), all);
        assert_eq!(idx.text(&new, s), "Gr<üße", "the text as it is now");
        assert!(index(&new).error.is_some());

        // A quote typed in the start tag of `a`: `a` is stale, measured again, and keeps its
        // children; `d` moves by one byte.
        let mut idx = index(src.as_bytes());
        let (at, new) = edit(src, "1\"", 0, "\"");
        let s = idx.keep_stale(&new, at, 0, 1, None);
        assert_eq!(idx.name(&new, s), b"a");
        assert_eq!(names(&idx, &new), all);
        let d = (1..idx.len() as NodeId).last().unwrap();
        assert_eq!(idx.raw(&new, d), b"<d/>");

        // Deleting the `<` of `c`: `c` is gone, its parent `a` is stale.
        let mut idx = index(src.as_bytes());
        let (at, new) = edit(src, "<c/>", 1, "");
        let s = idx.keep_stale(&new, at, 1, 0, None);
        assert_eq!(idx.name(&new, s), b"a");
        assert_eq!(names(&idx, &new), ["r", "a", "b", "d"]);

        // Then a change in `d`: the stale element is the one around both, `r`.
        let new_s = std::str::from_utf8(&new).unwrap().to_string();
        let (at, newer) = edit(&new_s, "d/>", 0, "<");
        let s = idx.keep_stale(&newer, at, 0, 1, Some(s));
        assert_eq!(idx.name(&newer, s), b"r");
        check_structure(&idx, &newer);
    }

    #[test]
    fn reports_unclosed_element() {
        let idx = index(b"<a><b></b>");
        assert!(idx.error.unwrap().message.contains("<a>"));
    }
}
