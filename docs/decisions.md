# Decisions

Why XML Atlas is built the way it is. Each entry gives the decision, the reason, and what would
make us revisit it. Add new entries at the end; mark replaced ones as superseded rather than
deleting them.

## Context

XML Atlas began as a Qt 6 / C++ app (github.com/glaure/xml-atlas, about 1,800 lines on pugixml),
modelled on XML Marker 1.1, a small, very fast, free Windows XML editor (symbolclick.com).
Many users run XML Marker on Linux under wine. The Qt version never matched its speed. The
likely causes, not profiled:

- conversions between QString (UTF-16) and UTF-8;
- the whole `QAbstractItemModel` rebuilt from a full DOM on every content change;
- `QSyntaxHighlighter` and `QPlainTextEdit` slowing down on large files.

The rewrite follows the conventions of the ArtCraft "Craft" apps (github.com/storytold):
pure Rust, egui, a Cargo workspace that separates logic crates from the UI, CLI and MCP front
ends, MIT OR Apache-2.0, and a clean-room spec. XML Atlas is **not** a Craft app and does not
use the Craft name, CI or branding.

**Goal:** open, navigate and edit large XML files (hundreds of MiB) at least as fast as XML
Marker, on Windows, Linux and macOS, natively and without wine.

## D1: Pure Rust, egui 0.37 + eframe

- **Why:** the Craft apps show egui working for large tools. Because it is immediate mode,
  virtualized views are simple: we paint the rows on screen and nothing else. One codebase runs
  on Windows, Linux and macOS, and WebAssembly comes almost for free. There is no web view.
- **Rejected:** Qt bindings (they keep the problems we are leaving); Tauri or a web UI (needs a
  web view, and large text is slow in the DOM); GPUI (built for fast editors, but few docs and
  an unstable API); Slint and Iced (weaker virtualized tables).
- **Risk:** egui's built-in `TextEdit` does not scale to large files. The editable text view
  (version 0.2) will be our own widget over a rope. Revisit if that widget costs more than
  switching to GPUI would.

## D2: A flat, span-based index instead of a DOM

- **What:** `crates/xml` makes one pass with quick-xml and writes one fixed-size `Node` (48
  bytes) per node into a single `Vec`. Links (parent, first child, next sibling) are `u32` ids.
  (Since D11, 32 bytes: a parent id and a subtree size, from which the other links follow.)
  Names, attributes and text are byte ranges into the source, decoded only when a view shows
  them. Whitespace-only text is skipped. Text split around entity references is merged back
  into one node.
- **Why:** opening should cost about one tokenizer pass, with no allocation per node. Memory
  is predictable: about 48 B × the number of nodes.
- **Measured** (2026-11-14, 100 MiB generated file, 3.4 M nodes, release build, Linux):
  - opening takes about 0.40 s: read 56 ms + index 332 ms + line index 16 ms;
  - the index runs at about 300 MiB/s;
  - peak memory is about 280 MB.
- **Open question (biggest risk):** editing. A span index is cheap to build but has to be
  updated when text changes. The plan for 0.2:
  1. Find the smallest element containing the edit.
  2. Re-index only that element's span.
  3. Splice the new nodes in, and shift the offsets after the edit by the size change.

  Shifting offsets is O(n) per edit. Alternatives: store spans relative to the parent, use a
  piece table, or keep the index in chunks. **Resolved by D9, D10 and D11.**
- **Revisit when:** the index costs more than reading the file, or memory per node becomes the
  limit. A SIMD scanner (like simdjson) needs `unsafe`, which is currently forbidden
  workspace-wide.

## D3: Malformed files open up to the error

- **Why:** people open broken XML to fix it. The parser keeps every node read before the error,
  closes the elements still open at the error position, and reports the byte offset. The status
  bar links to that line.
- **Limit:** everything after the first error is missing from the tree and grid. That is
  acceptable for a viewer. It is not for an editor, where the text is malformed during most
  keystrokes. See D9 and D14.

## D4: The engine owns every command

- **What:** `crates/engine` holds `Document` (source, index, line starts, timings) and the
  operations (open, stats, format). The GUI, `xml-atlas-cli` and the planned MCP server and JSON
  control channel only call them.
