//! Word wrap (`docs/spec/text.md`, TEXT-13): where each line of the source breaks into rows
//! of a given width, and moving the cursor up and down by rows.
//!
//! Widths are display columns, as everywhere in the text view: a tab is
//! [`TAB_WIDTH`](crate::text::TAB_WIDTH), any other character one. A row breaks after a blank
//! or before a `<`, at the last such place that fits; a word wider than the row is cut where
//! the row is full. Blanks at a break stay on the row before, past its edge, so every row of
//! a wrapped line starts with something visible. A short line's rows are worked out when they
//! are asked for; a long line's are kept and follow the edits typed into it, so typing in a
//! minified file costs a few rows, not the line.

use std::time::{Duration, Instant};

use crate::Document;
use crate::text::{self, TAB_WIDTH};

/// Lines longer than this keep their rows between frames (TEXT-5).
pub const KEPT_LINE_BYTES: usize = 4000;

/// How many long lines keep their rows at most.
const KEPT_LINES: usize = 64;

/// A line this long (about 3 ms to break) makes a new width wait until it has settled.
const HEAVY_LINE_BYTES: usize = 4 << 20;

/// How long a width must stay the same before a document with a heavy line is wrapped to it.
pub const WIDTH_SETTLES: Duration = Duration::from_millis(150);

/// A row of the wrapped text: row `row` of line `line`, both from 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowPos {
    pub line: usize,
    pub row: usize,
}

/// The rows of a document's lines at one width. Follows the document: after a change inside
/// one line only that line's rows around it are worked out again.
#[derive(Clone, Debug, Default)]
pub struct Wrap {
    width: usize,
    /// The document revision and line count `long` is for.
    revision: u64,
    lines: usize,
    /// Row starts of long lines, as [`row_starts`] gives them.
    long: Vec<(usize, Vec<usize>)>,
    /// Row starts of the last short line asked for.
    short: Vec<usize>,
    /// The longest line wrapped so far, in bytes; kept when the width changes.
    heaviest: usize,
    /// A width asked for but held back, and since when ([`Wrap::offer_width`]).
    wanted: Option<(usize, Instant)>,
}

impl Wrap {
    /// Rows `width` columns wide, at least one.
    pub fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            ..Self::default()
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Changes the width; every line breaks anew.
    pub fn set_width(&mut self, width: usize) {
        let width = width.max(1);
        if width != self.width {
            let heaviest = self.heaviest;
            *self = Self::new(width);
            self.heaviest = heaviest;
        }
    }

    /// The view is `width` columns wide now (TEXT-13). Normally this is [`Self::set_width`].
    /// With a line of megabytes, breaking it anew for every width a resize passes through
    /// costs more than a frame (a few ms per 4 MiB), so the new width is held back until it
    /// has stayed the same for [`WIDTH_SETTLES`]; meanwhile the rows are the old width's.
    /// Returns how long to wait before asking again, when it was held back.
    pub fn offer_width(&mut self, width: usize, now: Instant) -> Option<Duration> {
        let width = width.max(1);
        if width == self.width || self.heaviest < HEAVY_LINE_BYTES {
            self.wanted = None;
            self.set_width(width);
            return None;
        }
        let since = match self.wanted {
            Some((w, since)) if w == width => since,
            _ => {
                self.wanted = Some((width, now));
                return Some(WIDTH_SETTLES);
            }
        };
        let waited = now.saturating_duration_since(since);
        if waited < WIDTH_SETTLES {
            return Some(WIDTH_SETTLES - waited);
        }
        self.wanted = None;
        self.set_width(width);
        None
    }

    /// Where the rows of line `n` start, in bytes from the line's start (after the byte order
    /// mark on the first line); the first is 0.
    pub fn starts(&mut self, doc: &Document, n: usize) -> &[usize] {
        self.follow(doc);
        let (_, line) = line_bytes(doc, n);
        if line.len() <= KEPT_LINE_BYTES {
            self.short = row_starts(line, self.width);
            return &self.short;
        }
        let i = match self.long.iter().position(|(l, _)| *l == n) {
            Some(i) => i,
            None => {
                if self.long.len() >= KEPT_LINES {
                    self.long.clear();
                }
                let t = std::time::Instant::now();
                self.heaviest = self.heaviest.max(line.len());
                let starts = row_starts(line, self.width);
                log::debug!(
                    "wrapped line {n} of {} bytes into {} rows in {:?}",
                    line.len(),
                    starts.len(),
                    t.elapsed()
                );
                self.long.push((n, starts));
                self.long.len() - 1
            }
        };
        &self.long[i].1
    }

