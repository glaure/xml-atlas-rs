//! Format: re-indent a document without changing what it means (`docs/spec/format.md`).
//!
//! Only whitespace between tags changes, and only where it cannot be content: inside an element
//! whose children are all elements, comments or processing instructions. A start tag with more
//! attributes than [`FormatOptions::max_attributes`] gets one attribute per line; only the
//! whitespace between its attributes changes. Everything else is copied byte for byte:
//!
//! - elements that contain text or CDATA (mixed content keeps its spacing);
//! - elements with `xml:space="preserve"`, including everything inside them;
//! - comments, processing instructions, the declaration and the DOCTYPE;
//! - entity references, the byte order mark and the line-ending style (LF or CRLF).
//!
//! A document can carry its own options in an `Indentation Settings` comment before the root
//! element (FMT-15); [`options_for`] reads it.

use xml_atlas_xml::{Index, NodeId, NodeKind, ROOT, UTF8_BOM};

/// How a comment that carries format settings starts (FMT-15).
const SETTINGS_TITLE: &str = "Indentation Settings";

/// How each settings line in that comment starts.
const SETTINGS_PREFIX: &str = "XML Marker:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatOptions {
    /// Spaces per level. Ignored when `tabs` is set.
    pub indent_size: usize,
    /// Indent with one tab per level instead of spaces.
    pub tabs: bool,
    /// A start tag with more attributes than this gets one attribute per line. 0: never.
    pub max_attributes: usize,
    /// Leave out comments where Format controls the layout, except the settings comment
    /// (FMT-12). Changes content, so off by default.
    pub remove_comments: bool,
    /// Trim whitespace at the start and end of elements that hold only text (FMT-13). Changes
    /// content, so off by default.
    pub trim_text: bool,
}

/// XML Marker's defaults (XM-50, FMT-10, FMT-11).
impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent_size: 4,
            tabs: false,
            max_attributes: 6,
            remove_comments: false,
            trim_text: false,
        }
    }
}

impl std::fmt::Display for FormatOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.tabs {
            write!(f, "tabs")?;
        } else {
            write!(f, "indent {}", self.indent_size)?;
        }
        match self.max_attributes {
            0 => write!(f, ", attributes never wrapped")?,
            n => write!(f, ", at most {n} attributes per tag line")?,
        }
        if self.remove_comments {
            write!(f, ", comments removed")?;
        }
        if self.trim_text {
            write!(f, ", text trimmed")?;
        }
        Ok(())
    }
}

/// The options to format one document with, and where they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentOptions {
    pub options: FormatOptions,
    /// The settings the document's own comment set, by name, e.g. `["Indent Size"]`. Empty
    /// when the document has no settings comment.
    pub from_document: Vec<&'static str>,
}

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("the document is not well-formed: {0}")]
    Malformed(#[from] xml_atlas_xml::ParseError),
}

/// Re-indents `src`. See the module documentation for what is and is not changed.
pub fn format(src: &[u8], options: &FormatOptions) -> Result<Vec<u8>, FormatError> {
    format_indexed(src, &xml_atlas_xml::index(src), options)
}

/// [`format`] for a source that is already indexed.
pub fn format_indexed(
    src: &[u8],
    index: &Index,
    options: &FormatOptions,
) -> Result<Vec<u8>, FormatError> {
    write(src, index, options, false)
}

/// The opposite of [`format`] (FMT-14): the whitespace Format would add or change is left out,
/// so the document is one line. A start tag that spans lines is joined with one space between
/// attributes. Everything Format keeps exactly is kept exactly here too. `remove_comments`
/// and `trim_text` apply; indent and `max_attributes` do not.
pub fn compact(src: &[u8], options: &FormatOptions) -> Result<Vec<u8>, FormatError> {
    compact_indexed(src, &xml_atlas_xml::index(src), options)
}

/// [`compact`] for a source that is already indexed.
pub fn compact_indexed(
    src: &[u8],
    index: &Index,
    options: &FormatOptions,
) -> Result<Vec<u8>, FormatError> {
    write(src, index, options, true)
}

