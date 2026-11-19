//! Repeated elements in a preview (`docs/spec/references.md`, REF-11; `docs/decisions.md`,
//! D34).
//!
//! Some programs read an element with a count attribute as that many copies of it, numbered,
//! with placeholders inside replaced by the copy's number. [`expand`] writes a resolved
//! preview again that way. Every piece it copies from the preview keeps the preview's mapping
//! back to the document ([`Preview::source_of`]), so a copy maps to the element it was copied
//! from; imported and noted runs stay marked.

use std::ops::Range;

use crate::Document;
use crate::references::{Copied, NOTE, Preview};
use crate::xml::{Folded, NodeId, NodeKind, ROOT};

/// The `repeat` rule of a rules file: which attributes and placeholders mean what.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repeat {
    /// The attribute that holds how many copies an element stands for.
    pub count: String,
    /// The attribute that holds the first copy's number; 0 without it.
    pub start: Option<String>,
    /// An element inside a copy with this attribute (not `"false"`) gets the number appended
    /// to its name.
    pub suffix: Option<String>,
    /// With `suffix`, this attribute (not `"false"`) appends the relative number instead.
    pub relative: Option<String>,
    /// Placeholders replaced by the copy's number, in attribute values and text.
    pub index: Vec<String>,
    /// Placeholders replaced by the copy's number less the start.
    pub relative_index: Vec<String>,
    /// Whether the preview expands by default; `"enabled": false` in the file.
    pub enabled: bool,
}

/// The numbers of the copy being written.
#[derive(Clone, Copy, Debug)]
struct Copy {
    index: i64,
    relative: i64,
}

/// `preview` with every element that has the rule's count attribute written as its copies,
/// stopping after `limit` bytes as the preview does.
pub fn expand(preview: &Preview, rule: &Repeat, limit: usize) -> Preview {
    let doc = Document::from_bytes(preview.text.clone().into_bytes());
    let newline = if preview.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut w = Writer {
        doc: &doc,
        rule,
        newline,
        limit,
        out: String::new(),
        moved: Vec::new(),
        notes: Vec::new(),
        cut: false,
    };
    w.children(ROOT, 0..doc.src.len(), None);
    let mut out = Preview {
        imported: w.carry(&preview.imported),
        notes: w.carry(&preview.notes),
        copied: w.carry_copied(&preview.copied),
        references: preview.references,
        unresolved: preview.unresolved,
        cut: preview.cut || w.cut,
        text: String::new(),
    };
    out.notes.extend(w.notes.iter().cloned());
    out.text = w.out;
    out
}

/// Writes the expanded text and notes which runs of it were moved from where in the input.
struct Writer<'a> {
    doc: &'a Document,
    rule: &'a Repeat,
    newline: &'static str,
    limit: usize,
    out: String,
    /// Runs of `out` taken unchanged from the input: (input start, output start, length).
    moved: Vec<(usize, usize, usize)>,
    /// Runs of `out` this writer added: the cut note.
    notes: Vec<Range<usize>>,
    cut: bool,
}