    /// Rows of line `n`.
    pub fn rows(&mut self, doc: &Document, n: usize) -> usize {
        self.starts(doc, n).len()
    }

    /// The row that holds position `at`. A position where a row breaks is on the row it
    /// starts.
    pub fn row_of(&mut self, doc: &Document, at: usize) -> RowPos {
        let line = doc.line_of(at as u64);
        let (start, _) = line_bytes(doc, line);
        let rel = at.saturating_sub(start);
        let starts = self.starts(doc, line);
        RowPos {
            line,
            row: starts.partition_point(|&s| s <= rel) - 1,
        }
    }

    /// The bytes of row `pos` in the source.
    pub fn row_range(&mut self, doc: &Document, pos: RowPos) -> std::ops::Range<usize> {
        let (start, line) = line_bytes(doc, pos.line);
        let len = line.len();
        let starts = self.starts(doc, pos.line);
        let row = pos.row.min(starts.len() - 1);
        let end = starts.get(row + 1).copied().unwrap_or(len);
        start + starts[row]..start + end
    }

    /// The row `rows` rows below `pos` (above when negative), and whether it is that far:
    /// at the first or the last row the move stops short.
    pub fn moved(&mut self, doc: &Document, pos: RowPos, rows: isize) -> (RowPos, bool) {
        let mut pos = pos;
        let mut left = rows.unsigned_abs();
        if rows < 0 {
            while left > 0 {
                if pos.row >= left {
                    pos.row -= left;
                    return (pos, true);
                }
                left -= pos.row;
                if pos.line == 0 {
                    pos.row = 0;
                    return (pos, false);
                }
                pos.line -= 1;
                pos.row = self.rows(doc, pos.line) - 1;
                left -= 1;
            }
        } else {
            let last_line = doc.line_count() - 1;
            while left > 0 {
                let rows_here = self.rows(doc, pos.line);
                if pos.row + left < rows_here {
                    pos.row += left;
                    return (pos, true);
                }
                if pos.line == last_line {
                    pos.row = rows_here - 1;
                    return (pos, false);
                }
                left -= rows_here - pos.row;
                pos.line += 1;
                pos.row = 0;
            }
        }
        (pos, true)
    }

    /// The display column of `at` in its row.
    pub fn column(&mut self, doc: &Document, at: usize) -> usize {
        let pos = self.row_of(doc, at);
        let r = self.row_range(doc, pos);
        columns(&doc.src[r.start..at.max(r.start)])
    }

    /// The position in row `pos` closest to display column `col`. On a row that is not its
    /// line's last, at most before the row's last character, so the cursor stays on the row.
    pub fn at_column(&mut self, doc: &Document, pos: RowPos, col: usize) -> usize {
        let r = self.row_range(doc, pos);
        let last_row = pos.row + 1 >= self.rows(doc, pos.line);
        let src = &doc.src;
        let mut at = r.start;
        let mut here = 0;
        while at < r.end {
            let w = if src[at] == b'\t' { TAB_WIDTH } else { 1 };
            if here + w > col {
                if col - here > w / 2 {
                    at = text::next_char(src, at);
                }
                break;
            }
            here += w;
            at = text::next_char(src, at);
        }
        if !last_row && at >= r.end && r.end > r.start {
            at = text::prev_char(src, r.end);
        }
        at.min(r.end)
    }

    /// The position `rows` rows below `at` (above when negative), at display column `col` of
    /// its row: Up, Down, Page Up and Page Down with word wrap. Above the first row is the
    /// document's start; below the last, its end.
    pub fn vertical(&mut self, doc: &Document, at: usize, rows: isize, col: usize) -> usize {
        let pos = self.row_of(doc, at);
        match self.moved(doc, pos, rows) {
            (to, true) => self.at_column(doc, to, col),
            _ if rows < 0 => text::text_start(&doc.src),
            _ => doc.src.len(),
        }
    }

