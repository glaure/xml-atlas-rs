//! Bookmarks: elements marked to come back to (`docs/spec/tree.md`, TREE-15).
//!
//! A bookmark is the byte offset of an element's start tag. Every edit, undo and redo moves
//! the offsets with the text ([`Document::apply`](crate::Document::apply)); a bookmark whose
//! start tag was replaced is gone. Bookmarks are not part of the document: no undo step, not
//! modified, not saved.

use xml_atlas_xml::{Index, NodeId, NodeKind};

use crate::Splice;

/// The bookmarks of a document, by the offset of the element's start, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bookmarks {
    starts: Vec<usize>,
}

impl Bookmarks {
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// The offsets of the marked elements' start tags, in document order.
    pub fn starts(&self) -> &[usize] {
        &self.starts
    }

    /// Whether the element starting at `start` is marked.
    pub fn contains(&self, start: usize) -> bool {
        self.starts.binary_search(&start).is_ok()
    }

    /// Marks the element starting at `start`, or takes the mark off. Returns whether it is
    /// marked now.
    pub fn toggle(&mut self, start: usize) -> bool {
        match self.starts.binary_search(&start) {
            Ok(at) => {
                self.starts.remove(at);
                false
            }
            Err(at) => {
                self.starts.insert(at, start);
                true
            }
        }
    }

    /// Takes every mark off.
    pub fn clear(&mut self) {
        self.starts.clear();
    }

    /// The bookmark after `offset`, going round to the first at the end: its number from 1
    /// and its offset. `None` when there are none.
    pub fn next(&self, offset: usize) -> Option<(usize, usize)> {
        let at = self.starts.partition_point(|&s| s <= offset);
        let at = if at == self.starts.len() { 0 } else { at };
        self.starts.get(at).map(|&s| (at + 1, s))
    }

    /// The bookmark before `offset`, going round to the last at the start.
    pub fn previous(&self, offset: usize) -> Option<(usize, usize)> {
        let at = self.starts.partition_point(|&s| s < offset);
        let at = at
            .checked_sub(1)
            .unwrap_or(self.starts.len().wrapping_sub(1));
        self.starts.get(at).map(|&s| (at + 1, s))
    }

    /// The marked elements, as the index has them now: a bookmark that does not start an
    /// element (the text around it is broken) is left out.
    pub fn elements<'a>(&'a self, index: &'a Index) -> impl Iterator<Item = NodeId> + 'a {
        self.starts
            .iter()
            .filter_map(|&s| element_starting_at(index, s))
    }

    /// Replaces the marks with those at `starts`.
    pub fn set(&mut self, starts: impl Iterator<Item = usize>) {
        self.starts = starts.collect();
        self.starts.sort_unstable();
        self.starts.dedup();
    }

    /// Moves the marks with `changes`, as [`History::landed`](crate::History::landed) gives
    /// them: ascending, in the offsets of the new source. A start inside replaced bytes is
    /// dropped.
    pub(crate) fn follow(&mut self, changes: &[Splice]) {
        if self.starts.is_empty() {
            return;
        }
        let mut kept = Vec::with_capacity(self.starts.len());
        let mut changes = changes.iter().peekable();
        // New offset minus old offset, from the changes passed.
        let mut shift = 0isize;
        for &start in &self.starts {
            let mut gone = false;
            while let Some(c) = changes.peek() {
                let old_at = c.at.wrapping_add_signed(-shift);
                if start >= old_at + c.removed {
                    shift += c.inserted as isize - c.removed as isize;
                    changes.next();
                } else {
                    gone = start >= old_at;
                    break;
                }
            }
            if !gone {
                kept.push(start.wrapping_add_signed(shift));
            }
        }
        self.starts = kept;
    }
}

