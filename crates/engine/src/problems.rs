//! Warnings and errors about a document, for the error list (`docs/spec/open.md`, OPEN-13).
//!
//! The list holds the parse errors (OPEN-10) and what is wrong with the file's encoding
//! (OPEN-8). Only the first bytes are read for the latter, so building the list costs
//! nothing measurable.

use std::fmt;

use xml_atlas_xml::ParseReason;

use crate::{Document, IgnoredEncoding, encoding};

/// Encoding names that need no conversion.
const UTF8_NAMES: &[&str] = &["utf-8", "utf8", "us-ascii", "ascii"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    /// Byte where the problem is.
    pub offset: u64,
    pub message: Message,
}

/// What a problem is. Its words (`Display`) are the English the CLI and the MCP tools give;
/// the window words each case in its own language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// Text that is not well-formed (OPEN-10).
    Parse(ParseReason),
    /// The encoding the file declares is not used.
    IgnoredEncoding(IgnoredEncoding),
    /// The file has bytes its encoding does not allow.
    InvalidBytes { encoding: String },
    /// The declaration names another encoding than the UTF-8 the document is saved in.
    SavedAsUtf8 { declared: String },
    /// A reference that finds nothing (REF-4), already in words.
    Reference(String),
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(reason) => reason.fmt(f),
            Self::IgnoredEncoding(why) => why.fmt(f),
            Self::InvalidBytes { encoding } => write!(
                f,
                "The file has bytes that are not valid {encoding}; they are shown as \u{FFFD}. It \
                 can only be saved as UTF-8"
            ),
            Self::SavedAsUtf8 { declared } => write!(
                f,
                "The declaration says encoding {declared}, but the document is saved as UTF-8"
            ),
            Self::Reference(words) => f.write_str(words),
        }
    }
}

impl Document {
    /// Problems in document order.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        if let Some(p) = encoding_problem(self) {
            out.push(p);
        }
        for e in self.index.error.iter().chain(&self.index.more_errors) {
            out.push(Problem {
                severity: Severity::Error,
                offset: e.offset,
                message: Message::Parse(e.reason.clone()),
            });
        }
        out.sort_by_key(|p| p.offset);
        out
    }
}

/// What is wrong with the file's encoding: a declaration that is not used, bytes that do
/// not convert back, or a declaration that no longer says what the document is saved in.
fn encoding_problem(doc: &Document) -> Option<Problem> {
    let declared = encoding::declared(&doc.src);
    let at = declared.as_ref().map_or(0, |d| d.1 as u64);
    if let Some(note) = doc.encoding_note() {
        return Some(Problem {
            severity: Severity::Warning,
            offset: at,
            message: Message::IgnoredEncoding(note.clone()),
        });
    }
    if let Some(enc) = doc.encoding() {
        if enc.lossless {
            return None;
        }
        // Where the first byte that did not convert is shown.
        let bad = memchr::memmem::find(&doc.src, "\u{FFFD}".as_bytes()).unwrap_or(0);
        return Some(Problem {
            severity: Severity::Warning,
            offset: bad as u64,
            message: Message::InvalidBytes {
                encoding: enc.label.clone(),
            },
        });
    }
    // A UTF-8 document whose declaration names another encoding: after Save as UTF-8 was
    // undone.
    let (name, at) = declared?;
    if UTF8_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
        return None;
    }
    Some(Problem {
        severity: Severity::Warning,
        offset: at as u64,
        message: Message::SavedAsUtf8 { declared: name },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(src: &[u8]) -> Vec<Problem> {
        Document::from_bytes(src.to_vec()).problems()
    }

    #[test]
    fn well_formed_utf8_has_none() {
        assert_eq!(
            problems(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><a/>"),
            []
        );
        assert_eq!(
            problems(b"\xEF\xBB\xBF<?xml version='1.0' encoding='utf8'?><a/>"),
            []
        );
        assert_eq!(problems(b"<a/>"), []);
    }

    #[test]
    fn latin1_and_utf16_have_none() {
        let src = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r>Gr\u{f6}\u{df}e</r>";
        let bytes: Vec<u8> = src.chars().map(|c| c as u8).collect();
        assert_eq!(problems(&bytes), []);
        let mut src = b"\xFF\xFE".to_vec();
        src.extend("<r/>".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(problems(&src), []);
    }

    #[test]
    fn an_unknown_encoding_is_a_warning_at_its_name() {
        let src = b"<?xml version=\"1.0\" encoding=\"klingon\"?><r/>";
        let p = problems(src);
        assert_eq!(p[0].severity, Severity::Warning);
        assert_eq!(&src[p[0].offset as usize..][..7], b"klingon");
    }

    #[test]
    fn bytes_that_do_not_convert_back_are_a_warning_where_they_show() {
        let mut src = b"\xFF\xFE".to_vec();
        for u in "<r>"
            .encode_utf16()
            .chain([0xDC00])
            .chain("</r>".encode_utf16())
        {
            src.extend(u.to_le_bytes());
        }
        let p = problems(&src);
        assert_eq!((p[0].severity, p[0].offset), (Severity::Warning, 3));
        assert!(p[0].message.to_string().contains("only be saved as UTF-8"));
    }

    #[test]
    fn parse_error_is_listed() {
        // XML Marker's example (XM-11): `</a>` closes `c`, which is reported where it starts.
        let p = problems(b"<a><b>one</b><c>two</a>");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].severity, p[0].offset), (Severity::Error, 13));
        assert!(p[0].message.to_string().contains("<c>"), "{}", p[0].message);
    }
}
