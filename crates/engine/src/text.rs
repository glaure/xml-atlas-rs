//! Editing the source as text: cursor movement and the edits keys make
//! (`docs/spec/text.md`, TEXT-16 to TEXT-21; `docs/decisions.md`, D19).
//!
//! Positions are byte offsets into the source. Every position these functions return is a
//! character boundary: never inside a UTF-8 character, never between the CR and LF of a line
//! ending, never before a byte order mark. A byte that is not valid UTF-8 counts as one
//! character. The text view maps keys to these functions and applies the edits they return.

use std::ops::Range;

use crate::edit::Typing;
use crate::format::FormatOptions;
use crate::{Document, Edit};

/// A tab is shown as this many columns (TEXT-4).
pub const TAB_WIDTH: usize = 4;

/// The UTF-8 byte order mark.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// A selection: from `anchor`, where it started, to `head`, where the cursor is. Empty when
/// they are equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

/// An edit a key makes, and where the cursor is after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub edit: Edit,
    pub selection: Selection,
}

/// Kinds of characters that words are made of, for moving and deleting by word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Other,
}

impl Selection {
    /// A cursor at `at`, with nothing selected.
    pub fn cursor(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    /// The selected bytes, from the lower end to the higher.
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Moves the cursor to `at`: with `extend` (Shift held) the anchor stays, otherwise the
    /// selection collapses there.
    pub fn moved(self, at: usize, extend: bool) -> Self {
        if extend {
            Self {
                anchor: self.anchor,
                head: at,
            }
        } else {
            Self::cursor(at)
        }
    }
}

/// Letters, digits and `_` make up words.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The nearest character boundary at or before `at` (TEXT-21): for a position that another
/// edit left inside a character or a CRLF.
pub fn snap(src: &[u8], at: usize) -> usize {
    let start = text_start(src);
    let at = at.min(src.len());
    if at <= start {
        return start;
    }
    if src[at - 1] == b'\r' && src.get(at) == Some(&b'\n') {
        return at - 1;
    }
    (1..=3)
        .filter(|&k| at >= start + k)
        .find(|&k| char_len(src, at - k) > k)
        .map_or(at, |k| at - k)
}

/// The word at `at`, for a double-click: a run of word characters, of spaces, or of other
/// characters. Empty at the end of the text or at a line break.
pub fn word_at(src: &[u8], at: usize) -> Range<usize> {
    let at = snap(src, at);
    if at >= src.len() || matches!(src[at], b'\r' | b'\n') {
        return at..at;
    }
    let kind = class(src, at);
    let same = |p: usize| !matches!(src[p], b'\r' | b'\n') && class(src, p) == kind;
    let start_of_text = text_start(src);
    let mut a = at;
    while a > start_of_text && same(prev_char(src, a)) {
        a = prev_char(src, a);
    }
    let mut b = at;
    while b < src.len() && same(b) {
        b = next_char(src, b);
    }
    a..b
}

/// The whole line that holds `at`, with its line ending, for a triple-click.
pub fn line_at(doc: &Document, at: usize) -> Range<usize> {
    let line = doc.line_of(at as u64);
    let end = doc
        .line_starts
        .get(line + 1)
        .map_or(doc.src.len(), |&s| s as usize);
    line_start(doc, at)..end
}

/// Where the text starts: after the byte order mark, if there is one.
pub fn text_start(src: &[u8]) -> usize {
    if src.starts_with(BOM) { BOM.len() } else { 0 }
}

/// The boundary after the character at `at`; `at` itself at the end.
pub fn next_char(src: &[u8], at: usize) -> usize {
    if at >= src.len() {
        return src.len();
    }
    if src[at] == b'\r' && src.get(at + 1) == Some(&b'\n') {
        return at + 2;
    }
    at + char_len(src, at)
}

/// The boundary before the character that ends at `at`; the start of the text at most.
pub fn prev_char(src: &[u8], at: usize) -> usize {
    let start = text_start(src);
    if at <= start {
        return start;
    }
    if at >= 2 && src[at - 1] == b'\n' && src[at - 2] == b'\r' && at - 2 >= start {
        return at - 2;
    }
    // A UTF-8 character of two to four bytes that ends exactly here; else one byte.
    (2..=4)
        .filter(|&k| at >= start + k)
        .find(|&k| char_len(src, at - k) == k)
        .map_or(at - 1, |k| at - k)
}

/// The end of the next word, past any spaces before it (Ctrl+Right).
pub fn next_word(src: &[u8], at: usize) -> usize {
    let mut at = at.max(text_start(src));
    while at < src.len() && class(src, at) == Class::Space {
        at = next_char(src, at);
    }
    if at >= src.len() {
        return src.len();
    }
    let kind = class(src, at);
    while at < src.len() && class(src, at) == kind {
        at = next_char(src, at);
    }
    at
}

/// The start of the previous word, past any spaces after it (Ctrl+Left).
pub fn prev_word(src: &[u8], at: usize) -> usize {
    let start = text_start(src);
    let mut at = at;
    while at > start && class(src, prev_char(src, at)) == Class::Space {
        at = prev_char(src, at);
    }
    if at <= start {
        return start;
    }
    let kind = class(src, prev_char(src, at));
    while at > start && class(src, prev_char(src, at)) == kind {
        at = prev_char(src, at);
    }
    at
}

/// The start of the line that holds `at`; after the byte order mark on the first line.
pub fn line_start(doc: &Document, at: usize) -> usize {
    let line = doc.line_of(at as u64);
    (doc.line_starts[line] as usize).max(text_start(&doc.src))
}

/// The end of the line that holds `at`, before its line ending.
pub fn line_end(doc: &Document, at: usize) -> usize {
    let line = doc.line_of(at as u64);
    line_end_of(doc, line)
}

/// Home (TEXT-16): the first non-blank character of the line, or the line's start when the
/// cursor is there already.
pub fn home(doc: &Document, at: usize) -> usize {
    let start = line_start(doc, at);
    let end = line_end(doc, at);
    let first = start
        + doc.src[start..end]
            .iter()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
    if at == first { start } else { first }
}

/// The display column of `at` in its line: characters before it, a tab counting as
/// [`TAB_WIDTH`].
pub fn column(doc: &Document, at: usize) -> usize {
    let mut pos = line_start(doc, at);
    let mut col = 0;
    while pos < at {
        col += width(&doc.src, pos);
        pos = next_char(&doc.src, pos);
    }
    col
}

/// The position on line `line` closest to display column `col`, at most the line's end.
pub fn at_column(doc: &Document, line: usize, col: usize) -> usize {
    let line = line.min(doc.line_count() - 1);
    let mut pos = (doc.line_starts[line] as usize).max(text_start(&doc.src));
    let end = line_end_of(doc, line);
    let mut here = 0;
    while pos < end {
        let w = width(&doc.src, pos);
        // Past the middle of the character: after it.
        if here + w > col {
            if col - here > w / 2 {
                pos = next_char(&doc.src, pos);
            }
            break;
        }
        here += w;
        pos = next_char(&doc.src, pos);
    }
    pos
}

/// The position `lines` lines up (negative) or down from `at`, at display column `col`
/// (Up, Down, Page Up, Page Down). Above the first line is the document's start; below the
/// last, its end.
pub fn vertical(doc: &Document, at: usize, lines: isize, col: usize) -> usize {
    let line = doc.line_of(at as u64);
    match line.checked_add_signed(lines) {
        None => text_start(&doc.src),
        Some(l) if l >= doc.line_count() => doc.src.len(),
        Some(l) => at_column(doc, l, col),
    }
}

/// The text of `bytes` as the text view shows it: each byte that is not valid UTF-8 becomes
/// one `�` (TEXT-21), so characters and positions correspond one to one.
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        out.extend(std::iter::repeat_n('\u{FFFD}', chunk.invalid().len()));
    }
    out
}

