//! Document model and commands.
//!
//! Every user-visible operation is a function here, so the GUI, the CLI and (later) the MCP
//! server share one implementation and one set of tests.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use xml_atlas_xml::{Index, NodeId};

use crate::format::newline_style;

pub use disk::{DiskChange, FileStamp};
pub use edit::{Edit, EditError, History, Splice, Typing};
pub use encoding::{FileEncoding, Unencodable};
pub use format::{
    DocumentOptions, FormatError, FormatOptions, compact, compact_indexed, format, format_indexed,
};
pub use xml_atlas_xml as xml;

pub mod command;
pub mod disk;
pub mod edit;
pub mod encoding;
pub mod find;
pub mod format;
pub mod grid;
pub mod problems;
pub mod text;
pub mod tree;
pub mod wrap;

/// How long typing pauses before a document whose index lags behind its text is read again
/// (`docs/spec/text.md`, TEXT-22).
pub const REPARSE_PAUSE: Duration = Duration::from_millis(500);

/// Documents up to this size are read again on the UI thread; larger ones on a worker
/// thread with [`Document::reparse_job`] (`docs/decisions.md`, D19).
pub const REPARSE_INLINE_BYTES: usize = 20 << 20;

/// A document read again off the UI thread: run it anywhere, then hand the result to
/// [`Document::finish_reparse`].
pub struct ReparseJob {
    revision: u64,
    src: Vec<u8>,
}

/// What a [`ReparseJob`] found.
pub struct Reparsed {
    revision: u64,
    index: Index,
    took: Duration,
}

/// The node index lags behind the text: a typed change broke it (TEXT-10, D19).
#[derive(Clone, Copy, Debug)]
struct Stale {
    /// The innermost element around everything typed since the index was last whole.
    element: NodeId,
    /// The nodes kept are the last well-formed state; `false` when the document was not
    /// well-formed when they were read.
    good: bool,
    /// When the last key was typed; `None` once the document was read again since.
    typed: Option<Instant>,
}

/// An open XML document: the source bytes, its node index and a line index.
pub struct Document {
    pub path: Option<PathBuf>,
    pub src: Vec<u8>,
    pub index: Index,
    /// Byte offset where each line starts. Always contains at least `0`.
    pub line_starts: Vec<u64>,
    pub timings: Timings,
    /// Undo and redo, and whether the source differs from the file.
    pub history: History,
    /// Counts changes to the source, so views can tell that what they computed is stale.
    revision: u64,
    /// The file as it was when last read or written; `None` when there was none (OPEN-14).
    disk: Option<FileStamp>,
    /// Where the last edit, undo or redo landed, so the text cursor can go there (TEXT-20).
    last_change: Option<Splice>,
    /// The file's line ending, kept while an edit leaves no line break to tell it by.
    newline: &'static [u8],
    /// Set while typing has broken the text and the index lags behind it.
    stale: Option<Stale>,
    /// The file's encoding when it is not UTF-8: `src` holds the text converted to UTF-8,
    /// and saving converts it back (OPEN-8).
    encoding: Option<FileEncoding>,
    /// A declared encoding that is not used, and why (OPEN-13).
    encoding_note: Option<String>,
}

/// Why a document could not be saved.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The text cannot be written in the file's encoding; Save as UTF-8 can
    /// ([`Document::convert_to_utf8`]).
    #[error(transparent)]
    Encoding(#[from] Unencodable),
}

/// How long each stage of opening took.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub read: Duration,
    /// Converting a file that is not UTF-8 (OPEN-8); zero for UTF-8.
    pub decode: Duration,
    pub index: Duration,
    pub lines: Duration,
}

impl ReparseJob {
    /// Indexes the copied source; the slow part, safe on any thread.
    pub fn run(self) -> Reparsed {
        let t = Instant::now();
        let index = xml_atlas_xml::index(&self.src);
        Reparsed {
            revision: self.revision,
            index,
            took: t.elapsed(),
        }
    }
}