impl Writer<'_> {
    /// Writes the nodes inside `parent`, which fill `span` of the input, and what lies between
    /// them.
    fn children(&mut self, parent: NodeId, span: Range<usize>, copy: Option<Copy>) {
        let doc = self.doc;
        let mut at = span.start;
        for child in doc.index.children(parent) {
            let n = doc.index.node(child);
            self.text(at..n.start as usize, copy);
            self.node(child, copy);
            at = n.end as usize;
        }
        self.text(at..span.end, copy);
    }

    fn node(&mut self, id: NodeId, copy: Option<Copy>) {
        let n = self.doc.index.node(id);
        match n.kind {
            NodeKind::Element => match self.count(id) {
                Some((count, start)) => self.copies(id, count, start),
                None => self.element(id, copy, None),
            },
            NodeKind::Text | NodeKind::Comment => self.text(n.span(), copy),
            _ => self.take(n.span()),
        }
    }

    /// Writes `count` copies of element `id`, numbered from `start`, each on a line of its
    /// own at the element's indentation; none for 0.
    fn copies(&mut self, id: NodeId, count: i64, start: i64) {
        let doc = self.doc;
        let name = String::from_utf8_lossy(doc.index.name(&doc.src, id)).into_owned();
        let indent = self.line_indent();
        for j in 0..count {
            if self.cut {
                return;
            }
            if j > 0 {
                self.push(self.newline);
                self.push(&indent);
            }
            let copy = Copy {
                index: start + j,
                relative: j,
            };
            self.element(id, Some(copy), Some(format!("{name}{}", copy.index)));
        }
    }

    /// Writes element `id`: named `name` when given; inside a copy, with placeholders replaced,
    /// the rule's attributes left out and the suffix appended where asked for.
    fn element(&mut self, id: NodeId, copy: Option<Copy>, name: Option<String>) {
        let doc = self.doc;
        let src = &doc.src;
        let n = doc.index.node(id);
        let name = name.or_else(|| {
            let c = copy?;
            let (suffix, relative) = self.suffix(id);
            suffix.then(|| {
                let own = String::from_utf8_lossy(doc.index.name(src, id));
                format!("{own}{}", if relative { c.relative } else { c.index })
            })
        });
        let start = n.start as usize;
        let head_end = start + n.head_len as usize;
        let name_span = n.name_span();
        // The start tag: `<`, the name, the attributes kept, and what closes it.
        self.take(start..start + 1);
        match &name {
            Some(new) => self.push(new),
            None => self.take(name_span.clone()),
        }
        let mut at = name_span.end;
        for a in doc.index.attribute_spans(src, id) {
            let attr = &src[a.clone()];
            let key = attribute_name(attr);
            if copy.is_some() && self.is_rule_attribute(key) {
                // Left out with the white space before it.
                at = a.end;
                continue;
            }
            self.take(at..a.start);
            match copy {
                Some(c) if self.has_placeholder(attr) => {
                    let text = self.replaced(&String::from_utf8_lossy(attr), c);
                    self.push(&text);
                }
                _ => self.take(a.clone()),
            }
            at = a.end;
        }
        self.take(at..head_end);
        if head_end >= n.end as usize {
            return;
        }
        // The content, then the end tag, renamed as the start tag.
        let end = n.end as usize;
        let end_tag = src[head_end..end]
            .windows(2)
            .rposition(|w| w == b"</")
            .map(|i| head_end + i)
            .filter(|_| src[end - 1] == b'>');
        let content_end = end_tag.unwrap_or(end);
        if n.folded == Folded::No {
            self.children(id, head_end..content_end, copy);
        } else {
            self.text(head_end..content_end, copy);
        }
        if let Some(tag) = end_tag {
            match &name {
                Some(new) => {
                    self.take(tag..tag + 2);
                    self.push(new);
                    self.take(tag + 2 + name_span.len()..end);
                }
                None => self.take(tag..end),
            }
        }
    }

    /// Text or a comment: inside a copy with its placeholders replaced, and a whole text
    /// `name$(index+n)` computed.
    fn text(&mut self, span: Range<usize>, copy: Option<Copy>) {
        if span.is_empty() {
            return;
        }
        let Some(c) = copy else {
            return self.take(span);
        };
        let raw = &self.doc.src[span.clone()];
        let text = String::from_utf8_lossy(raw);
        if let Some(computed) = self.computed(&text, c) {
            return self.push(&computed);
        }
        if self.has_placeholder(raw) {
            let replaced = self.replaced(&text, c);
            self.push(&replaced);
        } else {
            self.take(span);
        }
    }

    /// The count and start of element `id`, when it has the count attribute with a number.
    fn count(&self, id: NodeId) -> Option<(i64, i64)> {
        let doc = self.doc;
        let attrs = doc.index.attributes(&doc.src, id);
        let value = |name: &str| attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v);
        let count = value(&self.rule.count)?.trim().parse::<i64>().ok()?;
        let start = self
            .rule
            .start
            .as_deref()
            .and_then(value)
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        Some((count.max(0), start))
    }

    /// Whether element `id` asks for the number on its name, and whether the relative one.
    fn suffix(&self, id: NodeId) -> (bool, bool) {
        let doc = self.doc;
        let attrs = doc.index.attributes(&doc.src, id);
        let on = |name: Option<&String>| {
            name.is_some_and(|name| {
                attrs
                    .iter()
                    .any(|(k, v)| k == name && !v.trim().eq_ignore_ascii_case("false"))
            })
        };
        (
            on(self.rule.suffix.as_ref()),
            on(self.rule.relative.as_ref()),
        )
    }

    fn is_rule_attribute(&self, key: &[u8]) -> bool {
        let r = self.rule;
        [
            Some(&r.count),
            r.start.as_ref(),
            r.suffix.as_ref(),
            r.relative.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|name| name.as_bytes() == key)
    }

    fn has_placeholder(&self, raw: &[u8]) -> bool {
        let r = self.rule;
        r.index
            .iter()
            .chain(&r.relative_index)
            .any(|p| !p.is_empty() && raw.windows(p.len()).any(|w| w == p.as_bytes()))
    }

    fn replaced(&self, text: &str, c: Copy) -> String {
        let mut text = text.to_string();
        // The longer placeholders first, so one inside another is not cut apart.
        let mut all: Vec<(&String, i64)> = (self.rule.index.iter().map(|p| (p, c.index)))
            .chain(self.rule.relative_index.iter().map(|p| (p, c.relative)))
            .filter(|(p, _)| !p.is_empty())
            .collect();
        all.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        for (p, number) in all {
            text = text.replace(p.as_str(), &number.to_string());
        }
        text
    }

    /// A whole text `name$(id+n)` or `name$(id-n)`, where `$id` is one of the placeholders:
    /// `name` followed by the computed number.
    fn computed(&self, text: &str, c: Copy) -> Option<String> {
        let (name, rest) = text.split_once("$(")?;
        let first = name.chars().next()?;
        if !(first.is_ascii_alphabetic() || first == '_')
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return None;
        }
        let inner = rest.strip_suffix(')')?;
        let at = inner.find(['+', '-'])?;
        let (id, sign, digits) = (&inner[..at], &inner[at..at + 1], &inner[at + 1..]);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let offset: i64 = digits.parse().ok()?;
        let named = |list: &[String]| list.iter().any(|p| p.strip_prefix('$') == Some(id));
        let base = if named(&self.rule.index) {
            c.index
        } else if named(&self.rule.relative_index) {
            c.relative
        } else {
            return None;
        };
        let number = if sign == "+" {
            base + offset
        } else {
            base - offset
        };
        Some(format!("{name}{number}"))
    }

    /// The white space the current output line starts with.
    fn line_indent(&self) -> String {
        let line = &self.out[self.out.rfind('\n').map_or(0, |i| i + 1)..];
        line[..line.len() - line.trim_start_matches([' ', '\t']).len()].to_string()
    }

    /// Writes input `span` unchanged, and notes where it came from.
    fn take(&mut self, span: Range<usize>) {
        if span.is_empty() || self.cut {
            return;
        }
        let text = String::from_utf8_lossy(&self.doc.src[span.clone()]).into_owned();
        let out = self.out.len();
        match self.moved.last_mut() {
            Some((from, to, len)) if *from + *len == span.start && *to + *len == out => {
                *len += span.len();
            }
            _ => self.moved.push((span.start, out, span.len())),
        }
        self.push(&text);
    }

    fn push(&mut self, s: &str) {
        if self.cut {
            return;
        }
        self.out.push_str(s);
        if self.out.len() >= self.limit {
            self.cut = true;
            let start = self.out.len();
            self.out.push_str(&format!(
                "{}<!-- {NOTE} cut after {} bytes -->",
                self.newline, self.limit
            ));
            self.notes.push(start..self.out.len());
        }
    }

    /// `ranges` of the input, where they were moved to in the output.
    fn carry(&self, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for &(from, to, len) in &self.moved {
            for r in ranges {
                let (a, b) = (r.start.max(from), r.end.min(from + len));
                if a < b {
                    out.push(to + (a - from)..to + (b - from));
                }
            }
        }
        out
    }

    /// The input's copied runs, where they were moved to in the output: so the output still
    /// maps back to the document.
    fn carry_copied(&self, copied: &[Copied]) -> Vec<Copied> {
        let mut out: Vec<Copied> = Vec::new();
        for &(from, to, len) in &self.moved {
            let first = copied.partition_point(|c| c.out + c.len <= from);
            for c in &copied[first..] {
                if c.out >= from + len {
                    break;
                }
                let (a, b) = (c.out.max(from), (c.out + c.len).min(from + len));
                let run = Copied {
                    out: to + (a - from),
                    src: c.src + (a - c.out),
                    len: b - a,
                };
                match out.last_mut() {
                    Some(last)
                        if last.out + last.len == run.out && last.src + last.len == run.src =>
                    {
                        last.len += run.len;
                    }
                    _ => out.push(run),
                }
            }
        }
        out
    }
}

