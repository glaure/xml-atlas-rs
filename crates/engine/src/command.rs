//! Structured edits: the commands behind grid cells (GRID-10), the tree's rename, delete,
//! duplicate and move (TREE-9), and the CLI (`docs/spec/edit.md`, EDIT-5 to EDIT-10).
//!
//! Each command looks at the document and returns an [`Edit`] for [`Document::apply`], so it
//! is one undo step and the views can show what it will do before it happens. Only the bytes
//! the command is about change: indentation, quotes, line endings and everything else stay as
//! they were (AGENTS.md, rule 7).

use std::ops::Range;

use xml_atlas_xml::{Folded, NodeId, NodeKind, ROOT};

use crate::grid::CellTarget;
use crate::{Document, Edit};

/// Why a command cannot be run. The document is unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("not an element")]
    NotAnElement,
    #[error("the root element cannot be deleted, duplicated or moved")]
    RootElement,
    #[error(
        "<{0}> has child elements, comments or processing instructions; only its text can be set when it has none"
    )]
    NotTextOnly(String),
    #[error("\"{0}\" is not a valid XML name")]
    BadName(String),
    #[error("<{element}> has no attribute \"{name}\"")]
    NoAttribute { element: String, name: String },
    #[error("<{0}> has no element sibling in that direction")]
    NoSibling(String),
    #[error("no element at {0}")]
    NoSuchPath(String),
    #[error("{path} matches {count} elements; add [n] to choose one")]
    AmbiguousPath { path: String, count: usize },
    #[error("\"{0}\" is not a path like /root/child[2]")]
    BadPath(String),
}

/// Which way [`move_element`] moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// Sets the text of an element that holds only text (or nothing): `<a>old</a>` becomes
/// `<a>new</a>`, `<a/>` becomes `<a>new</a>`. The text is escaped; a CDATA section stays a
/// CDATA section when the new text allows it. A line feed is written as the document's line
/// ending, so a CRLF file stays CRLF.
pub fn set_text(doc: &Document, id: NodeId, text: &str) -> Result<Edit, CommandError> {
    let n = element(doc, id)?;
    let idx = &doc.index;
    if !holds_only_text(doc, id) {
        return Err(CommandError::NotTextOnly(name(doc, id)));
    }
    let label = format!("Edit <{}>", name(doc, id));
    let head_end = n.start as usize + n.head_len as usize;
    if head_end == n.end as usize {
        // `<a/>`: open it up, unless there is nothing to put in.
        if text.is_empty() {
            return Ok(Edit::new(label));
        }
        let name = idx.name(&doc.src, id);
        let mut bytes = b">".to_vec();
        bytes.extend_from_slice(escape_text(text, doc.newline()).as_bytes());
        bytes.extend_from_slice(b"</");
        bytes.extend_from_slice(name);
        bytes.push(b'>');
        return Ok(Edit::new(label).replace(n.end as usize - 2..n.end as usize, bytes));
    }
    let content = head_end..end_tag_start(doc, id);
    let newline = doc.newline();
    let new = if n.folded == Folded::CData && !text.contains("]]>") && !text.contains('\r') {
        let text = text.replace('\n', std::str::from_utf8(newline).expect("ASCII"));
        format!("<![CDATA[{text}]]>")
    } else {
        escape_text(text, newline)
    };
    Ok(Edit::new(label).replace(content, new))
}