impl Timings {
    pub fn total(&self) -> Duration {
        self.read + self.decode + self.index + self.lines
    }
}

impl Document {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let t = Instant::now();
        // The stamp is taken before reading, so a write during the read shows as a change.
        let mut file = File::open(path)?;
        let meta = file.metadata()?;
        let mut src = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
        file.read_to_end(&mut src)?;
        let read = t.elapsed();
        let mut doc = Self::from_bytes(src);
        doc.timings.read = read;
        doc.path = Some(path.to_path_buf());
        doc.disk = Some(FileStamp::from_metadata(&meta));
        log::info!("opened {} in {:?}", path.display(), doc.timings.total());
        Ok(doc)
    }

    /// The document of a file's bytes: converted to UTF-8 when the file is in another
    /// encoding (OPEN-8), as they are otherwise.
    pub fn from_bytes(file: Vec<u8>) -> Self {
        let t = Instant::now();
        let (src, opened) = encoding::decode(file);
        let decode_time = t.elapsed();
        let t = Instant::now();
        let index = xml_atlas_xml::index(&src);
        let index_time = t.elapsed();
        let t = Instant::now();
        let line_starts = line_starts(&src);
        let lines_time = t.elapsed();
        let newline = newline_style(&src);
        Self {
            path: None,
            src,
            index,
            line_starts,
            timings: Timings {
                read: Duration::ZERO,
                decode: decode_time,
                index: index_time,
                lines: lines_time,
            },
            history: History::new(),
            revision: 0,
            disk: None,
            last_change: None,
            newline,
            stale: None,
            encoding: opened.encoding,
            encoding_note: opened.ignored,
        }
    }

    /// The file's encoding when it is not UTF-8 (OPEN-8).
    pub fn encoding(&self) -> Option<&FileEncoding> {
        self.encoding.as_ref()
    }

    /// The bytes [`save`](Self::save) writes: the source in the file's encoding.
    pub fn encoded(&self) -> Result<std::borrow::Cow<'_, [u8]>, Unencodable> {
        encoding::encode(&self.src, self.encoding.as_ref())
    }

    /// Save as UTF-8: from now on the document is saved in UTF-8, without a byte order
    /// mark. The XML declaration's `encoding` says UTF-8 too, changed as an edit that can be
    /// undone. Returns whether anything changed.
    pub fn convert_to_utf8(&mut self) -> bool {
        let Some(enc) = self.encoding.take() else {
            return false;
        };
        if let Some((label, at)) = encoding::declared(&self.src)
            && !label.eq_ignore_ascii_case("utf-8")
        {
            let edit =
                Edit::new("Convert to UTF-8").replace(at..at + label.len(), b"UTF-8".to_vec());
            if let Err(e) = self.apply(edit) {
                log::warn!("declaration not changed: {e}");
            }
        } else {
            // Nothing in the text changes, but the file will.
            self.history.mark_unsaved();
        }
        self.encoding_note = None;
        log::info!("converted from {} to UTF-8", enc.label);
        true
    }

    /// A declared encoding that is not used, and why.
    pub fn encoding_note(&self) -> Option<&str> {
        self.encoding_note.as_deref()
    }

    /// Applies `edit` as one undo step and indexes the document again. On error nothing
    /// changes.
    pub fn apply(&mut self, edit: Edit) -> Result<(), EditError> {
        let splice = self.history.apply(&mut self.src, edit)?;
        self.reindex(splice, false);
        Ok(())
    }

    /// [`Document::apply`] for what is typed, cut or pasted in the text view (D19): when the
    /// element around the change is no longer well-formed, the whole document is not read
    /// again. The nodes are kept (`Index::keep_stale`), the element is marked stale
    /// ([`Document::stale`]), and the document is read again after a pause in typing
    /// ([`Document::reparse_wait`]).
    pub fn apply_typed(&mut self, edit: Edit) -> Result<(), EditError> {
        let splice = self.history.apply(&mut self.src, edit)?;
        self.reindex(splice, true);
        Ok(())
    }

    /// Which node ids the last edit, undo or redo replaced, when only the nodes around it
    /// were indexed again; `None` when the whole document was. Views follow such a change
    /// without building their state again (D19).
    pub fn node_splice(&self) -> Option<xml::NodeSplice> {
        self.index.last_splice
    }

    /// The element whose nodes are kept from before the text around it was broken by typing,
    /// so that tree and grid can mark it (TEXT-10). `None` when the index is up to date.
    pub fn stale(&self) -> Option<NodeId> {
        self.stale.map(|s| s.element)
    }

    /// How long until the document is to be read again: zero when it is due, `None` when
    /// nothing waits.
    pub fn reparse_wait(&self, now: Instant) -> Option<Duration> {
        let typed = self.stale?.typed?;
        Some(REPARSE_PAUSE.saturating_sub(now.saturating_duration_since(typed)))
    }

    /// Reads the document again now if it waits for it (see [`Document::reparse_wait`]).
    /// Returns whether the index changed. Commands that work from the index call this
    /// first, so they never see a stale one.
    pub fn reparse(&mut self) -> bool {
        match self.reparse_job() {
            Some(job) => {
                let done = job.run();
                self.finish_reparse(done)
            }
            None => false,
        }
    }

    /// A copy of the source to read again on another thread, if the document waits for it.
    pub fn reparse_job(&self) -> Option<ReparseJob> {
        self.stale?.typed?;
        Some(ReparseJob {
            revision: self.revision,
            src: self.src.clone(),
        })
    }

    /// Takes in what a [`ReparseJob`] found, unless the text changed since it started or the
    /// document was read again meanwhile. A well-formed result replaces the index. A result
    /// with an error keeps the last well-formed nodes and only takes the error, for the
    /// problems list (TEXT-10). Returns whether the index changed.
    pub fn finish_reparse(&mut self, done: Reparsed) -> bool {
        let Some(stale) = self.stale.filter(|s| s.typed.is_some()) else {
            return false;
        };
        if done.revision != self.revision {
            return false;
        }
        if done.index.error.is_none() || !stale.good {
            self.index = done.index;
            self.stale = None;
        } else {
            self.index.error = done.index.error;
            self.index.more_errors = done.index.more_errors;
            self.index.last_splice = None;
            self.stale = Some(Stale {
                typed: None,
                ..stale
            });
        }
        self.timings.index = done.took;
        true
    }

    /// Undoes the last edit; returns its label.
    pub fn undo(&mut self) -> Option<String> {
        let (label, splice) = self.history.undo(&mut self.src)?;
        self.reindex(splice, false);
        Some(label)
    }

    /// Redoes the last undone edit; returns its label.
    pub fn redo(&mut self) -> Option<String> {
        let (label, splice) = self.history.redo(&mut self.src)?;
        self.reindex(splice, false);
        Some(label)
    }

    /// Whether the source differs from the file it was opened from or last saved to.
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Where the last edit, undo or redo landed: the bytes it removed and inserted.
    pub fn last_change(&self) -> Option<Splice> {
        self.last_change
    }

    /// The file's line ending, `\r\n` or `\n`, for line breaks that edits insert (EDIT-5,
    /// TEXT-17, TEXT-18): the one of the first line, or when no line break is left, the one
    /// the document had last. LF for a file that never had one.
    pub fn newline(&self) -> &'static [u8] {
        self.newline
    }

    /// Goes up with every change to the source: an edit, an undo, a redo.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Brings the node and line index up to date after the source changed. A single change
    /// re-indexes only the element around it when it can (D9). Otherwise a `typed` change
    /// keeps the nodes and marks them stale (D19), and anything else indexes the whole
    /// document, as does any change while the index is stale.
    fn reindex(&mut self, splice: Option<Splice>, typed: bool) {
        self.revision += 1;
        self.last_change = splice;
        let t = Instant::now();
        let stale = if typed { self.stale } else { self.stale.take() };
        self.index.last_splice = None;
        let updated = stale.is_none()
            && splice.is_some_and(|s| self.index.update(&self.src, s.at, s.removed, s.inserted));
        match splice {
            _ if updated => {}
            Some(s) if typed => {
                let good = stale.map_or(self.index.error.is_none(), |st| st.good);
                let before = stale.map(|st| st.element);
                let element = self
                    .index
                    .keep_stale(&self.src, s.at, s.removed, s.inserted, before);
                self.stale = Some(Stale {
                    element,
                    good,
                    typed: Some(Instant::now()),
                });
            }
            _ => {
                self.index = xml_atlas_xml::index(&self.src);
                self.stale = None;
            }
        }
        self.timings.index = t.elapsed();
        let t = Instant::now();
        match splice {
            Some(s) => update_line_starts(&mut self.line_starts, &self.src, s),
            None => self.line_starts = line_starts(&self.src),
        }
        self.timings.lines = t.elapsed();
        self.timings.read = Duration::ZERO;
        if self.line_starts.len() > 1 {
            self.newline = newline_style(&self.src);
        }
    }

    /// Re-indented copy of the source. See [`format`] for what is and is not changed.
    pub fn format(&self, options: &FormatOptions) -> Result<Vec<u8>, FormatError> {
        format_indexed(&self.src, &self.index, options)
    }

    /// One-line copy of the source; the opposite of [`format`](Self::format). See
    /// [`compact`].
    pub fn compact(&self, options: &FormatOptions) -> Result<Vec<u8>, FormatError> {
        compact_indexed(&self.src, &self.index, options)
    }

    /// The options Format uses for this document: `base`, overridden by the document's own
    /// settings comment if it has one (`docs/spec/format.md`, FMT-15).
    pub fn format_options(&self, base: FormatOptions) -> DocumentOptions {
        format::options_for(&self.src, &self.index, base)
    }

    /// Writes the source to `path` with [`save_atomic`], in the file's encoding, and makes
    /// it the document's path. Text that the encoding cannot hold is refused with where it
    /// is, and nothing is written (OPEN-8).
    pub fn save(&mut self, path: &Path) -> Result<(), SaveError> {
        save_atomic(path, &self.encoded()?)?;
        self.path = Some(path.to_path_buf());
        self.history.mark_saved();
        self.disk = FileStamp::of(path).unwrap_or(None);
        Ok(())
    }

    /// Whether the file was changed or removed on disk since the document last read or wrote
    /// it (OPEN-14). `None` also when there is no file to compare with, or it cannot be
    /// looked at.
    pub fn disk_change(&self) -> Option<DiskChange> {
        let path = self.path.as_deref()?;
        let now = FileStamp::of(path).ok()?;
        disk::compare(self.disk, now)
    }

    /// Keeps the document as it is although its file changed on disk: the change is not
    /// reported again, and the document counts as modified, so that Save writes it and
    /// closing asks first.
    pub fn keep_over_disk(&mut self) {
        if let Some(path) = self.path.as_deref()
            && let Ok(now) = FileStamp::of(path)
        {
            self.disk = now;
        }
        self.history.mark_unsaved();
    }

    /// Takes the contents of `fresh`, the same file read again: the edits and their undo
    /// history are gone. Views see a new [`revision`](Self::revision).
    pub fn reload_from(&mut self, fresh: Document) {
        let revision = self.revision + 1;
        *self = fresh;
        self.revision = revision;
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Zero-based line containing byte `offset`.
    pub fn line_of(&self, offset: u64) -> usize {
        self.line_starts
            .partition_point(|&s| s <= offset)
            .saturating_sub(1)
    }

    /// Bytes of line `n` without the line terminator.
    pub fn line(&self, n: usize) -> &[u8] {
        let start = self.line_starts[n] as usize;
        let end = self
            .line_starts
            .get(n + 1)
            .map_or(self.src.len(), |&e| e as usize);
        let line = &self.src[start..end];
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        line.strip_suffix(b"\r").unwrap_or(line)
    }

    /// Lines covered by a node, inclusive.
    pub fn node_lines(&self, id: NodeId) -> (usize, usize) {
        let n = self.index.node(id);
        (
            self.line_of(n.start),
            self.line_of(n.end.saturating_sub(1).max(n.start)),
        )
    }

    pub fn stats(&self) -> Stats {
        Stats {
            bytes: self.src.len() as u64,
            lines: self.line_count() as u64,
            nodes: self.index.len() as u64,
            elements: self.index.element_count as u64,
            max_depth: self.index.max_depth,
            index_memory: (self.index.nodes.capacity() * std::mem::size_of::<xml::Node>()
                + self.line_starts.capacity() * 8) as u64,
            error: self.index.error.as_ref().map(|e| e.to_string()),
            timings: self.timings,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Stats {
    pub bytes: u64,
    pub lines: u64,
    pub nodes: u64,
    pub elements: u64,
    pub max_depth: u16,
    pub index_memory: u64,
    pub error: Option<String>,
    pub timings: Timings,
}

fn line_starts(src: &[u8]) -> Vec<u64> {
    let mut starts = Vec::with_capacity(src.len() / 32 + 1);
    starts.push(0);
    starts.extend(memchr::memchr_iter(b'\n', src).map(|i| i as u64 + 1));
    starts
}

/// Updates `starts` after `s.removed` bytes at `s.at` became `s.inserted` bytes of `src`:
/// lines that began inside the removed bytes go, lines beginning in the inserted bytes come
/// in, and every later line start moves by the difference.
fn update_line_starts(starts: &mut Vec<u64>, src: &[u8], s: Splice) {
    let (at, old_end) = (s.at as u64, (s.at + s.removed) as u64);
    let delta = s.inserted as i64 - s.removed as i64;
    let lo = starts.partition_point(|&p| p <= at);
    let hi = starts.partition_point(|&p| p <= old_end);
    for p in &mut starts[hi..] {
        *p = p.wrapping_add_signed(delta);
    }
    let new =
        memchr::memchr_iter(b'\n', &src[s.at..s.at + s.inserted]).map(|i| (s.at + i + 1) as u64);
    starts.splice(lo..hi, new);
}

/// `s` without leading and trailing whitespace, cut after `max` characters with an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Writes `bytes` to `path` without ever leaving a half-written file there.
///
/// The data goes to a temporary file in the same directory, is flushed to disk, and then
/// replaces `path` in one rename. If anything fails, the old file is untouched. A symbolic
/// link is followed, and the permissions of an existing file are kept.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = write_and_replace(&tmp, &target, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_and_replace(tmp: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Ok(meta) = fs::metadata(target) {
        fs::set_permissions(tmp, meta.permissions())?;
    }
    fs::rename(tmp, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The index and line index after an edit, undo or redo equal those of opening the new
    /// source, whether the index was updated in place (D9) or built again.
    fn assert_fresh(doc: &Document, what: &str) {
        let fresh = Document::from_bytes(doc.src.clone());
        assert_eq!(doc.index.nodes, fresh.index.nodes, "{what}");
        assert_eq!(doc.index.element_count, fresh.index.element_count, "{what}");
        assert_eq!(
            doc.index
                .error
                .as_ref()
                .map(|e| (e.offset, e.message.clone())),
            fresh
                .index
                .error
                .as_ref()
                .map(|e| (e.offset, e.message.clone())),
            "{what}"
        );
        assert_eq!(doc.line_starts, fresh.line_starts, "{what}");
    }

    #[test]
    fn edits_undo_and_redo_keep_the_indexes_exact() {
        let src = "\u{feff}<?xml version=\"1.0\"?>\r\n<r>\r\n  <a x=\"1\">Grüße</a>\r\n  <b>\n    <c/>text<d>&amp;</d>\n  </b>\r\n  <![CDATA[x]]>\r\n</r>\r\n";
        let snippets = ["<e/>", "\r\n", "\n  <f g=\"h\">i</f>", "ü", "<", "", "x\ny"];
        let mut doc = Document::from_bytes(src.as_bytes().to_vec());
        let mut seed: u64 = 7;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 33) as usize) % n.max(1)
        };
        let mut steps = 0;
        for _ in 0..300 {
            let at = next(doc.src.len() + 1);
            let len = next(5).min(doc.src.len() - at);
            let bytes = snippets[next(snippets.len())];
            if doc
                .apply(Edit::new("e").replace(at..at + len, bytes))
                .is_ok()
            {
                steps += 1;
                assert_fresh(&doc, &format!("edit at {at} -{len} +{bytes:?}"));
            }
        }
        for i in 0..steps {
            assert!(doc.undo().is_some());
            assert_fresh(&doc, &format!("undo {i}"));
        }
        assert_eq!(doc.src, src.as_bytes());
        assert!(!doc.is_modified());
        for i in 0..steps {
            assert!(doc.redo().is_some());
            assert_fresh(&doc, &format!("redo {i}"));
        }
    }

    /// An edit, its undo and its redo each leave an index and line index that match the
    /// source; saving makes the document unmodified.
    #[test]
    fn edits_reindex_and_saving_clears_modified() {
        let src = b"<r>\r\n  <a>1</a>\r\n</r>".to_vec();
        let mut doc = Document::from_bytes(src.clone());
        let a = doc.src.windows(8).position(|w| w == b"<a>1</a>").unwrap();
        doc.apply(Edit::new("Add").replace(a + 8..a + 8, "\r\n  <b/>"))
            .unwrap();
        assert!(doc.is_modified());
        assert_eq!(doc.index.element_count, 3);
        assert_eq!(doc.line_count(), 4);
        assert_eq!(doc.line(2), b"  <b/>");
        assert_eq!(doc.undo().as_deref(), Some("Add"));
        assert_eq!(doc.src, src);
        assert_eq!(doc.index.element_count, 2);
        assert_eq!(doc.line_count(), 3);
        assert!(!doc.is_modified());
        doc.redo();
        assert_eq!(doc.index.element_count, 3);

        let dir = std::env::temp_dir().join(format!("xml-atlas-edit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.xml");
        doc.save(&path).unwrap();
        assert!(!doc.is_modified());
        assert_eq!(fs::read(&path).unwrap(), doc.src);
        doc.undo();
        assert!(doc.is_modified());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn line_index() {
        let doc = Document::from_bytes(b"<a>\r\n  <b/>\n</a>".to_vec());
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line(0), b"<a>");
        assert_eq!(doc.line(1), b"  <b/>");
        let b = doc
            .index
            .children(doc.index.root_element().unwrap())
            .next()
            .unwrap();
        assert_eq!(doc.node_lines(b), (1, 1));
        assert_eq!(doc.node_lines(doc.index.root_element().unwrap()), (0, 2));
    }

    #[test]
    fn save_replaces_file_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.xml");
        fs::write(&path, b"<old/>").unwrap();
        save_atomic(&path, b"<new/>").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"<new/>");
        save_atomic(&dir.join("fresh.xml"), b"<fresh/>").unwrap();
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["doc.xml", "fresh.xml"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A changed or removed file is reported until the document reloads, saves or keeps its
    /// own version (OPEN-14).
    #[test]
    fn disk_changes_are_seen() {
        let dir = std::env::temp_dir().join(format!("xml-atlas-disk-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bestellungen-ä.xml");
        fs::write(&path, "\u{feff}<a>\r\n  <b>Grüße</b>\r\n</a>").unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.disk_change(), None);

        // The same length and only a later time is a change too.
        let len = fs::metadata(&path).unwrap().len() as usize;
        let later = fs::metadata(&path).unwrap().modified().unwrap() + Duration::from_secs(2);
        fs::write(&path, "\u{feff}<a>\r\n  <b>Grüßa</b>\r\n</a>").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len() as usize, len);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(doc.disk_change(), Some(DiskChange::Changed));

        // Reloading takes the new contents and forgets the old.
        doc.apply(Edit::new("Edit").replace(3..3, "\n")).unwrap();
        assert!(doc.is_modified());
        let revision = doc.revision();
        doc.reload_from(Document::open(&path).unwrap());
        assert!(doc.revision() > revision);
        assert!(!doc.is_modified());
        assert!(doc.history.undo_label().is_none());
        assert!(String::from_utf8_lossy(&doc.src).contains("Grüßa"));
        assert_eq!(doc.disk_change(), None);

        // Keeping the open version: not reported again, and Save is offered.
        fs::write(&path, "<a/>").unwrap();
        assert_eq!(doc.disk_change(), Some(DiskChange::Changed));
        doc.keep_over_disk();
        assert_eq!(doc.disk_change(), None);
        assert!(doc.is_modified());
        doc.save(&path).unwrap();
        assert_eq!(doc.disk_change(), None, "our own save is no change");
        assert!(!doc.is_modified());

        fs::remove_file(&path).unwrap();
        assert_eq!(doc.disk_change(), Some(DiskChange::Removed));
        doc.keep_over_disk();
        assert_eq!(doc.disk_change(), None);
        assert!(doc.is_modified());
        // A file that appears again where it was is a change.
        fs::write(&path, "<a/>").unwrap();
        assert_eq!(doc.disk_change(), Some(DiskChange::Changed));

        // Saving under another name compares with that file from then on.
        let other = dir.join("kopie.xml");
        doc.save(&other).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(doc.disk_change(), None);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Document::from_bytes(b"<a/>".to_vec()).disk_change(), None);
    }

    /// Types `text` at the first `at` (after it with `after`), as the text view does.
    fn type_at(doc: &mut Document, at: &str, text: &str) {
        let i = doc
            .src
            .windows(at.len())
            .position(|w| w == at.as_bytes())
            .unwrap();
        let change = text::insert(doc, text::Selection::cursor(i), text).unwrap();
        doc.apply_typed(change.edit).unwrap();
    }

    /// Deletes the first `len` bytes of the first `at`, as Backspace on a selection does.
    fn delete_typed(doc: &mut Document, at: &str, len: usize) {
        let i = doc
            .src
            .windows(at.len())
            .position(|w| w == at.as_bytes())
            .unwrap();
        let sel = text::Selection {
            anchor: i,
            head: i + len,
        };
        let change = text::backspace(doc, sel, false).unwrap();
        doc.apply_typed(change.edit).unwrap();
    }

    /// The grid of every element, built as the views build it: nothing panics.
    fn build_views(doc: &Document) {
        for id in 0..doc.index.len() as NodeId {
            if doc.index.node(id).kind == xml::NodeKind::Element {
                let _ = grid::Grid::build(doc, id);
                let _ = doc.index.path(&doc.src, id);
            }
        }
    }

    /// Typing that breaks an element keeps the nodes and marks the element (TEXT-10); the
    /// document is read again after a pause and takes only the error; mending it brings
    /// back an exact index (D19).
    #[test]
    fn typing_that_breaks_the_text_keeps_the_last_good_nodes() {
        let src = "\u{FEFF}<r>\r\n  <a>Grüße</a>\r\n  <b n=\"1\"/>\r\n</r>\r\n";
        let mut doc = Document::from_bytes(src.as_bytes().to_vec());
        let elements = doc.index.element_count;
        let nodes = doc.index.len();

        // A letter: the local update, nothing stale.
        type_at(&mut doc, "üße", "x");
        assert_eq!(doc.stale(), None);
        assert_fresh(&doc, "a letter");

        // `<`: `a` is stale, every node stays, nothing waits for a full read yet.
        type_at(&mut doc, "üße", "<");
        let a = doc.stale().expect("stale");
        assert_eq!(doc.index.name(&doc.src, a), b"a");
        assert_eq!(
            (doc.index.element_count, doc.index.len()),
            (elements, nodes)
        );
        assert!(doc.index.error.is_none(), "not read yet");
        let now = Instant::now();
        let wait = doc.reparse_wait(now).expect("waits");
        assert!(wait > Duration::ZERO && wait <= REPARSE_PAUSE);
        assert_eq!(doc.reparse_wait(now + REPARSE_PAUSE), Some(Duration::ZERO));
        build_views(&doc);

        // Read again: still broken, so the nodes stay and the error is taken.
        assert!(doc.reparse());
        assert_eq!(doc.stale(), Some(a));
        assert_eq!(doc.index.len(), nodes);
        assert_eq!(doc.reparse_wait(now), None, "nothing waits after the read");
        assert!(!doc.reparse(), "nothing to do twice");
        // `<üße</a>` is read as a start tag, so it and `a` are not closed.
        let problems = doc.problems();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(
            problems
                .iter()
                .all(|p| p.severity == problems::Severity::Error)
        );

        // More typing in the broken part works as anywhere else; deleting the `<` mends it.
        type_at(&mut doc, "üße", "y");
        assert!(doc.reparse_wait(now).is_some());
        delete_typed(&mut doc, "<yü", 1);
        assert!(doc.stale().is_some(), "known only after the read");
        assert!(doc.reparse());
        assert_eq!(doc.stale(), None);
        assert_fresh(&doc, "mended");
        assert!(String::from_utf8_lossy(&doc.src).contains("Grxyüße"));
    }

    /// Anything but typing reads the whole document while it is stale; so do undo and redo.
    #[test]
    fn other_changes_read_a_stale_document_whole() {
        let src = "<r><a>t</a><b/></r>";
        let mut doc = Document::from_bytes(src.as_bytes().to_vec());
        type_at(&mut doc, "t<", "<");
        assert!(doc.stale().is_some());
        doc.undo();
        assert_eq!(doc.stale(), None);
        assert_fresh(&doc, "undo");
        doc.redo();
        assert_eq!(doc.stale(), None);
        assert_fresh(&doc, "redo");
        type_at(&mut doc, "/r>", "x");
        assert!(doc.stale().is_some());
        doc.apply(Edit::new("Replace").replace(0..0, b" ".to_vec()))
            .unwrap();
        assert_eq!(doc.stale(), None);
        assert_fresh(&doc, "a command");
    }

    /// A read that started before the last key is not taken (D19).
    #[test]
    fn a_reparse_from_before_the_last_key_is_dropped() {
        let mut doc = Document::from_bytes(b"<r><a>t</a></r>".to_vec());
        type_at(&mut doc, "t<", "<");
        let job = doc.reparse_job().expect("a job");
        type_at(&mut doc, "t<", "u");
        let done = std::thread::spawn(move || job.run()).join().unwrap();
        assert!(!doc.finish_reparse(done));
        assert!(doc.index.error.is_none());
        let done = doc.reparse_job().expect("still waits").run();
        assert!(doc.finish_reparse(done));
        assert!(doc.index.error.is_some());
    }

    /// A file that was not well-formed when opened has no good state to keep: after the
    /// pause it shows what a fresh read shows.
    #[test]
    fn a_document_opened_broken_is_read_again_as_is() {
        let mut doc = Document::from_bytes(b"<r><a>t</a><b>".to_vec());
        assert!(doc.index.error.is_some());
        type_at(&mut doc, "t<", "x");
        assert!(doc.stale().is_some());
        build_views(&doc);
        assert!(doc.reparse());
        assert_eq!(doc.stale(), None);
        assert_fresh(&doc, "opened broken");
    }
}
