//! Changes to the source, with undo and redo (`docs/decisions.md`, D10).
//!
//! The document is one `Vec<u8>`. An [`Edit`] replaces byte ranges; applying it records the
//! opposite change, so undo puts back exactly the bytes that were there, whatever they were
//! (a byte order mark, CRLF, bytes that are not UTF-8). One edit is one undo step, however
//! many places it changes.

use std::ops::Range;

/// Undo history kept at most, in bytes of text the steps would put back. The newest step is
/// kept even when it alone is larger.
pub const HISTORY_BYTES: usize = 512 << 20;

/// A change to make: replace each range of the current source with new bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    label: String,
    changes: Vec<(Range<usize>, Vec<u8>)>,
}

/// Why an edit was refused. The document is unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("bytes {start}..{end} are outside the document ({len} bytes)")]
    OutOfRange {
        start: usize,
        end: usize,
        len: usize,
    },
    #[error("two changes overlap at byte {0}")]
    Overlap(usize),
    #[error("the edit changes nothing")]
    Empty,
}

/// Where a step with one change landed: `removed` bytes at `at` became `inserted` bytes. Lets
/// the index update only around the change (D9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Splice {
    pub at: usize,
    pub removed: usize,
    pub inserted: usize,
}

/// Undo and redo steps, and which one matches the file on disk.
#[derive(Debug)]
pub struct History {
    /// Steps that undo the edits made, oldest first.
    undo: Vec<Step>,
    /// Steps that redo undone edits, the next one last.
    redo: Vec<Step>,
    /// `undo.len()` when the document matched the file; `None` when no state in reach does.
    saved: Option<usize>,
    /// Most bytes to keep; see [`HISTORY_BYTES`].
    limit: usize,
}

/// One change to make: replace `len` bytes at `at` with `inserted`. The bytes it removes are
/// in the source when it runs, so they are not stored.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    at: usize,
    len: usize,
    inserted: Vec<u8>,
}

/// Changes in source order, not overlapping, with offsets in the source they apply to.
#[derive(Debug)]
struct Step {
    label: String,
    changes: Vec<Change>,
}

impl Edit {
    /// An empty edit. `label` names it in Edit > Undo, e.g. "Format".
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            changes: Vec::new(),
        }
    }

    /// Replaces the whole document.
    pub fn whole(label: impl Into<String>, len: usize, bytes: Vec<u8>) -> Self {
        Self::new(label).replace(0..len, bytes)
    }

    /// Adds a change: replace `range` of the current source with `bytes`. Ranges refer to the
    /// source before the edit; they may come in any order but must not overlap.
    pub fn replace(mut self, range: Range<usize>, bytes: impl Into<Vec<u8>>) -> Self {
        self.changes.push((range, bytes.into()));
        self
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// History of a document that matches its file.
    pub fn new() -> Self {
        Self::with_limit(HISTORY_BYTES)
    }

    /// History that keeps at most `limit` bytes (the newest step always).
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            saved: Some(0),
            limit,
        }
    }

    /// Applies `edit` to `src` and records how to undo it. Returns where the change landed
    /// when the edit made exactly one.
    pub fn apply(&mut self, src: &mut Vec<u8>, edit: Edit) -> Result<Option<Splice>, EditError> {
        let Edit { label, mut changes } = edit;
        changes.sort_by_key(|(r, _)| (r.start, r.end));
        let mut end = 0;
        for (r, _) in &changes {
            if r.start > r.end || r.end > src.len() {
                return Err(EditError::OutOfRange {
                    start: r.start,
                    end: r.end,
                    len: src.len(),
                });
            }
            if r.start < end {
                return Err(EditError::Overlap(r.start));
            }
            end = r.end;
        }
        changes.retain(|(r, b)| src[r.clone()] != b[..]);
        if changes.is_empty() {
            return Err(EditError::Empty);
        }
        let step = replace(src, changes, label);
        let splice = step.splice();
        if self.saved.is_some_and(|s| s > self.undo.len()) {
            // The saved state was among the undone steps, which are now gone.
            self.saved = None;
        }
        self.redo.clear();
        self.undo.push(step);
        self.trim();
        Ok(splice)
    }

    /// Undoes the last edit. Returns its label and, for a step with one change, where it
    /// landed; `None` when there is nothing to undo.
    pub fn undo(&mut self, src: &mut Vec<u8>) -> Option<(String, Option<Splice>)> {
        let step = self.undo.pop()?;
        let label = step.label.clone();
        let reverse = run(src, step);
        let splice = reverse.splice();
        self.redo.push(reverse);
        Some((label, splice))
    }

    /// Redoes the last undone edit. Returns what [`History::undo`] returns.
    pub fn redo(&mut self, src: &mut Vec<u8>) -> Option<(String, Option<Splice>)> {
        let step = self.redo.pop()?;
        let label = step.label.clone();
        let reverse = run(src, step);
        let splice = reverse.splice();
        self.undo.push(reverse);
        Some((label, splice))
    }

    /// What Edit > Undo would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    /// What Edit > Redo would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Whether the document differs from the file it was opened from or last saved to.
    pub fn is_modified(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    /// The document now matches its file.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
    }

    /// Bytes the history holds.
    pub fn bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(Step::bytes).sum()
    }

    /// Drops the oldest undo steps while the history is over its limit.
    fn trim(&mut self) {
        let mut total = self.bytes();
        let mut drop = 0;
        while total > self.limit && drop + 1 < self.undo.len() {
            total -= self.undo[drop].bytes();
            drop += 1;
        }
        if drop > 0 {
            self.undo.drain(..drop);
            self.saved = self.saved.and_then(|s| s.checked_sub(drop));
        }
    }
}

