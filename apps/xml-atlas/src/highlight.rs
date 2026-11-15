//! XML syntax colouring, a line at a time.
//!
//! Cost scales with what is on screen, not with the file. The scanner walks bytes and cuts the
//! line only at ASCII markup characters, which are always character boundaries; lookahead
//! compares bytes and never slices in the middle of a character. A line starts in the
//! [`State`] the one above ended in; the first line on screen gets it from the index
//! ([`state_at`]), so comments, CDATA sections and attribute values that span lines are
//! coloured right (TEXT-3).

use std::ops::Range;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};
use xml_atlas_engine::xml::{Folded, Index, NodeKind, ROOT};

pub struct Palette {
    pub text: Color32,
    pub tag: Color32,
    pub attr: Color32,
    pub value: Color32,
    pub comment: Color32,
    pub punct: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                text: Color32::from_rgb(0xd4, 0xd4, 0xd4),
                tag: Color32::from_rgb(0x56, 0x9c, 0xd6),
                attr: Color32::from_rgb(0x9c, 0xdc, 0xfe),
                value: Color32::from_rgb(0xce, 0x91, 0x78),
                comment: Color32::from_rgb(0x6a, 0x99, 0x55),
                punct: Color32::from_rgb(0x80, 0x80, 0x80),
            }
        } else {
            Self {
                text: Color32::from_rgb(0x1f, 0x1f, 0x1f),
                tag: Color32::from_rgb(0x00, 0x00, 0xb0),
                attr: Color32::from_rgb(0xa0, 0x30, 0x00),
                value: Color32::from_rgb(0x00, 0x70, 0x30),
                comment: Color32::from_rgb(0x70, 0x70, 0x70),
                punct: Color32::from_rgb(0x60, 0x60, 0x60),
            }
        }
    }
}

/// Where in the markup a line starts or ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Text,
    TagName,
    InTag,
    /// Inside an attribute value quoted with this byte.
    Value(u8),
    Comment,
    CData,
}

/// What a piece of a line is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Text,
    Tag,
    Attr,
    Value,
    Comment,
    Punct,
}

