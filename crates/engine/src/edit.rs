//! Changes to the source, with undo and redo (`docs/decisions.md`, D10).
//!
//! The document is one `Vec<u8>`. An [`Edit`] replaces byte ranges; applying it records the
//! opposite change, so undo puts back exactly the bytes that were there, whatever they were
//! (a byte order mark, CRLF, bytes that are not UTF-8). One edit is one undo step, however
//! many places it changes.

use std::ops::Range;
use std::time::{Duration, Instant};

/// Up to this many changes in one step are spliced in place, last first; more are written
/// into a new buffer in one pass (D10).
const SPLICES: usize = 16;

/// Undo history kept at most, in bytes of text the steps would put back. The newest step is
/// kept even when it alone is larger.
pub const HISTORY_BYTES: usize = 512 << 20;

/// Typing that comes within this long of the last keystroke can join its undo step
/// (`docs/spec/text.md`, TEXT-20).
pub const TYPING_PAUSE: Duration = Duration::from_secs(1);

/// What an undo step is called, as Edit > Undo shows it. A value rather than a text, so that
/// the window can word it in its language (`docs/spec/language.md`, LANG-2); written out
/// ([`std::fmt::Display`]) it is the English the CLI and the MCP tools give. Element and
/// attribute names are the document's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepName {
    /// Typed in the text view.
    Typing,
    Paste,
    Cut,
    /// Backspace or Delete in the text view.
    Delete,
    Replace,
    /// Replace All, with how many matches.
    ReplaceAll(usize),
    Format,
    Compact,
    ConvertToUtf8,
    /// An element's text was set.
    EditText {
        element: String,
    },
    SetAttribute {
        attribute: String,
        element: String,
    },
    RemoveAttribute {
        attribute: String,
        element: String,
    },
    Rename {
        from: String,
        to: String,
    },
    DeleteNode(DeletedNode),
    Duplicate {
        element: String,
    },
    MoveUp {
        element: String,
    },
    MoveDown {
        element: String,
    },
    /// A caller's own name, shown as it is. The program's commands do not use it.
    Other(String),
}

/// What *Delete* removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeletedNode {
    Element(String),
    Comment,
    ProcessingInstruction,
    CData,
    Text,
}

/// A change to make: replace each range of the current source with new bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    label: StepName,
    changes: Vec<(Range<usize>, Vec<u8>)>,
    /// Typed in the text view: may join the previous undo step (TEXT-20).
    typing: Option<Typing>,
}

/// What a keystroke in the text view did. A run of one kind is undone word by word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typing {
    /// Text typed or pasted at the cursor, replacing the selection.
    Insert,
    /// Backspace or Delete.
    Delete,
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

/// Where a step landed: `removed` bytes at `at` became `inserted` bytes. For a step with
/// several changes it is the span from the first to the last. Lets the index update only
/// around the change (D9).
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
    /// The last undo step may take more typing (TEXT-20); [`History::seal`] ends it.
    open: bool,
    /// Where each change of the last edit, undo or redo landed ([`History::landed`]).
    landed: Vec<Splice>,
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
    label: StepName,
    changes: Vec<Change>,
    /// For a step of typing: its kind and when the last keystroke joined it.
    typed: Option<(Typing, Instant)>,
}

impl Edit {
    /// An empty edit. `label` names it in Edit > Undo.
    pub fn new(label: StepName) -> Self {
        Self {
            label,
            changes: Vec::new(),
            typing: None,
        }
    }

    /// Marks the edit as a keystroke of the text view, so that it can join the previous one
    /// in a single undo step (TEXT-20).
    pub fn typing(mut self, kind: Typing) -> Self {
        self.typing = Some(kind);
        self
    }

    /// Replaces the whole document.
    pub fn whole(label: StepName, len: usize, bytes: Vec<u8>) -> Self {
        Self::new(label).replace(0..len, bytes)
    }

    /// Adds a change: replace `range` of the current source with `bytes`. Ranges refer to the
    /// source before the edit; they may come in any order but must not overlap.
    pub fn replace(mut self, range: Range<usize>, bytes: impl Into<Vec<u8>>) -> Self {
        self.changes.push((range, bytes.into()));
        self
    }