impl Step {
    /// For the step that reverses a one-change step: where that change landed.
    fn splice(&self) -> Option<Splice> {
        match &self.changes[..] {
            [c] => Some(Splice {
                at: c.at,
                removed: c.inserted.len(),
                inserted: c.len,
            }),
            _ => None,
        }
    }

    fn bytes(&self) -> usize {
        self.changes.iter().map(|c| c.inserted.len()).sum()
    }
}

/// Applies `step` to `src` and returns the step that reverses it.
fn run(src: &mut Vec<u8>, step: Step) -> Step {
    let forward = step
        .changes
        .into_iter()
        .map(|c| (c.at..c.at + c.len, c.inserted))
        .collect();
    replace(src, forward, step.label)
}

/// Replaces each range of `src` (sorted, not overlapping) with its bytes and returns the step
/// that puts the old bytes back.
fn replace(src: &mut Vec<u8>, changes: Vec<(Range<usize>, Vec<u8>)>, label: String) -> Step {
    let mut reverse = Vec::with_capacity(changes.len());
    match changes.len() {
        1 if changes[0].0 == (0..src.len()) => {
            // The whole document: move the old bytes into the history, copy nothing.
            let (_, bytes) = changes.into_iter().next().expect("one change");
            let len = bytes.len();
            let old = std::mem::replace(src, bytes);
            reverse.push(Change {
                at: 0,
                len,
                inserted: old,
            });
        }
        1 => {
            let (range, bytes) = changes.into_iter().next().expect("one change");
            let at = range.start;
            let len = bytes.len();
            let old: Vec<u8> = src.splice(range, bytes).collect();
            reverse.push(Change {
                at,
                len,
                inserted: old,
            });
        }
        _ => {
            // Many places: build the new source in one pass instead of moving the tail once per
            // change (D10).
            let grow: usize = changes.iter().map(|(_, b)| b.len()).sum();
            let mut out = Vec::with_capacity(src.len() + grow);
            let mut prev = 0;
            for (range, bytes) in changes {
                out.extend_from_slice(&src[prev..range.start]);
                let at = out.len();
                out.extend_from_slice(&bytes);
                reverse.push(Change {
                    at,
                    len: bytes.len(),
                    inserted: src[range.clone()].to_vec(),
                });
                prev = range.end;
            }
            out.extend_from_slice(&src[prev..]);
            *src = out;
        }
    }
    Step {
        label,
        changes: reverse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte order mark, CRLF, a byte that is not UTF-8, and non-ASCII text.
    const SRC: &[u8] = b"\xEF\xBB\xBF<r>\r\n  <a x=\"\xE4\">Gr\xC3\xBC\xC3\x9Fe</a>\r\n</r>\r\n";

    fn at(needle: &[u8]) -> Range<usize> {
        let i = SRC.windows(needle.len()).position(|w| w == needle).unwrap();
        i..i + needle.len()
    }

    #[test]
    fn undo_and_redo_restore_every_byte() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        h.apply(&mut src, Edit::new("Rename").replace(at(b"<a"), "<item"))
            .unwrap();
        assert!(src.windows(5).any(|w| w == b"<item"));
        assert_eq!(h.undo_label(), Some("Rename"));
        // The undo reports where its one change landed, for the index (D9).
        let (label, splice) = h.undo(&mut src).unwrap();
        assert_eq!(label, "Rename");
        assert_eq!(
            splice,
            Some(Splice {
                at: at(b"<a").start,
                removed: 5,
                inserted: 2
            })
        );
        assert_eq!(src, SRC);
        assert_eq!(h.redo_label(), Some("Rename"));
        h.redo(&mut src);
        assert_eq!(h.undo(&mut src).map(|(l, _)| l).as_deref(), Some("Rename"));
        assert_eq!(src, SRC);
        assert_eq!(h.undo(&mut src), None);
    }

    /// Changes may come in any order; each range refers to the source before the edit.
    #[test]
    fn many_changes_are_one_step() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        let edit = Edit::new("Rename a")
            .replace(at(b"</a>"), "</bb>")
            .replace(at(b"<a"), "<bb")
            .replace(at(b"\xE4"), "\u{e4}x");
        h.apply(&mut src, edit).unwrap();
        let expected: &[u8] =
            b"\xEF\xBB\xBF<r>\r\n  <bb x=\"\xC3\xA4x\">Gr\xC3\xBC\xC3\x9Fe</bb>\r\n</r>\r\n";
        assert_eq!(src, expected);
        h.undo(&mut src);
        assert_eq!(src, SRC);
        h.redo(&mut src);
        assert_eq!(src, expected);
    }

    #[test]
    fn whole_document_moves_into_the_history() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        h.apply(&mut src, Edit::whole("Format", SRC.len(), b"<r/>".to_vec()))
            .unwrap();
        assert_eq!(src, b"<r/>");
        assert_eq!(h.bytes(), SRC.len());
        h.undo(&mut src);
        assert_eq!(src, SRC);
        assert_eq!(h.bytes(), 4);
    }

    #[test]
    fn refused_edits_change_nothing() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        let len = SRC.len();
        assert_eq!(
            h.apply(&mut src, Edit::new("x").replace(len - 1..len + 1, "y")),
            Err(EditError::OutOfRange {
                start: len - 1,
                end: len + 1,
                len
            })
        );
        assert_eq!(
            h.apply(
                &mut src,
                Edit::new("x").replace(5..9, "y").replace(8..10, "z")
            ),
            Err(EditError::Overlap(8))
        );
        assert_eq!(
            h.apply(&mut src, Edit::new("x").replace(at(b"<r>"), "<r>")),
            Err(EditError::Empty)
        );
        assert_eq!(src, SRC);
        assert_eq!(h.undo_label(), None);
        assert!(!h.is_modified());
    }

    #[test]
    fn modified_follows_the_saved_state() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        assert!(!h.is_modified());
        h.apply(&mut src, Edit::new("1").replace(0..3, "")).unwrap();
        assert!(h.is_modified());
        h.undo(&mut src);
        assert!(!h.is_modified(), "back where the file was");
        h.redo(&mut src);
        h.mark_saved();
        assert!(!h.is_modified());
        h.undo(&mut src);
        assert!(h.is_modified(), "the file has the edit");
        // A new edit drops the redo step that led to the saved state.
        h.apply(&mut src, Edit::new("2").replace(0..1, "x"))
            .unwrap();
        h.undo(&mut src);
        assert!(h.is_modified());
        assert_eq!(h.redo_label(), Some("2"));
    }

    #[test]
    fn oldest_steps_go_first_when_over_the_limit() {
        let mut src = vec![b'a'; 100];
        let mut h = History::with_limit(150);
        for i in 0..3u8 {
            let len = src.len();
            h.apply(
                &mut src,
                Edit::whole(format!("{i}"), len, vec![b'b' + i; 100]),
            )
            .unwrap();
        }
        assert_eq!(h.bytes(), 100);
        assert_eq!(h.undo(&mut src).map(|(l, _)| l).as_deref(), Some("2"));
        assert_eq!(h.undo(&mut src), None);
        assert!(h.is_modified(), "the saved state was dropped");
        // A step larger than the limit is still kept.
        let mut h = History::with_limit(10);
        h.apply(&mut src, Edit::whole("big", 100, vec![b'z'; 100]))
            .unwrap();
        assert_eq!(h.undo_label(), Some("big"));
    }
}
