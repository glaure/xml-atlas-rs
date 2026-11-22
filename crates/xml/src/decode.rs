//! Text and attributes decoded as far as a reader takes them, for views that show the start
//! of a value that may be as large as the file (`docs/spec/tree.md`, TREE-7).

use std::borrow::Cow;

use quick_xml::events::attributes::Attributes;

use crate::{AttributeSpans, is_space, split_attribute};

/// The characters of raw text or of a raw attribute value as [`crate::unescape`] gives them,
/// one at a time. Only what is taken is read, with one exception: the first `&` taken reads
/// the references after it, as one that is not known leaves all of the value as written.
#[derive(Clone, Debug)]
pub struct Decoded<'s> {
    rest: &'s [u8],
    references: References,
}

#[derive(Clone, Copy, Debug)]
enum References {
    Unchecked,
    Replaced,
    AsWritten,
}

impl<'s> Decoded<'s> {
    /// Characters as [`crate::unescape`] gives them.
    pub fn escaped(raw: &'s [u8]) -> Self {
        Self {
            rest: raw,
            references: References::Unchecked,
        }
    }

    /// Characters as written, as in CDATA.
    pub fn as_written(raw: &'s [u8]) -> Self {
        Self {
            rest: raw,
            references: References::AsWritten,
        }
    }
}

impl Iterator for Decoded<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        let &first = self.rest.first()?;
        if first == b'&' {
            if let References::Unchecked = self.references {
                // No `&` came before, so both readings are the same up to here.
                self.references = if all_known(self.rest) {
                    References::Replaced
                } else {
                    References::AsWritten
                };
            }
            if let References::Replaced = self.references
                && let Some((c, len)) = reference(self.rest)
            {
                self.rest = &self.rest[len..];
                return Some(c);
            }
        }
        let (c, len) = if first.is_ascii() {
            (char::from(first), 1)
        } else {
            first_char(self.rest)
        };
        self.rest = &self.rest[len..];
        Some(c)
    }
}

/// The attributes of one start tag, raw, as [`crate::Index::attributes`] finds them, each
/// read when it is taken. Allocates only for a tag that is broken, from the break on.
pub struct AttributeParts<'s> {
    walk: Walk<'s>,
}

enum Walk<'s> {
    /// Attributes written `name = "value"` so far.
    Good(AttributeSpans<'s>),
    /// The rest of a broken tag, as quick-xml recovers it.
    Broken(std::vec::IntoIter<(String, String)>),
}

impl<'s> AttributeParts<'s> {
    /// The attributes in `head`, a start tag without its brackets, after a name of `name_len`
    /// bytes.
    pub(crate) fn new(head: &'s [u8], name_len: usize) -> Self {
        let name_len = name_len.min(head.len());
        let walk = if std::str::from_utf8(&head[..name_len]).is_ok() {
            Walk::Good(AttributeSpans {
                src: head,
                at: name_len,
                end: head.len(),
            })
        } else {
            // Replaced bytes in the name move the place where the attributes start.
            Walk::Broken(recovered(head, name_len))
        };
        Self { walk }
    }
}

impl<'s> Iterator for AttributeParts<'s> {
    /// Name and value, without the quotes and not unescaped.
    type Item = (Cow<'s, [u8]>, Cow<'s, [u8]>);

    fn next(&mut self) -> Option<Self::Item> {
        if let Walk::Good(spans) = &mut self.walk {
            let (src, from) = (spans.src, spans.at);
            match spans.next() {
                Some(span) => {
                    let (name, value) = split_attribute(&src[span])?;
                    return Some((name.into(), value.into()));
                }
                None => {
                    let rest = &src[from..];
                    if rest.iter().all(is_space) {
                        return None;
                    }
                    self.walk = Walk::Broken(recovered(rest, 0));
                }
            }
        }
        let Walk::Broken(rest) = &mut self.walk else {
            return None;
        };
        rest.next()
            .map(|(name, value)| (name.into_bytes().into(), value.into_bytes().into()))
    }
}

/// The attributes quick-xml finds in `raw` from `at` on, skipping what is broken.
fn recovered(raw: &[u8], at: usize) -> std::vec::IntoIter<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let found: Vec<(String, String)> = Attributes::new(&text, at)
        .with_checks(false)
        .flatten()
        .map(|a| (a.key.as_ref().to_owned(), a.value.as_ref().to_owned()))
        .collect();
    found.into_iter()
}

/// Whether every reference in `raw` is one quick-xml replaces.
fn all_known(raw: &[u8]) -> bool {
    memchr::memchr_iter(b'&', raw).all(|at| reference(&raw[at..]).is_some())
}

