//! Indexing on several threads (`docs/decisions.md`, D25).
//!
//! The source is cut into chunks at start tags. Each chunk is read by the scanner of D14 on
//! a thread of its own, into nodes of its own: its top-level nodes get a stand-in parent, and
//! the end tags of elements opened in earlier chunks are recorded. One thread then stitches
//! the chunks in order, matching those end tags against the elements still open, and the
//! nodes are copied into one array, again in parallel. The result is the index the scanner
//! builds on one thread. At the first doubt the caller reads the document on one thread, as
//! before: the scanner, then quick-xml.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use memchr::memchr;

use crate::{Builder, Folded, Index, NONE, Node, NodeId, NodeKind, ROOT, UTF8_BOM, WideChildren};

/// Sources smaller than this are read on one thread.
pub(crate) const PARALLEL_MIN: usize = 4 << 20;

/// Chunks per thread: more chunks than threads, so that a slow core does not hold up the
/// others.
const CHUNKS_PER_THREAD: usize = 4;

/// Most threads used.
const MAX_THREADS: usize = 8;

/// The stand-in parent of a chunk's top-level nodes: after `k` end tags of elements opened
/// before the chunk, `OUTER - k`. Real ids stay below [`OUTER_MIN`].
const OUTER: NodeId = NONE - 1;
const MAX_OUTER: usize = 1 << 24;
const OUTER_MIN: NodeId = OUTER - MAX_OUTER as NodeId;

const FILLER: Node = Node {
    start: 0,
    end: 0,
    parent: NONE,
    descendants: 0,
    head_len: 0,
    name_len: 0,
    kind: NodeKind::Document,
    folded: Folded::No,
};

/// What a chunk records about the elements opened before it.
#[derive(Debug, Default)]
pub(crate) struct Outer {
    /// Their end tags, in order.
    pub closes: Vec<OuterClose>,
    /// The deepest stack, in the chunk's own count, in each stretch before, between and after
    /// those end tags; 0 for a stretch without elements.
    pub depths: Vec<u16>,
}

/// The end tag of an element opened before the chunk.
#[derive(Debug)]
pub(crate) struct OuterClose {
    /// The name in the end tag.
    pub name: Range<usize>,
    /// The chunk's nodes before it.
    pub before: usize,
    /// Where the end tag ends.
    pub end: u64,
}

/// One chunk, read.
struct Chunk {
    /// Where the reading started and ended.
    from: usize,
    end: usize,
    /// Its nodes; parents are ids in `nodes`, or stand-ins ([`OUTER`]).
    nodes: Vec<Node>,
    outer: Outer,
    /// Elements still open at its end, ids in `nodes`, outermost first.
    open: Vec<NodeId>,
    elements: u32,
}

/// A chunk's number, whether its bytes are UTF-8, and the chunk, unless reading it met a
/// doubt.
type ChunkRead = (usize, bool, Option<Chunk>);

/// A chunk placed in the index: its first id, and the open elements when it began.
struct Part {
    chunk: Chunk,
    first: usize,
    entry: Vec<NodeId>,
}

/// The stand-in parent after `closed` end tags of elements opened before the chunk.
pub(crate) fn outer_parent(closed: usize) -> Option<NodeId> {
    (closed < MAX_OUTER).then(|| OUTER - closed as NodeId)
}

/// The index of `src` built on several threads, or `None`: too small, one core, or a doubt.
pub(crate) fn index(src: &[u8]) -> Option<Index> {
    if src.len() < PARALLEL_MIN {
        return None;
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_THREADS);
    if threads < 2 {
        return None;
    }
    index_chunked(src, src.len() / (threads * CHUNKS_PER_THREAD), threads)
}

/// The index of `src` from chunks of about `chunk_len` bytes on `threads` threads.
pub(crate) fn index_chunked(src: &[u8], chunk_len: usize, threads: usize) -> Option<Index> {
    let base = if src.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    };
    let bounds = boundaries(src, base, chunk_len.max(1));
    let count = bounds.len() - 1;
    let next = AtomicUsize::new(0);
    let (mut read, nodes): (Vec<ChunkRead>, Vec<Node>) = std::thread::scope(|s| {
        // The final array is made while the chunks are read: writing it is memory traffic
        // that would otherwise come after them. Its size is the builder's guess.
        let filler = s.spawn(|| vec![FILLER; src.len() / 40 + 1]);
        let workers: Vec<_> = (0..threads.clamp(1, count))
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= count {
                            break done;
                        }
                        let (from, to) = (bounds[i], bounds[i + 1]);
                        // Cuts are at an ASCII `<`, so each chunk is checked on its own.
                        let utf8 = std::str::from_utf8(&src[from..to]).is_ok();
                        let chunk = utf8.then(|| scan_chunk(src, from, to)).flatten();
                        done.push((i, utf8, chunk));
                    }
                })
            })
            .collect();
        let read = workers
            .into_iter()
            .flat_map(|w| w.join().expect("reading a chunk"))
            .collect();
        (read, filler.join().expect("making the node array"))
    });
    if read.iter().any(|&(_, utf8, _)| !utf8) {
        return None;
    }
    read.sort_by_key(|r| r.0);
    let chunks = read.into_iter().map(|(_, _, chunk)| chunk).collect();
    stitch(src, &bounds, chunks, nodes, threads)
}