- **Why:** one implementation and one set of tests. The CLI is also the benchmark harness.

## D5: The views are virtualized and painted by hand

- **Tree:** a flat list of visible element rows, rebuilt only when expansion changes. Rows are
  painted directly (no widget per row). Selecting a node expands its ancestors and scrolls to it.
- **Grid:** like XML Marker. The child elements of the selection are rows. The columns are the
  union of attributes, leaf child elements and `text()`, taken from the first 1,000 rows. Cells
  are decoded only for visible rows. A click shows the row in the text; a double-click makes it
  the selection.
- **Text:** a line index built with `memchr`. Only visible lines are laid out. The highlighter
  works line by line, so constructs spanning lines are coloured wrongly (known limitation).
  Lines are cut at 4,000 bytes for display.
- **Risk:** minified files that are one huge line. They need wrapping or horizontal paging.

## D6: Licence MIT OR Apache-2.0

- **Why:** the owner prefers it, and it matches the Rust ecosystem and the Craft apps. The C++
  project stays GPL-3. The owner holds the copyright on both, so ideas can move between them.

## D7: Clean room

- **Rule:** build only from the XML specification, the Qt XML Atlas (our own code) and observed
  behaviour of XML Marker, written down in `docs/spec/`. Running XML Marker to observe it and
  to time it is allowed. Never copy, disassemble or decompile it, and never use its code, icons,
  screenshots or other assets. (Wording clarified 2026-11-14: the earlier text forbade
  "binaries" outright, which would also have ruled out the benchmark.)

## D8: Name and repository

- **What:** keep the "XML Atlas" name, because it already has users and release downloads. The
  new repository is `xml-atlas-rs`. The C++ repository will be archived when the Rust version
  replaces it.
- **Why not a Craft name:** the project is not affiliated with ArtCraft, and a "-Craft" name
  would suggest it is.

## Review, 2026-11-14

A second review read the plan and the code and ran experiments on the 100 MiB fixture. It
confirmed four bugs, which are fixed (D12, D13 and the notes below), and answered the open
questions (D9 to D11). Decisions D14 to D16 came out of the same review.

Fixed, each with a test:

- **Crash on non-ASCII text in a comment.** The highlighter sliced a line in the middle of a
  character. It now compares bytes for lookahead.
- **Wrong offsets in files with a UTF-8 byte order mark.** quick-xml drops the mark without
  counting it, so every offset was 3 bytes low. The index now adds it back.
- **The grid computed the selection's path on every frame.** `Index::path` walks siblings: 24 ms
  per call under a parent with 211,000 children. The grid now computes it once per selection.

Still open, found by the same review:

- Files that are not UTF-8 (Latin-1, UTF-16) stop at the first non-UTF-8 byte and show an empty
  tree. See D14.
- The grid takes its columns from the first 1,000 rows. An attribute that first appears later
  is not shown.
- A start tag with malformed attributes (`<a x=1 y="2">`) shows only the well-formed ones, with
  no warning.

## D9: The index is updated off the UI thread; the flat `Vec` stays

- **What:** typing changes only the text buffer and the line index. The node index catches up
  afterwards on a worker thread:
  1. Find the smallest element containing the edit.
  2. Re-index that element's source alone.
  3. If it is well-formed on its own, splice the new nodes in. If not, try its parent. If
     nothing works, keep the old nodes (with shifted offsets) and mark the region as stale.
- **Why:** measured on 3.4 M nodes (100 MiB), shifting all offsets takes 13 ms, inserting one
  node and fixing ids takes 26 ms, and shifting 2.3 M line starts takes 1.4 ms. That is fine in
  the background and too slow per keystroke at 1 GiB (about ten times as much). Taking the index
  out of the keystroke path removes the need for a cleverer structure now.
- **Must change first:** tree expansion is a `Vec<bool>` indexed by node id, and the
  selection, the grid and Find results hold node ids or byte offsets. Ids move on every
  re-index, so all of them need to be remapped: ids before the splice stay, ids after it
  shift by a constant.
