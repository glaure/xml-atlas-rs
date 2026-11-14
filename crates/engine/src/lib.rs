//! Document model and commands.
//!
//! Every user-visible operation is a function here, so the GUI, the CLI and (later) the MCP
//! server share one implementation and one set of tests.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use xml_atlas_xml::{Index, NodeId, NodeKind, ROOT, UTF8_BOM};

pub use xml_atlas_xml as xml;

pub mod grid;

/// An open XML document: the source bytes, its node index and a line index.
pub struct Document {
    pub path: Option<PathBuf>,
    pub src: Vec<u8>,
    pub index: Index,
    /// Byte offset where each line starts. Always contains at least `0`.
    pub line_starts: Vec<u64>,
    pub timings: Timings,
}

/// How long each stage of opening took.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub read: Duration,
    pub index: Duration,
    pub lines: Duration,
}

impl Timings {
    pub fn total(&self) -> Duration {
        self.read + self.index + self.lines
    }
}

impl Document {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let t = Instant::now();
        let src = std::fs::read(path)?;
        let read = t.elapsed();
        let mut doc = Self::from_bytes(src);
        doc.timings.read = read;
        doc.path = Some(path.to_path_buf());
        log::info!("opened {} in {:?}", path.display(), doc.timings.total());
        Ok(doc)
    }

    pub fn from_bytes(src: Vec<u8>) -> Self {
        let t = Instant::now();
        let index = xml_atlas_xml::index(&src);
        let index_time = t.elapsed();
        let t = Instant::now();
        let line_starts = line_starts(&src);
        let lines_time = t.elapsed();
        Self {
            path: None,
            src,
            index,
            line_starts,
            timings: Timings {
                read: Duration::ZERO,
                index: index_time,
                lines: lines_time,
            },
        }
    }

    /// Re-indented copy of the source. See [`format`] for what is and is not changed.
    pub fn format(&self, indent: usize) -> Result<Vec<u8>, FormatError> {
        format_indexed(&self.src, &self.index, indent)
    }

    /// Writes the source to `path` with [`save_atomic`] and makes it the document's path.
    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        save_atomic(path, &self.src)?;
        self.path = Some(path.to_path_buf());
        Ok(())
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

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("the document is not well-formed: {0}")]
    Malformed(#[from] xml::ParseError),
}

/// Re-indents `src` with `indent` spaces per level.
///
/// Only whitespace between tags changes, and only where it cannot be content: inside an
/// element whose children are all elements, comments or processing instructions. Everything
/// else is copied byte for byte:
///
/// - elements that contain text or CDATA (mixed content keeps its spacing);
/// - elements with `xml:space="preserve"`, including everything inside them;
/// - start tags, comments, processing instructions, the declaration and the DOCTYPE;
/// - entity references, the byte order mark and the line-ending style (LF or CRLF).
///
/// A document that is not well-formed is refused rather than formatted in part.
pub fn format(src: &[u8], indent: usize) -> Result<Vec<u8>, FormatError> {
    format_indexed(src, &xml_atlas_xml::index(src), indent)
}

/// [`format`] for a source that is already indexed.
pub fn format_indexed(src: &[u8], index: &Index, indent: usize) -> Result<Vec<u8>, FormatError> {
    if let Some(e) = &index.error {
        return Err(FormatError::Malformed(e.clone()));
    }
    let newline = newline_style(src);
    let mut out = Vec::with_capacity(src.len() + src.len() / 8);
    if src.starts_with(UTF8_BOM) {
        out.extend_from_slice(UTF8_BOM);
    }
    // An explicit stack: deeply nested documents must not overflow the call stack.
    let mut stack: Vec<Step> = index.children(ROOT).map(|c| Step::Node(c, 0)).collect();
    stack.reverse();
    while let Some(step) = stack.pop() {
        match step {
            Step::Node(id, depth) => {
                push_indent(&mut out, depth * indent);
                let n = index.node(id);
                if can_reindent(src, index, id) {
                    let head = n.start as usize..n.start as usize + n.head_len as usize;
                    out.extend_from_slice(&src[head]);
                    stack.push(Step::Close(id, depth));
                    let first = stack.len();
                    stack.extend(index.children(id).map(|c| Step::Node(c, depth + 1)));
                    stack[first..].reverse();
                } else {
                    out.extend_from_slice(index.raw(src, id));
                }
            }
            Step::Close(id, depth) => {
                push_indent(&mut out, depth * indent);
                out.extend_from_slice(end_tag(index.raw(src, id)));
            }
        }
        out.extend_from_slice(newline);
    }
    Ok(out)
}

