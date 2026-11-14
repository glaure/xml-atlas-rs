# Text view

The text view shows the source. The document's text is the truth; tree and grid are views
of it.

## Rules

- **TEXT-1** `[built]` The source is shown in a fixed-width font with line numbers.
- **TEXT-2** `[built]` Tag names, attribute names, attribute values, comments and punctuation
  have different colours, in light and dark themes.
- **TEXT-3** `[gap]` Colours are right for comments, CDATA and attribute values that span
  several lines. Today each line is coloured on its own.
- **TEXT-4** `[built]` A tab is shown as four spaces.
- **TEXT-5** `[gap]` Very long lines can be read to the end. Today a line is cut at 4,000
  bytes.
- **TEXT-6** `[done]` The selected element is highlighted from its `<` to its last `>`, not by
  whole lines, as in XML Marker (XM-22). On lines in between, the highlight covers the text from
  the first to the last non-blank character.
- **TEXT-7** `[built]` Clicking a line selects the innermost element that contains the first
  non-blank character of that line.
- **TEXT-8** `[gap]` Every line of a very large file can be reached by scrolling. Scroll
  positions lose precision above about 16 million pixels (`docs/decisions.md`, D16).
- **TEXT-9** `[planned]` The text can be edited: cursor, selection, clipboard, undo. Tree and
  grid follow shortly after each change.
- **TEXT-10** `[planned]` While the text is not well-formed, tree and grid keep showing the
  last good state for the broken part, and mark it.
- **TEXT-11** `[done]` What is clicked inside the selection gets a second, outlined highlight
  of its own, inside the first (XM-37): a grid row or a cell that stands for a child element,
  and in the tree a text row or an attribute row (just its `name="value"`).
- **TEXT-12** `[done]` Find (Ctrl+F, or Edit > Find) opens a bar above the text with match
  case and whole word (XM-46). Enter or F3 finds the next match, Shift+Enter or Shift+F3 the
  previous one, and the search continues from the other end when it reaches one. The query is
  plain text; without match case, case is ignored by Unicode rules (`SÖHNE` finds `Söhne`).
  Whole word means the characters next to the match are not letters, digits or `_`. A match
  selects the element that contains it, so tree and grid follow, and is outlined in the text
  (TEXT-11). The bar shows how many matches the document has. Escape closes it. The CLI has
  the same search: `xml-atlas-cli find FILE TEXT [--match-case] [--whole-word]`.
- **TEXT-14** `[planned]` Replace, with the same options, in the selection or the whole file,
  and Replace All (XM-46). It needs an editable document (roadmap 0.2).
- **TEXT-15** `[planned]` Regular expressions, and search by element name or attribute value.
- **TEXT-13** `[planned]` Word wrap can be turned on.

## Acceptance tests

- Open a file whose comments contain umlauts and CJK characters: no crash, text is intact.
- Click line 1,000,000 of the 100 MiB fixture: the tree shows the element on that line.
- Open `crates/engine/tests/corpus/one_line.xml` and click row `b` in the grid: only
  `<b><c/><c/></b>` is outlined, inside the root's highlight.
- Click an attribute row in the tree: only that attribute's `name="value"` is outlined.
- In the 100 MiB fixture, find `Customer 49857`: the bar says 4 matches. Enter goes to line 6
  and selects that `name`; Shift+F3 then continues from the bottom to line 2,060,923.
- Find `söhne` in `crates/engine/tests/corpus/utf8_bom_crlf.xml`: one match, `Söhne`.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker's text is editable and coloured, in a proportional font, without line numbers
  and without word wrap (XM-40). TEXT-1 differs on purpose; TEXT-13 adds wrap. The Qt XML
  Atlas has a fixed-width font and line numbers (QT-12).
- Moving the cursor in XML Marker moves nothing else; tree and grid follow on Ctrl+L (XM-41,
  XM-42). TEXT-7 and TREE-12 follow a click, not typing.
- XML Marker re-parses on every keystroke and the tree comes back collapsed with nothing
  selected (XM-44). The Qt XML Atlas does the same and comes back fully expanded (QT-8). That
  cannot work at 100 MiB, where indexing alone takes 0.3 s; TEXT-9 says "shortly after" and
  must keep selection and expansion.
- While the text is broken, XML Marker shows an error list and either a complete tree or an
  empty one, depending on the error (XM-11, XM-12). TEXT-10 asks for better.
- XML Marker highlights the selected element's exact extent, from `<` to `>`, with an outline;
  a click in the grid adds a second highlight inside it (XM-22, XM-37). TEXT-6 and TEXT-11
  follow.
- Find and Replace in XML Marker: whole word, match case, up or down, replace in selection or
  whole file, replace all. No regular expressions, no search in tree or grid (XM-46). The Qt
  XML Atlas finds plain text only (QT-9). TEXT-12 starts from XML Marker's set.
- XML Marker's text menu has structural commands: Insert Tag, Insert Attribute, Comment Out
  Selection, Mark Selection With Tag (XM-45). The owner does not use them.

## Decided

- Exact-extent highlight (TEXT-6) and a second highlight for the grid (TEXT-11).
- Find and Replace as in XML Marker first, more later (TEXT-12).
- Word wrap (TEXT-13).
- Daily editing is plain: type, cut, copy, paste. Cut, copy and paste are wanted in 0.2. The
  structural commands of XM-45 are not needed.

## Questions

1. The roadmap puts the editable text view in 0.3. Does "cut, copy and paste in 0.2" mean
   cutting, copying and pasting whole elements from the tree and grid, or should the text view
   become editable in 0.2?
