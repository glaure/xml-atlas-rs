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
- **OPEN-8** `[done]` Files in other encodings open correctly, and are saved in their own
  encoding. The encoding comes from the byte order mark, then from the XML declaration; a
  UTF-16 file without a byte order mark is told by its first `<?`. Opening converts the file
  to UTF-8, saving converts it back (`docs/decisions.md`, D23):
  - Opening and saving without a change gives the file back byte for byte, its byte order
    mark, line endings and declaration included. UTF-8 files are never converted.
  - Every encoding of the Encoding Standard is read: UTF-16, the ISO-8859 and Windows code
    pages, KOI8, Shift_JIS, EUC, GBK, Big5 and more. ISO-8859-1 and Latin-1 are read as
    Windows-1252, as web browsers do: the same characters, and in 0x80 to 0x9F the ones
    Windows puts there.
  - When a file does not convert back to the same bytes (a UTF-16 file with an unpaired
    surrogate, a byte its encoding does not define), the problems list says so where the
    first such byte shows as `�`, and the document can be saved only as UTF-8.
  - A character the encoding cannot hold, such as `日` typed into a Latin-1 file, makes Save
    refuse: a dialog names the character and its line and column, writes nothing, and offers
    Show it, Save as UTF-8 and Cancel (the owner's choice, 2026-11-15). Characters are never
    written as `?` or `&#N;`.
  - File > Save as UTF-8, there for a file that is not UTF-8, saves the document in UTF-8
    from then on, without a byte order mark, and changes the declaration's `encoding` to
    `UTF-8` as an edit that can be undone.
  - The status bar shows the encoding of a file that is not UTF-8; `xml-atlas-cli stats`
    prints it, and the CLI's commands write their output in it.
  Measured (release build, 100 MiB fixture written in each encoding): UTF-8 377 ms as before;
  Latin-1 553 ms, of which converting 157 ms; UTF-16 632 ms, converting 190 ms.
- **OPEN-9** `[done]` A file that is not well-formed still opens. Everything before the first
  error is shown. The error is listed and marked where it is (OPEN-13), also when the cause is
  a byte that is not UTF-8.
- **OPEN-10** `[done]` Elements after the first error are shown too, as far as they can be
  recovered (D14's rule):
  - An end tag closes the elements left open inside the element it matches; each of them is
    an error where its start tag is. `<a><b>one</b><c>two</a>` is `a` with `b` and `c`.
  - An end tag that matches no open element is an error and is skipped.
  - Elements still open at the end of the file end there, one error each.
  - A byte that is not UTF-8 is an error; reading goes on from the next `<`. In a Latin-1 file
    this can skip a start tag, and its end tag is a second error. Files whose declaration
    names their encoding are read right (OPEN-8); this happens only to a file in another
    encoding that does not say so.
  - Markup that is never closed, such as `<!--` or an open quote in a start tag, runs to the
    end of the file: nothing after it can be recovered.
  At most 1,000 errors are kept; reading goes on after the last. Reading the 100 MiB fixture
  takes as long as before (within 1 %).
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
  it does the same as clicking the entry. The list holds every parse error, up to 1,000
  (OPEN-10), and these encoding warnings (OPEN-8): a declared encoding that is unknown, or
  UTF-16 in a file that is not, and so read as UTF-8; bytes that do not convert back; a
  declaration that names another encoding than the UTF-8 the document is saved in (after
  Save as UTF-8 was undone).
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
- **OPEN-21** `[done]` A small file is on screen no later than XML Marker shows it. A tiny file
  is ready in 0.30 to 0.33 s with 132 MiB, against XML Marker's 0.49 s and 59 MiB under wine
  (`docs/benchmarks.md`); it took 0.55 to 0.60 s and 195 MiB before the OpenGL renderer
  (`docs/decisions.md`, D21).

## Acceptance tests

- Open each corpus file (byte order mark, CRLF, Latin-1, UTF-16, one huge line, deep nesting):
  the tree shows the root element with its real name and the text view shows readable text.
- Open `latin1.xml` and `utf16le_bom.xml` from the corpus and save them unchanged: both files
  are byte for byte as before. Type `日` into the Latin-1 one and save: the dialog names line
  and column; Save as UTF-8 writes UTF-8 with `encoding="UTF-8"`.
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
  OPEN-9 already shows more than XML Marker in the first case; OPEN-10 does what it does
  in the second, and shows the whole tree in the first too. OPEN-13 follows its error list.
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
  than OPEN-7 for them, but is in 1.0, so that other people's files open. (Owner, 2026-11-15.)
- Other encodings are converted to UTF-8 on open and back on save. A character the encoding
  cannot hold refuses the save and offers Save as UTF-8, rather than writing `&#N;`. (Owner,
  2026-11-15.)

## Questions

1. "Import other XML files" is wanted later. What should it do: insert another file's
   elements into this document, follow XInclude references, or something else?
