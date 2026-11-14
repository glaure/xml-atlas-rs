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
- **OPEN-5** `[built]` One document is open at a time. Opening another replaces it.
- **OPEN-6** `[built]` If the file cannot be read, the status bar says why and the document
  that was open stays open.
- **OPEN-7** `[done]` UTF-8 files open with or without a byte order mark.
- **OPEN-8** `[gap]` Files in other encodings open correctly. The encoding comes from the byte
  order mark, then from the XML declaration. Today a Latin-1 or UTF-16 file stops at the first
  byte that is not UTF-8 and shows an empty tree.
- **OPEN-9** `[built]` A file that is not well-formed still opens. Everything before the first
  error is shown. The status bar shows the error; clicking it goes to that line.
- **OPEN-10** `[gap]` Elements after the first error are shown too, as far as they can be
  recovered. Today nothing after the error appears in the tree or grid.
- **OPEN-11** `[planned]` Opening another file, or closing the window, asks first if there are
  unsaved changes.
- **OPEN-12** `[planned]` A list of recently opened files.

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

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker and the Qt XML Atlas both hold one document in one window (XM-5, QT-1). OPEN-5
  matches.
- XML Marker opens a file that is not well-formed without a dialog and switches to an error
  list with warnings and errors; each error is marked in the text (XM-10). For an unclosed tag
  its tree is empty (XM-11); for a stray `<` in a start tag the tree stayed complete (XM-12).
  OPEN-9 already shows more than XML Marker in the first case; OPEN-10 asks for what it does
  in the second.
- XML Marker shows UTF-8 files with wrong characters and the byte order mark as stray
  characters; Latin-1 is right; UTF-16 opens but loses characters outside the code page (XM-6
  to XM-8, under wine). OPEN-7 and OPEN-8 ask for more than XML Marker does.
- XML Marker starts with the root collapsed and nothing selected (XM-9). OPEN-3 differs on
  purpose: a file you just opened should show something.
- XML Marker marks unsaved changes with `*` in the title and asks before closing (XM-13).
  OPEN-11 matches. The Qt XML Atlas does not ask (QT-11).
- XML Marker's status bar shows only the cursor position (XM-4). OPEN-4 shows more.

## Decisions needed

1. Errors: keep the single message in the status bar (OPEN-9), or add an error list like XML
   Marker's, with warnings and errors and a marker in the text?
2. Do you want several documents at once (tabs or windows)? Neither reference program has it.

## Questions

Only you can answer these.

1. Which encodings do your real files use? Are any of them UTF-16?
2. How large are the files you work with, typically and at most?
3. What should happen when the file changes on disk while it is open? Not observed in XML
   Marker yet.