/// The name of an attribute from its raw `name = "value"`.
fn attribute_name(raw: &[u8]) -> &[u8] {
    let name = raw.split(|&b| b == b'=').next().unwrap_or(raw);
    let end = name
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .map_or(0, |i| i + 1);
    &name[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> Repeat {
        Repeat {
            count: "Count".into(),
            start: Some("Start".into()),
            suffix: Some("Suffix".into()),
            relative: Some("Relative".into()),
            index: vec!["[Index]".into(), "$Index".into()],
            relative_index: vec!["[RelativeIndex]".into(), "$RelativeIndex".into()],
            enabled: true,
        }
    }

    fn expanded(text: &str) -> String {
        let p = Preview {
            text: text.into(),
            ..Default::default()
        };
        expand(&p, &rule(), 1 << 20).text
    }

    #[test]
    fn copies_are_numbered_on_lines_of_their_own() {
        assert_eq!(
            expanded("<R>\r\n  <B Count=\"2\">Größe</B>\r\n</R>"),
            "<R>\r\n  <B0>Größe</B0>\r\n  <B1>Größe</B1>\r\n</R>"
        );
        assert_eq!(
            expanded("<R>\n  <B Start=\"3\" Count=\"2\"/>\n</R>"),
            "<R>\n  <B3/>\n  <B4/>\n</R>"
        );
        // None: the element goes.
        assert_eq!(expanded("<R><B Count=\"0\">x</B><C/></R>"), "<R><C/></R>");
        // Not a number: left as written.
        assert_eq!(
            expanded("<R><B Count=\"n\"/></R>"),
            "<R><B Count=\"n\"/></R>"
        );
        // Outside a copy, nothing else changes.
        assert_eq!(
            expanded("<R Default=\"[Index]\" Suffix=\"true\">$Index</R>"),
            "<R Default=\"[Index]\" Suffix=\"true\">$Index</R>"
        );
    }

    #[test]
    fn placeholders_take_the_copys_number() {
        let text = "<AI Count=\"2\" Start=\"1\" Default=\"[Index]\">\
                    <Ch Name=\"U[RelativeIndex]\">Kanal $Index</Ch>\
                    <Next>Ch$(Index+1)</Next><Prev>Ch$(RelativeIndex-1)</Prev></AI>";
        assert_eq!(
            expanded(text),
            "<AI1 Default=\"1\"><Ch Name=\"U0\">Kanal 1</Ch><Next>Ch2</Next>\
             <Prev>Ch-1</Prev></AI1>\n\
             <AI2 Default=\"2\"><Ch Name=\"U1\">Kanal 2</Ch><Next>Ch3</Next>\
             <Prev>Ch0</Prev></AI2>"
        );
    }

    #[test]
    fn suffixes_append_the_number_and_rule_attributes_go() {
        let text = "<AI Count=\"2\" Start=\"2\"><DI Suffix=\"true\">x</DI>\
                    <DO Suffix=\"TRUE\" Relative=\"true\"/><No Suffix=\"false\"/></AI>";
        assert_eq!(
            expanded(text),
            "<AI2><DI2>x</DI2><DO0/><No/></AI2>\n<AI3><DI3>x</DI3><DO1/><No/></AI3>"
        );
    }

    /// Inner counts are expanded with their own numbers, inside each outer copy. Copies go on
    /// lines of their own, also where the element stood on one line.
    #[test]
    fn inner_counts_take_their_own_numbers() {
        assert_eq!(
            expanded("<A Count=\"2\"><B Count=\"2\" Default=\"[Index]\"/></A>"),
            "<A0><B0 Default=\"0\"/>\n<B1 Default=\"1\"/></A0>\n\
             <A1><B0 Default=\"0\"/>\n<B1 Default=\"1\"/></A1>"
        );
    }

    /// Every copy maps back to the element it was copied from, and keeps its marks.
    #[test]
    fn copies_map_back_and_keep_their_marks() {
        let text = "<R>\r\n  <Größe Count=\"2\"><W/></Größe>\r\n</R>".to_string();
        let original = text.find("<Größe").unwrap();
        let w = text.find("<W/>").unwrap();
        let p = Preview {
            copied: vec![Copied {
                out: 0,
                src: 1000,
                len: text.len(),
            }],
            imported: std::iter::once(w..w + 4).collect(),
            text,
            ..Default::default()
        };
        let e = expand(&p, &rule(), 1 << 20);
        assert_eq!(
            e.text,
            "<R>\r\n  <Größe0><W/></Größe0>\r\n  <Größe1><W/></Größe1>\r\n</R>"
        );
        let copies: Vec<usize> = e.text.match_indices("<Größe").map(|(i, _)| i).collect();
        for c in copies {
            assert_eq!(e.source_of(c), Some(1000 + original));
        }
        // The renamed name is the expansion's own; the `<` before it is copied.
        assert_eq!(e.source_of(e.text.find("Größe0").unwrap()), None);
        let marked: Vec<&str> = e.imported.iter().map(|r| &e.text[r.clone()]).collect();
        assert_eq!(marked, ["<W/>", "<W/>"]);
    }

    #[test]
    fn copies_stop_at_the_limit() {
        let p = Preview {
            text: "<R><B Count=\"100000\">0123456789</B></R>".into(),
            ..Default::default()
        };
        let e = expand(&p, &rule(), 4096);
        assert!(e.cut);
        assert!(e.text.len() < 4096 + 100);
        assert!(e.text.ends_with("<!-- XML Atlas: cut after 4096 bytes -->"));
        assert_eq!(e.notes.len(), 1);
    }
}
