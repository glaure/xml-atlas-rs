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

## Questions

1. When you move the cursor in XML Marker's text, do tree and grid follow at once, or only on
   request?
2. When you type, when do tree and grid update: on every key, after a pause, on save?
3. What do tree and grid show while the text is broken in the middle of an edit?
4. Is there word wrap? How does it treat a file that is one very long line?
5. What do Find and Replace offer (case, whole word, regular expressions, search in the
   tree or grid)?
6. Which text editing features do you use every day, and which could wait?
