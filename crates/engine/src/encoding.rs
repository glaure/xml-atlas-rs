//! Files in encodings other than UTF-8 (`docs/spec/open.md`, OPEN-8; `docs/decisions.md`,
//! D23).
//!
//! A document is UTF-8 in memory, whatever its file is: everything else (index, views, find,
//! edits, format) works on UTF-8 only. Opening converts the file to UTF-8; saving converts it
//! back. The encoding comes from the byte order mark, then from the XML declaration. A UTF-8
//! file is taken as it is, byte for byte, and never converted.
//!
//! Converting must not change what the user did not change (`AGENTS.md`, rule 7). So at open
//! the converted text is converted back and compared with the file: when it does not come
//! back the same (a UTF-16 file with an unpaired surrogate, a byte the encoding does not
//! define), the document can be saved only as UTF-8. And a character typed that the
//! encoding cannot hold makes saving fail with where it is, never write `?` or `&#N;`.

use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE};

/// Bytes at the start of the file searched for the XML declaration.
const DECLARATION_BYTES: usize = 256;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const UTF16_LE_BOM: &[u8] = b"\xFF\xFE";
const UTF16_BE_BOM: &[u8] = b"\xFE\xFF";

/// The encoding of a file that is not UTF-8, which the document is saved in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEncoding {
    pub encoding: &'static Encoding,
    /// The name to show: as declared (`ISO-8859-1`), or `UTF-16LE` from the byte order mark.
    pub label: String,
    /// The file starts with a byte order mark, which saving writes again.
    pub bom: bool,
    /// Converting the file to UTF-8 and back gives the same bytes. When not, the document
    /// cannot be saved in this encoding.
    pub lossless: bool,
}

/// What opening found out about the encoding, besides the converted text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Opened {
    /// `None` for a UTF-8 file, which is kept as it is.
    pub encoding: Option<FileEncoding>,
    /// A declared encoding that is not used, and why: unknown, or UTF-16 in a file that is
    /// not UTF-16. Shown as a warning (OPEN-13).
    pub ignored: Option<String>,
}

/// Why the document cannot be written in its file's encoding.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unencodable {
    #[error("{ch:?} cannot be written in {encoding}")]
    Char {
        /// Byte of the document where the character is.
        offset: usize,
        ch: char,
        encoding: String,
    },
    #[error("the file has bytes that are not valid {encoding}; it can only be saved as UTF-8")]
    Lossy { encoding: String },
}

/// Converts the bytes of a file to the UTF-8 the document holds. A UTF-8 file comes back
/// unchanged, without copying.
pub fn decode(file: Vec<u8>) -> (Vec<u8>, Opened) {
    let (found, ignored) = detect(&file);
    let Some((encoding, label, bom_len)) = found else {
        return (
            file,
            Opened {
                encoding: None,
                ignored,
            },
        );
    };
    let body = &file[bom_len..];
    let (text, malformed) = encoding.decode_without_bom_handling(body);
    let lossless = if malformed {
        false
    } else if round_trips_by_itself(encoding) {
        true
    } else {
        // A multi-byte encoding may read two byte sequences as one character.
        encode_text(&text, encoding).is_ok_and(|back| back == body)
    };
    let src = match text {
        // ASCII only: the bytes are already the UTF-8.
        Cow::Borrowed(_) if bom_len == 0 => file,
        t => t.into_owned().into_bytes(),
    };
    let encoding = Some(FileEncoding {
        encoding,
        label,
        bom: bom_len > 0,
        lossless,
    });
    (src, Opened { encoding, ignored })
}

/// The bytes to write for the document's UTF-8 `src` in the file's encoding `enc`.
pub fn encode<'a>(src: &'a [u8], enc: Option<&FileEncoding>) -> Result<Cow<'a, [u8]>, Unencodable> {
    let Some(enc) = enc else {
        return Ok(Cow::Borrowed(src));
    };
    if !enc.lossless {
        return Err(Unencodable::Lossy {
            encoding: enc.label.clone(),
        });
    }
    let text = std::str::from_utf8(src).map_err(|e| Unencodable::Char {
        offset: e.valid_up_to(),
        ch: char::REPLACEMENT_CHARACTER,
        encoding: enc.label.clone(),
    })?;
    let mut out = Vec::with_capacity(src.len() + 2);
    if enc.bom {
        out.extend_from_slice(if enc.encoding == UTF_16BE {
            UTF16_BE_BOM
        } else if enc.encoding == UTF_16LE {
            UTF16_LE_BOM
        } else {
            UTF8_BOM
        });
    }
    match encode_text(text, enc.encoding) {
        Ok(bytes) => {
            out.extend_from_slice(&bytes);
            Ok(Cow::Owned(out))
        }
        Err((offset, ch)) => Err(Unencodable::Char {
            offset,
            ch,
            encoding: enc.label.clone(),
        }),
    }
}

