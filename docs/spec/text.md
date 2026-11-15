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
- **TEXT-8** `[gap]` Every line of a very large file can be reached by scrolling, and every
  wheel notch moves the view by the same amount. Far down very large files, some notches are
  lost: one in six near the end of the 300 MiB fixture (`docs/decisions.md`, D16). Every line
  can still be reached with Find, a click or the keyboard.
- **TEXT-9** `[planned]` The text can be edited: cursor, selection, clipboard, undo. Tree and
  grid follow shortly after each change. TEXT-16 to TEXT-22 say what that means; the design
  is `docs/decisions.md`, D19.
- **TEXT-10** `[done]` While the text is not well-formed, tree and grid keep showing the
  last good state for the broken part, and mark it: the broken element's tree row and its
  grid row are drawn in the error colour, and the problems list (OPEN-13) says where the
  error is. Structured edits stay refused until the text is well-formed again (EDIT-5 to
  EDIT-10). Typing on in the broken part works as anywhere else. The broken element is the
  innermost one whose `<` and end tag the typing left alone. It keeps its children, except
  those whose own bytes were changed (a child whose `<` was deleted goes until the text is
  read again); its own text shows as it is now. A file that was not well-formed when opened
  has no good state to keep: after the pause it shows what opening it would show.
- **TEXT-16** `[done]` Cursor and keys. A click puts the cursor where it was clicked, and
  still selects the element there (TEXT-7, TREE-12). Left and Right move by one character,
  Ctrl+Left and Ctrl+Right by one word; Up and Down by one line, keeping the column they
  started from; Home goes to the first non-blank character of the line, again to the start;
  End to the end of the line; Ctrl+Home and Ctrl+End to the start and end of the document;
  Page Up and Page Down by what fits on screen. Shift with any of them extends the selection.
  The view scrolls to keep the cursor visible. Moving the cursor with the keys does not move
  tree or grid (TREE-12).
- **TEXT-17** `[done]` Selection and typing. Dragging selects; Shift+click extends;
  double-click selects a word, triple-click the line; Ctrl+A, or Edit > Select All, selects
  all. Typing replaces the selection, or inserts at the cursor. Backspace and Delete remove the selection, or one
  character; Ctrl+Backspace and Ctrl+Delete one word. Enter inserts the file's line ending
  (CRLF in a CRLF file, as EDIT-5), followed by the indentation of the line it was pressed on.
  Tab inserts one indent as the Format options (FMT-10, FMT-15) give it: spaces, or a tab
  only when the options say to indent with tabs.
- **TEXT-18** `[done]` Clipboard: Edit > Cut, Copy and Paste, also Ctrl+X, Ctrl+C and
  Ctrl+V. Copy takes the selected text as UTF-8, with the file's line endings; a byte that is
  not valid UTF-8 goes to the clipboard as `�`, as the view shows it. Paste inserts the
  clipboard text with each line break written as the file's line ending, the one the file
  had last even when the text holds no line break any more (Select All, Cut, Paste gives the
  file back). Cut and Paste are one undo step each, never joined with typing (TEXT-20).
- **TEXT-19** `[done]` Input methods: dead keys, compose sequences and input method
  editors (Chinese, Japanese, Korean) work. While composing, the text being composed is shown
  underlined at the cursor, the part being worked on underlined more heavily; only what is
  committed goes into the document, as typing (TEXT-20). A click in the text, or leaving the
  text view, ends the composition without committing it. The input method's window opens at
  the cursor.
- **TEXT-20** `[done]` Undo while typing goes word by word (EDIT-1): a word typed after a
  space or punctuation starts a new undo step, so `Grüße, Welt` is undone as `Welt`, then
  `Grüße, `. A run of Backspace or Delete is one step. A step also ends when the cursor is
  moved by a click or a key, when typing turns into deleting or back, after a pause of one
  second, when the document is saved, and before any other command (Format, Replace, a
  structured edit). Undo and redo put the cursor where the change was.
- **TEXT-21** `[done]` Bytes are kept. Only the bytes typed, deleted or pasted change
  (`AGENTS.md`, rule 7). The cursor is never inside a UTF-8 character, never between the CR
  and LF of a CRLF line ending, and never before a byte order mark. A byte that is not valid
  UTF-8 is shown as `�`, counts as one character, and stays as it was unless it is deleted.