/// Select All (TEXT-17): the whole text, after the byte order mark.
pub fn select_all(doc: &Document) -> Selection {
    Selection {
        anchor: text_start(&doc.src),
        head: doc.src.len(),
    }
}

/// The selected text, for Copy (TEXT-18), with the file's line endings. A byte that is not
/// valid UTF-8 becomes `\u{FFFD}`, as the view shows it: the clipboard holds UTF-8 only.
pub fn selected_text(doc: &Document, sel: Selection) -> String {
    decode(&doc.src[sel.range()])
}

/// Typing `text` (TEXT-17): it replaces the selection, each line break written as the file's
/// line ending. Joins the undo step of the typing before it (TEXT-20). `None` when nothing
/// would change.
pub fn insert(doc: &Document, sel: Selection, text: &str) -> Option<TextEdit> {
    replace(
        doc,
        sel,
        line_endings(doc, text),
        "Typing",
        Some(Typing::Insert),
    )
}

/// Paste (TEXT-18): as typing, but one undo step of its own.
pub fn paste(doc: &Document, sel: Selection, text: &str) -> Option<TextEdit> {
    replace(doc, sel, line_endings(doc, text), "Paste", None)
}

/// Cut (TEXT-18): the selected text for the clipboard, as [`selected_text`], and the edit
/// that removes it, one undo step of its own. `None` when nothing is selected.
pub fn cut(doc: &Document, sel: Selection) -> Option<(String, TextEdit)> {
    if sel.is_empty() {
        return None;
    }
    let range = sel.range();
    let at = range.start;
    let edit = Edit::new("Cut").replace(range, Vec::new());
    let change = TextEdit {
        edit,
        selection: Selection::cursor(at),
    };
    Some((selected_text(doc, sel), change))
}

