//! The structure of a document, for the diagram (`docs/decisions.md`, D36).
//!
//! One [`PathInfo`] per distinct path of element names (`/orders/order/items/item`), with
//! how many elements have it, the names of their attributes and whether they hold text.
//! [`Structure::walk`] reads it from the index in one pass, on any thread, and stops when
//! asked. Names are compared as bytes, as XPath does (D35).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use xml_atlas_xml::{Folded, Index, NodeId, NodeKind, ROOT};

/// Paths kept at most. A recursive document (sections in sections) has a path per depth;
/// past this the walk leaves the deeper ones out and says so ([`Structure::capped`]).
pub const MAX_PATHS: usize = 2000;

/// Nodes walked between two looks at the stop flag.
const STOP_CHECK: u64 = 1 << 16;

/// A path's number: its place in [`Structure::paths`].
pub type PathId = u32;

/// The parent of the root element's path.
const NO_PATH: PathId = PathId::MAX;

/// The paths of a document's elements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Structure {
    /// Every path, a parent before its children, in the order the walk met them.
    pub paths: Vec<PathInfo>,
    /// The paths without a parent: the root element's (and a second top-level element's,
    /// in a document that is not well-formed).
    pub roots: Vec<PathId>,
    /// Whether paths were left out after [`MAX_PATHS`]; the counts of those kept are right.
    pub capped: bool,
}

/// One path: the elements with this name whose parents have the parent's path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathInfo {
    /// The element name, raw.
    pub name: Vec<u8>,
    pub parent: Option<PathId>,
    /// The child paths, in the order the walk met them.
    pub children: Vec<PathId>,
    /// 1 for the root element.
    pub depth: u16,
    /// How many elements have this path.
    pub count: u64,
    /// The attribute names these elements have, raw, in the order they were met.
    pub attributes: Vec<Vec<u8>>,
    /// Whether any of them holds text or CDATA.
    pub text: bool,
}

/// The walk was stopped before it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("stopped")]
pub struct Stopped;

impl Structure {
    /// Walks the whole index once. `stop` ends it early with [`Stopped`].
    pub fn walk(src: &[u8], index: &Index, stop: &AtomicBool) -> Result<Structure, Stopped> {
        let mut s = Structure::default();
        // (parent path, name) to path, borrowing the names from the source.
        let mut lookup: HashMap<(PathId, &[u8]), PathId> = HashMap::new();
        // Per path, the child path its last child element had.
        let mut last_child: Vec<PathId> = Vec::new();
        // The open elements with a path: where each subtree ends, and its path.
        let mut stack: Vec<(u64, PathId)> = Vec::new();
        let len = index.len() as u64;
        let mut id: u64 = 1;
        let mut ticks: u64 = 0;
        while id < len {
            ticks += 1;
            if ticks.is_multiple_of(STOP_CHECK) && stop.load(Ordering::Relaxed) {
                return Err(Stopped);
            }
            while stack.last().is_some_and(|&(end, _)| end <= id) {
                stack.pop();
            }
            let n = index.node(id as NodeId);
            let end = id + 1 + n.descendants as u64;
            match n.kind {
                NodeKind::Element => {
                    let parent = stack.last().map_or(NO_PATH, |&(_, p)| p);
                    let name = index.name(src, id as NodeId);
                    // Siblings mostly repeat a name: try the parent's last child first.
                    let last = match parent {
                        NO_PATH => NO_PATH,
                        p => last_child[p as usize],
                    };
                    let found = match last {
                        NO_PATH => None,
                        l if s.paths[l as usize].name == name => Some(l),
                        _ => None,
                    }
                    .or_else(|| lookup.get(&(parent, name)).copied());
                    let path = match found {
                        Some(p) => p,
                        None if s.paths.len() >= MAX_PATHS => {
                            // Its subtree has no paths either.
                            s.capped = true;
                            id = end;
                            continue;
                        }
                        None => {
                            let p = s.add(parent, name);
                            lookup.insert((parent, name), p);
                            last_child.push(NO_PATH);
                            p
                        }
                    };
                    if parent != NO_PATH {
                        last_child[parent as usize] = path;
                    }
                    let info = &mut s.paths[path as usize];
                    info.count += 1;
                    if n.folded != Folded::No {
                        info.text = true;
                    }
                    // `<a>` and `<a/>` have no room for an attribute.
                    if n.head_len as usize > n.name_len as usize + 3 {
                        for a in index.attribute_names(src, id as NodeId) {
                            if !info.attributes.iter().any(|known| known == a) {
                                info.attributes.push(a.to_vec());
                            }
                        }
                    }
                    if n.descendants > 0 {
                        stack.push((end, path));
                    }
                    id += 1;
                }
                NodeKind::Text | NodeKind::CData => {
                    if let Some(&(_, p)) = stack.last() {
                        s.paths[p as usize].text = true;
                    }
                    id = end;
                }
                _ => id = end,
            }
        }
        Ok(s)
    }

