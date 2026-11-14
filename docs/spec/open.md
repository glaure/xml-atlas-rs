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

## Questions

1. Does XML Marker open several files at once (tabs or windows), or one at a time? Do you
   want several in XML Atlas?
2. What does XML Marker show for a file that is not well-formed: an error dialog, a partial
   tree, nothing?
3. What does it do when the file changes on disk while it is open?
4. Which encodings do your real files use? Are any of them UTF-16?
5. How large are the files you work with, typically and at most?