/// Sets attribute `attr` of an element to `value`, adding it after the last attribute when
/// it is not there. The value keeps its quote character and is escaped for it.
pub fn set_attribute(
    doc: &Document,
    id: NodeId,
    attr: &str,
    value: &str,
) -> Result<Edit, CommandError> {
    let n = element(doc, id)?;
    check_name(attr)?;
    let label = format!("Set {attr} of <{}>", name(doc, id));
    if let Some(span) = attribute_span(doc, id, attr) {
        let raw = &doc.src[span.clone()];
        let quote = raw[raw.len() - 1];
        let open = raw
            .iter()
            .position(|&b| b == quote)
            .expect("a quoted value");
        let value_range = span.start + open + 1..span.end - 1;
        return Ok(Edit::new(label).replace(value_range, escape_attribute(value, quote)));
    }
    let at = doc
        .index
        .attribute_spans(&doc.src, id)
        .last()
        .map_or(n.start as usize + 1 + n.name_len as usize, |s| s.end);
    let bytes = format!(" {attr}=\"{}\"", escape_attribute(value, b'"'));
    Ok(Edit::new(label).replace(at..at, bytes))
}

/// Sets what a grid cell stands for (GRID-10): an attribute through [`set_attribute`], text
/// through [`set_text`]. `None` when `value` is what is there already, so that confirming a
/// cell unchanged leaves its bytes (entities, CDATA) as they were. A missing attribute counts
/// as empty, so an empty value does not add one.
pub fn set_cell(
    doc: &Document,
    target: &CellTarget,
    value: &str,
) -> Result<Option<Edit>, CommandError> {
    let idx = &doc.index;
    match target {
        CellTarget::Attribute { element, name } => {
            let current = idx
                .attributes(&doc.src, *element)
                .into_iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v)
                .unwrap_or_default();
            if current == value {
                return Ok(None);
            }
            set_attribute(doc, *element, name, value).map(Some)
        }
        CellTarget::Text(id) => {
            element(doc, *id)?;
            if as_parsed(&idx.element_text(&doc.src, *id)) == value {
                return Ok(None);
            }
            set_text(doc, *id, value).map(Some)
        }
    }
}

/// Removes attribute `attr` and the whitespace before it.
pub fn remove_attribute(doc: &Document, id: NodeId, attr: &str) -> Result<Edit, CommandError> {
    element(doc, id)?;
    let span = attribute_span(doc, id, attr).ok_or_else(|| CommandError::NoAttribute {
        element: name(doc, id),
        name: attr.to_string(),
    })?;
    let mut start = span.start;
    while start > 0 && is_space(doc.src[start - 1]) {
        start -= 1;
    }
    Ok(Edit::new(format!("Remove {attr} of <{}>", name(doc, id))).replace(start..span.end, ""))
}

/// Renames an element, in its start tag and its end tag.
pub fn rename(doc: &Document, id: NodeId, new_name: &str) -> Result<Edit, CommandError> {
    let n = element(doc, id)?;
    check_name(new_name)?;
    let old = name(doc, id);
    let mut edit = Edit::new(format!("Rename <{old}> to <{new_name}>"))
        .replace(n.name_span(), new_name.as_bytes());
    if n.start as usize + (n.head_len as usize) < n.end as usize {
        let at = end_tag_start(doc, id) + 2;
        edit = edit.replace(at..at + n.name_len as usize, new_name.as_bytes());
    }
    Ok(edit)
}

/// Deletes a node. When it stands on lines of its own, those lines go with it, so no empty
/// line is left behind.
pub fn delete(doc: &Document, id: NodeId) -> Result<Edit, CommandError> {
    let n = *doc.index.node(id);
    if id == ROOT {
        return Err(CommandError::NotAnElement);
    }
    if Some(id) == doc.index.root_element() {
        return Err(CommandError::RootElement);
    }
    let range = own_lines(&doc.src, n.span()).map_or(n.span(), |l| l.lines);
    Ok(Edit::new(format!("Delete {}", describe(doc, id))).replace(range, ""))
}