/// The reference `raw` starts with: what it stands for and its length.
fn reference(raw: &[u8]) -> Option<(char, usize)> {
    // The five that quick-xml knows by name come first and as bytes: a large value can hold
    // millions, and all are looked at when its first is taken.
    match raw {
        [b'&', b'a', b'm', b'p', b';', ..] => Some(('&', 5)),
        [b'&', b'l', b't', b';', ..] => Some(('<', 4)),
        [b'&', b'g', b't', b';', ..] => Some(('>', 4)),
        [b'&', b'a', b'p', b'o', b's', b';', ..] => Some(('\'', 6)),
        [b'&', b'q', b'u', b'o', b't', b';', ..] => Some(('"', 6)),
        [b'&', b'#', ..] => {
            let end = memchr::memchr2(b'&', b';', &raw[1..])? + 1;
            if raw[end] != b';' {
                return None;
            }
            let whole = std::str::from_utf8(&raw[..=end]).ok()?;
            let c = quick_xml::escape::unescape(whole).ok()?.chars().next()?;
            Some((c, end + 1))
        }
        _ => None,
    }
}

/// The first character of `raw`, which is not empty, as `String::from_utf8_lossy` reads it.
fn first_char(raw: &[u8]) -> (char, usize) {
    let replaced = (char::REPLACEMENT_CHARACTER, 1);
    // No character and no replaced run of bytes is longer than four.
    let Some(chunk) = raw[..raw.len().min(4)].utf8_chunks().next() else {
        return replaced;
    };
    match chunk.valid().chars().next() {
        Some(c) => (c, c.len_utf8()),
        None => (replaced.0, chunk.invalid().len()),
    }
}

#[cfg(test)]
mod tests {
    use quick_xml::events::BytesStart;

    use super::*;
    use crate::unescape;

    /// Every string of up to `len` of `pieces`.
    fn all_strings(pieces: &[&[u8]], len: usize) -> Vec<Vec<u8>> {
        let mut all = vec![Vec::new()];
        let mut from = 0;
        for _ in 0..len {
            let to = all.len();
            for i in from..to {
                for piece in pieces {
                    let longer = [all[i].as_slice(), piece].concat();
                    all.push(longer);
                }
            }
            from = to;
        }
        all
    }

    /// Every short value reads as `unescape` reads it whole.
    #[test]
    fn decoded_is_what_unescape_gives() {
        let pieces: [&[u8]; 21] = [
            b"&",
            b";",
            b"#",
            b"x",
            b"0",
            b"4",
            b"+",
            b" ",
            b"&amp;",
            b"&lt;",
            b"&gt;",
            b"&apos;",
            b"&quot;",
            b"&#252;",
            b"&#xD800;",
            b"&company;",
            "ü".as_bytes(),
            "😀".as_bytes(),
            b"\r\n",
            b"\xC3",
            b"\xF0\x9F\x98",
        ];
        for raw in all_strings(&pieces, 4) {
            let got: String = Decoded::escaped(&raw).collect();
            assert_eq!(got, unescape(&raw), "{}", raw.escape_ascii());
            let got: String = Decoded::as_written(&raw).collect();
            assert_eq!(got, String::from_utf8_lossy(&raw), "{}", raw.escape_ascii());
        }
    }

    /// Taking the start reads the start.
    #[test]
    fn decoded_reads_no_further_than_it_is_taken() {
        let raw = format!("{}&amp;", "Größe ".repeat(1000));
        let mut decoded = Decoded::escaped(raw.as_bytes());
        assert_eq!(decoded.by_ref().take(6).collect::<String>(), "Größe ");
        assert_eq!(decoded.rest.len(), raw.len() - "Größe ".len());
        assert!(matches!(decoded.references, References::Unchecked));
    }

    /// Every short start tag, broken or not, has the attributes quick-xml finds in it.
    #[test]
    fn attribute_parts_are_what_quick_xml_finds() {
        let pieces: [&[u8]; 14] = [
            b"a",
            b"=",
            b"\"",
            b"'",
            b" ",
            b"\r\n",
            b"\t",
            b"/",
            b">",
            b"&amp;",
            " größe=\"L\"".as_bytes(),
            b" b = '2'",
            b"x=1",
            b"\xFF",
        ];
        for tail in all_strings(&pieces, 4) {
            for name in ["e", "größe"] {
                let head = [name.as_bytes(), &tail].concat();
                let text = String::from_utf8_lossy(&head);
                let want: Vec<(String, String)> = BytesStart::from_content(&*text, name.len())
                    .attributes()
                    .with_checks(false)
                    .flatten()
                    .map(|a| (a.key.as_ref().to_owned(), a.value.as_ref().to_owned()))
                    .collect();
                let got: Vec<(String, String)> = AttributeParts::new(&head, name.len())
                    .map(|(name, value)| {
                        (
                            String::from_utf8_lossy(&name).into_owned(),
                            String::from_utf8_lossy(&value).into_owned(),
                        )
                    })
                    .collect();
                assert_eq!(got, want, "{}", head.escape_ascii());
            }
        }
    }

    /// Taking the first attribute does not read the second.
    #[test]
    fn attribute_parts_read_no_further_than_they_are_taken() {
        let head = format!("e a=\"1\" b=\"{}\"", "Größe ".repeat(1000));
        let mut parts = AttributeParts::new(head.as_bytes(), 1);
        assert!(parts.next().is_some());
        let Walk::Good(spans) = &parts.walk else {
            panic!("the tag is not broken");
        };
        assert_eq!(spans.at, "e a=\"1\"".len());
    }
}