/// Lays out `line`, which starts in state `start`. Returns the job and the state at its end:
/// what the next row of a wrapped line starts in (TEXT-13). The next line starts in
/// [`after_line_break`] of it.
pub fn line_job(line: &str, font: &FontId, p: &Palette, start: State) -> (LayoutJob, State) {
    let mut job = LayoutJob::default();
    let end = walk(line.as_bytes(), start, |r, role| {
        let color = match role {
            Role::Text => p.text,
            Role::Tag => p.tag,
            Role::Attr => p.attr,
            Role::Value => p.value,
            Role::Comment => p.comment,
            Role::Punct => p.punct,
        };
        job.append(
            &line[r],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    });
    (job, end)
}

/// The state a line starts in after a line that ended in `end`: the line break after an
/// element name ends the name.
pub fn after_line_break(end: State) -> State {
    if end == State::TagName {
        State::InTag
    } else {
        end
    }
}

/// The state at byte `at` of the source, from the node that holds it: what a line starting
/// there starts in. Reads only the start tag around `at`, if any.
pub fn state_at(index: &Index, src: &[u8], at: usize) -> State {
    let id = index.innermost_at(at as u64);
    if id == ROOT {
        return State::Text;
    }
    let n = index.node(id);
    let start = n.start as usize;
    if at <= start {
        return State::Text;
    }
    let markup = |end: usize| {
        walk(
            src.get(start..end).unwrap_or_default(),
            State::Text,
            |_, _| {},
        )
    };
    match n.kind {
        NodeKind::Comment => State::Comment,
        NodeKind::CData => State::CData,
        NodeKind::Element if at < start + n.head_len as usize => markup(at),
        NodeKind::Element => match index.folded_span(src, id) {
            Some(r) if n.folded == Folded::CData && r.start < at && at < r.end => State::CData,
            _ => State::Text,
        },
        NodeKind::ProcessingInstruction | NodeKind::Declaration | NodeKind::DocType => markup(at),
        NodeKind::Text | NodeKind::Document => State::Text,
    }
}

/// Walks `b` from `state`, handing each non-empty piece to `emit`, and returns the state at
/// its end.
fn walk(b: &[u8], mut state: State, mut emit: impl FnMut(Range<usize>, Role)) -> State {
    let mut push = |r: Range<usize>, role: Role| {
        if !r.is_empty() {
            emit(r, role);
        }
    };
    let mut seg = 0;
    // The opening quote of the value being walked, when it is on this line.
    let mut quote_at = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match state {
            State::Text if c == b'<' => {
                push(seg..i, Role::Text);
                if b[i..].starts_with(b"<!--") {
                    state = State::Comment;
                    seg = i;
                    i += 4;
                    continue;
                }
                if b[i..].starts_with(b"<![CDATA[") {
                    push(i..i + 9, Role::Punct);
                    state = State::CData;
                    i += 9;
                    seg = i;
                    continue;
                }
                let n = if b
                    .get(i + 1)
                    .is_some_and(|&n| n == b'/' || n == b'?' || n == b'!')
                {
                    2
                } else {
                    1
                };
                push(i..i + n, Role::Punct);
                i += n;
                seg = i;
                state = State::TagName;
                continue;
            }
            State::Comment if b[i..].starts_with(b"-->") => {
                push(seg..i + 3, Role::Comment);
                i += 3;
                seg = i;
                state = State::Text;
                continue;
            }
            State::CData if b[i..].starts_with(b"]]>") => {
                push(seg..i, Role::Text);
                push(i..i + 3, Role::Punct);
                i += 3;
                seg = i;
                state = State::Text;
                continue;
            }
            State::TagName if c.is_ascii_whitespace() || c == b'>' || c == b'/' || c == b'?' => {
                push(seg..i, Role::Tag);
                seg = i;
                state = State::InTag;
                continue;
            }
            State::InTag if c == b'"' || c == b'\'' => {
                push(seg..i, Role::Attr);
                seg = i;
                quote_at = Some(i);
                state = State::Value(c);
            }
            State::InTag if c == b'=' || c == b'>' || c == b'/' || c == b'?' => {
                push(seg..i, Role::Attr);
                push(i..i + 1, Role::Punct);
                seg = i + 1;
                if c == b'>' {
                    state = State::Text;
                }
            }
            State::Value(q) if c == q && quote_at != Some(i) => {
                push(seg..i + 1, Role::Value);
                seg = i + 1;
                state = State::InTag;
            }
            _ => {}
        }
        i += 1;
    }
    let role = match state {
        State::Text | State::CData => Role::Text,
        State::TagName => Role::Tag,
        State::InTag => Role::Attr,
        State::Value(_) => Role::Value,
        State::Comment => Role::Comment,
    };
    push(seg..b.len(), role);
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(line: &str) -> Vec<(String, Color32)> {
        pieces_from(line, State::Text).0
    }

    fn pieces_from(line: &str, start: State) -> (Vec<(String, Color32)>, State) {
        let (job, end) = line_job(line, &FontId::monospace(13.0), &Palette::new(true), start);
        let pieces = job
            .sections
            .iter()
            .map(|s| {
                let range = s.byte_range.start.0..s.byte_range.end.0;
                (job.text[range].to_string(), s.format.color)
            })
            .collect();
        (pieces, after_line_break(end))
    }

    /// A comment, a CDATA section and an attribute value over several lines, with a byte order
    /// mark, CRLF and umlauts.
    const SPANNING: &str = "\u{feff}<?xml version=\"1.0\"?>\r\n<r>\r\n<!-- Größe\r\n  <a x=\"1\"> Maße -->\r\n<c><![CDATA[ä\r\n<b x=\"\r\n]]></c>\r\n<d t=\"eins\r\nzwei\" u='\r\n'>Öl</d>\r\n<e\r\n  k=\"v\"/>\r\n</r>\r\n";

    /// The lines of `src` after the byte order mark, each with where it starts.
    fn lines(src: &str) -> Vec<(usize, &str)> {
        let bom = if src.starts_with('\u{feff}') { 3 } else { 0 };
        let mut at = bom;
        src[bom..]
            .split('\n')
            .map(|l| {
                let start = at;
                at += l.len() + 1;
                (start, l.trim_end_matches('\r'))
            })
            .collect()
    }

    /// Lines inside a comment, CDATA or a value are coloured as such, not from scratch
    /// (TEXT-3).
    #[test]
    fn constructs_over_lines_keep_their_colour() {
        let p = Palette::new(true);
        let mut state = State::Text;
        let mut got = Vec::new();
        for (_, line) in lines(SPANNING) {
            let (pieces, end) = pieces_from(line, state);
            got.push(pieces);
            state = end;
        }
        let at = |line: usize| got[line].clone();
        // Line 3: `  <a x="1"> Maße -->`, all comment.
        assert_eq!(at(3), [("  <a x=\"1\"> Maße -->".to_string(), p.comment)]);
        // Line 5: `<b x="` inside CDATA is text; then `]]>` ends it.
        assert_eq!(at(5), [("<b x=\"".to_string(), p.text)]);
        // egui joins neighbouring pieces of one colour.
        assert_eq!(at(6)[0], ("]]></".to_string(), p.punct));
        assert_eq!(at(6)[1], ("c".to_string(), p.tag));
        // Line 8: `zwei"` closes the value; ` u` is an attribute again.
        assert_eq!(at(8)[0], ("zwei\"".to_string(), p.value));
        assert_eq!(at(8)[1], (" u".to_string(), p.attr));
        // Line 9: `'>Öl</d>`: a value that is only its closing quote.
        assert_eq!(at(9)[0], ("'".to_string(), p.value));
        assert_eq!(at(9)[2], ("Öl".to_string(), p.text));
        // Line 11: `  k="v"/>`, an attribute after a line break in the start tag.
        assert_eq!(at(11)[0], ("  k".to_string(), p.attr));
        assert_eq!(state, State::Text);
    }

    /// The state the index gives for each line's start is the one carried down from the top,
    /// so the view may start at any line.
    #[test]
    fn state_from_the_index_equals_the_carried_state() {
        let src = SPANNING.as_bytes();
        let index = xml_atlas_engine::xml::index(src);
        assert!(index.error.is_none());
        let mut state = State::Text;
        for (n, (start, line)) in lines(SPANNING).into_iter().enumerate() {
            assert_eq!(state_at(&index, src, start), state, "line {n}: {line}");
            state = pieces_from(line, state).1;
        }
    }

    fn joined(line: &str) -> String {
        pieces(line).into_iter().map(|(text, _)| text).collect()
    }

    #[test]
    fn non_ascii_text_does_not_panic_and_is_kept() {
        for line in [
            "<!-- Größe und Maße -->",
            "<!-- 日本語 – ünïcödé",
            "ünïcödé --> <a>",
            "<größe maß=\"Füße\">Äpfel &amp; Öl</größe>",
            "<a b='€'/>€",
            "é",
        ] {
            assert_eq!(joined(line), line);
        }
    }

    #[test]
    fn colours_each_part() {
        let p = Palette::new(true);
        let got = pieces("<a x=\"1\">t</a><!-- c -->");
        let want = [
            ("<", p.punct),
            ("a", p.tag),
            (" x", p.attr),
            ("=", p.punct),
            ("\"1\"", p.value),
            (">", p.punct),
            ("t", p.text),
            ("</", p.punct),
            ("a", p.tag),
            (">", p.punct),
            ("<!-- c -->", p.comment),
        ];
        let want: Vec<_> = want.iter().map(|(t, c)| (t.to_string(), *c)).collect();
        assert_eq!(got, want);
    }
}
