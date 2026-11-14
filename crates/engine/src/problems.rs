//! Warnings and errors about a document, for the error list (`docs/spec/open.md`, OPEN-13).
//!
//! Today the list holds the first parse error, and what the start of the file says about its
//! encoding when that encoding is not UTF-8. Only the first bytes are read, so building the
//! list costs nothing measurable. Parsing on after the first error is OPEN-10.

use crate::Document;

/// Bytes at the start of the file searched for the XML declaration.
const DECLARATION_BYTES: usize = 256;

const UTF16_LE_BOM: &[u8] = b"\xFF\xFE";
const UTF16_BE_BOM: &[u8] = b"\xFE\xFF";

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
    pub message: String,
}

impl Document {
    /// Problems in document order.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        if let Some(p) = encoding_problem(&self.src) {
            out.push(p);
        }
        if let Some(e) = &self.index.error {
            out.push(Problem {
                severity: Severity::Error,
                offset: e.offset,
                message: e.message.clone(),
            });
        }
        out.sort_by_key(|p| p.offset);
        out
    }
}

/// A UTF-16 byte order mark, or a declared encoding other than UTF-8.
fn encoding_problem(src: &[u8]) -> Option<Problem> {
    if src.starts_with(UTF16_LE_BOM) || src.starts_with(UTF16_BE_BOM) {
        return Some(Problem {
            severity: Severity::Error,
            offset: 0,
            message: "UTF-16 is not supported yet; the file cannot be read".into(),
        });
    }
    let (name, at) = declared_encoding(src)?;
    if UTF8_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return None;
    }
    Some(Problem {
        severity: Severity::Warning,
        offset: at as u64,
        message: format!(
            "The file declares encoding {name}, which is not supported yet: it is shown as \
             UTF-8, so characters outside ASCII are wrong"
        ),
    })
}

/// The value of `encoding` in the XML declaration, and the byte where it starts.
fn declared_encoding(src: &[u8]) -> Option<(&str, usize)> {
    let head = &src[..src.len().min(DECLARATION_BYTES)];
    let start = if head.starts_with(crate::UTF8_BOM) {
        crate::UTF8_BOM.len()
    } else {
        0
    };
    let head = &head[start..];
    if !head.starts_with(b"<?xml") {
        return None;
    }
    let end = memchr::memmem::find(head, b"?>")?;
    let decl = &head[..end];
    let key = memchr::memmem::find(decl, b"encoding")?;
    let rest = &decl[key + b"encoding".len()..];
    let eq = rest.iter().position(|&b| b == b'=')?;
    let after = &rest[eq + 1..];
    let q = after.iter().position(|&b| b == b'"' || b == b'\'')?;
    let quote = after[q];
    let value = &after[q + 1..];
    let close = value.iter().position(|&b| b == quote)?;
    let name = std::str::from_utf8(&value[..close]).ok()?;
    let at = start + key + b"encoding".len() + eq + 1 + q + 1;
    Some((name, at))
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
    fn declared_latin1_is_a_warning_at_the_encoding_name() {
        let src = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r>Gr\u{f6}\u{df}e</r>";
        let bytes: Vec<u8> = src.chars().map(|c| c as u8).collect();
        let p = problems(&bytes);
        assert_eq!(p[0].severity, Severity::Warning);
        assert_eq!(&bytes[p[0].offset as usize..][..10], b"ISO-8859-1");
        assert!(p[0].message.contains("ISO-8859-1"));
        // The bytes that are not UTF-8 also stop the parser, after the warning.
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].severity, Severity::Error);
        assert!(p[1].offset > p[0].offset);
    }

    #[test]
    fn utf16_is_an_error_at_the_start() {
        let mut src = b"\xFF\xFE".to_vec();
        src.extend("<r/>".encode_utf16().flat_map(u16::to_le_bytes));
        let p = problems(&src);
        assert_eq!((p[0].severity, p[0].offset), (Severity::Error, 0));
        assert!(p[0].message.contains("UTF-16"));
    }

    #[test]
    fn parse_error_is_listed() {
        let p = problems(b"<a><b>one</b><c>two</a>");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].severity, Severity::Error);
        assert!(p[0].message.contains("</c>"), "{}", p[0].message);
    }
}