    pub fn label(&self) -> &StepName {
        &self.label
    }
}

impl std::fmt::Display for StepName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepName::Typing => f.write_str("Typing"),
            StepName::Paste => f.write_str("Paste"),
            StepName::Cut => f.write_str("Cut"),
            StepName::Delete => f.write_str("Delete"),
            StepName::Replace => f.write_str("Replace"),
            StepName::ReplaceAll(1) => f.write_str("Replace All (1 match)"),
            StepName::ReplaceAll(n) => write!(f, "Replace All ({n} matches)"),
            StepName::Format => f.write_str("Format"),
            StepName::Compact => f.write_str("Compact"),
            StepName::ConvertToUtf8 => f.write_str("Convert to UTF-8"),
            StepName::EditText { element } => write!(f, "Edit <{element}>"),
            StepName::SetAttribute { attribute, element } => {
                write!(f, "Set {attribute} of <{element}>")
            }
            StepName::RemoveAttribute { attribute, element } => {
                write!(f, "Remove {attribute} of <{element}>")
            }
            StepName::Rename { from, to } => write!(f, "Rename <{from}> to <{to}>"),
            StepName::DeleteNode(node) => write!(f, "Delete {node}"),
            StepName::Duplicate { element } => write!(f, "Duplicate <{element}>"),
            StepName::MoveUp { element } => write!(f, "Move <{element}> up"),
            StepName::MoveDown { element } => write!(f, "Move <{element}> down"),
            StepName::Other(name) => f.write_str(name),
        }
    }
}