/// The encoding of `file` when it is not UTF-8, its label and the length of its byte order
/// mark; and a declared encoding that is not used, with why.
#[allow(clippy::type_complexity)]
fn detect(file: &[u8]) -> (Option<(&'static Encoding, String, usize)>, Option<String>) {
    if file.starts_with(UTF8_BOM) {
        return (None, None);
    }
    if file.starts_with(UTF16_LE_BOM) {
        return (Some((UTF_16LE, "UTF-16LE".into(), 2)), None);
    }
    if file.starts_with(UTF16_BE_BOM) {
        return (Some((UTF_16BE, "UTF-16BE".into(), 2)), None);
    }
    // UTF-16 without a byte order mark: `<?` as two-byte units (XML 1.0, appendix F).
    if file.starts_with(b"<\0?\0") {
        return (Some((UTF_16LE, "UTF-16LE".into(), 0)), None);
    }
    if file.starts_with(b"\0<\0?") {
        return (Some((UTF_16BE, "UTF-16BE".into(), 0)), None);
    }
    let Some((label, _)) = declared(file) else {
        return (None, None);
    };
    match Encoding::for_label(label.as_bytes()) {
        Some(e) if e == encoding_rs::UTF_8 => (None, None),
        Some(e) if e == UTF_16LE || e == UTF_16BE => (
            None,
            Some(format!(
                "The file declares encoding {label}, but it is not UTF-16: it is read as UTF-8"
            )),
        ),
        // Labels the Encoding Standard maps to nothing usable.
        Some(e) if e.output_encoding() != e => (None, Some(unknown(&label))),
        Some(e) => (Some((e, label, 0)), None),
        None => (None, Some(unknown(&label))),
    }
}

fn unknown(label: &str) -> String {
    format!(
        "The file declares encoding {label}, which XML Atlas does not know: it is read as UTF-8, so characters outside ASCII may be wrong"
    )
}

/// The value of `encoding` in the XML declaration, and the byte where it starts. Reads only
/// the first bytes; the declaration is ASCII in every encoding this reads it for.
pub(crate) fn declared(src: &[u8]) -> Option<(String, usize)> {
    let head = &src[..src.len().min(DECLARATION_BYTES)];
    let start = if head.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
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
    Some((name.to_string(), at))
}

/// Whether every file that `encoding` decodes without error comes back the same when
/// encoded: for UTF-16 (whose errors are unpaired surrogates and a lone last byte) and the
/// single-byte encodings, each of whose bytes is its own character. Checked for all of them
/// by a test. Saves converting a large file back at open.
fn round_trips_by_itself(encoding: &'static Encoding) -> bool {
    encoding == UTF_16LE || encoding == UTF_16BE || encoding.is_single_byte()
}

/// `text` in `encoding`, or the byte offset and character that it cannot hold.
fn encode_text(text: &str, encoding: &'static Encoding) -> Result<Vec<u8>, (usize, char)> {
    if encoding == UTF_16LE || encoding == UTF_16BE {
        // The Encoding Standard decodes UTF-16 but does not encode it.
        let le = encoding == UTF_16LE;
        let mut out = Vec::with_capacity(text.len() * 2);
        for unit in text.encode_utf16() {
            out.extend_from_slice(&if le {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        return Ok(out);
    }
    let mut encoder = encoding.new_encoder();
    let mut out = vec![0; text.len() + 16];
    let (mut read, mut written) = (0, 0);
    loop {
        let (result, r, w) =
            encoder.encode_from_utf8_without_replacement(&text[read..], &mut out[written..], true);
        read += r;
        written += w;
        match result {
            encoding_rs::EncoderResult::InputEmpty => {
                out.truncate(written);
                return Ok(out);
            }
            encoding_rs::EncoderResult::OutputFull => {
                let more = out.len().max(64);
                out.resize(out.len() + more, 0);
            }
            encoding_rs::EncoderResult::Unmappable(c) => {
                return Err((read - c.len_utf8(), c));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latin1(text: &str) -> Vec<u8> {
        text.chars().map(|c| c as u32 as u8).collect()
    }

    #[test]
    fn utf8_stays_as_it_is() {
        for file in [
            b"<r>Gr\xC3\xBC\xC3\x9Fe</r>".to_vec(),
            b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r/>".to_vec(),
            b"<?xml version='1.0' encoding='utf-8'?><r>\xE4</r>".to_vec(),
        ] {
            let (src, opened) = decode(file.clone());
            assert_eq!(src, file);
            assert_eq!(opened.encoding, None);
            assert_eq!(encode(&src, None).unwrap(), &file[..]);
        }
    }

    #[test]
    fn latin1_round_trips_every_byte() {
        // Every byte from 0x20 to 0xFF, also those Windows-1252 leaves undefined.
        let mut file = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\r\n<r>".to_vec();
        file.extend((0x20u8..=0xFF).filter(|b| !matches!(b, b'<' | b'&')));
        file.extend_from_slice(b"</r>\r\n");
        let (src, opened) = decode(file.clone());
        let enc = opened.encoding.as_ref().unwrap();
        assert_eq!(enc.label, "ISO-8859-1");
        assert!(enc.lossless);
        let text = std::str::from_utf8(&src).unwrap();
        for c in ['ä', 'ö', 'ü', 'ß', 'ÿ', '€'] {
            assert!(text.contains(c), "{c} in {text}");
        }
        assert_eq!(encode(&src, Some(enc)).unwrap(), &file[..]);
    }

    #[test]
    fn utf16_round_trips_with_its_byte_order_mark() {
        for (bom, le) in [(UTF16_LE_BOM, true), (UTF16_BE_BOM, false)] {
            let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<r>Größe 日本 😀</r>";
            let mut file = bom.to_vec();
            for u in text.encode_utf16() {
                file.extend(if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            let (src, opened) = decode(file.clone());
            assert_eq!(src, text.as_bytes());
            let enc = opened.encoding.unwrap();
            assert!(enc.bom && enc.lossless);
            assert_eq!(encode(&src, Some(&enc)).unwrap(), &file[..]);
        }
        // Without a byte order mark, from `<?`.
        let file: Vec<u8> = "<?xml version=\"1.0\"?><r/>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let (src, opened) = decode(file.clone());
        assert_eq!(src, b"<?xml version=\"1.0\"?><r/>");
        assert_eq!(encode(&src, opened.encoding.as_ref()).unwrap(), &file[..]);
    }

    #[test]
    fn an_unpaired_surrogate_can_only_be_saved_as_utf8() {
        let mut file = UTF16_LE_BOM.to_vec();
        for u in "<r>"
            .encode_utf16()
            .chain([0xD800])
            .chain("</r>".encode_utf16())
        {
            file.extend(u.to_le_bytes());
        }
        let (src, opened) = decode(file);
        assert_eq!(std::str::from_utf8(&src).unwrap(), "<r>\u{FFFD}</r>");
        let enc = opened.encoding.unwrap();
        assert!(!enc.lossless);
        assert!(matches!(
            encode(&src, Some(&enc)),
            Err(Unencodable::Lossy { .. })
        ));
    }

    #[test]
    fn a_character_the_encoding_cannot_hold_is_refused_where_it_is() {
        let file = latin1("<?xml version=\"1.0\" encoding=\"latin1\"?><r>Größe</r>");
        let (mut src, opened) = decode(file);
        let enc = opened.encoding.unwrap();
        let at = src.len() - "</r>".len();
        src.splice(at..at, "日本".bytes());
        assert_eq!(
            encode(&src, Some(&enc)),
            Err(Unencodable::Char {
                offset: at,
                ch: '日',
                encoding: "latin1".into()
            })
        );
        // The euro sign is in Windows-1252, which ISO-8859-1 means on the web and here.
        let (mut src, opened) = decode(latin1(
            "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r/>",
        ));
        src.splice(3..3, "€".bytes());
        assert!(encode(&src, opened.encoding.as_ref()).is_ok());
    }

    /// [`round_trips_by_itself`]: in every single-byte encoding, each byte that decodes
    /// without error encodes back to itself.
    #[test]
    fn single_byte_encodings_round_trip_by_themselves() {
        let labels = [
            "ibm866",
            "iso-8859-2",
            "iso-8859-3",
            "iso-8859-4",
            "iso-8859-5",
            "iso-8859-6",
            "iso-8859-7",
            "iso-8859-8",
            "iso-8859-8-i",
            "iso-8859-10",
            "iso-8859-13",
            "iso-8859-14",
            "iso-8859-15",
            "iso-8859-16",
            "koi8-r",
            "koi8-u",
            "macintosh",
            "windows-874",
            "windows-1250",
            "windows-1251",
            "windows-1252",
            "windows-1253",
            "windows-1254",
            "windows-1255",
            "windows-1256",
            "windows-1257",
            "windows-1258",
            "x-mac-cyrillic",
        ];
        for label in labels {
            let enc = Encoding::for_label(label.as_bytes()).unwrap();
            assert!(enc.is_single_byte(), "{label}");
            for b in 0..=255u8 {
                let byte = [b];
                let (text, malformed) = enc.decode_without_bom_handling(&byte);
                if !malformed {
                    assert_eq!(encode_text(&text, enc), Ok(vec![b]), "{label} {b:#04x}");
                }
            }
        }
        // A byte a single-byte encoding leaves undefined cannot be saved back.
        let (_, opened) =
            decode(b"<?xml version=\"1.0\" encoding=\"windows-1253\"?><r>\xAA</r>".to_vec());
        assert!(!opened.encoding.unwrap().lossless);
    }

    #[test]
    fn declarations_that_are_not_used() {
        let (src, opened) = decode(b"<?xml version=\"1.0\" encoding=\"klingon\"?><r/>".to_vec());
        assert_eq!(opened.encoding, None);
        assert!(opened.ignored.unwrap().contains("klingon"));
        assert_eq!(src, b"<?xml version=\"1.0\" encoding=\"klingon\"?><r/>");
        let (_, opened) = decode(b"<?xml version=\"1.0\" encoding=\"UTF-16\"?><r/>".to_vec());
        assert_eq!(opened.encoding, None);
        assert!(opened.ignored.unwrap().contains("not UTF-16"));
    }
}