- **Since D11 (2026-11-15):** links are subtree sizes, so a splice changes the sizes of the
  edited element's ancestors and shifts the parent of every node after it; byte offsets
  after it shift too. The source stays one contiguous `&[u8]` (D10), so the accessors need
  no source type; that item is dropped.
- **Revisit when:** background updates lag visibly at 1 GiB. Then store offsets and parents
  relative to the parent, so an edit touches only the ancestors and their following
  siblings.
- **Built (2026-11-15), on the UI thread for now:** `Index::update` re-indexes the innermost
  element that contains the change with a byte to spare on each side, and splices its nodes
  in when its new bytes are one well-formed element on their own. Otherwise (a broken
  element, a change in the prolog, a document that already has an error, an edit with
  several changes such as Format) the whole document is indexed, so the error is reported
  as on opening. The parent is not tried: when an element no longer parses on its own, the
  document is not well-formed either, and only the full pass finds where. The line index is
  updated in place. `max_depth` becomes an upper bound until the next full index.
  - Measured on the 100 MiB fixture: changing one character of a price 10 ms (238 ms
    before), its undo and redo 11 ms, inserting an element 17 ms; on 10 MiB 0.2 to 0.5 ms.
    Nearly all of it is shifting the nodes after the change.
  - Why not a worker thread yet: at these times it would add snapshots and a stale index to
    every view for no visible gain. It comes when 1 GiB files are edited (about 150 ms).
  - Checked: 6,200 random edits on the corpus, of which 1,349 took the fast path, each equal
    to a full index of the new source; random edits, all undone and all redone, each leave
    the node and line index equal to opening the new source.
- **Children only (2026-11-15):** when the change lies between the children of that element,
  only the children it touches are indexed again, from the end of the last child before it
  to the start of the first child after it. Without this, moving, deleting or duplicating a
  child of the root element read the whole document (move: 380 ms on 100 MiB, now 33 ms).
  A text or CDATA child next to the change, or a change that touches every child, still
  reads the whole element, because text may merge or be folded (D11). An edit with a few
  changes (rename: start and end tag) is spliced in place and reported as the span from the
  first change to the last, so it updates locally too.

## D10: One contiguous buffer, edited in place