/// Puts a copy of an element right after it, on its own lines with the same indentation when
/// the element stands on lines of its own.
pub fn duplicate(doc: &Document, id: NodeId) -> Result<Edit, CommandError> {
    let n = element(doc, id)?;
    if Some(id) == doc.index.root_element() {
        return Err(CommandError::RootElement);
    }
    let src = &doc.src;
    let label = format!("Duplicate <{}>", name(doc, id));
    let edit = match own_lines(src, n.span()) {
        Some(l) if l.ends_with_newline => {
            let copy = src[l.lines.clone()].to_vec();
            Edit::new(label).replace(l.lines.end..l.lines.end, copy)
        }
        Some(l) => {
            // The last line of the file, with no line ending after it.
            let mut copy = doc.newline().to_vec();
            copy.extend_from_slice(&src[l.lines.start..n.end as usize]);
            Edit::new(label).replace(n.end as usize..n.end as usize, copy)
        }
        None => {
            let copy = src[n.span()].to_vec();
            Edit::new(label).replace(n.end as usize..n.end as usize, copy)
        }
    };
    Ok(edit)
}

/// Swaps an element with the previous or next element among its siblings. What lies between
/// the two (whitespace, comments, text) stays where it is.
pub fn move_element(
    doc: &Document,
    id: NodeId,
    direction: Direction,
) -> Result<Edit, CommandError> {
    element(doc, id)?;
    if Some(id) == doc.index.root_element() {
        return Err(CommandError::RootElement);
    }
    let idx = &doc.index;
    let parent = idx.node(id).parent;
    let siblings: Vec<NodeId> = idx
        .children(parent)
        .filter(|&c| idx.node(c).kind == NodeKind::Element)
        .collect();
    let i = siblings
        .iter()
        .position(|&s| s == id)
        .expect("a child of its parent");
    let other = match direction {
        Direction::Up => i.checked_sub(1).map(|j| siblings[j]),
        Direction::Down => siblings.get(i + 1).copied(),
    }
    .ok_or_else(|| CommandError::NoSibling(name(doc, id)))?;
    let (first, second) = match direction {
        Direction::Up => (other, id),
        Direction::Down => (id, other),
    };
    let (a, b) = (idx.node(first).span(), idx.node(second).span());
    let src = &doc.src;
    let mut bytes = Vec::with_capacity(b.end - a.start);
    bytes.extend_from_slice(&src[b.clone()]);
    bytes.extend_from_slice(&src[a.end..b.start]);
    bytes.extend_from_slice(&src[a.clone()]);
    let label = format!(
        "Move <{}> {}",
        name(doc, id),
        if direction == Direction::Up {
            "up"
        } else {
            "down"
        }
    );
    Ok(Edit::new(label).replace(a.start..b.end, bytes))
}

/// The element at `path`, written as [`xml_atlas_xml::Index::path`] writes it:
/// `/catalog/book[2]/title`. A step without `[n]` must match exactly one element.
pub fn resolve(doc: &Document, path: &str) -> Result<NodeId, CommandError> {
    let bad = || CommandError::BadPath(path.to_string());
    let rest = path.strip_prefix('/').ok_or_else(bad)?;
    let idx = &doc.index;
    let mut cur = ROOT;
    let mut walked = String::new();
    for step in rest.split('/') {
        let (step_name, n) = match step.strip_suffix(']').and_then(|s| s.split_once('[')) {
            Some((name, n)) => (
                name,
                Some(n.parse::<usize>().ok().filter(|&n| n > 0).ok_or_else(bad)?),
            ),
            None => (step, None),
        };
        if step_name.is_empty() {
            return Err(bad());
        }
        walked.push('/');
        walked.push_str(step);
        let matching: Vec<NodeId> = idx
            .children(cur)
            .filter(|&c| {
                idx.node(c).kind == NodeKind::Element
                    && idx.name(&doc.src, c) == step_name.as_bytes()
            })
            .collect();
        cur = match (n, matching.len()) {
            (_, 0) => return Err(CommandError::NoSuchPath(walked)),
            (None, 1) => matching[0],
            (None, count) => {
                return Err(CommandError::AmbiguousPath {
                    path: walked,
                    count,
                });
            }
            (Some(n), _) => *matching
                .get(n - 1)
                .ok_or_else(|| CommandError::NoSuchPath(walked.clone()))?,
        };
    }
    Ok(cur)
}