    /// Brings the kept rows up to the document.
    fn follow(&mut self, doc: &Document) {
        if self.revision == doc.revision() && self.lines == doc.line_count() {
            return;
        }
        let change = doc.last_change().filter(|c| {
            !self.long.is_empty()
                && self.revision + 1 == doc.revision()
                && self.lines == doc.line_count()
                && !doc.src[c.at..c.at + c.inserted].contains(&b'\n')
        });
        match change {
            Some(c) => {
                let n = doc.line_of(c.at as u64);
                let (start, line) = line_bytes(doc, n);
                let width = self.width;
                self.long.retain_mut(|(l, starts)| {
                    if *l != n {
                        return true;
                    }
                    if line.len() <= KEPT_LINE_BYTES || c.at < start {
                        return false;
                    }
                    follow_edit(starts, line, width, c.at - start, c.removed, c.inserted);
                    true
                });
            }
            None => self.long.clear(),
        }
        self.revision = doc.revision();
        self.lines = doc.line_count();
    }
}

/// Where the rows of `line` (without its line ending) start when it wraps at `width`
/// columns; the first is 0.
pub fn row_starts(line: &[u8], width: usize) -> Vec<usize> {
    let mut starts = vec![0];
    let mut at = 0;
    // One check of the whole line saves one for each of its rows.
    let utf8 = std::str::from_utf8(line).is_ok();
    loop {
        at = next_start(line, at, width, utf8);
        if at >= line.len() {
            return starts;
        }
        starts.push(at);
    }
}

/// The start of line `n` in the source and its bytes, without the line ending and, on the
/// first line, without the byte order mark.
fn line_bytes(doc: &Document, n: usize) -> (usize, &[u8]) {
    let start = (doc.line_starts[n] as usize).max(text::text_start(&doc.src));
    let end = text::line_end_of(doc, n).max(start);
    (start, &doc.src[start..end])
}

/// Where the row after the one starting at `from` starts; the line's length when the row
/// takes the rest of it.
///
/// A row of well-formed text without tabs, nearly all of them, is measured and searched as a
/// whole; the rest goes character by character ([`next_start_scan`]), with the same answer.
/// This is what a new width of the view costs for a minified file (TEXT-13): every row of a
/// line of megabytes is worked out again.
fn next_start(line: &[u8], from: usize, width: usize, utf8: bool) -> usize {
    let width = width.max(1);
    match next_start_fast(line, from, width, utf8) {
        Some(at) => at,
        None => next_start_scan(line, from, width),
    }
}