- **What:** the document stays one `Vec<u8>`. An edit replaces a byte range with new bytes
  (`Vec::splice`); undo and redo keep, per edit, the offset, the removed bytes and the
  inserted bytes. This supersedes the roadmap's plan to use `ropey`, and the piece table this
  decision first chose (2026-11-14; changed 2026-11-15 with the owner's agreement).
- **Why:**
  - Measured (2026-11-15, release build): replacing a few bytes costs 0.08 ms in the middle of
    10 MiB, 3.9 ms in the middle of 100 MiB and 8.3 ms at its start (the worst case, every
    byte moves), 44 to 86 ms at 1 GiB. The owner's files are about 10 MiB. Re-indexing after
    the edit shifts every following offset anyway (13 ms at 100 MiB, D9), so a piece table
    would not remove the cost per edit, only part of it.
  - The piece table's other advantages do not hold here: the file is read into memory
    anyway (D11), so there is no zero-copy open to keep; and a splice also saves untouched
    bytes as they were, including the byte order mark, CRLF line endings and bytes that are
    not valid UTF-8.
  - The source is read as one `&[u8]` in 54 places in 13 files, and Find searches it with
    `memchr`. Both keep working unchanged; with pieces every one would need a source type, and
    Find would have to catch matches across a piece boundary.
  - The worker thread (D9) gets a copy of the edited element's bytes, not of the document.
- **Limit:** an edit that touches many places (Replace All, a rename across a million rows)
  must not be a million splices; it builds the new buffer in one pass, as Format does, and
  is undone as one step.
- **Revisit when:** a single edit lags visibly on the largest files people edit. Then a piece
  table or a gap buffer behind the same edit and undo API.
- **Built (2026-11-15):** `Edit` (byte ranges and their new bytes, one undo step) and
  `History` in `crates/engine/src/edit.rs`. A step stores the bytes it would put back, not
  the ones it removes, so the history holds each old text once; a whole-document change
  (Format) moves the old source into the history without copying it. Measured on the 100 MiB
  fixture: Format 214 ms, applying it 414 ms, undo 342 ms, redo 413 ms, a one-byte edit
  238 ms; nearly all of it is indexing the whole document again, which D9 replaces.

## D11: No memory-mapped open for now; shrink the index first

- **Why:** the index, not the source, is the memory limit. On the fixture the index is 1.6
  times the file size (157 MiB for 100 MiB). Memory mapping also needs `unsafe`, and a mapped
  file that another program truncates crashes the process.
- **What instead:**
  - Fold the single text child of a leaf element into its parent. On the fixture that is 1.06 M
    of 3.42 M nodes.
  - Shrink `Node` from 48 to about 32 bytes: nodes are stored in document order, so the first
    child of node `n` is `n + 1`, and `child_count` and `depth` can be computed when needed.
  - Do this before the splice code of D9 is written, because that code depends on the layout.
- **Done (2026-11-15): 32-byte nodes.** A node keeps its byte range, its parent, the number of
  nodes in its subtree, the start-tag length, a 16-bit name length and its kind. First child,
  next sibling, child count and depth are derived. An element name longer than 65,535 bytes
  stops the index with an error, like any other parse error (D3). On the 100 MiB fixture:
  - index memory (nodes and line starts) 181.8 MiB before, 129.5 MiB after;
  - CLI open 442-456 ms before, 357-366 ms after, over three alternating rounds; the index
    stage 374-386 ms before, 290-298 ms after. Building no sibling links is the gain, and it
    is far outside the layout noise of D18;
  - GUI, launch until loaded: 1.10-1.11 s best before, 1.06-1.07 s after; peak memory 505 MiB
    before, 453 MiB after.
- **Done (2026-11-15): folded text.** An element whose whole content is one text or CDATA
  section keeps it in the element (`Node::folded`, in the node's spare byte) instead of in a
  node of its own; mixed content, CDATA with whitespace around it and elements cut short by
  an error keep their text nodes. The tree shows folded text as a row like any other text.
  On the 100 MiB fixture:
  - nodes 3.42 M before, 2.35 M after (`stats` counts nodes, so its number drops);
  - index memory 129.5 MiB before, 96.6 MiB after;
  - CLI open 357-372 ms on both, over three alternating rounds: removing the node costs
    about what pushing it did;
  - GUI peak memory 452 MiB before, 420 MiB after; launch until loaded 1.05-1.09 s against
    1.06 s, within noise;
  - Format and Compact give the same bytes as before on the fixture and the corpus.
- **Kept open:** D10 keeps the original bytes behind one type, so a mapped file can replace
  the `Vec` later without touching the views.

## D12: Format changes only layout whitespace

- **What:** `format` puts the children of an element on their own lines only when all of them
  are elements, comments or processing instructions. Everything else is copied byte for byte:
  elements containing text or CDATA, anything under `xml:space="preserve"`, start tags,
  comments, the declaration, entity references, the byte order mark and the line-ending style.
  A document that is not well-formed is refused.
- **Why:** the first version used quick-xml's `trim_text` and changed content:
  `Customer 5 &amp; Sons` became `Customer 5&amp;Sons`, and mixed content lost its spaces.
- **Checked:** on the 100 MiB fixture the output equals the input apart from whitespace
  between tags, and formatting twice gives the same bytes. It takes 0.64 s.
- **Limit:** start tags are not rewrapped, and long text is not reflowed.
- **Exceptions (2026-11-14):** three settings may ask for more (`docs/spec/format.md`, FMT-11
  to FMT-13). Breaking a start tag with many attributes over lines changes only the whitespace
  between attributes; it is on by default, above six attributes, as in XML Marker. Removing
  comments and trimming text change content; they are off unless the user or the document's
  settings comment turns them on. With all three off, the rule above holds.

## D13: Saving is atomic

- **What:** `save_atomic` writes to a temporary file in the same directory, flushes it to disk
  and renames it over the target. A symbolic link is followed and the old file's permissions
  are kept.
- **Why:** a crash or a full disk during a plain overwrite of a large file destroys it.
- **Limit:** needs free space for a second copy. Ownership and extended attributes are not
  copied. Not yet tested on Windows or macOS.

## D14: Evaluate our own tokenizer (proposed, not decided)

- **Idea:** replace quick-xml in `crates/xml` with a scanner over raw bytes built on `memchr`
  (which is SIMD inside and safe to call, so the `unsafe` ban stays).
- **Expected gains, none measured yet:**
  - Speed: quick-xml 0.42 validates UTF-8 and builds an event per token.
  - Latin-1 and Windows-1252 files index without transcoding, because all markup is ASCII.
    UTF-16 still needs transcoding at load.
  - Error recovery: on a mismatched end tag, close elements up to the matching ancestor and
    carry on, collecting a list of errors instead of stopping at the first (D3, D9).
  - No hidden offset rules such as the byte order mark.
- **Decide by:** a prototype benchmarked against the current 300 MiB/s and checked against the
  corpus (D15). Keep quick-xml if the gain is small.

## D15: Order of work

- **What:**
  1. Baseline: time XML Marker (under wine) and the Qt XML Atlas on the same files. Without
     it the 0.4 s figure has nothing to be compared with, and the causes listed under
     "Context" remain guesses.
  2. Corpus: small real-world files as tests (byte order mark, CRLF, UTF-16, Latin-1, one
     huge line, deep nesting, namespaces, DOCTYPE). The synthetic fixture hid three of the
     bugs above.
  3. Spec: `docs/spec/` is still empty and the code came first. The owner writes it, from
     using XML Marker.
  4. Find and encodings, before any editing. A viewer for large files needs search.
  5. Editing in two steps. First the edit model in the engine (piece table, undo, index
     update), driven from grid cells, the tree and the CLI. Then the free-form text editor
     widget, which is the largest single piece of work: cursor, selection, clipboard, input
     methods.
- **Why two steps:** structured edits are local, which suits D9, and deliver most of the
  value for maintaining large files long before a full editor widget is done.

## D16: Large views need integer scrolling

- **Problem (calculated, not yet observed on screen):** egui positions are 32-bit floats. The
  100 MiB fixture is 35 million pixels tall, where a float can only step by 4 px. At 1 GiB the
  step is 32 px, so single lines cannot be reached.
- **What:** the text view, tree and grid keep the top row as an integer plus a pixel remainder
  and draw their own scroll bar, instead of `ScrollArea::show_rows` with a pixel offset.
- **When:** with the first of: the custom text widget, or a file where it is visible.
- **Observed (2026-11-14):** in the 300 MiB fixture (7.0 million lines, 119 million pixels),
  near the end, one mouse-wheel notch in six moved the text by nothing; the others moved it
  by 4 or 5 lines. Near the top of the 10 MiB fixture every notch moved a steady 80 px. egui
  animates a scroll in small steps per frame, and steps smaller than half the float spacing
  (8 px there) are lost. Drawing was not affected: rows stayed evenly spaced and highlights
  exact, and every line could still be reached with Find, a click, the keyboard or another
  notch. Files of the owner's size (about 10 MiB, 3.4 million pixels) are far below this.
- **Built for the text view (2026-11-15), as D19 planned:** `scroll::RowScroll` keeps the
  top line as an integer and the pixels of it out of sight; the wheel, Page Up and Down,
  Ctrl+End, Find and the scroll bars all go through it, and only the lines on screen are
  laid out. Checked on the 300 MiB fixture: 12 wheel notches up near the end moved exactly
  960 px (80 px each), 12 down came back; dragging the thumb reaches the first and the last
  line. Sideways stays a float of pixels: lines are at most a few thousand pixels wide.
- **Tree and grid keep egui's `ScrollArea`:** at 1.0's sizes they stay below 2^24 pixels,
  where a float still steps by a pixel (300 MiB: 633,556 grid rows, about 14 million px).
  They move to `RowScroll` when a file shows lost steps there.

## D17: The baseline changes the priorities

- **Measured** (2026-11-14, details in `docs/benchmarks.md`): launch until loaded, XML Marker
  under wine against XML Atlas.
  - 10 MiB: 1.11 s against 0.59 s.
  - 100 MiB: 10.7 s against 1.02 s.
  - 300 MiB: 58 s against 1.94 s.
  - Start-up with a tiny file: 0.49 s and 59 MiB against 0.59 s and 193 MiB.
- **What follows:**
  - Opening large files is already far ahead. More parser speed (D14) is not what users will
    feel; D14 now stands or falls on encodings and error recovery, not on speed.
  - Start-up is where XML Atlas loses, and most files people open are small. It is now on
    the roadmap.
  - "Fast" for XML Marker users probably means how it feels while working: instant start,
    instant selection and scrolling. That needs measuring before we claim to match it.

## D18: Open time moves with code layout

- **Seen:** in PRs #4, #8, #9 and #15, open time on the 100 MiB fixture moved by 20 to 60 ms
  although no open-path code changed: sometimes in the GUI, once in the CLI, sometimes in our
  favour. `perf` shows the same function sizes and nearly the same instructions, with more
  cycles: where the index loop lands in the binary decides how fast it runs.
- **Tried (2026-11-14):** `-C llvm-args=-align-loops=64`. It does not remove the effect, it
  moves it: with it, `main` opened in 440-450 ms and the branch in 403-431 ms; without it,
  411-433 and 441-454.
- **So:** compare open times over alternating rounds and report the spread; a difference that
  appears without a change to the open path is layout until shown otherwise. The fix belongs
  to the index loop itself (D14), or profile-guided optimization.

## D19: The text editor is our own view, and the index waits while the text is broken

Agreed with the owner (2026-11-15), with the answers in `docs/spec/text.md` ("Decided"):
undo word by word, Tab from the Format options, editable from the start. Rules: TEXT-9,
TEXT-10, TEXT-16 to TEXT-22 in `docs/spec/text.md`.

- **Measured first** (release build, one edit in the middle of the file, `Document::apply`
  with today's index update):

  | | 10 MiB | 100 MiB |
  |---|---|---|
  | type a letter in a text (local update, D9) | 0.6–0.7 ms | 15 ms |
  | type `<` there (breaks the element; read up to the error) | 16 ms | 166–169 ms |
  | delete the `<` again (whole document read) | 30–31 ms | 300–306 ms |
  | `Grid::build` of the root element (21,183 and 211,383 rows) | 1.7 ms | 16 ms |

- **What:**
  1. **Our own widget, grown from today's text view.** egui's `TextEdit` needs the whole
     document as one `String` and lays all of it out; at 100 MiB that is neither possible
     every frame nor true to bytes that are not UTF-8. The text view already lays out only the
     lines on screen (D5). It gains a cursor and a selection, and reads egui's input events
     itself: `Event::Text`, `Event::Key`, `Event::Copy`, `Event::Cut`, `Event::Paste` and
     `Event::Ime` (`ImeEvent::Preedit`, `ImeEvent::Commit`), and it tells egui where the
     cursor is through `PlatformOutput::ime` (`IMEOutput { rect, cursor_rect, .. }`), so input
     method windows open next to it. These names were checked in egui 0.37's source.
  2. **Positions are byte offsets in the source**, always on a character boundary (TEXT-21).
     Line and column come from the line index. The column Up and Down keep is a display
     column (TEXT-4 tabs included), held by the view.
  3. **The engine owns the editing commands** (D4): moving by character, word and line,
     word boundaries, the edit for typed text, Backspace, Delete, Enter with indentation,
     Tab, paste with line endings converted. They are functions from a document and a
     selection to a new selection or an `Edit`, tested on files with a byte order mark, CRLF,
     non-ASCII text and bytes that are not UTF-8. The view only maps keys to them.
  4. **Undo groups typing in `History`.** An edit can be marked as typing; it joins the
     previous step when that step was typing of the same kind at the same place less than a
     second ago, and a word typed after a space or punctuation starts a new step, so undo goes
     word by word (TEXT-20). One step stays one entry, so EDIT-1 to EDIT-3 hold as they are.
  5. **The index while typing.** Each edit updates the buffer and the line index at once.
     Then:
     - The local update of D9 is tried. When it works (the element is still well-formed),
       tree and grid follow at once: 0.7 ms on 10 MiB, 15 ms on 100 MiB.
     - When it does not, the whole document is **not** read again on that key. The old nodes
       stay, their offsets after the edit shifted (D9's "keep the old nodes with shifted
       offsets", about 13 ms on 100 MiB), and the edited element is marked stale. Tree and grid
       show the last good state (TEXT-10).
     - After half a second without typing, the whole document is read again: on the UI thread
       up to about 20 MiB (30 ms on 10 MiB), on a worker thread above that, with a copy of the
       source; the result is used only if no key was typed meanwhile. This is the worker thread
       D9 deferred; typing in a broken element is what now needs it.
  6. **Views follow by position, not on every key.** The selection, expansion, sort and hidden
     columns are kept by element position, as `Open::edit` does today (FMT-8). The grid is
     rebuilt when the change lies inside the element it shows, and at most once per frame.
- **Must come with it or first:**
  - **Long lines (TEXT-5).** Lines are cut at 4,000 bytes for display; the cursor cannot be
    put where nothing is drawn. Until lines are paged sideways, the cursor stops at the cut and
    the status bar says why. Minified files, which are one line, need the paging.
    **Built (2026-11-15):** a line over 4,000 bytes is laid out only from a `<` a little
    left of the view to a margin past it. `text::LineColumns` keeps a column every 4 KiB of
    the line, so finding what is on screen costs a few kilobytes; a change inside the line
    counts only its stretch again (`LineColumns::edit`). The sideways position is an `f64`
    of pixels, as a 100 MiB line is about 800 million pixels wide; drawing is relative to
    the laid-out part. Up and Down on a long line still count its columns from its start
    (`text::column`), so they cost the length of the line; not measured yet.
  - **Integer scrolling (D16).** D16 is due with the first custom text widget. Ctrl+End and
    Page Down must reach every line of a 300 MiB file, which float offsets do not guarantee.
- **Order of pull requests:**
  1. Engine: cursor movement, the editing commands and undo grouping, with tests. No GUI.
     **Built (2026-11-15):** `crates/engine/src/text.rs` and `Typing` in `edit.rs`; a typed
     letter still costs 0.7 ms on 10 MiB and 15 ms on 100 MiB.
  2. Text view: cursor, keys, mouse selection, typing, Backspace, Delete, Enter, Tab; the index
     as in 5, on the UI thread.
     **Built (2026-11-15)** without the index of 5: each key is `Document::apply` and
     `Open::edit`, which rebuilds tree state, grid and problems list. Measured in the app:
     a typed letter 1.9–2.2 ms on 10 MiB, 34–37 ms on 100 MiB; a key that breaks or mends an
     element 31 ms and 317 ms. The text view is one egui widget for the whole area (egui keeps
     the keyboard focus only on an id a widget uses each frame); its lines only paint.
  3. Clipboard and Select All; Replace's *In selection* uses the text selection (TEXT-14).
     **Built (2026-11-15):** `text::cut`, `text::paste`, `text::select_all`; the view reads
     egui's `Event::Cut`, `Event::Copy` and `Event::Paste`, and Edit > Cut, Copy and Paste
     send `ViewportCommand::RequestCut`, `RequestCopy` and `RequestPaste`, so menu and keys
     take one path. The document keeps its line ending (`Document::newline`) for when no
     line break is left to tell it by.
  4. Input methods (TEXT-19).
     **Built (2026-11-15):** the view reads `Event::Ime` and sets `PlatformOutput::ime`
     every frame it has the keyboard; egui-winit turns the platform input method on only
     then. The preedit is view state (`Open::text_preedit`), drawn into the cursor's line;
     a commit is `text::insert`. `ImeEvent::DeleteSurrounding` is ignored, as egui's own
     `TextEdit` does. Checked on screen under X11 with ibus over XIM (US international
     layout): a dead key shows its preedit and commits `é`; a click mid-composition drops
     it. Not checked: a CJK input method (none installed), Wayland, Windows, macOS.
  5. The broken state: stale marks in tree and grid (TEXT-10), and the worker thread.
     **Built (2026-11-15):** `Document::apply_typed` (what is typed, cut or pasted) tries
     D9's local update; when it fails, `Index::keep_stale` keeps the nodes instead of
     reading the file. One change from item 5 above: nodes whose own bytes the change
     touched are dropped, not shifted, because the accessors slice by stored lengths and a
     node over changed bytes could make them panic; every node kept reads its own bytes
     (checked on random edits of the corpus). The stale element is the innermost one whose
     `<` and end tag the change left alone; its start tag is measured again when the change
     was there. After `REPARSE_PAUSE` (0.5 s) the whole document is read: on the UI thread
     up to `REPARSE_INLINE_BYTES` (20 MiB), else as a `ReparseJob` on a worker thread whose
     result is dropped if the text changed meanwhile. A well-formed result replaces the
     index; a broken one only brings its error. Every other change, undo and redo, and every
     command that works from the index (structured edits, Format, Compact) reads the
     document first, so none of them sees a stale index. Measured in the app: a key that
     breaks or mends an element 2 ms on 10 MiB (31 ms before), 34 ms on 100 MiB (317 ms),
     of which 7 ms is the index and the rest the views (step 6). Open: copying 100 MiB for
     the worker takes 54 ms on the UI thread, mostly fresh pages; reusing the buffer, or a
     shared source, would remove it.
  6. Integer scrolling (D16) and sideways paging of long lines (TEXT-5).
     **Built (2026-11-15):** integer scrolling for the text view (D16, TEXT-8), then long
     lines (TEXT-5). With this every step of D19 is built.
     **Done first, as item 6 of "What" (2026-11-15): views follow by the replaced nodes.**
     The index records which ids a local change replaced (`NodeSplice`, from
     `splice_nodes`); `Open::type_edit` follows it in tree and grid instead of building them
     again. `tree::follow` makes only the rows of the replaced nodes again, and the parent's
     own attribute and text rows; `Grid::follow` moves ids, puts new child elements in place
     of replaced ones, and samples the columns again only when the change lies in the
     sampled rows. Both are checked against a full rebuild on random typed changes. Before,
     every key rebuilt the tree's 211,000 rows under `orders` (17 ms on 100 MiB). Measured
     in the app: 1.4 to 2.3 ms per key on 10 MiB, 17 to 25 ms on 100 MiB, all of it the
     edit (moving the text, D10) and the index. A grid that is sorted, or shows only its
     element (GRID-5), is still built again when its rows change. Not changed: after the
     read that follows a pause, the views are built again (60 ms on 10 MiB, 25 to 42 ms
     taking a worker result on 100 MiB), once per pause.
- **Not chosen:**
  - `egui::TextEdit` per visible line: the cursor and the selection must cross lines, and
    input method state lives in one widget.
  - Reading the whole document again on every key, as XML Marker does (XM-44): 300 ms per key
    at 100 MiB.
  - A rope or piece table now: D10 stands; a typed letter costs 0.7 ms on 10 MiB.
- **Risks:**
  - A key costs 15 ms on 100 MiB, nearly a frame; nearly all of it is shifting the nodes after
    the change. D9's "revisit when" (offsets relative to the parent) is the answer if it shows.
  - Input methods depend on the platform; egui's support on Linux under X11 and Wayland needs
    checking on screen with a real input method. X11 with ibus is checked (step 4); a CJK
    input method, Wayland, Windows and macOS are not.

## D20: What 1.0 holds

- **What (owner, 2026-11-15):** 1.0 is everything on the roadmap up to and including 0.3,
  with packages for Windows and Linux (`docs/spec/install.md`). After 1.0 come all of 0.4
  (the XPath query panel, schema (XSD) validation, importing other XML files, memory-mapped
  open for files larger than RAM) and the macOS package.
- **Packages:** for Windows the MSI and the zip (PKG-10 to PKG-14); for Linux the `.deb` and
  the `.rpm` (PKG-30, PKG-31, PKG-33, PKG-34). The Flatpak stays after them, as PKG-32 says.
  Until there is a macOS package, a release waits only for the Windows signing (PKG-14).

## Open questions

The five questions asked before the review are answered: index updates in D9, rope or not in
D10, memory mapping in D11, roadmap order in D15, slow spots in the review notes and D16.

New ones:

1. Does the tokenizer of D14 pay for itself?
2. How should node identity survive a full re-index (after Format, or when a splice fails)?
   Paths of child positions are one option.
3. Which encodings do the owner's real files use? That sets the priority of UTF-16.