/// Enter (TEXT-17): the file's line ending, then the indentation of the line it was pressed
/// on, as far as it lies before the cursor.
pub fn newline(doc: &Document, sel: Selection) -> Option<TextEdit> {
    let at = sel.range().start;
    let start = line_start(doc, at);
    let indent = doc.src[start..at]
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    let mut bytes = doc.newline().to_vec();
    bytes.extend_from_slice(&doc.src[start..start + indent]);
    replace(doc, sel, bytes, "Typing", Some(Typing::Insert))
}

/// Tab (TEXT-17): one indent as the Format options give it, a tab or spaces.
pub fn tab(doc: &Document, sel: Selection, options: &FormatOptions) -> Option<TextEdit> {
    let bytes = if options.tabs {
        b"\t".to_vec()
    } else {
        vec![b' '; options.indent_size.max(1)]
    };
    replace(doc, sel, bytes, "Typing", Some(Typing::Insert))
}

/// Backspace (TEXT-17): the selection, or the character before the cursor; with `word`
/// (Ctrl) back to the start of the word.
pub fn backspace(doc: &Document, sel: Selection, word: bool) -> Option<TextEdit> {
    let range = if sel.is_empty() {
        let at = sel.head;
        let from = if word {
            prev_word(&doc.src, at)
        } else {
            prev_char(&doc.src, at)
        };
        from..at
    } else {
        sel.range()
    };
    remove(range)
}

/// Delete (TEXT-17): the selection, or the character after the cursor; with `word` (Ctrl)
/// to the end of the word.
pub fn delete(doc: &Document, sel: Selection, word: bool) -> Option<TextEdit> {
    let range = if sel.is_empty() {
        let at = sel.head.max(text_start(&doc.src));
        let to = if word {
            next_word(&doc.src, at)
        } else {
            next_char(&doc.src, at)
        };
        at..to
    } else {
        sel.range()
    };
    remove(range)
}

/// `text` with each line break, LF, CRLF or CR, written as the file's line ending.
fn line_endings(doc: &Document, text: &str) -> Vec<u8> {
    let newline = std::str::from_utf8(doc.newline()).expect("ASCII");
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', newline)
        .into_bytes()
}