/// [`next_start`] for a row that is valid UTF-8 and has no tab, else `None`. `utf8`: the whole
/// line is valid UTF-8, so a row needs no check of its own.
fn next_start_fast(line: &[u8], from: usize, width: usize, utf8: bool) -> Option<usize> {
    // The bytes holding `width` characters: as many bytes to begin with, and more while
    // characters are wider than a byte.
    let mut end = from.checked_add(width)?.min(line.len());
    let (cols, window) = loop {
        let window = &line[from..end];
        // A character cut by the end of the window is taken whole.
        let valid = if utf8 {
            let mut whole = end;
            while whole < line.len() && line[whole] & 0xC0 == 0x80 {
                whole += 1;
            }
            whole
        } else {
            match std::str::from_utf8(window) {
                Ok(_) => end,
                Err(e) if e.error_len().is_none() => {
                    let lead = window[e.valid_up_to()];
                    let len = match lead {
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let whole = from + e.valid_up_to() + len;
                    if whole > line.len() {
                        return None;
                    }
                    whole
                }
                Err(_) => return None,
            }
        };
        if valid != end {
            end = valid;
            continue;
        }
        let cols = window.iter().filter(|&&b| b & 0xC0 != 0x80).count();
        if cols >= width || end == line.len() {
            break (cols, window);
        }
        end = (end + (width - cols)).min(line.len());
    };
    if memchr::memchr(b'\t', window).is_some() {
        return None;
    }
    if cols < width || end == line.len() && cols <= width {
        // Everything left fits.
        return Some(line.len());
    }
    // The character at `end` does not fit. A blank there hangs past the edge, and the next
    // row starts after the blanks; else the row breaks after the last blank or before the
    // last `<` that is not its first character, or here.
    if matches!(line[end], b' ' | b'\t') {
        let blanks = line[end..]
            .iter()
            .take_while(|&&b| matches!(b, b' ' | b'\t'))
            .count();
        return Some(end + blanks);
    }
    let brk = memchr::memrchr2(b'<', b' ', window).and_then(|i| match window[i] {
        b' ' => Some(from + i + 1),
        _ if i > 0 => Some(from + i),
        _ => None,
    });
    Some(brk.unwrap_or(end))
}

/// [`next_start`] character by character, for any bytes.
fn next_start_scan(line: &[u8], from: usize, width: usize) -> usize {
    let (mut at, mut cols) = (from, 0);
    // The last place after `from` where the row may break.
    let mut brk = None;
    while at < line.len() {
        let b = line[at];
        let blank = b == b' ' || b == b'\t';
        let w = if b == b'\t' { TAB_WIDTH } else { 1 };
        if cols + w > width && at > from {
            if blank {
                // Blanks hang past the edge; the next row starts after them.
                while at < line.len() && matches!(line[at], b' ' | b'\t') {
                    at += 1;
                }
                return at;
            }
            return brk.unwrap_or(at);
        }
        if b == b'<' && at > from {
            brk = Some(at);
        }
        cols += w;
        // ASCII is most of markup; only other bytes need decoding.
        at = if b < 0x80 {
            at + 1
        } else {
            text::next_char(line, at)
        };
        if blank {
            brk = Some(at);
        }
    }
    line.len()
}

/// Follows a change inside a long line: `removed` bytes at byte `at` replaced by `inserted`
/// ones, `line` being the line after it. Rows are worked out again from shortly before the
/// change until a row starts where one started before; the rows after it only move.
fn follow_edit(
    starts: &mut Vec<usize>,
    line: &[u8],
    width: usize,
    at: usize,
    removed: usize,
    inserted: usize,
) {
    let delta = inserted as isize - removed as isize;
    // The row of the change, and two before it: the row before can take words back from
    // it, and the one before that is a margin.
    let k = starts.partition_point(|&s| s <= at) - 1;
    let j = k.saturating_sub(2);
    let mut fresh = Vec::new();
    let mut s = starts[j];
    loop {
        s = next_start(line, s, width, false);
        if s >= line.len() {
            starts.truncate(j + 1);
            starts.extend(fresh);
            return;
        }
        if s >= at + inserted
            && let Some(old) = s.checked_add_signed(-delta)
            && let Ok(m) = starts[k..].binary_search(&old)
        {
            let m = k + m;
            let kept = fresh.len();
            starts.splice(j + 1..m, fresh);
            for x in &mut starts[j + 1 + kept..] {
                *x = x.wrapping_add_signed(delta);
            }
            return;
        }
        fresh.push(s);
    }
}

/// Display columns of `bytes`, which start and end on character boundaries.
fn columns(bytes: &[u8]) -> usize {
    let mut at = 0;
    let mut cols = 0;
    while at < bytes.len() {
        cols += if bytes[at] == b'\t' { TAB_WIDTH } else { 1 };
        at = text::next_char(bytes, at);
    }
    cols
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Edit;

    /// The rows of `line` at `width`, as text.
    fn rows(line: &str, width: usize) -> Vec<&str> {
        let starts = row_starts(line.as_bytes(), width);
        let mut out = Vec::new();
        for (i, &s) in starts.iter().enumerate() {
            let e = starts.get(i + 1).copied().unwrap_or(line.len());
            out.push(&line[s..e]);
        }
        out
    }

    /// The rows measured as a whole give the answer of going character by character, on
    /// lines with blanks, tags, tabs, two to four byte characters, and bytes that are not
    /// UTF-8 (a Latin-1 `ä`, a cut character), at every width from 1 to 40.
    #[test]
    fn the_fast_rows_are_the_scanned_rows() {
        const PIECES: &[&[u8]] = &[
            b" ",
            b" ",
            b"<",
            b"<a>",
            b"</a>",
            b"ab",
            b"x",
            b"\t",
            "ä".as_bytes(),
            "日".as_bytes(),
            "😀".as_bytes(),
            b"\xE4",
            b"\x80",
            b"\xF0\x9F",
            b"  ",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for case in 0..3000 {
            // Mostly clean text, so that the fast path is what is tried.
            let dirty = case % 3 == 0;
            let mut line = Vec::new();
            for _ in 0..next(60) {
                let p = PIECES[next(PIECES.len())];
                let bad = p == b"\t" || p == b"\xE4" || p == b"\x80" || p == b"\xF0\x9F";
                if dirty || !bad {
                    line.extend_from_slice(p);
                }
            }
            for width in 1..=40 {
                let mut from = 0;
                while from < line.len() {
                    let utf8 = std::str::from_utf8(&line).is_ok();
                    let fast = next_start(&line, from, width, utf8);
                    let scan = next_start_scan(&line, from, width);
                    if !dirty {
                        assert!(
                            next_start_fast(&line, from, width, utf8).is_some(),
                            "valid text without tabs takes the fast path"
                        );
                    }
                    assert_eq!(
                        fast,
                        scan,
                        "from {from}, width {width}, line {:?}",
                        String::from_utf8_lossy(&line)
                    );
                    from = scan;
                }
            }
        }
    }

    /// A width that is still changing is held back for a document with a heavy line, and
    /// taken at once for any other.
    #[test]
    fn a_heavy_line_waits_for_the_width_to_settle() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let line = "<a>x</a> ".repeat(HEAVY_LINE_BYTES / 9 + 1);
        let doc = Document::from_bytes(line.into_bytes());
        let mut w = Wrap::new(60);
        // Nothing heavy has been wrapped yet: the width is taken at once.
        assert_eq!(w.offer_width(70, at(0)), None);
        assert_eq!(w.width(), 70);
        assert!(w.rows(&doc, 0) > 1000);
        // Now it is heavy: a new width waits, and a changing one starts the wait again.
        assert_eq!(w.offer_width(80, at(10)), Some(WIDTH_SETTLES));
        assert_eq!(w.width(), 70);
        assert_eq!(w.offer_width(90, at(60)), Some(WIDTH_SETTLES));
        assert_eq!(
            w.offer_width(90, at(100)),
            Some(WIDTH_SETTLES - Duration::from_millis(40))
        );
        assert_eq!(w.width(), 70);
        // Settled: taken, and the rows are the new width's.
        assert_eq!(w.offer_width(90, at(60 + 150)), None);
        assert_eq!(w.width(), 90);
        // Back to the width it has: nothing is waiting any more.
        assert_eq!(w.offer_width(90, at(400)), None);
        // A small document takes any width at once.
        let mut small = Wrap::new(60);
        assert_eq!(small.offer_width(61, at(0)), None);
        assert_eq!(small.width(), 61);
    }

    #[test]
    fn breaks_after_blanks_and_before_tags() {
        assert_eq!(
            rows("<a x=\"1\" y=\"2\"/>", 10),
            ["<a x=\"1\" ", "y=\"2\"/>"]
        );
        assert_eq!(rows("<a><b/><c/></a>", 8), ["<a><b/>", "<c/></a>"]);
        // A short line is one row; so is an empty one.
        assert_eq!(rows("<a/>", 80), ["<a/>"]);
        assert_eq!(rows("", 80), [""]);
        // A word wider than the row is cut where the row is full.
        assert_eq!(rows("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        // Blanks at a break hang past the edge.
        assert_eq!(rows("abcd    efg", 4), ["abcd    ", "efg"]);
        assert_eq!(rows("ab cd   ", 5), ["ab cd   "]);
    }

    #[test]
    fn counts_characters_and_tabs() {
        // `ü` and `日` are one column each, a tab four; nothing is cut inside a character.
        assert_eq!(rows("Grüße Welt", 6), ["Grüße ", "Welt"]);
        assert_eq!(rows("日本語の文字", 4), ["日本語の", "文字"]);
        assert_eq!(rows("\t\tab", 10), ["\t\tab"]);
        assert_eq!(rows("\t\tab", 9), ["\t\t", "ab"]);
        assert_eq!(rows("\tx\ty", 5), ["\tx\t", "y"]);
        // A character wider than the row still gets one.
        assert_eq!(rows("x\ty", 2), ["x\t", "y"]);
        assert_eq!(rows("\tx", 2), ["\t", "x"]);
        // A byte that is not UTF-8 is one column.
        assert_eq!(row_starts(b"ab\xE4cd", 3), [0, 3]);
    }

    /// Typing anywhere in a long line, its kept rows are those it would get anew.
    #[test]
    fn kept_rows_follow_edits() {
        let mut line: Vec<u8> = Vec::new();
        for i in 0..400 {
            line.extend(format!("<item id=\"{i}\" name=\"Grüße {i}\">x</item>").bytes());
        }
        let width = 37;
        let mut starts = row_starts(&line, width);
        // A small generator, so the test needs no crate.
        let mut seed = 12345u64;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n
        };
        let inserts: [&[u8]; 5] = [b" ", b"<", b"abc", "ü".as_bytes(), b"   x  "];
        for round in 0..400 {
            let mut at = next(line.len());
            while at < line.len() && line[at] & 0xC0 == 0x80 {
                at += 1;
            }
            let (removed, inserted) = if round % 3 == 0 {
                let mut end = (at + next(8)).min(line.len());
                while end < line.len() && line[end] & 0xC0 == 0x80 {
                    end += 1;
                }
                line.drain(at..end);
                (end - at, 0)
            } else {
                let ins = inserts[next(inserts.len())];
                line.splice(at..at, ins.iter().copied());
                (0, ins.len())
            };
            follow_edit(&mut starts, &line, width, at, removed, inserted);
            assert_eq!(starts, row_starts(&line, width), "round {round}");
        }
    }

    /// `<r>` on line 0 after a byte order mark, then a line of 12 columns: `ab cd ef gh`.
    fn doc() -> Document {
        Document::from_bytes(b"\xEF\xBB\xBF<r>\r\nab cd ef gh\r\n</r>".to_vec())
    }

    #[test]
    fn up_and_down_go_by_rows() {
        let d = doc();
        let mut w = Wrap::new(3);
        // Line 1 wraps as `ab `, `cd `, `ef `, `gh`.
        assert_eq!(w.rows(&d, 1), 4);
        let line1 = d.line_starts[1] as usize;
        let at = |s: &str| line1 + "ab cd ef gh".find(s).unwrap();
        assert_eq!(w.row_of(&d, at("cd")), RowPos { line: 1, row: 1 });
        assert_eq!(w.column(&d, at("d")), 1);
        // Down from `d` goes to `f`, then `h`, then to column 1 of `</r>`.
        assert_eq!(w.vertical(&d, at("d"), 1, 1), at("f"));
        assert_eq!(w.vertical(&d, at("f"), 1, 1), at("h"));
        assert_eq!(w.vertical(&d, at("h"), 1, 1), d.line_starts[2] as usize + 1);
        // Up from `c` past the end of `<r>` stops before the line ending.
        assert_eq!(w.vertical(&d, at("ab"), -1, 9), 6);
        // A goal past a row's end stays on that row: before its last character.
        assert_eq!(w.vertical(&d, at("ab"), 1, 9), at("d") + 1);
        // Past either end: the document's start or end.
        assert_eq!(w.vertical(&d, at("ab"), -5, 0), 3);
        assert_eq!(w.vertical(&d, at("ab"), 50, 0), d.src.len());
        // Moving stops at the ends and says so.
        let first = RowPos::default();
        assert_eq!(w.moved(&d, first, -1), (first, false));
        assert_eq!(w.moved(&d, first, 5), (RowPos { line: 2, row: 0 }, true));
        assert_eq!(
            w.moved(&d, RowPos { line: 2, row: 0 }, -2),
            (RowPos { line: 1, row: 2 }, true)
        );
    }

    #[test]
    fn kept_rows_follow_the_document() {
        let long = "<a x=\"1\"/> ".repeat(500);
        let mut d = Document::from_bytes(format!("<r>\n{long}\n</r>").into_bytes());
        let mut w = Wrap::new(40);
        w.rows(&d, 1);
        // Into the blank after the 273rd element.
        let at = d.line_starts[1] as usize + 11 * 272 + 10;
        d.apply(
            Edit::new(crate::StepName::Other("Type".into()))
                .replace(at..at, b"\xC3\xBC  <b>".to_vec()),
        )
        .unwrap();
        let (_, line) = line_bytes(&d, 1);
        let expect = row_starts(line, 40);
        assert_eq!(w.starts(&d, 1), expect);
        // A line break joins two lines: all is worked out anew.
        let nl = d.line_starts[2] as usize - 1;
        d.apply(Edit::new(crate::StepName::Delete).replace(nl..nl + 1, Vec::new()))
            .unwrap();
        let (_, line) = line_bytes(&d, 1);
        let expect = row_starts(line, 40);
        assert_eq!(w.starts(&d, 1), expect);
        // A new width breaks every line anew.
        w.set_width(25);
        let expect = row_starts(line, 25);
        assert_eq!(w.starts(&d, 1), expect);
    }
}