impl std::fmt::Display for DeletedNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeletedNode::Element(name) => write!(f, "<{name}>"),
            DeletedNode::Comment => f.write_str("comment"),
            DeletedNode::ProcessingInstruction => f.write_str("processing instruction"),
            DeletedNode::CData => f.write_str("CDATA"),
            DeletedNode::Text => f.write_str("text"),
        }
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
            open: false,
            landed: Vec::new(),
        }
    }

    /// Applies `edit` to `src` and records how to undo it. Returns where it landed.
    pub fn apply(&mut self, src: &mut Vec<u8>, edit: Edit) -> Result<Option<Splice>, EditError> {
        self.apply_at(src, edit, Instant::now())
    }

    /// [`History::apply`] at a given time: typing joins the last step when it continues it
    /// within [`TYPING_PAUSE`] (TEXT-20).
    pub fn apply_at(
        &mut self,
        src: &mut Vec<u8>,
        edit: Edit,
        now: Instant,
    ) -> Result<Option<Splice>, EditError> {
        let Edit {
            label,
            mut changes,
            typing,
        } = edit;
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
        let joins = match (typing, &changes[..]) {
            (Some(kind), [(range, bytes)]) => self.joins(src, kind, range, bytes, now),
            _ => false,
        };
        let mut step = replace(src, changes, label);
        let splice = step.splice();
        self.note_landed(&step);
        if self.saved.is_some_and(|s| s > self.undo.len()) {
            // The saved state was among the undone steps, which are now gone.
            self.saved = None;
        }
        self.redo.clear();
        match (joins, self.undo.last_mut()) {
            (true, Some(last)) => {
                let new = step.changes.pop().expect("one change");
                let old = &mut last.changes[0];
                *old = match typing {
                    Some(Typing::Insert) => Change {
                        at: old.at,
                        len: old.len + new.len,
                        inserted: std::mem::take(&mut old.inserted),
                    },
                    // Backspace: the new bytes came before the old ones.
                    _ if new.at < old.at => Change {
                        at: new.at,
                        len: 0,
                        inserted: [new.inserted, std::mem::take(&mut old.inserted)].concat(),
                    },
                    // Delete: after them.
                    _ => Change {
                        at: old.at,
                        len: 0,
                        inserted: [std::mem::take(&mut old.inserted), new.inserted].concat(),
                    },
                };
                last.typed = typing.map(|k| (k, now));
            }
            _ => {
                step.typed = typing.map(|k| (k, now));
                self.undo.push(step);
            }
        }
        self.open = typing.is_some();
        self.trim();
        Ok(splice)
    }

    /// Ends the run of typing: the next keystroke starts a new undo step. The text view calls
    /// it when the cursor is moved (TEXT-20).
    pub fn seal(&mut self) {
        self.open = false;
    }

    /// Whether a keystroke that replaces `range` of `src` with `bytes` continues the last
    /// step: typing of the same kind, right where it ended, within [`TYPING_PAUSE`], with
    /// nothing saved or undone since. A typed word starts a new step after anything that is
    /// not part of a word, so that undo goes word by word.
    fn joins(
        &self,
        src: &[u8],
        kind: Typing,
        range: &Range<usize>,
        bytes: &[u8],
        now: Instant,
    ) -> bool {
        let Some(last) = self.undo.last() else {
            return false;
        };
        let [old] = &last.changes[..] else {
            return false;
        };
        let continues = match last.typed {
            Some((k, at)) => k == kind && now.saturating_duration_since(at) < TYPING_PAUSE,
            None => false,
        };
        if !self.open || !continues || self.saved == Some(self.undo.len()) {
            return false;
        }
        match kind {
            Typing::Insert => {
                let typed = &src[old.at..old.at + old.len];
                range.is_empty()
                    && range.start == old.at + old.len
                    && !(starts_word(bytes) && !ends_word(typed))
            }
            Typing::Delete => {
                bytes.is_empty() && old.len == 0 && (range.end == old.at || range.start == old.at)
            }
        }
    }

    /// Undoes the last edit. Returns its label and where the change landed; `None` when there
    /// is nothing to undo.
    pub fn undo(&mut self, src: &mut Vec<u8>) -> Option<(StepName, Option<Splice>)> {
        let step = self.undo.pop()?;
        let label = step.label.clone();
        let reverse = run(src, step);
        let splice = reverse.splice();
        self.note_landed(&reverse);
        self.redo.push(reverse);
        self.open = false;
        Some((label, splice))
    }

    /// Redoes the last undone edit. Returns what [`History::undo`] returns.
    pub fn redo(&mut self, src: &mut Vec<u8>) -> Option<(StepName, Option<Splice>)> {
        let step = self.redo.pop()?;
        let label = step.label.clone();
        let reverse = run(src, step);
        let splice = reverse.splice();
        self.note_landed(&reverse);
        self.undo.push(reverse);
        self.open = false;
        Some((label, splice))
    }

    /// Where each change of the last edit, undo or redo landed, in source order: `removed`
    /// bytes of before became `inserted` ones at `at`, an offset of the new source. Unlike the
    /// [`Splice`] they return, it leaves out the bytes between the changes, so what lies
    /// there can be followed (bookmarks, TREE-15).
    pub fn landed(&self) -> &[Splice] {
        &self.landed
    }

    fn note_landed(&mut self, step: &Step) {
        self.landed.clear();
        self.landed.extend(step.changes.iter().map(|c| Splice {
            at: c.at,
            removed: c.inserted.len(),
            inserted: c.len,
        }));
    }

    /// What Edit > Undo would undo.
    pub fn undo_label(&self) -> Option<&StepName> {
        self.undo.last().map(|s| &s.label)
    }

    /// What Edit > Redo would redo.
    pub fn redo_label(&self) -> Option<&StepName> {
        self.redo.last().map(|s| &s.label)
    }

    /// Whether the document differs from the file it was opened from or last saved to.
    pub fn is_modified(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    /// The document now matches its file.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
        self.open = false;
    }

    /// The file no longer matches the document, whatever undo or redo does, until the next
    /// save.
    pub fn mark_unsaved(&mut self) {
        self.saved = None;
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
    /// For the step that reverses a step: where that step landed, from its first change to
    /// its last.
    fn splice(&self) -> Option<Splice> {
        let (first, last) = (self.changes.first()?, self.changes.last()?);
        let inserted = last.at + last.len - first.at;
        let new: usize = self.changes.iter().map(|c| c.len).sum();
        let old: usize = self.changes.iter().map(|c| c.inserted.len()).sum();
        Some(Splice {
            at: first.at,
            removed: inserted - new + old,
            inserted,
        })
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
fn replace(src: &mut Vec<u8>, changes: Vec<(Range<usize>, Vec<u8>)>, label: StepName) -> Step {
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
        n if n <= SPLICES => {
            // A few places: splice each, the last first so earlier offsets stay valid. In the
            // new source a change starts later by what the changes before it added.
            let mut shift = 0i64;
            let ats: Vec<usize> = changes
                .iter()
                .map(|(r, b)| {
                    let at = r.start.wrapping_add_signed(shift as isize);
                    shift += b.len() as i64 - r.len() as i64;
                    at
                })
                .collect();
            let mut olds = Vec::with_capacity(n);
            for (range, bytes) in changes.iter().rev() {
                olds.push(
                    src.splice(range.clone(), bytes.iter().copied())
                        .collect::<Vec<u8>>(),
                );
            }
            olds.reverse();
            for ((at, (_, bytes)), old) in ats.into_iter().zip(&changes).zip(olds) {
                reverse.push(Change {
                    at,
                    len: bytes.len(),
                    inserted: old,
                });
            }
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
        typed: None,
    }
}

/// Whether `bytes` begin with a letter, digit or `_`.
fn starts_word(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(&bytes[..bytes.len().min(4)])
        .chars()
        .next()
        .is_some_and(crate::text::is_word_char)
}

/// Whether `bytes` end with a letter, digit or `_`.
fn ends_word(bytes: &[u8]) -> bool {
    let tail = &bytes[bytes.len().saturating_sub(4)..];
    String::from_utf8_lossy(tail)
        .chars()
        .next_back()
        .is_some_and(crate::text::is_word_char)
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
        h.apply(
            &mut src,
            Edit::new(StepName::Other("Rename".into())).replace(at(b"<a"), "<item"),
        )
        .unwrap();
        assert!(src.windows(5).any(|w| w == b"<item"));
        assert_eq!(h.undo_label(), Some(&StepName::Other("Rename".into())));
        // The undo reports where its one change landed, for the index (D9).
        let (label, splice) = h.undo(&mut src).unwrap();
        assert_eq!(label, StepName::Other("Rename".into()));
        assert_eq!(
            splice,
            Some(Splice {
                at: at(b"<a").start,
                removed: 5,
                inserted: 2
            })
        );
        assert_eq!(src, SRC);
        assert_eq!(h.redo_label(), Some(&StepName::Other("Rename".into())));
        h.redo(&mut src);
        assert_eq!(
            h.undo(&mut src).map(|(l, _)| l),
            Some(StepName::Other("Rename".into()))
        );
        assert_eq!(src, SRC);
        assert_eq!(h.undo(&mut src), None);
    }

    /// Changes may come in any order; each range refers to the source before the edit.
    #[test]
    fn many_changes_are_one_step() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        let edit = Edit::new(StepName::Other("Rename a".into()))
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

    /// A few changes are spliced in place, many are written in one pass; both give the same
    /// bytes as building the result by hand, undo exactly, and report the span from the
    /// first change to the last.
    #[test]
    fn few_and_many_changes_agree() {
        let base: Vec<u8> = (0..400u32).map(|i| b'a' + (i % 26) as u8).collect();
        for count in [2usize, 5, SPLICES, SPLICES + 1, 40] {
            // Every ninth byte range, replaced by a string of varying length.
            let ranges: Vec<(Range<usize>, Vec<u8>)> = (0..count)
                .map(|i| {
                    let start = 3 + i * 9;
                    (start..start + i % 4, vec![b'Z'; (i * 7) % 5])
                })
                .collect();
            let mut expected = Vec::new();
            let mut prev = 0;
            for (r, b) in &ranges {
                expected.extend_from_slice(&base[prev..r.start]);
                expected.extend_from_slice(b);
                prev = r.end;
            }
            expected.extend_from_slice(&base[prev..]);

            let mut src = base.clone();
            let mut h = History::new();
            let mut edit = Edit::new(StepName::Other("many".into()));
            for (r, b) in ranges.iter().rev() {
                edit = edit.replace(r.clone(), b.clone());
            }
            let splice = h.apply(&mut src, edit).unwrap().unwrap();
            assert_eq!(src, expected, "{count} changes");
            let first = ranges
                .iter()
                .find(|(r, b)| base[r.clone()] != b[..])
                .unwrap();
            let last = ranges
                .iter()
                .rev()
                .find(|(r, b)| base[r.clone()] != b[..])
                .unwrap();
            assert_eq!(splice.at, first.0.start, "{count}");
            assert_eq!(splice.removed, last.0.end - first.0.start, "{count}");
            assert_eq!(base.len() - splice.removed + splice.inserted, src.len());
            assert_eq!(src[..splice.at], base[..splice.at]);
            assert_eq!(
                src[splice.at + splice.inserted..],
                base[splice.at + splice.removed..]
            );
            let (_, back) = h.undo(&mut src).unwrap();
            assert_eq!(src, base, "{count} undone");
            assert_eq!(back.unwrap().inserted, splice.removed);
            h.redo(&mut src);
            assert_eq!(src, expected, "{count} redone");
        }
    }

    #[test]
    fn whole_document_moves_into_the_history() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        h.apply(
            &mut src,
            Edit::whole(StepName::Format, SRC.len(), b"<r/>".to_vec()),
        )
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
            h.apply(
                &mut src,
                Edit::new(StepName::Other("x".into())).replace(len - 1..len + 1, "y")
            ),
            Err(EditError::OutOfRange {
                start: len - 1,
                end: len + 1,
                len
            })
        );
        assert_eq!(
            h.apply(
                &mut src,
                Edit::new(StepName::Other("x".into()))
                    .replace(5..9, "y")
                    .replace(8..10, "z")
            ),
            Err(EditError::Overlap(8))
        );
        assert_eq!(
            h.apply(
                &mut src,
                Edit::new(StepName::Other("x".into())).replace(at(b"<r>"), "<r>")
            ),
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
        h.apply(
            &mut src,
            Edit::new(StepName::Other("1".into())).replace(0..3, ""),
        )
        .unwrap();
        assert!(h.is_modified());
        h.undo(&mut src);
        assert!(!h.is_modified(), "back where the file was");
        h.redo(&mut src);
        h.mark_saved();
        assert!(!h.is_modified());
        h.undo(&mut src);
        assert!(h.is_modified(), "the file has the edit");
        // A new edit drops the redo step that led to the saved state.
        h.apply(
            &mut src,
            Edit::new(StepName::Other("2".into())).replace(0..1, "x"),
        )
        .unwrap();
        h.undo(&mut src);
        assert!(h.is_modified());
        assert_eq!(h.redo_label(), Some(&StepName::Other("2".into())));
    }

    #[test]
    fn oldest_steps_go_first_when_over_the_limit() {
        let mut src = vec![b'a'; 100];
        let mut h = History::with_limit(150);
        for i in 0..3u8 {
            let len = src.len();
            h.apply(
                &mut src,
                Edit::whole(StepName::Other(format!("{i}")), len, vec![b'b' + i; 100]),
            )
            .unwrap();
        }
        assert_eq!(h.bytes(), 100);
        assert_eq!(
            h.undo(&mut src).map(|(l, _)| l),
            Some(StepName::Other("2".into()))
        );
        assert_eq!(h.undo(&mut src), None);
        assert!(h.is_modified(), "the saved state was dropped");
        // A step larger than the limit is still kept.
        let mut h = History::with_limit(10);
        h.apply(
            &mut src,
            Edit::whole(StepName::Other("big".into()), 100, vec![b'z'; 100]),
        )
        .unwrap();
        assert_eq!(h.undo_label(), Some(&StepName::Other("big".into())));
    }
}