/// Writes `src` laid out by Format, or compacted.
fn write(
    src: &[u8],
    index: &Index,
    options: &FormatOptions,
    compact: bool,
) -> Result<Vec<u8>, FormatError> {
    if let Some(e) = &index.error {
        return Err(FormatError::Malformed(e.clone()));
    }
    let newline = newline_style(src);
    let w = Writer {
        src,
        index,
        options,
        newline,
        compact,
        settings: settings_comment_id(src, index),
    };
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
                let n = index.node(id);
                if w.options.remove_comments
                    && n.kind == NodeKind::Comment
                    && w.settings != Some(id)
                {
                    continue;
                }
                w.indent(&mut out, depth);
                if can_reindent(src, index, id) {
                    w.start_tag(&mut out, id, depth);
                    stack.push(Step::Close(id, depth));
                    let first = stack.len();
                    stack.extend(index.children(id).map(|c| Step::Node(c, depth + 1)));
                    stack[first..].reverse();
                } else if n.kind == NodeKind::Element && !preserves_space(src, index, id) {
                    // Content copied exactly, or trimmed; only the start tag may change.
                    w.start_tag(&mut out, id, depth);
                    w.content(&mut out, id);
                } else {
                    out.extend_from_slice(index.raw(src, id));
                }
            }
            Step::Close(id, depth) => {
                w.indent(&mut out, depth);
                out.extend_from_slice(end_tag(index.raw(src, id)));
            }
        }
        if !compact {
            out.extend_from_slice(newline);
        }
    }
    if compact {
        out.extend_from_slice(newline);
    }
    Ok(out)
}

/// `base`, with what the document's settings comment sets (FMT-15). The comment must come
/// before the root element and start with `Indentation Settings`; each line it applies looks
/// like `XML Marker: Indent Size = 4`. Lines that cannot be read are ignored.
pub fn options_for(src: &[u8], index: &Index, base: FormatOptions) -> DocumentOptions {
    let mut out = DocumentOptions {
        options: base,
        from_document: Vec::new(),
    };
    let Some(text) = settings_comment(src, index) else {
        return out;
    };
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix(SETTINGS_PREFIX) else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let value = value.trim().to_ascii_lowercase();
        let number = value
            .split_whitespace()
            .next()
            .and_then(|v| v.parse::<usize>().ok());
        match name.to_ascii_lowercase().as_str() {
            "indent size" => {
                if let Some(n) = number {
                    out.options.indent_size = n;
                    out.from_document.push("Indent Size");
                }
            }
            "maximum attributes in tag line" => {
                if let Some(n) = number {
                    out.options.max_attributes = n;
                    out.from_document.push("Maximum attributes in tag line");
                }
            }
            "tab size" => {
                // `4 as spaces` or `4 as tabs`: whether to indent with tabs.
                if value.ends_with("as tabs") {
                    out.options.tabs = true;
                    out.from_document.push("Tab size");
                } else if value.ends_with("as spaces") {
                    out.options.tabs = false;
                    out.from_document.push("Tab size");
                }
            }
            _ => {}
        }
    }
    out
}

enum Step {
    /// Write a node at this depth.
    Node(NodeId, usize),
    /// Write the end tag of an element whose children were written one per line.
    Close(NodeId, usize),
}

struct Writer<'a> {
    src: &'a [u8],
    index: &'a Index,
    options: &'a FormatOptions,
    newline: &'static [u8],
    /// Compact (FMT-14) instead of Format.
    compact: bool,
    /// The settings comment, which Remove comments keeps (FMT-12, FMT-15).
    settings: Option<NodeId>,
}

