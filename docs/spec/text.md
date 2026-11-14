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
- **TEXT-6** `[built]` The lines of the element highlighted from the tree or grid have a
  tinted background.
- **TEXT-7** `[built]` Clicking a line selects the innermost element that contains the first
  non-blank character of that line.
- **TEXT-8** `[gap]` Every line of a very large file can be reached by scrolling. Scroll
  positions lose precision above about 16 million pixels (`docs/decisions.md`, D16).
- **TEXT-9** `[planned]` The text can be edited: cursor, selection, clipboard, undo. Tree and
  grid follow shortly after each change.
- **TEXT-10** `[planned]` While the text is not well-formed, tree and grid keep showing the
  last good state for the broken part, and mark it.

## Acceptance tests

- Open a file whose comments contain umlauts and CJK characters: no crash, text is intact.
- Click line 1,000,000 of the 100 MiB fixture: the tree shows the element on that line.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker's text is editable and coloured, in a proportional font, without line numbers
  and without word wrap (XM-40). TEXT-1 differs on purpose. The Qt XML Atlas has a fixed-width
  font and line numbers (QT-12).
- Moving the cursor in XML Marker moves nothing else; tree and grid follow on Ctrl+L (XM-41,
  XM-42). TEXT-7 follows on every click.
- XML Marker re-parses on every keystroke and the tree comes back collapsed with nothing
  selected (XM-44). The Qt XML Atlas does the same and comes back fully expanded (QT-8). That
  cannot work at 100 MiB, where indexing alone takes 0.3 s; TEXT-9 says "shortly after" and
  must keep selection and expansion.
- While the text is broken, XML Marker shows an error list and either a complete tree or an
  empty one, depending on the error (XM-11, XM-12). TEXT-10 asks for better.
- XML Marker highlights the selected element's exact extent, from `<` to `>`, with an outline;
  a click in the grid adds a second highlight inside it (XM-22, XM-37). TEXT-6 tints whole
  lines and has one highlight.
- Find and Replace in XML Marker: whole word, match case, up or down, replace in selection or
  whole file, replace all. No regular expressions, no search in tree or grid (XM-46). The Qt
  XML Atlas finds plain text only (QT-9).
- XML Marker's text menu has structural commands: Insert Tag, Insert Attribute, Comment Out
  Selection, Mark Selection With Tag (XM-45).

## Decisions needed

1. Highlight the exact extent of the element instead of whole lines? It matters for files
   with several elements on one line, and for one-line files it is the only useful form.
2. A second highlight for the item clicked in the grid, apart from the selected element?
3. Find: start with XML Marker's set (case, whole word, direction), and add what it lacks
   (regular expressions, search by element name or attribute value) later?
4. Which of the structural commands of XM-45 belong in 0.2 or 0.3?

## Questions

1. Which text editing features do you use every day, and which could wait?
2. Do you want word wrap? XML Marker has none; a file that is one long line is hard to read
   in it.
