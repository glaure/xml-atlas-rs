//! Find and replace text in the source (`docs/spec/text.md`, TEXT-12, TEXT-14).
//!
//! The query is a literal, not a pattern: `a.b` finds `a.b`. Matching case ignores case by
//! Unicode rules (`SÖHNE` finds `Söhne`). Whole word means the characters next to the match are
//! not letters, digits or `_`. Searching wraps around the end of the document.

use std::ops::Range;

use regex::bytes::{Regex, RegexBuilder};

use crate::Edit;

/// How far back a backward search first looks. The window grows until it finds a match or
/// reaches the start.
const BACKWARD_WINDOW: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// A match, and whether the search went past the end (or start) of the document to find it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub range: Range<usize>,
    pub wrapped: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FindError {
    #[error("nothing to find")]
    Empty,
    #[error("cannot search for this text: {0}")]
    Query(#[from] regex::Error),
}

/// A compiled query. Build it once per query and options, then call [`Finder::next`].
#[derive(Clone, Debug)]
pub struct Finder {
    regex: Regex,
    whole_word: bool,
    /// Longest a match can be, in bytes. Case folding can change the length (`ß` and `SS`),
    /// so this is generous.
    max_len: usize,
}

impl Finder {
    pub fn new(query: &str, options: FindOptions) -> Result<Self, FindError> {
        if query.is_empty() {
            return Err(FindError::Empty);
        }
        let regex = RegexBuilder::new(&regex::escape(query))
            .case_insensitive(!options.match_case)
            .build()?;
        Ok(Self {
            regex,
            whole_word: options.whole_word,
            max_len: query.len() * 4 + 16,
        })
    }

    /// The next match in `direction`. Forward: the first match that starts at or after `from`.
    /// Backward: the last match that starts before `from`. When there is none, the search
    /// wraps around once.
    pub fn next(&self, src: &[u8], from: usize, direction: Direction) -> Option<Found> {
        let from = from.min(src.len());
        let (found, wrapped) = match direction {
            Direction::Forward => match self.forward(src, from, src.len()) {
                Some(r) => (r, false),
                None => (self.forward(src, 0, from)?, true),
            },
            Direction::Backward => match self.backward(src, 0, from) {
                Some(r) => (r, false),
                None => (self.backward(src, from, src.len())?, true),
            },
        };
        Some(Found {
            range: found,
            wrapped,
        })
    }

    /// First match starting in `start..end`.
    fn forward(&self, src: &[u8], start: usize, end: usize) -> Option<Range<usize>> {
        let hay = &src[..(end + self.max_len).min(src.len())];
        let mut at = start;
        while let Some(m) = self.regex.find_at(hay, at) {
            if m.start() >= end {
                return None;
            }
            if self.accept(src, m.range()) {
                return Some(m.range());
            }
            at = m.start() + 1;
        }
        None
    }

    /// Last match starting in `start..end`. Looks back from `end` in growing windows, so a
    /// match just before `end` is found without scanning the whole document.
    fn backward(&self, src: &[u8], start: usize, end: usize) -> Option<Range<usize>> {
        let mut window = BACKWARD_WINDOW;
        let mut stop = end;
        loop {
            let from = stop.saturating_sub(window).max(start);
            let mut last = None;
            let mut at = from;
            let hay = &src[..(stop + self.max_len).min(src.len())];
            while let Some(m) = self.regex.find_at(hay, at) {
                if m.start() >= stop {
                    break;
                }
                if self.accept(src, m.range()) {
                    last = Some(m.range());
                }
                at = m.start() + 1;
            }
            if last.is_some() || from == start {
                return last;
            }
            // Nothing in this window: look further back, without searching it again.
            stop = from;
            window = window.saturating_mul(4);
        }
    }

    /// Every match that lies wholly inside `within`, in order, not overlapping.
    pub fn all(&self, src: &[u8], within: Range<usize>) -> Vec<Range<usize>> {
        let end = within.end.min(src.len());
        let mut out = Vec::new();
        let mut at = within.start;
        while at < end
            && let Some(m) = self.forward(src, at, end)
        {
            if m.end <= end {
                at = m.end;
                out.push(m);
            } else {
                // Runs past the end of `within`: not inside it.
                at = m.start + 1;
            }
        }
        out
    }

    /// Whether `range` is exactly a match: the one Find found, before Replace replaces it.
    pub fn is_match(&self, src: &[u8], range: Range<usize>) -> bool {
        range.end <= src.len()
            && self
                .regex
                .find_at(&src[..range.end], range.start)
                .is_some_and(|m| m.range() == range)
            && self.accept(src, range)
    }

    fn accept(&self, src: &[u8], range: Range<usize>) -> bool {
        !self.whole_word
            || (!char_before(src, range.start).is_some_and(is_word)
                && !char_after(src, range.end).is_some_and(is_word))
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The character that ends just before byte `i`, if the bytes there are UTF-8.
/// Replace (TEXT-14): replaces the match at `range` with `with`, as typed. `None` when
/// `range` is not a match of `finder`, for instance because the document changed.
pub fn replace(finder: &Finder, src: &[u8], range: Range<usize>, with: &str) -> Option<Edit> {
    finder
        .is_match(src, range.clone())
        .then(|| Edit::new("Replace").replace(range, with.as_bytes()))
}

/// Replace All (TEXT-14): one edit, so one undo step, that replaces every match inside
/// `within` with `with`, as typed; and how many there are. `None` when there is none.
pub fn replace_all(
    finder: &Finder,
    src: &[u8],
    within: Range<usize>,
    with: &str,
) -> Option<(Edit, usize)> {
    let matches = finder.all(src, within);
    if matches.is_empty() {
        return None;
    }
    let count = matches.len();
    let label = match count {
        1 => "Replace All (1 match)".to_string(),
        n => format!("Replace All ({n} matches)"),
    };
    let edit = matches
        .into_iter()
        .fold(Edit::new(label), |e, m| e.replace(m, with.as_bytes()));
    Some((edit, count))
}

fn char_before(src: &[u8], i: usize) -> Option<char> {
    let lo = i.saturating_sub(4);
    (lo..i)
        .rev()
        .filter(|&s| src[s] & 0b1100_0000 != 0b1000_0000)
        .find_map(|s| std::str::from_utf8(&src[s..i]).ok()?.chars().next())
}

/// The character that starts at byte `i`, if the bytes there are UTF-8.
fn char_after(src: &[u8], i: usize) -> Option<char> {
    let hi = (i + 4).min(src.len());
    (i + 1..=hi).find_map(|e| std::str::from_utf8(&src[i..e]).ok()?.chars().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "<a>Söhne</a><b>söhne</b><c>Söhnefeld</c><d>SÖHNE</d>";

    fn find(query: &str, options: FindOptions, from: usize, dir: Direction) -> Option<Found> {
        Finder::new(query, options)
            .unwrap()
            .next(SRC.as_bytes(), from, dir)
    }

    fn texts(query: &str, options: FindOptions) -> Vec<&'static str> {
        let finder = Finder::new(query, options).unwrap();
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(f) = finder.next(SRC.as_bytes(), from, Direction::Forward) {
            if f.wrapped {
                break;
            }
            out.push(&SRC[f.range.clone()]);
            from = f.range.start + 1;
        }
        out
    }

    const CASE: FindOptions = FindOptions {
        match_case: true,
        whole_word: false,
    };
    const WORD: FindOptions = FindOptions {
        match_case: false,
        whole_word: true,
    };

    #[test]
    fn ignores_case_by_default_including_umlauts() {
        assert_eq!(
            texts("söhne", FindOptions::default()),
            ["Söhne", "söhne", "Söhne", "SÖHNE"]
        );
    }

    #[test]
    fn match_case() {
        assert_eq!(texts("Söhne", CASE), ["Söhne", "Söhne"]);
    }

    #[test]
    fn whole_word_rejects_letters_next_to_the_match() {
        // `Söhnefeld` is not a whole-word match; `<` and `>` are not word characters.
        assert_eq!(texts("söhne", WORD), ["Söhne", "söhne", "SÖHNE"]);
        assert_eq!(texts("feld", WORD), Vec::<&str>::new());
    }

    #[test]
    fn query_is_literal() {
        let src = b"a.b axb <order id=\"1\">";
        let f = Finder::new("a.b", FindOptions::default()).unwrap();
        assert_eq!(f.next(src, 1, Direction::Forward).unwrap().range, 0..3);
        let f = Finder::new("<order id=\"", FindOptions::default()).unwrap();
        assert_eq!(f.next(src, 0, Direction::Forward).unwrap().range, 8..19);
    }

    #[test]
    fn forward_and_backward_wrap_around() {
        let o = CASE;
        let first = SRC.find("Söhne").unwrap();
        let third = SRC.rfind("Söhne").unwrap();
        // Forward from just after the last match wraps to the first.
        let f = find("Söhne", o, third + 1, Direction::Forward).unwrap();
        assert_eq!((f.range.start, f.wrapped), (first, true));
        // Backward from the second match finds the first, without wrapping.
        let f = find("Söhne", o, third, Direction::Backward).unwrap();
        assert_eq!((f.range.start, f.wrapped), (first, false));
        // Backward from the first wraps to the last.
        let f = find("Söhne", o, first, Direction::Backward).unwrap();
        assert_eq!((f.range.start, f.wrapped), (third, true));
    }

    #[test]
    fn backward_finds_a_match_far_back() {
        let mut src = b"needle".to_vec();
        src.extend(std::iter::repeat_n(b'x', BACKWARD_WINDOW * 20));
        let f = Finder::new("needle", FindOptions::default()).unwrap();
        let found = f.next(&src, src.len(), Direction::Backward).unwrap();
        assert_eq!((found.range, found.wrapped), (0..6, false));
    }

    #[test]
    fn nothing_found_and_empty_query() {
        assert!(find("missing", FindOptions::default(), 0, Direction::Forward).is_none());
        assert!(find("missing", FindOptions::default(), 10, Direction::Backward).is_none());
        assert!(matches!(
            Finder::new("", FindOptions::default()),
            Err(FindError::Empty)
        ));
    }

    #[test]
    fn replace_all_changes_every_match_and_undoes_in_one_step() {
        let src = "\u{feff}<r>\r\n  <a k=\"Söhne\">Söhne &amp; SÖHNE</a>\r\n  <b>Söhnefeld</b>\r\n</r>\r\n";
        let mut doc = crate::Document::from_bytes(src.as_bytes().to_vec());
        let finder = Finder::new("söhne", WORD).unwrap();
        let (edit, count) = replace_all(&finder, &doc.src, 0..doc.src.len(), "Töchter").unwrap();
        assert_eq!(count, 3);
        assert_eq!(edit.label(), "Replace All (3 matches)");
        let revision = doc.revision();
        doc.apply(edit).unwrap();
        assert!(doc.revision() > revision);
        assert_eq!(
            String::from_utf8(doc.src.clone()).unwrap(),
            src.replace("Söhne\"", "Töchter\"")
                .replace("Söhne &amp; SÖHNE", "Töchter &amp; Töchter")
        );
        assert!(doc.index.error.is_none());
        doc.undo();
        assert_eq!(doc.src, src.as_bytes());
        // Nothing to replace.
        assert!(
            replace_all(
                &Finder::new("x y", CASE).unwrap(),
                &doc.src,
                0..doc.src.len(),
                "z"
            )
            .is_none()
        );
    }

    #[test]
    fn replace_all_within_an_element_keeps_to_it() {
        let src = "<r><a>xx</a><b>x<c>x</c></b><d>x</d></r>";
        let finder = Finder::new("x", CASE).unwrap();
        let b = src.find("<b>").unwrap()..src.find("<d>").unwrap();
        let (edit, count) = replace_all(&finder, src.as_bytes(), b, "y").unwrap();
        assert_eq!(count, 2);
        let mut doc = crate::Document::from_bytes(src.as_bytes().to_vec());
        doc.apply(edit).unwrap();
        assert_eq!(doc.src, b"<r><a>xx</a><b>y<c>y</c></b><d>x</d></r>");
        // A match that runs past the end of the range is not inside it.
        let finder = Finder::new("ab", CASE).unwrap();
        assert_eq!(finder.all(b"abab", 0..3), vec![0..2]);
        assert_eq!(finder.all(b"abab", 1..4), vec![2..4]);
    }

    #[test]
    fn replace_needs_the_range_to_be_a_match() {
        let finder = Finder::new("söhne", WORD).unwrap();
        let src = SRC.as_bytes();
        let first = find("söhne", WORD, 0, Direction::Forward).unwrap().range;
        let edit = replace(&finder, src, first.clone(), "Kinder").unwrap();
        let mut doc = crate::Document::from_bytes(src.to_vec());
        doc.apply(edit).unwrap();
        assert!(String::from_utf8_lossy(&doc.src).starts_with("<a>Kinder</a>"));
        // Shifted by a byte, or a part of a longer word with whole word on: not a match.
        assert!(replace(&finder, src, first.start + 1..first.end + 1, "x").is_none());
        let feld = SRC.find("Söhnefeld").unwrap();
        assert!(replace(&finder, src, feld..feld + "Söhne".len(), "x").is_none());
    }
}