    /// The path of element `id`, if it has one: `None` for other nodes and for elements
    /// left out after [`MAX_PATHS`]. Walks up from `id`.
    pub fn path_of(&self, src: &[u8], index: &Index, id: NodeId) -> Option<PathId> {
        if index.len() <= id as usize || index.node(id).kind != NodeKind::Element {
            return None;
        }
        let mut chain = Vec::new();
        let mut cur = id;
        while cur != ROOT && cur != xml_atlas_xml::NONE {
            chain.push(cur);
            cur = index.node(cur).parent;
        }
        let mut path = NO_PATH;
        for &e in chain.iter().rev() {
            path = self.child_named(path, index.name(src, e))?;
        }
        Some(path)
    }

    /// The first element with path `path` after node `after` in document order, going on
    /// from the start when there is none after it; the first in the document when `after`
    /// is `None`. Skips every subtree that cannot hold the path.
    pub fn next(
        &self,
        src: &[u8],
        index: &Index,
        path: PathId,
        after: Option<NodeId>,
    ) -> Option<NodeId> {
        let path = self.paths.get(path as usize).map(|_| path)?;
        let mut on_chain = vec![false; self.paths.len()];
        let mut p = Some(path);
        while let Some(q) = p {
            on_chain[q as usize] = true;
            p = self.paths[q as usize].parent;
        }
        let start = after.map_or(1, |a| a as u64 + 1);
        self.next_from(src, index, path, &on_chain, start)
            .or_else(|| after.and_then(|_| self.next_from(src, index, path, &on_chain, 1)))
    }

    /// The names from the root element down to `path`, joined with `/`, without a leading
    /// one: `orders/order/items`. A key that stays the same when the walk is done again.
    pub fn key(&self, path: PathId) -> Vec<u8> {
        let mut names = Vec::new();
        let mut p = Some(path);
        while let Some(q) = p {
            names.push(&self.paths[q as usize].name[..]);
            p = self.paths[q as usize].parent;
        }
        names.reverse();
        names.join(&b'/')
    }

    fn add(&mut self, parent: PathId, name: &[u8]) -> PathId {
        let p = self.paths.len() as PathId;
        let depth = match parent {
            NO_PATH => {
                self.roots.push(p);
                1
            }
            _ => {
                self.paths[parent as usize].children.push(p);
                self.paths[parent as usize].depth + 1
            }
        };
        self.paths.push(PathInfo {
            name: name.to_vec(),
            parent: (parent != NO_PATH).then_some(parent),
            children: Vec::new(),
            depth,
            count: 0,
            attributes: Vec::new(),
            text: false,
        });
        p
    }

    /// The child path of `parent` named `name`.
    fn child_named(&self, parent: PathId, name: &[u8]) -> Option<PathId> {
        let kids = match parent {
            NO_PATH => &self.roots,
            p => &self.paths.get(p as usize)?.children,
        };
        kids.iter()
            .copied()
            .find(|&k| self.paths[k as usize].name == name)
    }