/// The element that starts at byte `start`, if one does.
pub fn element_starting_at(index: &Index, start: usize) -> Option<NodeId> {
    let id = index.innermost_at(start as u64);
    let node = index.node(id);
    (id != xml_atlas_xml::ROOT && node.kind == NodeKind::Element && node.start == start as u64)
        .then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(starts: &[usize]) -> Bookmarks {
        Bookmarks {
            starts: starts.to_vec(),
        }
    }

    fn change(at: usize, removed: usize, inserted: usize) -> Splice {
        Splice {
            at,
            removed,
            inserted,
        }
    }

    #[test]
    fn toggling_keeps_the_marks_in_order() {
        let mut b = Bookmarks::default();
        assert!(b.toggle(30));
        assert!(b.toggle(10));
        assert!(b.toggle(20));
        assert_eq!(b.starts(), [10, 20, 30]);
        assert!(!b.toggle(20));
        assert_eq!(b.starts(), [10, 30]);
        assert!(b.contains(10) && !b.contains(20));
    }

    #[test]
    fn next_and_previous_go_round() {
        let b = at(&[10, 20, 30]);
        assert_eq!(b.next(0), Some((1, 10)));
        assert_eq!(b.next(10), Some((2, 20)));
        assert_eq!(b.next(15), Some((2, 20)));
        assert_eq!(b.next(30), Some((1, 10)));
        assert_eq!(b.previous(40), Some((3, 30)));
        assert_eq!(b.previous(30), Some((2, 20)));
        assert_eq!(b.previous(10), Some((3, 30)));
        assert_eq!(b.previous(0), Some((3, 30)));
        assert_eq!(Bookmarks::default().next(0), None);
        assert_eq!(Bookmarks::default().previous(0), None);
    }

    #[test]
    fn marks_move_with_the_changes_around_them() {
        // Before: 5 bytes at 12 become 2: later marks move 3 back, those in it are gone.
        let mut b = at(&[4, 12, 14, 17, 30]);
        b.follow(&[change(12, 5, 2)]);
        assert_eq!(b.starts(), [4, 14, 27]);
        // Two changes: bytes between them keep their marks, shifted by the first alone.
        // 2 bytes at 10 become 5 (+3); 4 bytes at 23 (new) become 0.
        let mut b = at(&[5, 10, 15, 20, 30]);
        b.follow(&[change(10, 2, 5), change(23, 4, 0)]);
        assert_eq!(b.starts(), [5, 18, 29]);
        // An insertion at a mark moves it on; a removal ending at it does not drop it.
        let mut b = at(&[10]);
        b.follow(&[change(10, 0, 4)]);
        assert_eq!(b.starts(), [14]);
        let mut b = at(&[10]);
        b.follow(&[change(6, 4, 0)]);
        assert_eq!(b.starts(), [6]);
    }

    use crate::{Document, FormatOptions, command};

    const SRC: &str = "\u{feff}<r>\r\n  <a>Zoë</a>\r\n  <b x=\"ü\"><c/></b>\r\n  <d/>\r\n</r>";

    fn doc() -> Document {
        Document::from_bytes(SRC.as_bytes().to_vec())
    }

    fn mark(doc: &mut Document, path: &str) {
        let id = command::resolve(doc, path).unwrap();
        let start = doc.index.node(id).start as usize;
        assert!(doc.bookmarks.toggle(start));
    }

    /// The paths of the marked elements, as they are now.
    fn marked(doc: &Document) -> Vec<String> {
        let idx = &doc.index;
        doc.bookmarks
            .elements(idx)
            .map(|id| String::from_utf8_lossy(idx.name(&doc.src, id)).into_owned())
            .collect()
    }

    #[test]
    fn marks_stay_on_their_elements_through_edits() {
        let mut d = doc();
        mark(&mut d, "/r/b");
        mark(&mut d, "/r/b/c");
        mark(&mut d, "/r/d");
        assert_eq!(marked(&d), ["b", "c", "d"]);

        // Text before them, and a rename of the element around one: the other follow.
        let id = command::resolve(&d, "/r/a").unwrap();
        d.apply(command::set_text(&d, id, "Zoë und Jörg").unwrap())
            .unwrap();
        assert_eq!(marked(&d), ["b", "c", "d"]);
        let id = command::resolve(&d, "/r/b").unwrap();
        d.apply(command::rename(&d, id, "boxes").unwrap()).unwrap();
        assert_eq!(marked(&d), ["boxes", "c", "d"]);

        // Format and Compact replace everything, and the marks stay on their elements.
        let formatted = d.format(&FormatOptions::default()).unwrap();
        d.apply(crate::Edit::whole(
            crate::StepName::Format,
            d.src.len(),
            formatted,
        ))
        .unwrap();
        assert_eq!(marked(&d), ["boxes", "c", "d"]);

        // Undo and redo, in the same way.
        d.undo();
        assert_eq!(marked(&d), ["boxes", "c", "d"]);
        d.undo();
        assert_eq!(marked(&d), ["b", "c", "d"]);
        d.redo();
        d.redo();
        assert_eq!(marked(&d), ["boxes", "c", "d"]);

        // Deleting an element ends its marks and those of what it holds.
        let id = command::resolve(&d, "/r/boxes").unwrap();
        d.apply(command::delete(&d, id).unwrap()).unwrap();
        assert_eq!(marked(&d), ["d"]);
        assert_eq!(d.bookmarks.len(), 1);
    }

    #[test]
    fn marks_do_not_change_what_is_modified_or_undone() {
        let mut d = doc();
        mark(&mut d, "/r/a");
        assert!(!d.is_modified());
        assert_eq!(d.history.undo_label(), None);
    }

    #[test]
    fn marks_follow_typing() {
        let mut d = doc();
        mark(&mut d, "/r/d");
        let at = d.index.node(command::resolve(&d, "/r/a").unwrap()).start as usize;
        d.apply_typed(
            crate::Edit::new(crate::StepName::Other("Type".into())).replace(at..at, "<new/>"),
        )
        .unwrap();
        assert_eq!(marked(&d), ["d"]);
        let start = d.bookmarks.starts()[0];
        assert_eq!(&d.src[start..start + 4], b"<d/>");
    }
}