/// Where the chunks begin, from about every `chunk_len` bytes, and the end of `src`. Each
/// cut is a start tag right after the end of a tag, whitespace between: inside a comment, a
/// CDATA section or a quoted value that is rare, and stitching notices it anyway.
fn boundaries(src: &[u8], base: usize, chunk_len: usize) -> Vec<usize> {
    let mut bounds = vec![base];
    let mut at = base.saturating_add(chunk_len);
    while at < src.len() {
        let Some(cut) = start_tag_from(src, at) else {
            break;
        };
        bounds.push(cut);
        at = cut.saturating_add(chunk_len);
    }
    bounds.push(src.len());
    bounds
}

/// The first `<` at or after `from` that starts a start tag right after another tag.
fn start_tag_from(src: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    loop {
        let lt = at + memchr(b'<', &src[at..])?;
        let next = *src.get(lt + 1)?;
        let named = next.is_ascii_alphabetic() || next == b'_' || next == b':' || next >= 0x80;
        let before = src[..lt].iter().rev().find(|b| !b.is_ascii_whitespace());
        if named && before == Some(&b'>') {
            return Some(lt);
        }
        at = lt + 1;
    }
}

/// Reads the chunk from `from` to `to`; `None` at a doubt.
fn scan_chunk(src: &[u8], from: usize, to: usize) -> Option<Chunk> {
    let mut b = Builder::chunk(src, to.saturating_sub(from));
    let end = b.scan_range(from, to)?;
    let mut outer = b.outer.take().unwrap_or_default();
    outer.depths.push(b.max_depth);
    Some(Chunk {
        from,
        end,
        open: b.stack[1..].to_vec(),
        nodes: b.nodes,
        outer,
        elements: b.element_count,
    })
}

/// Puts the chunks together in order and makes the index; `None` when they do not fit:
/// an end tag that does not match, an element left open, a chunk that cannot be read.
fn stitch(
    src: &[u8],
    bounds: &[usize],
    chunks: Vec<Option<Chunk>>,
    mut nodes: Vec<Node>,
    threads: usize,
) -> Option<Index> {
    // The open elements and their names, the document at the bottom.
    let mut stack: Vec<NodeId> = vec![ROOT];
    // The document has no name.
    let mut names: Vec<Range<usize>> = vec![Range::default()];
    let mut parts: Vec<Part> = Vec::with_capacity(chunks.len());
    // The ends and subtree sizes of elements closed in a later chunk than their start.
    let mut fixes: Vec<(NodeId, u64, u32)> = Vec::new();
    let mut total = 1;
    let mut pos = bounds[0];
    let mut elements = 0u32;
    let mut max_depth = 0usize;
    for (i, chunk) in chunks.into_iter().enumerate() {
        let to = bounds[i + 1];
        if pos >= to {
            // The chunk before read past this one.
            continue;
        }
        let chunk = match chunk {
            Some(c) if c.from == pos => c,
            // A doubt in a chunk that began where it should is a doubt in the document.
            None if bounds[i] == pos => return None,
            // The cut fell inside markup: read again from where the chunk before ended.
            _ => scan_chunk(src, pos, to)?,
        };
        let first = total;
        let entry = stack.clone();
        for close in &chunk.outer.closes {
            if stack.len() < 2 {
                return None;
            }
            let top = stack.pop()?;
            let name = names.pop()?;
            if src[name] != src[close.name.clone()] {
                return None;
            }
            let before = first + close.before;
            if before == top as usize + 2
                && node(&parts, &chunk, first, top as usize + 1).kind != NodeKind::Element
            {
                // Its content may be one text that the one-thread reading folds (D11).
                return None;
            }
            fixes.push((top, close.end, (before - 1 - top as usize) as u32));
        }
        for (k, &depth) in chunk.outer.depths.iter().enumerate() {
            if depth > 0 {
                max_depth = max_depth.max(entry.len() - k + depth as usize - 1);
            }
        }
        for &local in &chunk.open {
            stack.push((first + local as usize) as NodeId);
            names.push(chunk.nodes[local as usize].name_span());
        }
        elements = elements.checked_add(chunk.elements)?;
        total += chunk.nodes.len();
        if total >= OUTER_MIN as usize {
            return None;
        }
        pos = chunk.end;
        parts.push(Part {
            chunk,
            first,
            entry,
        });
    }
    if stack.len() != 1 || pos != src.len() {
        return None;
    }
    let max_depth = u16::try_from(max_depth).ok()?;

    nodes.resize(total, FILLER);
    nodes.shrink_to_fit();
    nodes[0] = Node {
        start: 0,
        end: src.len() as u64,
        parent: NONE,
        descendants: (total - 1) as u32,
        head_len: 0,
        name_len: 0,
        kind: NodeKind::Document,
        folded: Folded::No,
    };
    let mut rest = &mut nodes[1..];
    let workers = threads.max(1);
    let mut work: Vec<Vec<(&mut [Node], &Part)>> = (0..workers).map(|_| Vec::new()).collect();
    for (i, part) in parts.iter().enumerate() {
        let (head, tail) = rest.split_at_mut(part.chunk.nodes.len());
        work[i % workers].push((head, part));
        rest = tail;
    }
    std::thread::scope(|s| {
        for jobs in work {
            s.spawn(move || {
                for (dst, part) in jobs {
                    place(dst, part);
                }
            });
        }
    });
    for (id, end, descendants) in fixes {
        let n = &mut nodes[id as usize];
        n.end = end;
        n.descendants = descendants;
    }
    Some(Index {
        nodes,
        error: None,
        more_errors: Vec::new(),
        element_count: elements,
        max_depth,
        last_splice: None,
        wide: WideChildren::default(),
    })
}

