//! Find text in the source (`docs/spec/text.md`, TEXT-12).
//!
//! The query is a literal, not a pattern: `a.b` finds `a.b`. Matching case ignores case by
//! Unicode rules (`SÖHNE` finds `Söhne`). Whole word means the characters next to the match are
//! not letters, digits or `_`. Searching wraps around the end of the document.

use std::ops::Range;

use regex::bytes::{Regex, RegexBuilder};

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
}