/// Replaces the selection with `bytes`; the cursor goes after them. With `typing` the edit
/// may join the undo step before it.
fn replace(
    doc: &Document,
    sel: Selection,
    bytes: Vec<u8>,
    label: &str,
    typing: Option<Typing>,
) -> Option<TextEdit> {
    let range = sel.range();
    if doc.src[range.clone()] == bytes[..] {
        return None;
    }
    let after = range.start + bytes.len();
    let mut edit = Edit::new(label).replace(range, bytes);
    if let Some(kind) = typing {
        edit = edit.typing(kind);
    }
    Some(TextEdit {
        edit,
        selection: Selection::cursor(after),
    })
}

/// Removes `range`; the cursor goes where it was.
fn remove(range: Range<usize>) -> Option<TextEdit> {
    if range.is_empty() {
        return None;
    }
    let at = range.start;
    let edit = Edit::new("Delete")
        .replace(range, Vec::new())
        .typing(Typing::Delete);
    Some(TextEdit {
        edit,
        selection: Selection::cursor(at),
    })
}

/// The end of line `line`, before its LF or CRLF.
fn line_end_of(doc: &Document, line: usize) -> usize {
    let src = &doc.src;
    let Some(&next) = doc.line_starts.get(line + 1) else {
        return src.len();
    };
    let mut end = next as usize - 1;
    if end > 0 && src[end - 1] == b'\r' {
        end -= 1;
    }
    end.max(text_start(src))
}

/// Bytes of the character at `at`: a valid UTF-8 character, or one byte that is not.
fn char_len(src: &[u8], at: usize) -> usize {
    let window = &src[at..src.len().min(at + 4)];
    let chunk = window.utf8_chunks().next().expect("not empty");
    match chunk.valid().chars().next() {
        Some(c) => c.len_utf8(),
        None => 1,
    }
}

/// The character at `at`; `None` for a byte that is not valid UTF-8.
fn char_at(src: &[u8], at: usize) -> Option<char> {
    let len = char_len(src, at);
    std::str::from_utf8(&src[at..at + len]).ok()?.chars().next()
}

fn class(src: &[u8], at: usize) -> Class {
    match char_at(src, at) {
        Some(c) if is_word_char(c) => Class::Word,
        Some(c) if c.is_whitespace() => Class::Space,
        _ => Class::Other,
    }
}