impl Writer<'_> {
    fn indent(&self, out: &mut Vec<u8>, depth: usize) {
        if self.compact {
            return;
        }
        if self.options.tabs {
            out.resize(out.len() + depth, b'\t');
        } else {
            out.resize(out.len() + depth * self.options.indent_size, b' ');
        }
    }

    /// The start tag of element `id`, wrapped to one attribute per line when it has more than
    /// `max_attributes` (FMT-11).
    fn start_tag(&self, out: &mut Vec<u8>, id: NodeId, depth: usize) {
        let n = self.index.node(id);
        let head = &self.src[n.start as usize..n.start as usize + n.head_len as usize];
        if self.compact {
            self.joined_start_tag(out, id, head);
            return;
        }
        let max = self.options.max_attributes;
        // Every attribute has an `=`: with no more `=` than allowed, there is nothing to wrap,
        // and the attributes need not be found at all.
        if max == 0 || memchr::memchr_iter(b'=', head).count() <= max {
            out.extend_from_slice(head);
            return;
        }
        let spans = self.index.attribute_spans(self.src, id);
        if spans.len() <= max {
            out.extend_from_slice(head);
            return;
        }
        out.extend_from_slice(&head[..1 + n.name_len as usize]);
        for span in spans {
            out.extend_from_slice(self.newline);
            self.indent(out, depth + 1);
            out.extend_from_slice(&self.src[span]);
        }
        out.extend_from_slice(if head.ends_with(b"/>") { b"/>" } else { b">" });
    }

    /// A start tag that spans lines, on one line with one space between attributes (FMT-14).
    /// A one-line start tag is copied exactly.
    fn joined_start_tag(&self, out: &mut Vec<u8>, id: NodeId, head: &[u8]) {
        if memchr::memchr2(b'\n', b'\r', head).is_none() {
            out.extend_from_slice(head);
            return;
        }
        let n = self.index.node(id);
        out.extend_from_slice(&head[..1 + n.name_len as usize]);
        for span in self.index.attribute_spans(self.src, id) {
            out.push(b' ');
            out.extend_from_slice(&self.src[span]);
        }
        out.extend_from_slice(if head.ends_with(b"/>") { b"/>" } else { b">" });
    }

    /// Writes what follows the start tag of an element whose content is copied: exactly, or
    /// with the text trimmed when the element holds only text and `trim_text` is set (FMT-13).
    fn content(&self, out: &mut Vec<u8>, id: NodeId) {
        let n = self.index.node(id);
        let rest = &self.src[n.start as usize + n.head_len as usize..n.end as usize];
        // Look at the children only when trimming is asked for: this runs for most elements.
        if !self.options.trim_text
            || rest.is_empty()
            || !self
                .index
                .children(id)
                .all(|c| self.index.node(c).kind == NodeKind::Text)
        {
            out.extend_from_slice(rest);
            return;
        }
        let (text, close) = rest.split_at(rest.len() - end_tag(rest).len());
        let start = text
            .iter()
            .position(|&b| !is_xml_space(b))
            .unwrap_or(text.len());
        let end = text
            .iter()
            .rposition(|&b| !is_xml_space(b))
            .map_or(start, |i| i + 1);
        out.extend_from_slice(&text[start..end]);
        out.extend_from_slice(close);
    }
}

/// The text of the settings comment (FMT-15), without `<!--` and `-->`.
fn settings_comment<'s>(src: &'s [u8], index: &Index) -> Option<&'s str> {
    settings_comment_id(src, index).and_then(|id| comment_text(src, index, id))
}

/// The first comment before the root element whose text starts with [`SETTINGS_TITLE`].
fn settings_comment_id(src: &[u8], index: &Index) -> Option<NodeId> {
    index
        .children(ROOT)
        .take_while(|&c| index.node(c).kind != NodeKind::Element)
        .filter(|&c| index.node(c).kind == NodeKind::Comment)
        .find(|&c| {
            comment_text(src, index, c).is_some_and(|t| t.trim_start().starts_with(SETTINGS_TITLE))
        })
}

fn comment_text<'s>(src: &'s [u8], index: &Index, id: NodeId) -> Option<&'s str> {
    let body = index
        .raw(src, id)
        .strip_prefix(b"<!--")?
        .strip_suffix(b"-->")?;
    std::str::from_utf8(body).ok()
}