/// The lines a span stands on alone: from the start of its first line to after the line
/// ending of its last, when only whitespace shares those lines with it.
struct OwnLines {
    lines: Range<usize>,
    ends_with_newline: bool,
}

fn own_lines(src: &[u8], span: Range<usize>) -> Option<OwnLines> {
    let start = src[..span.start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    if !src[start..span.start]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
    {
        return None;
    }
    let rest = &src[span.end..];
    let line_end = rest.iter().position(|&b| b == b'\n');
    let tail = &rest[..line_end.unwrap_or(rest.len())];
    if !tail.iter().all(|&b| is_space(b)) {
        return None;
    }
    Some(OwnLines {
        lines: start..line_end.map_or(src.len(), |i| span.end + i + 1),
        ends_with_newline: line_end.is_some(),
    })
}

/// Whether an element holds nothing but text and CDATA, so that [`set_text`] can replace it.
pub(crate) fn holds_only_text(doc: &Document, id: NodeId) -> bool {
    let idx = &doc.index;
    idx.children(id)
        .all(|c| matches!(idx.node(c).kind, NodeKind::Text | NodeKind::CData))
}

fn element(doc: &Document, id: NodeId) -> Result<&xml_atlas_xml::Node, CommandError> {
    let n = doc.index.node(id);
    if n.kind == NodeKind::Element {
        Ok(n)
    } else {
        Err(CommandError::NotAnElement)
    }
}

fn name(doc: &Document, id: NodeId) -> String {
    String::from_utf8_lossy(doc.index.name(&doc.src, id)).into_owned()
}

/// `<name>` for an element, the kind for other nodes: for undo labels.
fn describe(doc: &Document, id: NodeId) -> String {
    match doc.index.node(id).kind {
        NodeKind::Element => format!("<{}>", name(doc, id)),
        NodeKind::Comment => "comment".into(),
        NodeKind::ProcessingInstruction => "processing instruction".into(),
        NodeKind::CData => "CDATA".into(),
        _ => "text".into(),
    }
}

fn attribute_span(doc: &Document, id: NodeId, attr: &str) -> Option<Range<usize>> {
    let names = doc.index.attributes(&doc.src, id);
    let i = names.iter().position(|(k, _)| k == attr)?;
    doc.index.attribute_spans(&doc.src, id).into_iter().nth(i)
}

/// Where the end tag of a non-empty element starts (its `<`).
fn end_tag_start(doc: &Document, id: NodeId) -> usize {
    let n = doc.index.node(id);
    let span = &doc.src[n.span()];
    n.start as usize + memchr::memrchr(b'<', span).expect("an end tag")
}

/// A name XML accepts, near enough: starts with a letter, `_`, `:` or any non-ASCII
/// character; continues with those, digits, `-` and `.`.
fn check_name(name: &str) -> Result<(), CommandError> {
    let start = |c: char| c.is_alphabetic() || c == '_' || c == ':' || !c.is_ascii();
    let rest = |c: char| start(c) || c.is_ascii_digit() || c == '-' || c == '.';
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if start(c) && chars.all(rest) => Ok(()),
        _ => Err(CommandError::BadName(name.to_string())),
    }
}

