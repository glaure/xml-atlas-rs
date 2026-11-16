//! A scanner over the raw bytes for well-formed UTF-8 documents (`docs/decisions.md`, D14).
//!
//! It finds markup by searching the bytes (a short loop, then `memchr`) instead of reading
//! an event per token, and builds the same
//! nodes as the quick-xml path through the builder's own steps. At the first thing it is
//! not sure to read as quick-xml does (an error of any kind, a byte that is not UTF-8, a
//! DOCTYPE with an internal subset, an empty name) it gives up, and the caller indexes the
//! document with quick-xml, which reports the error and goes on after it (OPEN-10).

use memchr::{memchr2, memmem};

use crate::{Builder, MAX_NAME_LEN, NodeKind, parallel};

/// Bytes looked at one by one before a search hands over to `memchr`. Most gaps in markup
/// are a few bytes (a name, a value, the indentation), where a call to `memchr` costs more
/// than the loop.
const SHORT: usize = 32;

/// Where the first `a` in `hay` is.
#[inline]
fn find(a: u8, hay: &[u8]) -> Option<usize> {
    let head = &hay[..hay.len().min(SHORT)];
    match head.iter().position(|&b| b == a) {
        Some(k) => Some(k),
        None if hay.len() > SHORT => memchr::memchr(a, &hay[SHORT..]).map(|k| SHORT + k),
        None => None,
    }
}

/// Where the first `a`, `b` or `c` in `hay` is.
#[inline]
fn find3(a: u8, b: u8, c: u8, hay: &[u8]) -> Option<usize> {
    let head = &hay[..hay.len().min(SHORT)];
    match head.iter().position(|&x| x == a || x == b || x == c) {
        Some(k) => Some(k),
        None if hay.len() > SHORT => memchr::memchr3(a, b, c, &hay[SHORT..]).map(|k| SHORT + k),
        None => None,
    }
}