/// Whitespace as XML defines it: space, tab, carriage return, line feed.
fn is_xml_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// True if the whitespace inside the element is layout only, so its children can be put on
/// their own lines.
fn can_reindent(src: &[u8], index: &Index, id: NodeId) -> bool {
    let n = index.node(id);
    if n.kind != NodeKind::Element || !n.has_children() {
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

/// The line ending of the first line; LF if the source has a single line.
fn newline_style(src: &[u8]) -> &'static [u8] {
    match memchr::memchr(b'\n', src) {
        Some(i) if i > 0 && src[i - 1] == b'\r' => b"\r\n",
        _ => b"\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two spaces, no attribute wrapping: the options the older tests were written for.
    const TWO: FormatOptions = FormatOptions {
        indent_size: 2,
        tabs: false,
        max_attributes: 0,
        remove_comments: false,
        trim_text: false,
    };

    fn fmt_with(src: &str, options: &FormatOptions) -> String {
        String::from_utf8(format(src.as_bytes(), options).unwrap()).unwrap()
    }

    fn fmt(src: &str) -> String {
        fmt_with(src, &TWO)
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
        assert!(format(b"<a><b></a>", &TWO).is_err());
    }

    #[test]
    fn format_survives_deep_nesting() {
        let depth = 50_000;
        let src = format!("{}<x/>{}", "<a>".repeat(depth), "</a>".repeat(depth));
        // Zero indent: at this depth, real indentation would be gigabytes of spaces.
        let zero = FormatOptions {
            indent_size: 0,
            ..TWO
        };
        let out = format(src.as_bytes(), &zero).unwrap();
        assert_eq!(xml_atlas_xml::index(&out).max_depth as usize, depth + 1);
    }

    /// FMT-10: the default is four spaces; tabs put one tab per level.
    #[test]
    fn indent_size_and_tabs() {
        let src = "<a><b><c/></b></a>";
        let four = FormatOptions::default();
        assert_eq!(
            fmt_with(src, &four),
            "<a>\n    <b>\n        <c/>\n    </b>\n</a>\n"
        );
        let tabs = FormatOptions { tabs: true, ..four };
        assert_eq!(fmt_with(src, &tabs), "<a>\n\t<b>\n\t\t<c/>\n\t</b>\n</a>\n");
    }

    /// FMT-11: more attributes than allowed puts each on its own line, one level deeper;
    /// only whitespace between them changes; at or below the limit nothing changes.
    #[test]
    fn long_start_tags_get_one_attribute_per_line() {
        let three = FormatOptions {
            max_attributes: 2,
            ..TWO
        };
        let src = "<r><e a=\"1\"  b = 'x=y'\n c=\"&amp;\"><f/></e><g a=\"1\" b=\"2\"/></r>";
        assert_eq!(
            fmt_with(src, &three),
            "<r>\n  <e\n    a=\"1\"\n    b = 'x=y'\n    c=\"&amp;\">\n    <f/>\n  </e>\n  <g a=\"1\" b=\"2\"/>\n</r>\n"
        );
        // Empty elements and elements with text are wrapped too; their content stays exact.
        let src = "<r><e a=\"1\" b=\"2\" c=\"3\"/><t a=\"1\" b=\"2\" c=\"3\"> x </t></r>";
        assert_eq!(
            fmt_with(src, &three),
            "<r>\n  <e\n    a=\"1\"\n    b=\"2\"\n    c=\"3\"/>\n  <t\n    a=\"1\"\n    b=\"2\"\n    c=\"3\"> x </t>\n</r>\n"
        );
        // Wrapped output formats to itself.
        let once = fmt_with(src, &three);
        assert_eq!(fmt_with(&once, &three), once);
    }

    #[test]
    fn preserved_elements_keep_their_start_tag() {
        let one = FormatOptions {
            max_attributes: 1,
            ..TWO
        };
        let kept = "<p xml:space=\"preserve\" a=\"1\"> x </p>";
        assert_eq!(
            fmt_with(&format!("<r>{kept}</r>"), &one),
            format!("<r>\n  {kept}\n</r>\n")
        );
    }

    const SETTINGS: &str = "<?xml version=\"1.0\"?>
<!--Indentation Settings
XML Marker: Indent Size = 2
XML Marker: Maximum attributes in tag line = 3
XML Marker: Tab size = 4 as spaces
-->
<r/>";

    fn options(src: &str, base: FormatOptions) -> DocumentOptions {
        options_for(src.as_bytes(), &xml_atlas_xml::index(src.as_bytes()), base)
    }

    /// FMT-15: the document's settings comment overrides the base options.
    #[test]
    fn settings_comment_overrides_the_base() {
        let base = FormatOptions {
            indent_size: 8,
            tabs: true,
            max_attributes: 0,
            ..FormatOptions::default()
        };
        let got = options(SETTINGS, base);
        assert_eq!(
            got.options,
            FormatOptions {
                indent_size: 2,
                tabs: false,
                max_attributes: 3,
                ..FormatOptions::default()
            }
        );
        assert_eq!(
            got.from_document,
            ["Indent Size", "Maximum attributes in tag line", "Tab size"]
        );
    }

    #[test]
    fn settings_comment_partly_or_not_readable() {
        let base = FormatOptions::default();
        // Only what the comment names, and can be read, is taken.
        let partial = "<!--Indentation Settings\nXML Marker: Indent Size = two\nXML Marker: Maximum attributes in tag line = 9\n-->\n<r/>";
        let got = options(partial, base);
        assert_eq!(got.options.indent_size, 4);
        assert_eq!(got.options.max_attributes, 9);
        assert_eq!(got.from_document, ["Maximum attributes in tag line"]);
        // Another comment, or the settings after the root element, are not settings.
        for src in [
            "<!-- Indentation is great\nXML Marker: Indent Size = 1 --><r/>",
            "<r/><!--Indentation Settings\nXML Marker: Indent Size = 1\n-->",
            "<r/>",
        ] {
            let got = options(src, base);
            assert_eq!((got.options, got.from_document.len()), (base, 0), "{src}");
        }
    }

    #[test]
    fn settings_comment_is_kept_when_formatting() {
        let got = options(SETTINGS, FormatOptions::default());
        let out = fmt_with(SETTINGS, &got.options);
        assert!(out.contains("<!--Indentation Settings\nXML Marker: Indent Size = 2\n"));
    }

    fn compact_with(src: &str, options: &FormatOptions) -> String {
        String::from_utf8(compact(src.as_bytes(), options).unwrap()).unwrap()
    }

    /// FMT-12: comments go where Format controls the layout; the settings comment and
    /// comments in content copied exactly stay.
    #[test]
    fn remove_comments() {
        let o = FormatOptions {
            remove_comments: true,
            ..TWO
        };
        let src = "<!--Indentation Settings\nXML Marker: Indent Size = 2\n-->\n<!-- top -->\n<r><!-- a --><b/><p>x <!-- in text --> y</p><q xml:space=\"preserve\"><!-- kept --></q></r><!-- end -->";
        assert_eq!(
            fmt_with(src, &o),
            "<!--Indentation Settings\nXML Marker: Indent Size = 2\n-->\n<r>\n  <b/>\n  <p>x <!-- in text --> y</p>\n  <q xml:space=\"preserve\"><!-- kept --></q>\n</r>\n"
        );
    }

    /// FMT-13: only elements that hold nothing but text are trimmed.
    #[test]
    fn trim_text() {
        let o = FormatOptions {
            trim_text: true,
            ..TWO
        };
        let src = "<r><t>  Gr\u{f6}\u{df}e &amp; mehr \n</t><w>   </w><e/><m> a <b> b </b> c </m><c><![CDATA[  x  ]]></c><k xml:space=\"preserve\">  y  </k></r>";
        assert_eq!(
            fmt_with(src, &o),
            "<r>\n  <t>Gr\u{f6}\u{df}e &amp; mehr</t>\n  <w></w>\n  <e/>\n  <m> a <b> b </b> c </m>\n  <c><![CDATA[  x  ]]></c>\n  <k xml:space=\"preserve\">  y  </k>\n</r>\n"
        );
        // Off by default: text stays as written.
        assert_eq!(fmt("<t> x </t>"), "<t> x </t>\n");
    }

    /// FMT-14: one line; layout whitespace and wrapped start tags undone; content exact.
    #[test]
    fn compact_is_one_line() {
        let src = "<?xml version=\"1.0\"?>\n<r>\n  <e\n    a=\"1\"\n    b = 'x'>\n    <f/>\n  </e>\n  <p>Hello <b>big</b> world</p>\n  <!-- c -->\n</r>\n";
        assert_eq!(
            compact_with(src, &TWO),
            "<?xml version=\"1.0\"?><r><e a=\"1\" b = 'x'><f/></e><p>Hello <b>big</b> world</p><!-- c --></r>\n"
        );
    }

    #[test]
    fn compact_keeps_bom_crlf_and_round_trips_with_format() {
        let src = "\u{feff}<?xml version=\"1.0\"?>\r\n<a>\r\n  <b x=\"1\" y=\"2\" z=\"3\">t</b>\r\n  <c/>\r\n</a>\r\n";
        let small = compact_with(src, &TWO);
        assert_eq!(
            small,
            "\u{feff}<?xml version=\"1.0\"?><a><b x=\"1\" y=\"2\" z=\"3\">t</b><c/></a>\r\n"
        );
        assert_eq!(compact_with(&small, &TWO), small);
        let wrap = FormatOptions {
            max_attributes: 2,
            ..TWO
        };
        assert_eq!(fmt_with(&small, &wrap), fmt_with(src, &wrap));
        assert_eq!(compact_with(&fmt_with(src, &wrap), &TWO), small);
    }
}