/// Display columns of the character at `at`.
fn width(src: &[u8], at: usize) -> usize {
    if src[at] == b'\t' { TAB_WIDTH } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::History;
    use std::time::{Duration, Instant};

    /// A byte order mark, CRLF, a tab, non-ASCII and CJK text, and a byte that is not UTF-8
    /// (`\xE4`, Latin-1 `ä`).
    const SRC: &[u8] = b"\xEF\xBB\xBF<r>\r\n\t<a x=\"\xE4\">Gr\xC3\xBC\xC3\x9Fe \xE6\x97\xA5\xE6\x9C\xAC</a>\r\n</r>\r\n";

    fn doc() -> Document {
        Document::from_bytes(SRC.to_vec())
    }

    fn find(doc: &Document, s: &str) -> usize {
        doc.src
            .windows(s.len())
            .position(|w| w == s.as_bytes())
            .unwrap()
    }

    fn apply(doc: &mut Document, make: impl FnOnce(&Document) -> Option<TextEdit>) -> Selection {
        let change = make(doc).expect("a change");
        doc.apply(change.edit).unwrap();
        change.selection
    }

    /// Every boundary from the text start to the end, stepping forward.
    fn boundaries(src: &[u8]) -> Vec<usize> {
        let mut at = text_start(src);
        let mut all = vec![at];
        while at < src.len() {
            at = next_char(src, at);
            all.push(at);
        }
        all
    }

    #[test]
    fn characters_are_whole() {
        let src = SRC;
        let all = boundaries(src);
        assert_eq!(all[0], 3, "after the byte order mark");
        assert_eq!(prev_char(src, 3), 3);
        assert_eq!(prev_char(src, 0), 3);
        // Stepping back meets the same boundaries.
        let mut back = vec![src.len()];
        let mut at = src.len();
        while at > 3 {
            at = prev_char(src, at);
            back.push(at);
        }
        back.reverse();
        assert_eq!(back, all);
        // CRLF is one step; `ü` two bytes, `日` three, the stray `\xE4` one.
        let cr = 6;
        assert_eq!(&src[cr..cr + 2], b"\r\n");
        assert!(!all.contains(&(cr + 1)), "never between CR and LF");
        let u = src.windows(2).position(|w| w == b"\xC3\xBC").unwrap();
        assert_eq!(next_char(src, u), u + 2);
        let ri = src.windows(3).position(|w| w == "日".as_bytes()).unwrap();
        assert_eq!(next_char(src, ri), ri + 3);
        assert_eq!(prev_char(src, ri + 3), ri);
        let bad = src.iter().position(|&b| b == 0xE4).unwrap();
        assert_eq!(
            (next_char(src, bad), prev_char(src, bad + 1)),
            (bad + 1, bad)
        );
        // As many characters as the view shows, where CR and LF are two but one step.
        let crlf = src.windows(2).filter(|w| w == b"\r\n").count();
        assert_eq!(decode(&src[3..]).chars().count(), all.len() - 1 + crlf);
        assert_eq!(decode(b"a\xE4b"), "a\u{FFFD}b");
    }

    #[test]
    fn words() {
        let d = doc();
        let src = &d.src;
        let gr = find(&d, "Gr");
        let space = find(&d, " 日");
        let end_cjk = find(&d, "</a>");
        assert_eq!(next_word(src, gr), space, "Grüße is one word");
        assert_eq!(next_word(src, space), end_cjk, "the space, then 日本");
        assert_eq!(prev_word(src, end_cjk), space + 1);
        assert_eq!(prev_word(src, space + 1), gr);
        // Punctuation is a word of its own: `</` then `a`.
        assert_eq!(next_word(src, end_cjk), end_cjk + 2);
        assert_eq!(prev_word(src, 4), 3, "not before the byte order mark");
        assert_eq!(next_word(src, src.len()), src.len());
    }

    #[test]
    fn lines_home_and_columns() {
        let d = doc();
        assert_eq!(line_start(&d, 4), 3, "after the byte order mark");
        assert_eq!(line_end(&d, 4), 6, "before CRLF");
        let a = find(&d, "<a");
        let line1 = a - 1;
        assert_eq!(line_start(&d, a + 5), line1);
        // Home: the first non-blank character, then the start, then back.
        assert_eq!(home(&d, a + 5), a);
        assert_eq!(home(&d, a), line1);
        assert_eq!(home(&d, line1), a);
        // A tab is four columns, `ü` and `日` one each.
        assert_eq!(column(&d, a), TAB_WIDTH);
        let gr = find(&d, "Gr");
        assert_eq!(column(&d, gr + 4), column(&d, gr) + 3, "G r ü");
        // Every position of line 1 is found again from its column.
        let mut at = line1;
        while at <= line_end(&d, line1) {
            assert_eq!(at_column(&d, 1, column(&d, at)), at);
            if at == line_end(&d, line1) {
                break;
            }
            at = next_char(&d.src, at);
        }
        // Inside the tab: the nearer side.
        assert_eq!(at_column(&d, 1, 1), line1);
        assert_eq!(at_column(&d, 1, 3), a);
        // Past the end of a shorter line: its end. Above the first: the start.
        assert_eq!(vertical(&d, gr, 1, column(&d, gr)), find(&d, "</r>") + 4);
        assert_eq!(vertical(&d, gr, -1, column(&d, gr)), 6);
        assert_eq!(vertical(&d, gr, -5, 0), 3);
        assert_eq!(vertical(&d, gr, 9, 0), d.src.len());
    }

    #[test]
    fn typing_keeps_every_other_byte() {
        let mut d = doc();
        let e = find(&d, "e 日");
        // Backspace twice deletes `e` and `ß` (two bytes); typing puts them back.
        let mut sel = Selection::cursor(e + 1);
        sel = apply(&mut d, |d| backspace(d, sel, false));
        sel = apply(&mut d, |d| backspace(d, sel, false));
        assert_eq!(sel.head, e - 2);
        sel = apply(&mut d, |d| insert(d, sel, "ße"));
        assert_eq!(sel.head, e + 1);
        assert_eq!(d.src, SRC);
        // Nothing before the text start or after the end to delete.
        assert_eq!(backspace(&d, Selection::cursor(3), false), None);
        assert_eq!(backspace(&d, Selection::cursor(0), false), None);
        assert_eq!(delete(&d, Selection::cursor(d.src.len()), false), None);
        // Backspace after a line break removes CR and LF together; the stray byte goes alone.
        let lf = find(&d, "\r\n\t") + 2;
        let c = backspace(&d, Selection::cursor(lf), false).unwrap();
        assert_eq!(c.selection.head, lf - 2);
        let bad = d.src.iter().position(|&b| b == 0xE4).unwrap();
        let c = delete(&d, Selection::cursor(bad), false).unwrap();
        let mut copy = d.src.clone();
        History::new().apply(&mut copy, c.edit).unwrap();
        assert_eq!(copy.len(), d.src.len() - 1);
        // Ctrl+Backspace takes the word.
        let end = find(&d, "</a>");
        let c = backspace(&d, Selection::cursor(end), true).unwrap();
        assert_eq!(c.selection.head, find(&d, "日"));
    }

    #[test]
    fn line_breaks_follow_the_file() {
        let mut d = doc();
        let gr = find(&d, "Gr");
        // Paste with LF, CRLF and CR: all become CRLF.
        let sel = Selection {
            anchor: gr,
            head: gr + 2,
        };
        let after = apply(&mut d, |d| insert(d, sel, "a\nb\r\nc\rd"));
        assert_eq!(&d.src[gr..after.head], b"a\r\nb\r\nc\r\nd");
        assert_eq!(
            selected_text(
                &d,
                Selection {
                    anchor: gr,
                    head: after.head
                }
            ),
            "a\r\nb\r\nc\r\nd"
        );
        // Enter keeps the indentation (a tab) and writes CRLF.
        let mut d = doc();
        let gr = find(&d, "Gr");
        let sel = apply(&mut d, |d| newline(d, Selection::cursor(gr)));
        assert_eq!(&d.src[gr..sel.head], b"\r\n\t");
        // Within the indentation only the part before the cursor is repeated.
        let line1 = find(&d, "\t<a");
        let sel = apply(&mut d, |d| newline(d, Selection::cursor(line1)));
        assert_eq!(&d.src[line1..sel.head], b"\r\n");
        // An LF file gets LF.
        let lf = Document::from_bytes(b"<r>\n  <a/>\n</r>".to_vec());
        let c = newline(&lf, Selection::cursor(10)).unwrap();
        assert_eq!(c.selection.head, 13);
    }

    #[test]
    fn tab_is_one_indent_from_the_options() {
        let d = doc();
        let spaces = FormatOptions {
            indent_size: 3,
            tabs: false,
            ..FormatOptions::default()
        };
        let c = tab(&d, Selection::cursor(3), &spaces).unwrap();
        assert_eq!(c.selection.head, 6);
        let tabs = FormatOptions {
            tabs: true,
            ..spaces
        };
        assert_eq!(
            tab(&d, Selection::cursor(3), &tabs).unwrap().selection.head,
            4
        );
    }

    /// Undo goes word by word (TEXT-20).
    #[test]
    fn typing_is_undone_word_by_word() {
        let mut src = SRC.to_vec();
        let mut h = History::new();
        let t0 = Instant::now();
        let at = 3;
        let type_text =
            |src: &mut Vec<u8>, h: &mut History, from: usize, text: &str, t: Instant| {
                let mut pos = from;
                for c in text.chars() {
                    let bytes = c.to_string().into_bytes();
                    let n = bytes.len();
                    let e = Edit::new("Typing")
                        .replace(pos..pos, bytes)
                        .typing(Typing::Insert);
                    h.apply_at(src, e, t).unwrap();
                    pos += n;
                }
                pos
            };
        let end = type_text(&mut src, &mut h, at, "Grüße, Welt", t0);
        // Steps: `Grüße, ` and `Welt`.
        h.undo(&mut src);
        assert_eq!(&src[at..at + 9], "Grüße, ".as_bytes());
        h.undo(&mut src);
        assert_eq!(src, SRC);
        h.redo(&mut src);
        h.redo(&mut src);
        assert_eq!(src.len(), SRC.len() + (end - at));

        // A pause of a second, a moved cursor, or switching to deleting starts a new step.
        let mut src = SRC.to_vec();
        let mut h = History::new();
        let p = type_text(&mut src, &mut h, at, "ab", t0);
        let p = type_text(&mut src, &mut h, p, "cd", t0 + Duration::from_millis(1500));
        h.seal();
        type_text(&mut src, &mut h, p, "ef", t0 + Duration::from_millis(1600));
        let del = Edit::new("Delete")
            .replace(p + 1..p + 2, Vec::new())
            .typing(Typing::Delete);
        h.apply_at(&mut src, del, t0 + Duration::from_millis(1700))
            .unwrap();
        let del = Edit::new("Delete")
            .replace(p..p + 1, Vec::new())
            .typing(Typing::Delete);
        h.apply_at(&mut src, del, t0 + Duration::from_millis(1800))
            .unwrap();
        assert_eq!(&src[at..at + 4], b"abcd");
        h.undo(&mut src); // both deletes
        assert_eq!(&src[at..at + 6], b"abcdef");
        h.undo(&mut src); // `ef`
        h.undo(&mut src); // `cd`
        assert_eq!(&src[at..at + 3], b"ab<");
        h.undo(&mut src); // `ab`
        assert_eq!(src, SRC);
        assert!(h.undo(&mut src).is_none());

        // Saving ends the step, so undo goes back to exactly the saved bytes.
        let mut src = SRC.to_vec();
        let mut h = History::new();
        let p = type_text(&mut src, &mut h, at, "x", t0);
        h.mark_saved();
        type_text(&mut src, &mut h, p, "y", t0);
        h.undo(&mut src);
        assert!(!h.is_modified());
        assert_eq!(&src[at..at + 2], b"x<");
    }

    #[test]
    fn snap_word_and_line() {
        let d = doc();
        let src = &d.src;
        // Inside `ü`, between CR and LF, inside the byte order mark: the boundary before.
        let u = find(&d, "ü");
        assert_eq!(snap(src, u + 1), u);
        assert_eq!(snap(src, 7), 6);
        assert_eq!(snap(src, 1), 3);
        assert_eq!(snap(src, 999), src.len());
        for &b in &boundaries(src) {
            assert_eq!(snap(src, b), b);
        }
        // A double-click on `ü` takes `Grüße`; on the space, the space; on `<`, `</`.
        let gr = find(&d, "Gr");
        assert_eq!(word_at(src, u), gr..find(&d, " 日"));
        assert_eq!(
            word_at(src, find(&d, " 日")),
            find(&d, " 日")..find(&d, "日")
        );
        let end = find(&d, "</a>");
        assert_eq!(word_at(src, end), end..end + 2);
        assert_eq!(word_at(src, 6), 6..6, "a line break");
        // A triple-click takes the line with its CRLF.
        let line1 = find(&d, "\t<a");
        assert_eq!(line_at(&d, gr), line1..find(&d, "</r>"));
        assert_eq!(line_at(&d, 4), 3..line1);
    }

    /// Cut, Copy, Paste and Select All (TEXT-17, TEXT-18).
    #[test]
    fn clipboard() {
        let mut d = doc();
        // Select All leaves the byte order mark out.
        let all = select_all(&d);
        assert_eq!(all.range(), 3..SRC.len());
        let copied = selected_text(&d, all);
        assert!(copied.starts_with("<r>\r\n"), "the file's line endings");
        assert!(
            copied.contains("x=\"\u{FFFD}\""),
            "a stray byte as the view shows it"
        );

        // Typing, then a paste, then typing again: three undo steps, the paste one of them
        // although it comes right after a typed letter.
        let gr = find(&d, "Gr");
        let mut sel = apply(&mut d, |d| insert(d, Selection::cursor(gr), "a"));
        sel = apply(&mut d, |d| paste(d, sel, "Hallo\nWelt"));
        assert_eq!(
            &d.src[gr + 1..sel.head],
            b"Hallo\r\nWelt",
            "pasted with CRLF"
        );
        apply(&mut d, |d| insert(d, sel, "b"));
        assert_eq!(d.history.undo_label(), Some("Typing"));
        d.undo();
        assert_eq!(d.history.undo_label(), Some("Paste"));
        d.undo();
        assert_eq!(&d.src[gr..gr + 3], b"aGr");
        d.undo();
        assert_eq!(d.src, SRC);

        // Cut takes `Grüße` to the clipboard and out of the text in one step; Paste puts it
        // back byte for byte.
        let word = word_at(&d.src, gr);
        let sel = Selection {
            anchor: word.end,
            head: word.start,
        };
        let (text, change) = cut(&d, sel).unwrap();
        assert_eq!(text, "Grüße");
        assert_eq!(change.selection, Selection::cursor(gr));
        d.apply(change.edit).unwrap();
        assert_eq!(d.history.undo_label(), Some("Cut"));
        apply(&mut d, |d| paste(d, Selection::cursor(gr), &text));
        assert_eq!(d.src, SRC);
        d.undo();
        d.undo();
        assert_eq!(d.src, SRC);
        assert!(!d.is_modified());

        // Nothing selected: nothing to cut. Pasting what is there already changes nothing.
        assert_eq!(cut(&d, Selection::cursor(gr)), None);
        assert_eq!(paste(&d, sel, "Grüße"), None);
        // Select All, Cut, Paste gives back the file byte for byte, though no line break was
        // left in between to tell CRLF by; the clipboard comes back with LF, as egui hands it.
        let (text, change) = cut(&d, select_all(&d)).unwrap();
        d.apply(change.edit).unwrap();
        assert_eq!(d.src, b"\xEF\xBB\xBF");
        let lf_text = text.replace("\r\n", "\n");
        let all = select_all(&d);
        let pasted = paste(&d, all, &lf_text).unwrap();
        d.apply(pasted.edit).unwrap();
        // Byte for byte, but for the stray byte, which went to the clipboard as `\u{FFFD}`.
        let expected = [&SRC[..15], "\u{FFFD}".as_bytes(), &SRC[16..]].concat();
        assert_eq!(SRC[15], 0xE4);
        assert_eq!(d.src, expected);

        // An LF file gets LF, whatever the clipboard holds.
        let lf = Document::from_bytes(b"<r>\n</r>".to_vec());
        let mut lf = lf;
        let after = apply(&mut lf, |d| paste(d, Selection::cursor(3), "\r\n<a/>\r"));
        assert_eq!(lf.src, b"<r>\n<a/>\n\n</r>");
        assert_eq!(after.head, 9);
    }

    #[test]
    fn the_last_change_is_known() {
        let mut d = doc();
        let sel = apply(&mut d, |d| insert(d, Selection::cursor(3), "ä"));
        let c = d.last_change().unwrap();
        assert_eq!((c.at, c.at + c.inserted), (3, sel.head));
        d.undo();
        let c = d.last_change().unwrap();
        assert_eq!((c.at, c.inserted, c.removed), (3, 0, 2));
    }
}