enum Step {
    /// Write a node at this depth.
    Node(NodeId, usize),
    /// Write the end tag of an element whose children were written one per line.
    Close(NodeId, usize),
}

/// True if the whitespace inside the element is layout only, so its children can be put on
/// their own lines.
fn can_reindent(src: &[u8], index: &Index, id: NodeId) -> bool {
    let n = index.node(id);
    if n.kind != NodeKind::Element || n.child_count == 0 {
        return false;
    }
    let layout_only = index.children(id).all(|c| {
        matches!(
            index.node(c).kind,
            NodeKind::Element | NodeKind::Comment | NodeKind::ProcessingInstruction
        )
    });
    layout_only && !preserves_space(src, index, id)
}

fn preserves_space(src: &[u8], index: &Index, id: NodeId) -> bool {
    let n = index.node(id);
    let head = &src[n.start as usize..n.start as usize + n.head_len as usize];
    // Cheap byte search first; attributes are decoded only for the rare element that has it.
    memchr::memmem::find(head, b"xml:space").is_some()
        && index
            .attributes(src, id)
            .iter()
            .any(|(k, v)| k == "xml:space" && v == "preserve")
}

/// The `</name>` at the end of an element's source. An end tag cannot contain `<`.
fn end_tag(element: &[u8]) -> &[u8] {
    let start = memchr::memrchr(b'<', element).unwrap_or(0);
    &element[start..]
}

fn push_indent(out: &mut Vec<u8>, spaces: usize) {
    out.resize(out.len() + spaces, b' ');
}

/// The line ending of the first line; LF if the source has a single line.
fn newline_style(src: &[u8]) -> &'static [u8] {
    match memchr::memchr(b'\n', src) {
        Some(i) if i > 0 && src[i - 1] == b'\r' => b"\r\n",
        _ => b"\n",
    }
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

    fn fmt(src: &str) -> String {
        String::from_utf8(format(src.as_bytes(), 2).unwrap()).unwrap()
    }

    #[test]
    fn formats() {
        assert_eq!(
            fmt("<a><b x=\"1\">t</b><c/></a>"),
            "<a>\n  <b x=\"1\">t</b>\n  <c/>\n</a>\n"
        );
    }

    #[test]
    fn format_fixes_only_layout_whitespace() {
        assert_eq!(
            fmt("<a>\n<b>\n        <c/>\n</b>   <!-- n --><?pi x?></a>"),
            "<a>\n  <b>\n    <c/>\n  </b>\n  <!-- n -->\n  <?pi x?>\n</a>\n"
        );
    }

    #[test]
    fn format_keeps_text_and_entities() {
        let name = "<name>Customer 5 &amp; Sons</name>";
        assert_eq!(fmt(name), format!("{name}\n"));
        let mixed = "<p>Hello <b>big</b> world</p>";
        assert_eq!(
            fmt(&format!("<r>{mixed}</r>")),
            format!("<r>\n  {mixed}\n</r>\n")
        );
        let cdata = "<s><![CDATA[ a < b ]]></s>";
        assert_eq!(fmt(cdata), format!("{cdata}\n"));
        let padded = "<a>  </a>";
        assert_eq!(fmt(padded), format!("{padded}\n"));
    }

    #[test]
    fn format_respects_xml_space_preserve() {
        let kept = "<a xml:space=\"preserve\"> <b/>  <c/> </a>";
        assert_eq!(
            fmt(&format!("<r>{kept}</r>")),
            format!("<r>\n  {kept}\n</r>\n")
        );
    }

    #[test]
    fn format_keeps_declaration_bom_and_crlf() {
        let src = "\u{feff}<?xml version=\"1.0\"?>\r\n<a><b/></a>\r\n";
        assert_eq!(
            fmt(src),
            "\u{feff}<?xml version=\"1.0\"?>\r\n<a>\r\n  <b/>\r\n</a>\r\n"
        );
    }

    #[test]
    fn format_is_stable() {
        let once = fmt("<a><b><c>t</c><d/></b><!-- x --><e>1 &lt; 2</e></a>");
        assert_eq!(fmt(&once), once);
    }

    #[test]
    fn format_refuses_malformed_input() {
        assert!(format(b"<a><b></a>", 2).is_err());
    }

    #[test]
    fn format_survives_deep_nesting() {
        let depth = 50_000;
        let src = format!("{}<x/>{}", "<a>".repeat(depth), "</a>".repeat(depth));
        // Zero indent: at this depth, real indentation would be gigabytes of spaces.
        let out = format(src.as_bytes(), 0).unwrap();
        assert_eq!(xml::index(&out).max_depth as usize, depth + 1);
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
}