/// Copies a chunk's nodes to their place, with ids of the whole index.
fn place(dst: &mut [Node], part: &Part) {
    let first = part.first as NodeId;
    let top = part.entry.len() - 1;
    for (d, n) in dst.iter_mut().zip(&part.chunk.nodes) {
        let mut n = *n;
        n.parent = if n.parent >= OUTER_MIN {
            part.entry[top - (OUTER - n.parent) as usize]
        } else {
            n.parent + first
        };
        *d = n;
    }
}

/// Node `id` of the whole index, from the chunks placed so far and `current`, which begins
/// at `first`.
fn node<'c>(parts: &'c [Part], current: &'c Chunk, first: usize, id: usize) -> &'c Node {
    if id >= first {
        return &current.nodes[id - first];
    }
    let p = parts.partition_point(|p| p.first <= id) - 1;
    &parts[p].chunk.nodes[id - parts[p].first]
}

impl<'s> Builder<'s> {
    /// A builder for one chunk (D25): no document node, and a stand-in parent at the bottom
    /// of the stack.
    fn chunk(src: &'s [u8], len: usize) -> Self {
        Self {
            src,
            base: 0,
            nodes: Vec::with_capacity(len / 40 + 1),
            stack: vec![OUTER],
            names: Vec::new(),
            name_starts: Vec::new(),
            element_count: 0,
            max_depth: 0,
            outer: Some(Outer::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one-thread scanner's index of `src` (D14), or `None` where it is unsure.
    fn one_thread(src: &[u8]) -> Option<Index> {
        let mut b = Builder::new(src);
        b.scan().then(|| b.finish(Vec::new()))
    }

    /// Reads `src` in chunks of many sizes, also of one byte, so that cuts fall everywhere:
    /// each reading is the one-thread index or refused, and refused where that is. Returns
    /// how many readings were not refused.
    fn check(src: &[u8]) -> usize {
        let show = String::from_utf8_lossy(&src[..src.len().min(120)]).into_owned();
        let want = one_thread(src);
        let mut read = 0;
        for chunk_len in [1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 1000] {
            for threads in [1, 3] {
                let Some(got) = index_chunked(src, chunk_len, threads) else {
                    continue;
                };
                let want = want
                    .as_ref()
                    .unwrap_or_else(|| panic!("read in chunks, not on one thread: {show}"));
                assert_eq!(got.nodes, want.nodes, "chunks of {chunk_len}: {show}");
                assert_eq!(got.element_count, want.element_count, "{show}");
                assert_eq!(got.max_depth, want.max_depth, "{show}");
                assert!(got.error.is_none());
                read += 1;
            }
        }
        read
    }

    #[test]
    fn small_documents_read_in_chunks_like_on_one_thread() {
        let cases: &[&[u8]] = &[
            b"<r/>",
            b"\xEF\xBB\xBF<r>\r\n  <a x=\"1\">t</a>\r\n</r>\r\n",
            b"<?xml version=\"1.0\"?>\n<!-- c -->\n<?pi data?>\n<r/>\n<!-- after -->\n",
            b"<!DOCTYPE r SYSTEM \"r.dtd\">\n<r/>",
            b"<r>  x &amp; y  </r>",
            b"<r>a<b/>c<![CDATA[ d ]]>e</r>",
            b"<r><a x='>' y=\"'>'\">gt</a></r>",
            b"<r>\n  <a>1</a >\n</r >",
            b"<r>caf\xC3\xA9 \xE2\x82\xAC</r>",
            b"text before <r/> text after",
            b"<r>\n<a>\n<b>\n<c>t</c>\n</b>\n</a>\n<a>\n<b/>\n</a>\n</r>",
        ];
        for &src in cases {
            check(src);
        }
        // What the one-thread scanner refuses is refused in chunks too.
        let refused: &[&[u8]] = &[
            b"<r>\n<a>\n</r>",
            b"<r>\n<a>",
            b"<r>\n</a>\n</r>",
            b"<r>\n</r>\n</s>",
            b"<r>\n<a>\xFF</a>\n</r>",
            b"<r>\n<a/>\n<!-- open </r>",
            b"<r>\n<a>x</b>\n</r>",
        ];
        for &src in refused {
            assert!(one_thread(src).is_none());
            assert_eq!(check(src), 0);
        }
    }

    /// Cuts inside comments, CDATA, processing instructions and quoted values look like
    /// start tags after a tag; the chunk before reads past them, and the result is still the
    /// one-thread index, or refused.
    #[test]
    fn cuts_inside_markup_are_noticed() {
        let doc = |extra: &str| {
            let mut src = String::from("\u{feff}<?xml version=\"1.0\"?>\r\n<liste>\r\n");
            for i in 0..40 {
                src.push_str(&format!(
                    concat!(
                        "  <zeile nr=\"{i}\" v=\">\r\n<falsch\">\r\n",
                        "    <!-- a>\r\n<kommentar> -->\r\n",
                        "    <![CDATA[x>\r\n<cdata>]]>\r\n",
                        "    <?pi a>\r\n<pi?>\r\n",
                        "    <größe>Grüße &amp; mehr</größe>\r\n",
                        "    <leer/>\r\n",
                        "{extra}",
                        "    <tief><a><b><c>{i}</c></b></a></tief>\r\n",
                        "  </zeile>\r\n"
                    ),
                    i = i,
                    extra = extra
                ));
            }
            src.push_str("</liste>\r\n");
            src.into_bytes()
        };
        // Every cut that looks like a start tag is one at one-byte chunks; those inside
        // markup are read again from where the chunk before ended.
        let src = doc("");
        let want = one_thread(&src).expect("well-formed");
        let got = index_chunked(&src, 1, 3).expect("read again past the false cuts");
        assert_eq!(got.nodes, want.nodes);
        assert!(check(&src) > 0);
        // A false cut in the start tag of an element that holds only text: read again from
        // after the tag, its text would have to be folded across chunks. That is refused.
        let src = doc("    <text v=\">\r\n<b\">nur Text</text>\r\n");
        assert!(one_thread(&src).is_some());
        assert!(index_chunked(&src, 1, 3).is_none());
        check(&src);
    }

    #[test]
    fn corpus_reads_in_chunks_like_on_one_thread() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../engine/tests/corpus");
        let mut read = 0;
        for entry in std::fs::read_dir(dir).expect("corpus") {
            let src = std::fs::read(entry.expect("entry").path()).expect("read");
            read += check(&src);
        }
        assert!(read > 0, "nothing read in chunks");
    }

    /// A document large enough for [`index`] to use threads is read in chunks, and the
    /// result is the one-thread index.
    #[test]
    fn large_documents_use_threads() {
        let mut src = String::from("<orders>\n");
        let mut i = 0;
        while src.len() < PARALLEL_MIN + 1000 {
            src.push_str(&format!(
                "  <order id=\"{i}\">\n    <name>Kunde {i} &amp; Söhne</name>\n    <items><item qty=\"{i}\"/></items>\n  </order>\n"
            ));
            i += 1;
        }
        src.push_str("</orders>\n");
        let src = src.into_bytes();
        let want = one_thread(&src).expect("well-formed");
        let got = index_chunked(&src, src.len() / 32, 8).expect("read in chunks");
        assert_eq!(got.nodes, want.nodes);
        assert_eq!(got.element_count, want.element_count);
        assert_eq!(got.max_depth, want.max_depth);
        if std::thread::available_parallelism().map_or(1, |n| n.get()) > 1 {
            assert_eq!(crate::index(&src).nodes, want.nodes);
        }
    }
}