impl Builder<'_> {
    /// Indexes the whole source if it is a well-formed UTF-8 document the scanner reads as
    /// quick-xml would. Returns `false` at the first doubt; the builder is then half filled
    /// and must be dropped.
    pub(crate) fn scan(&mut self) -> bool {
        let src = self.src;
        let base = self.base as usize;
        // quick-xml reports a byte that is not UTF-8 as an error; leave those to it.
        if std::str::from_utf8(&src[base..]).is_err() {
            return false;
        }
        // An element left open is an error.
        self.scan_range(base, src.len()) == Some(src.len()) && self.stack.len() == 1
    }

    /// Reads from `from`, which is outside any markup, up to `to`, which is a `<` or the end
    /// of the source. Returns where the reading ended: `to`, or later when markup that began
    /// before `to` runs past it. `None` at the first doubt.
    pub(crate) fn scan_range(&mut self, from: usize, to: usize) -> Option<usize> {
        let src = self.src;
        let len = src.len();
        let mut at = from;
        while at < to {
            let lt = find(b'<', &src[at..]).map_or(len, |k| at + k);
            if lt > at && !self.scan_text(at, lt) {
                return None;
            }
            if lt >= to {
                return Some(lt);
            }
            at = self.scan_markup(lt)?;
        }
        Some(at)
    }

    /// The text between two pieces of markup. quick-xml splits it around entity references
    /// and the builder drops a piece that is only whitespace before the node starts, so the
    /// node starts at the first byte that is not whitespace when that is an `&`, and at the
    /// start of the text otherwise. Text that is all whitespace makes no node.
    fn scan_text(&mut self, start: usize, end: usize) -> bool {
        let text = &self.src[start..end];
        let Some(first) = text.iter().position(|b| !b.is_ascii_whitespace()) else {
            return true;
        };
        // Every `&` starts a reference that ends with `;` before the markup.
        let mut from = 0;
        while let Some(amp) = find(b'&', &text[from..]) {
            let amp = from + amp;
            match find(b';', &text[amp..]) {
                Some(semi) if semi > 1 => from = amp + semi + 1,
                _ => return false,
            }
        }
        let node_start = if text[first] == b'&' {
            start + first
        } else {
            start
        };
        self.add(NodeKind::Text, node_start as u64, end as u64, 0);
        true
    }

    /// The markup starting with the `<` at `lt`; returns where it ends.
    fn scan_markup(&mut self, lt: usize) -> Option<usize> {
        let src = self.src;
        let rest = &src[lt..];
        let end = match *rest.get(1)? {
            b'/' => return self.scan_end_tag(lt),
            b'?' => {
                let end = lt + memmem::find(rest, b"?>")? + 2;
                let target = &rest[2..];
                let target_len = target
                    .iter()
                    .position(|&b| b.is_ascii_whitespace() || b == b'?')?;
                let target = &target[..target_len];
                let kind = if target == b"xml" {
                    NodeKind::Declaration
                } else if target.is_empty() || target.eq_ignore_ascii_case(b"xml") {
                    return None;
                } else {
                    NodeKind::ProcessingInstruction
                };
                self.add(kind, lt as u64, end as u64, 0);
                end
            }
            b'!' if rest.starts_with(b"<!--") => {
                let end = lt + 4 + memmem::find(&rest[4..], b"-->")? + 3;
                self.add(NodeKind::Comment, lt as u64, end as u64, 0);
                end
            }
            b'!' if rest.starts_with(b"<![CDATA[") => {
                let end = lt + 9 + memmem::find(&rest[9..], b"]]>")? + 3;
                self.add(NodeKind::CData, lt as u64, end as u64, 0);
                end
            }
            b'!' if rest.starts_with(b"<!DOCTYPE") => {
                // An internal subset (`[...]`) has its own rules; leave it to quick-xml.
                let gt = memchr2(b'>', b'[', rest)?;
                if rest[gt] != b'>' {
                    return None;
                }
                let end = lt + gt + 1;
                self.add(NodeKind::DocType, lt as u64, end as u64, 0);
                end
            }
            b'!' => return None,
            _ => return self.scan_start_tag(lt),
        };
        Some(end)
    }

    /// A start tag or an empty-element tag at `lt`. A `>` inside a quoted attribute value
    /// does not end it.
    fn scan_start_tag(&mut self, lt: usize) -> Option<usize> {
        let rest = &self.src[lt..];
        let mut from = 1;
        let gt = loop {
            let k = from + find3(b'>', b'"', b'\'', &rest[from..])?;
            match rest[k] {
                b'>' => break k,
                quote => from = k + 1 + find(quote, &rest[k + 1..])? + 1,
            }
        };
        let tag = &rest[1..gt];
        let empty = tag.last() == Some(&b'/');
        let body = if empty { &tag[..tag.len() - 1] } else { tag };
        let name_len = body
            .iter()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(body.len());
        if name_len == 0 || name_len > MAX_NAME_LEN {
            return None;
        }
        let end = lt + gt + 1;
        let id = self.add(NodeKind::Element, lt as u64, end as u64, name_len);
        if !empty {
            self.stack.push(id);
            self.name_starts.push(self.names.len());
            self.names.extend_from_slice(&body[..name_len]);
        }
        Some(end)
    }

    /// An end tag at `lt`; it must close the innermost open element.
    fn scan_end_tag(&mut self, lt: usize) -> Option<usize> {
        let src = self.src;
        let gt = lt + find(b'>', &src[lt..])?;
        let name = src[lt + 2..gt].trim_ascii_end();
        if name.is_empty() || name.iter().any(|b| b.is_ascii_whitespace()) {
            return None;
        }
        let Some(&from) = self.name_starts.last() else {
            // No element open in this chunk: the end tag closes one opened before it (D25).
            let name = lt + 2..lt + 2 + name.len();
            return self.close_outer(name, gt + 1);
        };
        if self.names[from..] != *name {
            return None;
        }
        let open = self.pop();
        self.fold(open, lt as u64);
        self.close(open, gt as u64 + 1);
        Some(gt + 1)
    }

    /// An end tag, at `name` up to `end`, of an element opened before this chunk (D25): it is
    /// recorded for stitching, and the chunk's top-level nodes after it get the parent one
    /// level further out. `None` when the builder is not reading a chunk.
    fn close_outer(&mut self, name: std::ops::Range<usize>, end: usize) -> Option<usize> {
        let outer = self.outer.as_mut()?;
        outer.depths.push(self.max_depth);
        self.max_depth = 0;
        outer.closes.push(parallel::OuterClose {
            name,
            before: self.nodes.len(),
            end: end as u64,
        });
        self.stack[0] = parallel::outer_parent(outer.closes.len())?;
        Some(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The searches agree with `memchr` wherever the byte is: in the bytes looked at one by
    /// one, at the handover and after it, also among non-ASCII bytes.
    #[test]
    fn short_searches_agree_with_memchr() {
        for len in 0..3 * SHORT {
            for at in 0..=len {
                let mut hay: Vec<u8> = "ä ü\r\n".bytes().cycle().take(len).collect();
                if at < len {
                    hay[at] = b'<';
                }
                if at + 3 < len {
                    hay[at + 3] = b'"';
                }
                assert_eq!(find(b'<', &hay), memchr::memchr(b'<', &hay), "{len} {at}");
                assert_eq!(
                    find3(b'>', b'"', b'<', &hay),
                    memchr::memchr3(b'>', b'"', b'<', &hay),
                    "{len} {at}"
                );
                assert_eq!(
                    find3(b'>', b'"', b'\'', &hay),
                    memchr::memchr3(b'>', b'"', b'\'', &hay)
                );
            }
        }
    }
}