/// Text as a parser reports it: a CRLF or a lone CR is a line feed. (An escaped `&#13;` is
/// already a CR here and becomes one too.)
pub(crate) fn as_parsed(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Text content escaped: `&`, `<` and `>` as entities, a line feed as `newline`, and a
/// carriage return as a character reference so that it survives (a parser would turn a bare
/// one into a line feed).
fn escape_text(s: &str, newline: &[u8]) -> String {
    let newline = std::str::from_utf8(newline).expect("ASCII");
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str(newline),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

/// An attribute value escaped for the quote it stands in: also tabs and line breaks as
/// character references, which a parser would otherwise turn into spaces.
fn escape_attribute(s: &str, quote: u8) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' if quote == b'"' => out.push_str("&quot;"),
            '\'' if quote == b'\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
    out
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte order mark, CRLF, non-ASCII names and text, both quote styles, CDATA, a comment.
    const SRC: &str = "\u{feff}<?xml version=\"1.0\"?>\r\n<shop name='Graz'>\r\n  <owner id=\"7\">Günther &amp; Söhne</owner>\r\n  <!-- note -->\r\n  <größe/>\r\n  <script><![CDATA[a < b]]></script>\r\n  <mixed>x<b/>y</mixed>\r\n</shop>\r\n";

    fn doc() -> Document {
        Document::from_bytes(SRC.as_bytes().to_vec())
    }

    fn at(doc: &Document, path: &str) -> NodeId {
        resolve(doc, path).unwrap()
    }

    /// Applies the command's edit and returns the new source; checks that undo restores it.
    fn run(
        doc: &mut Document,
        command: impl FnOnce(&Document) -> Result<Edit, CommandError>,
    ) -> String {
        let edit = command(doc).unwrap();
        doc.apply(edit).unwrap();
        assert!(doc.index.error.is_none(), "{:?}", doc.index.error);
        let after = String::from_utf8(doc.src.to_vec()).unwrap();
        doc.undo();
        assert_eq!(*doc.src, SRC.as_bytes());
        after
    }

    /// `SRC` with one piece replaced.
    fn with(from: &str, to: &str) -> String {
        assert_eq!(SRC.matches(from).count(), 1, "{from}");
        SRC.replace(from, to)
    }

    #[test]
    fn set_text_escapes_and_keeps_everything_else() {
        let mut d = doc();
        let owner = at(&d, "/shop/owner");
        assert_eq!(
            run(&mut d, |d| set_text(d, owner, "A & B <C>\r")),
            with("Günther &amp; Söhne", "A &amp; B &lt;C&gt;&#13;")
        );
        let größe = at(&d, "/shop/größe");
        assert_eq!(
            run(&mut d, |d| set_text(d, größe, "3 m")),
            with("<größe/>", "<größe>3 m</größe>")
        );
        assert_eq!(
            d.apply(set_text(&d, größe, "").unwrap()),
            Err(crate::EditError::Empty)
        );
        let script = at(&d, "/shop/script");
        assert_eq!(
            run(&mut d, |d| set_text(d, script, "x < y")),
            with("a < b", "x < y")
        );
        assert_eq!(
            run(&mut d, |d| set_text(d, script, "]]>")),
            with("<![CDATA[a < b]]>", "]]&gt;")
        );
        let mixed = at(&d, "/shop/mixed");
        assert_eq!(
            set_text(&d, mixed, "z").unwrap_err(),
            CommandError::NotTextOnly("mixed".into())
        );
        assert!(set_text(&d, at(&d, "/shop"), "z").is_err());
    }

    #[test]
    fn set_and_remove_attributes() {
        let mut d = doc();
        let shop = at(&d, "/shop");
        assert_eq!(
            run(&mut d, |d| set_attribute(
                d,
                shop,
                "name",
                "Wien's \"Mitte\"\n"
            )),
            with("name='Graz'", "name='Wien&apos;s \"Mitte\"&#10;'")
        );
        let owner = at(&d, "/shop/owner");
        assert_eq!(
            run(&mut d, |d| set_attribute(d, owner, "größe", "a\"b")),
            with("id=\"7\"", "id=\"7\" größe=\"a&quot;b\"")
        );
        let größe = at(&d, "/shop/größe");
        assert_eq!(
            run(&mut d, |d| set_attribute(d, größe, "x", "1")),
            with("<größe/>", "<größe x=\"1\"/>")
        );
        assert_eq!(
            set_attribute(&d, owner, "1x", "v").unwrap_err(),
            CommandError::BadName("1x".into())
        );
        assert_eq!(
            run(&mut d, |d| remove_attribute(d, owner, "id")),
            with("<owner id=\"7\">", "<owner>")
        );
        assert!(matches!(
            remove_attribute(&d, owner, "nope"),
            Err(CommandError::NoAttribute { .. })
        ));
    }

    #[test]
    fn rename_changes_both_tags() {
        let mut d = doc();
        let owner = at(&d, "/shop/owner");
        let renamed = run(&mut d, |d| rename(d, owner, "Eigentümer"));
        assert_eq!(
            renamed,
            with(
                "<owner id=\"7\">Günther &amp; Söhne</owner>",
                "<Eigentümer id=\"7\">Günther &amp; Söhne</Eigentümer>"
            )
        );
        let größe = at(&d, "/shop/größe");
        assert_eq!(
            run(&mut d, |d| rename(d, größe, "size")),
            with("<größe/>", "<size/>")
        );
        assert!(rename(&d, owner, "a b").is_err());
        assert!(rename(&d, owner, "").is_err());
    }

    #[test]
    fn delete_takes_its_lines_along() {
        let mut d = doc();
        let owner = at(&d, "/shop/owner");
        assert_eq!(
            run(&mut d, |d| delete(d, owner)),
            with("  <owner id=\"7\">Günther &amp; Söhne</owner>\r\n", "")
        );
        // Not alone on its line: only the element goes.
        let b = d.index.children(at(&d, "/shop/mixed")).nth(1).unwrap();
        assert_eq!(run(&mut d, |d| delete(d, b)), with("x<b/>y", "xy"));
        assert_eq!(
            delete(&d, at(&d, "/shop")).unwrap_err(),
            CommandError::RootElement
        );
    }

    #[test]
    fn duplicate_copies_lines_with_their_indentation() {
        let mut d = doc();
        let größe = at(&d, "/shop/größe");
        assert_eq!(
            run(&mut d, |d| duplicate(d, größe)),
            with("  <größe/>\r\n", "  <größe/>\r\n  <größe/>\r\n")
        );
        let b = d.index.children(at(&d, "/shop/mixed")).nth(1).unwrap();
        assert_eq!(
            run(&mut d, |d| duplicate(d, b)),
            with("x<b/>y", "x<b/><b/>y")
        );
        // Sharing its line with the parent's end tag: copied right after it.
        let mut e = Document::from_bytes(b"<r>\n  <a/></r>".to_vec());
        let a = resolve(&e, "/r/a").unwrap();
        e.apply(duplicate(&e, a).unwrap()).unwrap();
        assert_eq!(*e.src, b"<r>\n  <a/><a/></r>");
        // Alone on the last line, with no line ending (a file cut short): a new line.
        let mut e = Document::from_bytes(b"<r>\r\n  <a/>".to_vec());
        let a = resolve(&e, "/r/a").unwrap();
        e.apply(duplicate(&e, a).unwrap()).unwrap();
        assert_eq!(*e.src, b"<r>\r\n  <a/>\r\n  <a/>");
    }

    #[test]
    fn move_swaps_with_the_element_sibling() {
        let mut d = doc();
        let größe = at(&d, "/shop/größe");
        // Past the comment: the comment stays where it is.
        assert_eq!(
            run(&mut d, |d| move_element(d, größe, Direction::Up)),
            with(
                "<owner id=\"7\">Günther &amp; Söhne</owner>\r\n  <!-- note -->\r\n  <größe/>",
                "<größe/>\r\n  <!-- note -->\r\n  <owner id=\"7\">Günther &amp; Söhne</owner>"
            )
        );
        assert_eq!(
            run(&mut d, |d| move_element(d, größe, Direction::Down)),
            with(
                "<größe/>\r\n  <script><![CDATA[a < b]]></script>",
                "<script><![CDATA[a < b]]></script>\r\n  <größe/>"
            )
        );
        let owner = at(&d, "/shop/owner");
        assert_eq!(
            move_element(&d, owner, Direction::Up).unwrap_err(),
            CommandError::NoSibling("owner".into())
        );
    }

    #[test]
    fn paths_resolve_as_path_writes_them() {
        let d = Document::from_bytes(b"<r><x/><y><z/></y><x><z/></x></r>".to_vec());
        let second = resolve(&d, "/r/x[2]").unwrap();
        assert_eq!(d.index.path(&d.src, second), "/r/x[2]");
        assert_eq!(resolve(&d, "/r/y/z").unwrap(), 4);
        assert_eq!(
            resolve(&d, "/r/x").unwrap_err(),
            CommandError::AmbiguousPath {
                path: "/r/x".into(),
                count: 2
            }
        );
        assert_eq!(
            resolve(&d, "/r/x[3]").unwrap_err(),
            CommandError::NoSuchPath("/r/x[3]".into())
        );
        assert!(matches!(resolve(&d, "r/x"), Err(CommandError::BadPath(_))));
        assert!(matches!(
            resolve(&d, "/r/x[0]"),
            Err(CommandError::BadPath(_))
        ));
        assert!(matches!(
            resolve(&d, "/r//x"),
            Err(CommandError::BadPath(_))
        ));
    }

    #[test]
    fn set_cell_changes_only_a_changed_value() {
        let mut d = doc();
        let owner = at(&d, "/shop/owner");
        let id = CellTarget::Attribute {
            element: owner,
            name: "id".into(),
        };
        let missing = CellTarget::Attribute {
            element: owner,
            name: "größe".into(),
        };
        let text = CellTarget::Text(owner);
        // Unchanged values, including decoded entities and a missing attribute left empty,
        // are no edit at all: `&amp;` is not rewritten.
        assert_eq!(set_cell(&d, &id, "7"), Ok(None));
        assert_eq!(set_cell(&d, &missing, ""), Ok(None));
        assert_eq!(set_cell(&d, &text, "Günther & Söhne"), Ok(None));
        let script = CellTarget::Text(at(&d, "/shop/script"));
        assert_eq!(set_cell(&d, &script, "a < b"), Ok(None));
        assert_eq!(
            run(&mut d, |d| set_cell(d, &id, "8").map(Option::unwrap)),
            with("id=\"7\"", "id=\"8\"")
        );
        assert_eq!(
            run(&mut d, |d| set_cell(d, &missing, "ß").map(Option::unwrap)),
            with("id=\"7\"", "id=\"7\" größe=\"ß\"")
        );
        assert_eq!(
            run(&mut d, |d| set_cell(d, &text, "Söhne").map(Option::unwrap)),
            with("Günther &amp; Söhne", "Söhne")
        );
        // In a CRLF file a line feed is written as CRLF, and text with CRLF line endings
        // reads back as line feeds, unchanged.
        assert_eq!(
            run(&mut d, |d| set_cell(d, &text, "eins\nzwei")
                .map(Option::unwrap)),
            with("Günther &amp; Söhne", "eins\r\nzwei")
        );
        let lines = "<r>\r\n  <t>eins\r\nzwei</t>\r\n  <c><![CDATA[x]]></c>\r\n</r>\r\n";
        let mut l = Document::from_bytes(lines.as_bytes().to_vec());
        let t = CellTarget::Text(at(&l, "/r/t"));
        assert_eq!(set_cell(&l, &t, "eins\nzwei"), Ok(None));
        let c = at(&l, "/r/c");
        l.apply(set_text(&l, c, "a\nb").unwrap()).unwrap();
        assert!(String::from_utf8_lossy(&l.src).contains("<c><![CDATA[a\r\nb]]></c>"));
        let mixed = CellTarget::Text(at(&d, "/shop/mixed"));
        assert_eq!(
            set_cell(&d, &mixed, "z").unwrap_err(),
            CommandError::NotTextOnly("mixed".into())
        );
    }
}
