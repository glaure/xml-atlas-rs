//! Per-line XML syntax colouring.
//!
//! Works on one line at a time so cost scales with what is on screen, not with the file.
//! The scanner walks bytes and cuts the line only at ASCII markup characters, which are always
//! character boundaries; lookahead compares bytes and never slices in the middle of a character.
//! Constructs that span lines (multi-line comments, CDATA, attribute values) are coloured
//! from where the line starts; a stateful highlighter is on the roadmap.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

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

#[derive(Clone, Copy, PartialEq)]
enum State {
    Text,
    TagName,
    InTag,
    Value(u8),
    Comment,
}

pub fn line_job(line: &str, font: &FontId, p: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut push = |s: &str, color: Color32| {
        if !s.is_empty() {
            job.append(
                s,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color,
                    ..Default::default()
                },
            );
        }
    };
    let b = line.as_bytes();
    let mut state = State::Text;
    let mut seg = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match state {
            State::Text if c == b'<' => {
                push(&line[seg..i], p.text);
                if b[i..].starts_with(b"<!--") {
                    state = State::Comment;
                    seg = i;
                    i += 4;
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
                push(&line[i..i + n], p.punct);
                i += n;
                seg = i;
                state = State::TagName;
                continue;
            }
            State::Comment if b[i..].starts_with(b"-->") => {
                push(&line[seg..i + 3], p.comment);
                i += 3;
                seg = i;
                state = State::Text;
                continue;
            }
            State::TagName if c.is_ascii_whitespace() || c == b'>' || c == b'/' || c == b'?' => {
                push(&line[seg..i], p.tag);
                seg = i;
                state = State::InTag;
                continue;
            }
            State::InTag if c == b'"' || c == b'\'' => {
                push(&line[seg..i], p.attr);
                seg = i;
                state = State::Value(c);
            }
            State::InTag if c == b'=' || c == b'>' || c == b'/' || c == b'?' => {
                push(&line[seg..i], p.attr);
                push(&line[i..i + 1], p.punct);
                seg = i + 1;
                if c == b'>' {
                    state = State::Text;
                }
            }
            State::Value(q) if c == q && i > seg => {
                push(&line[seg..=i], p.value);
                seg = i + 1;
                state = State::InTag;
            }
            _ => {}
        }
        i += 1;
    }
    let color = match state {
        State::Text => p.text,
        State::TagName => p.tag,
        State::InTag => p.attr,
        State::Value(_) => p.value,
        State::Comment => p.comment,
    };
    push(&line[seg..], color);
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(line: &str) -> Vec<(String, Color32)> {
        let job = line_job(line, &FontId::monospace(13.0), &Palette::new(true));
        job.sections
            .iter()
            .map(|s| {
                let range = s.byte_range.start.0..s.byte_range.end.0;
                (job.text[range].to_string(), s.format.color)
            })
            .collect()
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
