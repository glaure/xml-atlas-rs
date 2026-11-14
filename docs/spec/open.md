# Opening files

## Rules

- **OPEN-1** `[built]` A file opens in three ways: as the program's first argument, with
  File > Open (Ctrl+O), or by dropping it on the window.
- **OPEN-2** `[built]` Loading happens in the background. The window stays responsive and the
  status bar shows "Opening" with the path.
- **OPEN-3** `[built]` When loading ends, the root element is selected: expanded in the tree,
  its children in the grid, its first line in the text view.
- **OPEN-4** `[built]` The status bar shows the file name, size, line count, element count and
  how long opening took.
- **OPEN-5** `[gap]` Several documents can be open at once. Today one document is open at a
  time and opening another replaces it.
- **OPEN-6** `[built]` If the file cannot be read, the status bar says why and the document
  that was open stays open.
- **OPEN-7** `[done]` UTF-8 files open with or without a byte order mark.
- **OPEN-8** `[gap]` Files in other encodings open correctly. The encoding comes from the byte
  order mark, then from the XML declaration. Today a Latin-1 or UTF-16 file stops at the first
  byte that is not UTF-8 and shows an empty tree.
- **OPEN-9** `[done]` A file that is not well-formed still opens. Everything before the first
  error is shown. The error is listed and marked where it is (OPEN-13), also when the cause is
  a byte that is not UTF-8.
- **OPEN-10** `[gap]` Elements after the first error are shown too, as far as they can be
  recovered. Today nothing after the error appears in the tree or grid.
- **OPEN-11** `[planned]` Opening another file, or closing the window, asks first if there are
  unsaved changes.
- **OPEN-12** `[planned]` A list of recently opened files.
- **OPEN-13** `[done]` A list below the text shows every warning and error of the document,
  as `Line N, column C: message`; it is there only when there are any. Clicking an entry
  selects the element at that place and outlines it. Each problem is also marked in the text:
  a sign left of the line number, a wavy line from the place to the end of the line, and the
  message when the pointer rests on the line. The status bar shows the first error; clicking
  it does the same as clicking the entry. Today the list holds the first parse error (see
  OPEN-10) and these encoding problems: a UTF-16 byte order mark is an error, and a declared
  encoding other than UTF-8 is a warning that says the text is shown as UTF-8 (see OPEN-8).
- **OPEN-14** `[planned]` When the open file changes on disk, a notice offers to reload it or
  to save the open document, also under another name. XML Marker does not notice (XM-14).

## Speed targets

Measured from starting the program until the file is loaded (`docs/benchmarks.md`).

- **OPEN-20** `[done]` 100 MiB in about 1 s; 300 MiB in about 2 s.
- **OPEN-21** `[gap]` A small file is on screen no later than XML Marker shows it. Today
  start-up takes 0.59 s against XML Marker's 0.49 s under wine.

## Acceptance tests

- Open each corpus file (byte order mark, CRLF, Latin-1, UTF-16, one huge line, deep nesting):
  the tree shows the root element with its real name and the text view shows readable text.
- Open `<a><b>one</b><c>two</a>`: the tree shows `a`, `b` and `c`; the status bar names the
  mismatched tag; clicking the error goes to line 1.
- Open a path that does not exist while a document is open: the document stays.
- Open `crates/engine/tests/corpus/latin1.xml`: the list shows a warning on line 1 at
  `ISO-8859-1` and an error on line 2, column 10, at the `Ä`. Clicking the error outlines that
  byte.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker and the Qt XML Atlas both hold one document in one window (XM-5, QT-1). OPEN-5
  differs on purpose.
- XML Marker opens a file that is not well-formed without a dialog and switches to an error
  list with warnings and errors; each error is marked in the text (XM-10). For an unclosed tag
  its tree is empty (XM-11); for a stray `<` in a start tag the tree stayed complete (XM-12).
  OPEN-9 already shows more than XML Marker in the first case; OPEN-10 asks for what it does
  in the second. OPEN-13 follows its error list.
- XML Marker shows UTF-8 files with wrong characters and the byte order mark as stray
  characters; Latin-1 is right; UTF-16 opens but loses characters outside the code page (XM-6
  to XM-8, under wine). OPEN-7 and OPEN-8 ask for more than XML Marker does.
- XML Marker starts with the root collapsed and nothing selected (XM-9). OPEN-3 differs on
  purpose: a file you just opened should show something.
- XML Marker marks unsaved changes with `*` in the title and asks before closing (XM-13).
  OPEN-11 matches. The Qt XML Atlas does not ask (QT-11).
- XML Marker's status bar shows only the cursor position (XM-4). OPEN-4 shows more.

## Decided

- Errors: an error list and markers in the text (OPEN-13), not only the status bar message.
- Several documents at once (OPEN-5).
- A changed file on disk raises a notice (OPEN-14).
- The owner's files are UTF-8 and about 10 MiB, growing. OPEN-8 (other encodings) matters less
  than OPEN-7 for them.

## Questions

1. Several documents: tabs in one window, or one window each?
2. "Import other XML files" is wanted later. What should it do: insert another file's
   elements into this document, follow XInclude references, or something else?
