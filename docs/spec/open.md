# Opening files

## Rules

- **OPEN-1** `[built]` A file opens in three ways: as a program argument, with File > Open
  (Ctrl+O), or by dropping it on the window. Each way takes several files at once.
- **OPEN-2** `[built]` Loading happens in the background. The window stays responsive and the
  status bar shows "Opening" with the path.
- **OPEN-3** `[built]` When loading ends, the root element is selected: expanded in the tree,
  its children in the grid, its first line in the text view.
- **OPEN-4** `[built]` The status bar shows the file name, size, line count, element count and
  how long opening took.
- **OPEN-5** `[done]` Several documents can be open at once, as tabs in one window, above the
  views. Each file opened gets a new tab, which is shown; a file whose tab is already open
  is shown again, not loaded twice. Clicking a tab, Ctrl+Tab and Ctrl+Page Down (next),
  Ctrl+Shift+Tab and Ctrl+Page Up (previous) switch tabs; after the last comes the first. A
  tab shows the file name, with `*` when it has unsaved changes, and the path on hover. Its
  ×, a middle click, File > Close or Ctrl+W close it (OPEN-11); then the tab after it is
  shown, or the one before when it was the last. Each tab keeps its own selection, expanded
  rows, scroll positions, grid sort and undo history. The window title names the file of the
  tab shown. View settings apply to every tab.
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
- **OPEN-11** `[done]` Closing a tab (OPEN-5), File > Quit and the window's close button ask
  first if there are unsaved changes: Save, Don't save or Cancel. Quitting shows each tab
  with unsaved changes in turn and asks about it. The action goes ahead after Don't save, or
  after Save has written the file; if saving fails or is cancelled, nothing is lost, the
  document stays open, and quitting stops. Without unsaved changes nothing is asked. Opening a
  file asks nothing: it gets its own tab.
- **OPEN-12** `[done]` File > Open Recent lists the last 10 files opened or saved, newest
  first, as name and folder; the full path shows on hover. Opening one again moves it to the
  top. A listed file that no longer exists is reported and leaves the list. Clear list empties
  it. A file from the list opens in a tab like any other (OPEN-5). The list
  is kept with the other settings (WIN-3); a path that is not valid UTF-8 is not remembered.
- **OPEN-13** `[done]` A list below the text shows every warning and error of the document,
  as `Line N, column C: message`; it is there only when there are any. Clicking an entry
  selects the element at that place and outlines it. Each problem is also marked in the text:
  a sign left of the line number, a wavy line from the place to the end of the line, and the
  message when the pointer rests on the line. The status bar shows the first error; clicking
  it does the same as clicking the entry. Today the list holds the first parse error (see
  OPEN-10) and these encoding problems: a UTF-16 byte order mark is an error, and a declared
  encoding other than UTF-8 is a warning that says the text is shown as UTF-8 (see OPEN-8).
- **OPEN-14** `[done]` When the file of an open document is changed, deleted or moved on disk
  by another program, a notice above the views says so within about a second, and the tab's
  name turns to the warning colour. The notice offers Reload (not when the file is gone;
  "Reload, discarding changes" when there are unsaved ones), Save, Save as… and Keep. Reload
  reads the file in the background; the selection and the expanded rows stay where they were
  as far as the elements are still there, and the undo history starts afresh. Save writes the
  open document over the file; Save as… asks for another name, which the tab then keeps. Keep
  leaves the file alone and counts the document as modified, so Save is offered and closing
  asks (OPEN-11); the notice comes back only on the next change. Saving from XML Atlas itself
  is no change. File > Save As… (Ctrl+Shift+S) is in the menu too. Files are looked at once a
  second on a thread of their own, and the window is woken only when one changed. XML Marker
  does not notice (XM-14).

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
- Open two files, delete the first, choose it from File > Open Recent: an error says it is
  gone and removed from the list; the other document stays open.
- Format a document, then close the window: the dialog asks. Cancel keeps it open; Don't save
  closes and leaves the file as it was; Save writes the formatted file, then closes.
- Open three files, the second with a non-ASCII name: three tabs, the last shown. Scroll the
  tree in one, switch tabs and back: it is where it was. Change two of them and quit: each is
  shown and asked about in turn; Don't save, then Save writes only the second, keeping its
  byte order mark and CRLF line endings.
- Open a file that is open already: its tab is shown, and no second tab appears.
- Open a UTF-8 file with a byte order mark, CRLF line endings and a non-ASCII name; change it
  in another program: the notice appears; Reload shows the new contents. Delete it: the
  notice says it is gone and has no Reload; Save writes it back byte for byte. Change it
  again and Keep: the file stays as the other program wrote it, the tab shows `*`, and Ctrl+S
  writes the open version.
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
- Several documents at once (OPEN-5), as tabs in one window. (Owner, 2026-11-15.)
- A changed file on disk raises a notice (OPEN-14).
- The owner's files are UTF-8 and about 10 MiB, growing. OPEN-8 (other encodings) matters less
  than OPEN-7 for them.

## Questions

1. "Import other XML files" is wanted later. What should it do: insert another file's
   elements into this document, follow XInclude references, or something else?