- **TEXT-22** `[done]` Speed while typing. Each key shows its change in the text at once,
  on the owner's file sizes (about 10 MiB) within one frame. Tree and grid catch up at once
  when the change stays inside one well-formed element; when it breaks one, they keep the
  last good state (TEXT-10) and the whole document is read again after a pause in typing of
  half a second, off the UI thread above 20 MiB. Tree and grid follow a key by the nodes it
  replaced, without being built again, so a key costs what it changes, not the size of the
  file. Measured in the app (release build, keys near the start of the file, the worst
  case): 1.4 to 2.3 ms on 10 MiB and 17 to 25 ms on 100 MiB, whether the key breaks an
  element or not. On 100 MiB that is moving the text after the key in memory (D10) and the
  index (7 to 13 ms); tree and grid add nothing measurable. A sorted grid is not sorted
  again while typing, so a row does not jump away under the cursor. Still open: copying
  the text for the worker thread costs 54 ms on 100 MiB, once per pause, on the UI thread.
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
- **TEXT-14** `[done]` Replace and Replace All (XM-46): Ctrl+H, or Edit > Replace, opens the
  Find bar with a Replace row; the same options as Find. Replace replaces the outlined match
  and finds the next; when nothing is outlined, or the outlined text is no longer a match, it
  only finds, so the first press shows what the next replaces. Enter in the replacement field
  is Replace. Replace All replaces every match in the whole document, or with *In
  selection* only inside the text selected in the text view or, when none is, inside the
  element selected in tree or grid (its tags included); the text selection then covers the
  replaced text. Like Find, Replace
  works on the whole source, tags and attribute names too, and inserts the replacement as
  typed (the owner's choice, 2026-11-15). When that leaves the document not well-formed, the
  status bar says so, and the problems list shows where. Replace All is one undo step. The
  CLI has it too: `xml-atlas-cli replace FILE TEXT WITH [--match-case] [--whole-word]
  [--within PATH] [-o OUT]`, which refuses a result that is not well-formed.
- **TEXT-15** `[planned]` Regular expressions, and search by element name or attribute value.
- **TEXT-13** `[planned]` Word wrap can be turned on.

## Acceptance tests

- Open a file whose comments contain umlauts and CJK characters: no crash, text is intact.
- Click line 1,000,000 of the 100 MiB fixture: the tree shows the element on that line.
- In a CRLF file with a byte order mark, Ctrl+H, find `total`, replace with `sum`: Enter
  outlines the first `total`, Enter again replaces it and the status bar warns that the
  document is no longer well-formed. Replace All replaces the other five; one Ctrl+Z undoes
  them, a second the first, and the file is byte for byte as before.
- Select one `order`, tick *In selected element*, Replace All: only that order's tags change.
- Open `crates/engine/tests/corpus/one_line.xml` and click row `b` in the grid: only
  `<b><c/><c/></b>` is outlined, inside the root's highlight.
- Click an attribute row in the tree: only that attribute's `name="value"` is outlined.
- In the 100 MiB fixture, find `Customer 49857`: the bar says 4 matches. Enter goes to line 6
  and selects that `name`; Shift+F3 then continues from the bottom to line 2,060,923.
- Find `söhne` in `crates/engine/tests/corpus/utf8_bom_crlf.xml`: one match, `Söhne`.
- In `utf8_bom_crlf.xml`, put the cursor after `Söhne`, press Backspace twice and type `ne`:
  the text reads `Söhne` again, the file saves byte for byte as before, and one Ctrl+Z undoes
  the typing, a second the deleting.
- In the same file, Ctrl+Home, then Left: the cursor stays before the first `<`, after the
  byte order mark. End on a line, then Right: the cursor goes to the start of the next line in
  one step, over CR and LF.
- In the 100 MiB fixture, type `<` inside a customer's name: the letter appears at once, that
  order's tree row turns to the error colour, and the problems list names the line. Delete
  the `<`: within about a second tree and grid are as before, with the same row selected.
- Type `ü` with a dead key and `日本` with an input method: both arrive as typed.

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
- Daily editing is plain: type, cut, copy, paste. The structural commands of XM-45 are not
  needed.
- Cut, copy and paste are those of the text view, as in XML Marker; copying whole elements in
  tree and grid may come later. (Owner, 2026-11-15.)
- Enter keeps the line's indentation; Tab inserts one indent from the Format options, not a
  tab character. (Owner, 2026-11-15.)
- The text is editable as soon as a file opens; there is no read-only mode. (Owner,
  2026-11-15.)
- Undo goes word by word while typing (TEXT-20), as code editors do. (Owner, 2026-11-15.)
