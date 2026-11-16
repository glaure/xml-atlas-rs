//! A scanner over the raw bytes for well-formed UTF-8 documents (`docs/decisions.md`, D14).
//!
//! It finds markup with `memchr` instead of reading an event per token, and builds the same
//! nodes as the quick-xml path through the builder's own steps. At the first thing it is
//! not sure to read as quick-xml does (an error of any kind, a byte that is not UTF-8, a
//! DOCTYPE with an internal subset, an empty name) it gives up, and the caller indexes the
//! document with quick-xml, which reports the error and goes on after it (OPEN-10).

use memchr::{memchr, memchr2, memchr3, memmem};

use crate::{Builder, MAX_NAME_LEN, NodeKind};

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
        let len = src.len();
        let mut at = base;
        while at < len {
            let lt = memchr(b'<', &src[at..]).map_or(len, |k| at + k);
            if lt > at && !self.scan_text(at, lt) {
                return false;
            }
            if lt == len {
                break;
            }
            match self.scan_markup(lt) {
                Some(end) => at = end,
                None => return false,
            }
        }
        // An element left open is an error.
        self.stack.len() == 1
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
        while let Some(amp) = memchr(b'&', &text[from..]) {
            let amp = from + amp;
            match memchr(b';', &text[amp..]) {
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
            let k = from + memchr3(b'>', b'"', b'\'', &rest[from..])?;
            match rest[k] {
                b'>' => break k,
                quote => from = k + 1 + memchr(quote, &rest[k + 1..])? + 1,
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
        let gt = lt + memchr(b'>', &src[lt..])?;
        let name = src[lt + 2..gt].trim_ascii_end();
        if name.is_empty() || name.iter().any(|b| b.is_ascii_whitespace()) {
            return None;
        }
        let from = *self.name_starts.last()?;
        if self.names[from..] != *name {
            return None;
        }
        let open = self.pop();
        self.fold(open, lt as u64);
        self.close(open, gt as u64 + 1);
        Some(gt + 1)
    }
}