    /// [`Structure::next`] from node `start` on, without going round.
    fn next_from(
        &self,
        src: &[u8],
        index: &Index,
        target: PathId,
        on_chain: &[bool],
        start: u64,
    ) -> Option<NodeId> {
        let len = index.len() as u64;
        if start >= len {
            return None;
        }
        let end_of = |id: u64| id + 1 + index.node(id as NodeId).descendants as u64;
        // The ancestors of `start` that lie on the way to the path; past the first that
        // does not, the walk goes on after it.
        let mut stack: Vec<(u64, PathId)> = Vec::new();
        let mut id = start;
        let mut ancestors = Vec::new();
        let mut cur = index.node(start as NodeId).parent;
        while cur != ROOT && cur != xml_atlas_xml::NONE {
            ancestors.push(cur);
            cur = index.node(cur).parent;
        }
        let mut parent = NO_PATH;
        for &a in ancestors.iter().rev() {
            match self.child_named(parent, index.name(src, a)) {
                Some(p) if on_chain[p as usize] && p != target => {
                    stack.push((end_of(a as u64), p));
                    parent = p;
                }
                _ => {
                    id = end_of(a as u64);
                    break;
                }
            }
        }
        while id < len {
            while stack.last().is_some_and(|&(end, _)| end <= id) {
                stack.pop();
            }
            let n = index.node(id as NodeId);
            if n.kind != NodeKind::Element {
                id = end_of(id);
                continue;
            }
            let parent = stack.last().map_or(NO_PATH, |&(_, p)| p);
            match self.child_named(parent, index.name(src, id as NodeId)) {
                Some(p) if p == target => return Some(id as NodeId),
                Some(p) if on_chain[p as usize] => {
                    stack.push((end_of(id), p));
                    id += 1;
                }
                _ => id = end_of(id),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(src: &str) -> (Structure, Index) {
        let index = xml_atlas_xml::index(src.as_bytes());
        let s = Structure::walk(src.as_bytes(), &index, &AtomicBool::new(false)).unwrap();
        (s, index)
    }

    fn keys(s: &Structure) -> Vec<String> {
        (0..s.paths.len() as PathId)
            .map(|p| String::from_utf8(s.key(p)).unwrap())
            .collect()
    }

    const ORDERS: &str = "\u{feff}<?xml version=\"1.0\"?>\r\n<orders>\r\n\
        <order id=\"1\" date=\"2026\"><customer><name>Jörg</name></customer>\
        <items><item sku=\"a\"><price>1</price></item><item><price>2</price></item></items>\
        </order>\r\n<!-- a comment -->\r\n\
        <order id=\"2\" status=\"open\"><items><item sku=\"b\"/></items><note>größer</note></order>\r\n\
        </orders>";

    #[test]
    fn one_path_per_distinct_name_chain_with_counts() {
        let (s, _) = walk(ORDERS);
        assert_eq!(
            keys(&s),
            [
                "orders",
                "orders/order",
                "orders/order/customer",
                "orders/order/customer/name",
                "orders/order/items",
                "orders/order/items/item",
                "orders/order/items/item/price",
                "orders/order/note",
            ]
        );
        let count = |k: &str| s.paths[keys(&s).iter().position(|x| x == k).unwrap()].count;
        assert_eq!(count("orders/order"), 2);
        assert_eq!(count("orders/order/items/item"), 3);
        assert_eq!(count("orders/order/items/item/price"), 2);
        assert_eq!(s.roots, [0]);
        assert_eq!(s.paths[1].children, [2, 4, 7]);
        assert_eq!(s.paths[5].depth, 4);
        assert!(!s.capped);
    }

    #[test]
    fn attributes_and_text_are_gathered_over_all_elements() {
        let (s, _) = walk(ORDERS);
        let order = &s.paths[1];
        assert_eq!(order.attributes, [&b"id"[..], b"date", b"status"]);
        assert!(!order.text, "only whitespace and a comment between orders");
        assert!(s.paths[3].text, "name holds folded text");
        assert!(s.paths[7].text, "non-ASCII text");
        assert!(!s.paths[0].text);
        assert_eq!(s.paths[5].attributes, [&b"sku"[..]]);
    }

    #[test]
    fn mixed_content_and_cdata_count_as_text() {
        let (s, _) = walk("<r><p>a <b>bold</b> word</p><c><![CDATA[x]]><d/></c></r>");
        assert!(s.paths[1].text, "p has text beside b");
        assert!(s.paths[3].text, "c has CDATA beside d");
        assert!(!s.paths[4].text);
    }

    #[test]
    fn same_name_on_different_paths_is_two_boxes() {
        let (s, _) = walk("<r><a><x/></a><b><x/><x/></b></r>");
        assert_eq!(keys(&s), ["r", "r/a", "r/a/x", "r/b", "r/b/x"]);
        assert_eq!(s.paths[2].count, 1);
        assert_eq!(s.paths[4].count, 2);
    }

    #[test]
    fn a_recursive_document_stops_at_the_cap() {
        let depth = MAX_PATHS + 50;
        let src = format!("{}{}", "<s>".repeat(depth), "</s>".repeat(depth));
        let (s, index) = walk(&src);
        assert!(s.capped);
        assert_eq!(s.paths.len(), MAX_PATHS);
        assert!(s.paths.iter().all(|p| p.count == 1));
        // An element past the cap has no path.
        let deep = (MAX_PATHS + 10) as NodeId;
        assert_eq!(s.path_of(src.as_bytes(), &index, deep), None);
        assert_eq!(s.path_of(src.as_bytes(), &index, 5), Some(4));
    }

    #[test]
    fn path_of_finds_an_elements_box() {
        let (s, index) = walk(ORDERS);
        let src = ORDERS.as_bytes();
        let item = |n: usize| {
            (1..index.len() as NodeId)
                .filter(|&i| index.name(src, i) == b"item")
                .nth(n)
                .unwrap()
        };
        assert_eq!(s.path_of(src, &index, item(2)), Some(5));
        assert_eq!(s.path_of(src, &index, ROOT), None);
    }

    #[test]
    fn next_steps_through_a_paths_elements_and_goes_round() {
        let (s, index) = walk(ORDERS);
        let src = ORDERS.as_bytes();
        let items: Vec<NodeId> = (1..index.len() as NodeId)
            .filter(|&i| index.name(src, i) == b"item")
            .collect();
        assert_eq!(items.len(), 3);
        assert_eq!(s.next(src, &index, 5, None), Some(items[0]));
        assert_eq!(s.next(src, &index, 5, Some(items[0])), Some(items[1]));
        assert_eq!(s.next(src, &index, 5, Some(items[1])), Some(items[2]));
        assert_eq!(s.next(src, &index, 5, Some(items[2])), Some(items[0]));
        // From inside another branch: the customer's name comes before the first item.
        let name = s.next(src, &index, 3, None).unwrap();
        assert_eq!(s.next(src, &index, 5, Some(name)), Some(items[0]));
        // From the last node of the document it goes round.
        let last = index.len() as NodeId - 1;
        assert_eq!(
            s.next(src, &index, 7, Some(last)),
            s.next(src, &index, 7, None)
        );
    }

    #[test]
    fn next_follows_the_path_not_the_name() {
        let src = "<r><a><x i=\"1\"/></a><b><x i=\"2\"/></b><a><x i=\"3\"/></a></r>";
        let (s, index) = walk(src);
        let found: Vec<String> =
            std::iter::successors(s.next(src.as_bytes(), &index, 2, None), |&e| {
                s.next(src.as_bytes(), &index, 2, Some(e))
                    .filter(|&n| n > e)
            })
            .map(|e| index.attributes(src.as_bytes(), e)[0].1.clone())
            .collect();
        assert_eq!(found, ["1", "3"]);
    }

    #[test]
    fn a_stopped_walk_says_so() {
        let src = "<r>".to_string() + &"<a/>".repeat(200_000) + "</r>";
        let index = xml_atlas_xml::index(src.as_bytes());
        let stop = AtomicBool::new(true);
        assert_eq!(Structure::walk(src.as_bytes(), &index, &stop), Err(Stopped));
    }

    #[test]
    fn an_empty_or_broken_document_gives_what_it_has() {
        let (s, _) = walk("");
        assert!(s.paths.is_empty());
        let (s, _) = walk("<r><a><b></a>");
        assert_eq!(keys(&s)[..2], ["r", "r/a"]);
    }
}
