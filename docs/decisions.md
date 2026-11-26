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
  union of attributes, leaf child elements and `text()`, taken from every row: the first 1,000
  at once, the rest over the next frames (GRID-3). Cells
  are decoded only for visible rows. A click shows the row in the text; a double-click makes it
  the selection.
- **Text:** a line index built with `memchr`. Only visible lines are laid out. The highlighter
  works line by line and carries its state to the next line; the first line on screen gets it
  from the index (TEXT-3).
  Lines are cut at 4,000 bytes for display.
- **Risk:** minified files that are one huge line. They need wrapping or horizontal paging.
- **Since D42:** the grid is virtualized sideways too, by a table of our own.

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
  is not shown. Done: every row is examined now (GRID-3).
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

## D14: Our own scanner for well-formed UTF-8, quick-xml for the rest

- **Idea:** replace quick-xml in `crates/xml` with a scanner over raw bytes built on `memchr`
  (which is SIMD inside and safe to call, so the `unsafe` ban stays).
- **Expected gains, none measured yet:**
  - Speed: quick-xml 0.42 validates UTF-8 and builds an event per token.
  - Latin-1 and Windows-1252 files index without transcoding, because all markup is ASCII.
    UTF-16 still needs transcoding at load. Not needed any more: D23 converts at load.
  - Error recovery: on a mismatched end tag, close elements up to the matching ancestor and
    carry on, collecting a list of errors instead of stopping at the first (D3, D9).
  - No hidden offset rules such as the byte order mark.
- **Decide by:** a prototype benchmarked against the current 300 MiB/s and checked against the
  corpus (D15). Keep quick-xml if the gain is small.
- **Error recovery built without it (OPEN-10, 2026-11-15):** quick-xml 0.42 carries on after
  malformed markup, so the rule above runs on quick-xml: the index matches end tags itself,
  against the open names kept one after another in a buffer (reading them from the nodes and
  the source cost 60 ms on the 100 MiB fixture; the buffer costs what quick-xml's own check
  did). After a byte that is not UTF-8 a new reader starts at the next `<`. Without any end
  tag check the index took 265 ms instead of 310 ms there: a hint for the prototype.
- **Prototype (2026-11-16, Windows):** `crates/xml/src/scan.rs` reads a well-formed UTF-8
  document with `memchr` and builds its nodes through the builder's own steps, so the index
  is the same. At the first doubt (any error, a byte that is not UTF-8, a DOCTYPE with an
  internal subset, an empty name, an `&` without `;`) it gives up and quick-xml indexes the
  document from the start, with its errors and recovery as before. The whole source is
  checked for UTF-8 first (15 ms for 300 MiB).
  - Same index: the nodes, element count and depth equal quick-xml's on the three
    fixtures, on every well-formed UTF-8 file of the corpus, and on 18 edge cases
    (entities between whitespace, `>` inside quoted values, CDATA, PIs, DOCTYPE, text
    outside the root, byte order mark, CRLF). Ten broken or unusual inputs go to quick-xml.
  - In one process, best of five: 300 MiB 534-563 ms with quick-xml, 403-420 ms with the
    scanner (533-562 against 714-745 MiB/s); 100 MiB 173-182 against 132-141 ms.
  - `xml-atlas-cli stats`, three alternating rounds against `main`: 300 MiB index
    718-728 ms before, 559-588 ms after; open 929-951 ms before, 779-799 ms after. 100 MiB
    index 238 ms before, 198 ms after in the quietest round.
  - Room left: on 300 MiB, finding every `<` takes 11 ms and pushing its 7.0 M nodes
    61 ms; with the UTF-8 check that is about 90 ms of the scanner's 403 ms. The rest is
    per-token work (about 57 ns a node), not tuned yet.
  - Cost: a document the scanner gives up on is read twice up to that point, so a file
    broken near its end opens up to about 1.75 times slower than before.
- **Decided (owner, 2026-11-16):** the scanner is kept as built; quick-xml stays for every
  document it gives up on. Tuning the scanner's per-token work is a later step.
- **Tuned (2026-11-16):** the 11 ms above was `memchr`'s count of the `<`, which has a
  routine of its own; stepping from one `<` to the next costs about 10 ns each, 141 ms with
  `memchr_iter` and 175 ms with a call per tag on 300 MiB, because most gaps are a few bytes.
  The scanner now looks at the first 32 bytes of every search with a plain loop and calls
  `memchr` only beyond them. Walking all 14.1 M tags of the 300 MiB fixture, best of five in
  one process: 495 ms with `memchr` for every search, 205 ms with the loop first, 379 ms
  testing eight bytes at a time in a `u64`. Packing 64 byte comparisons into a bit mask is not
  vectorized by the compiler (0.9-1.2 s).
  - `xml-atlas-cli stats`, 300 MiB, three alternating rounds against `main`, best of each
    round: index 712, 798, 562 ms before; 684, 469, 463 ms after. The machine was slower
    than when the prototype was measured, and noisy.
  - Left: writing the 7.0 M nodes (about 225 MB) costs about 150-200 ms, and it is the
    memory traffic, not page faults (touching the pages first on 8 threads took 13-24 ms
    and saved about 35 ms). The rest of the time goes to the searches (about 210 ms),
    reading text and names a second time (about 65 ms) and the UTF-8 check (about 20 ms).

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

## D21: Draw with OpenGL, fall back to wgpu

- **What:** eframe draws with OpenGL (`glow`) instead of its default wgpu. Both are compiled
  in. If OpenGL fails before the window is up, the app starts itself again with wgpu (a second
  event loop is not allowed in one process). `XML_ATLAS_RENDERER=wgpu` chooses wgpu by hand,
  for a driver that crashes instead of reporting an error.
- **Why (measured 2026-11-15, Ubuntu, GNOME on Wayland):** wgpu loads every Vulkan driver
  installed, here NVIDIA's and Mesa's, before the first frame (`strace`).
  `tools/bench_open.py`, two alternating rounds against `main`:

  | File | wgpu | OpenGL |
  |---|---|---|
  | tiny | 0.55-0.60 s, 195 MiB | 0.31-0.33 s, 132 MiB |
  | 10 MiB | 0.59-0.61 s, 216 MiB | 0.35-0.38 s, 176 MiB |
  | 100 MiB | 0.93-0.96 s, 390 MiB | 0.73 s, 349 MiB |

  The binary grows by 0.15 MB (26.60 to 26.75 MB) with OpenGL added to wgpu.
- **Checked:** drawing looks the same under X11 with Mesa's software OpenGL (Xephyr); a clean
  OpenGL failure (an impossible pixel format, forced for the test) logs the error and comes
  up with wgpu. A driver made to load nothing crashed inside Mesa instead; that is what the
  variable is for.
- **Not checked:** Windows and macOS, where OpenGL is older or missing (Remote Desktop,
  some virtual machines); the fallback is meant for them, CI only compiles there.
- **Not chosen:** wgpu with only one backend or adapter (start-up still pays for loading
  Vulkan); keeping wgpu and hiding the window until ready (the time stays).

## D22: Linux packages are built against glibc 2.34

- **What:** `cargo xtask package` builds both programs once with `cargo zigbuild` for
  `x86_64-unknown-linux-gnu.2.34`, stripped, and packs the same executables into a `.deb`
  (with `dpkg-deb`) and an `.rpm` (with the `rpm` crate, so no RPM tools are needed). It
  refuses executables that name a newer glibc than 2.34.
- **Why:** PKG-33 asks for one build that runs on the oldest supported release of each
  family. RHEL 9 has glibc 2.34, Ubuntu 22.04 has 2.35; a plain `cargo build` on a current
  system needs the build machine's glibc (2.43 on Ubuntu 26.04). zig links against any older
  glibc without a container, so CI and a developer machine build the same way.
- **Dependencies:** the window links only libc and libm; everything else it loads at run time
  (EGL and Wayland, or GLX, X11 and xcb, plus xkbcommon and fontconfig), so neither package
  manager finds them by itself. The packages name them: Debian package names in the `.deb`,
  library names (`libEGL.so.1()(64bit)`) in the `.rpm`, which RHEL and Fedora share. Found
  with `strace` under X11 and from the crates' `dlopen` names for Wayland.
- **Licenses (PKG-2):** the packages carry `THIRD-PARTY-LICENSES.txt`, made from `cargo
  metadata`: every crate compiled into the two programs, each license text once with the
  crates that ship it, and the fonts' OFL and Ubuntu Font Licence.
- **Not chosen:** building inside a Rocky Linux 9 container (needs Docker or Podman, which a
  developer machine may not allow); `cargo-deb` and `cargo-generate-rpm` (two tools with two
  configurations for one file list).

## D23: Other encodings are converted at open and back at save

- **What:** `crates/engine/src/encoding.rs` converts a file that is not UTF-8 to UTF-8 when it
  is opened, with `encoding_rs`, and back when it is saved. The document remembers the
  encoding, its label and the byte order mark. A UTF-8 file is never converted.
- **Why:** everything else (index, views, find, edits, format) works on UTF-8 and stays as
  it is. A tokenizer that reads Latin-1 itself (D14) would move the conversion into every
  place that shows or takes text, and would not help UTF-16. (Owner, 2026-11-15.)
- **Bytes kept (`AGENTS.md`, rule 7):** a file that does not convert back the same can be
  saved only as UTF-8. For UTF-16 and the single-byte encodings the decoder's error report
  says so, which a test checks for each of the 28 single-byte encodings and every byte;
  multi-byte encodings such as Shift_JIS are converted back at open and compared.
- **A character the encoding cannot hold:** saving fails with its place, and the app offers
  Save as UTF-8. Numeric character references are not written: they would change the
  meaning inside comments and CDATA and are not allowed in names. (Owner, 2026-11-15.)
- **Cost:** converting a 100 MiB file at open: 157 ms for Latin-1, 190 ms for UTF-16, about
  what `encoding_rs` takes to decode; the round trip at open would have doubled it. UTF-8
  files cost nothing more (377 ms, as before).
- **Offsets:** problems, find results and the CLI's positions count bytes of the UTF-8
  text, not of the file.

## D24: A list of the child elements of wide parents

- **Problem (measured 2026-11-16, Windows, 300 MiB fixture, `docs/benchmarks.md`):** what
  is still slow on large files is a walk over the children of one wide element, the 633,506
  orders under the root:
  - `Index::path` of the last order: 38 ms, of which walking the siblings alone is 36 ms
    and comparing their names 2 ms. Selecting an order near the end costs 53-63 ms in all.
  - `Grid::start` for the root, which lists every child element: 37-73 ms.
  - Collapsing or expanding the root, which rebuilds the tree's rows: a 71 ms frame
    (from the order of the log, not timed on its own).

  A step from one child to the next skips that child's subtree in the node array (about
  350 bytes per order), so every step reads new memory: about 57 ns each. Tuning the code
  does not change that; only not walking does.
- **Proposal:** the document keeps, on demand, the ids of the child elements of any parent
  with at least 10,000 children (a first guess, to be set from the prototype), in document
  order, and whether they all have the same name.
  - Built by the first caller that needs it: one walk, the cost of today.
  - Dropped whenever the document changes (its revision), and built again on the next
    demand. An edit does not have to keep it up to date (D9, D19).
  - `path`: with one name for all children, the position is a binary search in the list
    (ids increase). With mixed names it walks the list, comparing names as today.
  - `Grid::start`: copies the list instead of walking (633,506 ids, under a millisecond).
  - The tree: takes a wide parent's children from the list. Its rows are still built one
    per visible child, so collapse and expand gain less.
- **Cost:** 4 bytes per child of a wide parent: 2.5 MiB for the 300 MiB fixture, whose index
  is about 700 MiB (D11).
- **Not chosen:**
  - A next-sibling link in every node: 4 more bytes per node on every file, against the
    32-byte node of D11, and kept up to date on every splice.
  - A cache in each of the grid, the tree and `path`: three lists of the same ids, each
    with its own invalidation.
  - Computing the path in the background: helps only the path, and the grid header shows
    it a moment later.
- **Decide by:** a prototype measured on the 300 MiB fixture before and after: the path of
  the last order, the grid of the root, and collapsing and expanding the root, each to
  within one frame (16 ms), with `xml-atlas-cli stats` and the open time unchanged.
- **Prototype (2026-11-16), for `path` and the grid only:** `Index::child_elements` keeps
  the list for parents with at least 10,000 nodes below them (`WIDE_DESCENDANTS`; counting
  children would itself need the walk); `Index::update` and `Index::keep_stale` drop it, a
  new or cloned index starts without. The tree is unchanged: it filters every child with
  `is_row`, which reads the child's node, so a list of element ids would not spare it the
  walk. Measured on the 300 MiB fixture:
  - In the engine, one session: the path of the last order 72-79 ms before, 5-8 µs after;
    the first call after opening or an edit builds the list, 92 ms. The grid of the root
    68-98 ms before, 1.1-2.7 ms after; the grid of the last order 68-83 ms before, under
    15 µs after (it was the path).
  - `xml-atlas-cli stats` on 100 MiB, six alternating rounds: the same within noise (index
    best 227.4 against 226.1 ms and 227.3 against 227.2 ms in the two quiet rounds; this
    laptop swings between 227 and 349 ms).
  - The window (`tools/bench_windows.py`, median of three, settled): tree End 100 -> 33 ms,
    a click near the end of the tree 140 -> 60 ms, text 20 x Page Down 185 -> 62 ms, the
    search for the last order 263 -> 198 ms. Unchanged: expanding the root (101-131 ms,
    the tree) and tree Home (about 630 ms, the grid's column scan over 36 frames, GRID-3).
  - Met: the path and the grid of the root. Not met: collapsing and expanding the root,
    which needs a list of the tree's rows rather than of element ids.
- **Decided (owner, 2026-11-16):** kept for the path and the grid as built. The tree is a
  separate step.

## D25: Index on several threads

- **Problem (measured 2026-11-16, Windows, `xml-atlas-cli stats`, 300 MiB fixture):**
  opening takes about 700 ms at best: read 126-152 ms, index 463-469 ms, lines 82-110 ms.
  The index runs on one thread while the machine has eight. After the tuning in D14 its time
  is the searches (about 210 ms), writing 7.0 M nodes (150-200 ms of memory traffic),
  reading text and names a second time (about 65 ms) and the UTF-8 check (about 20 ms). No
  cheap single-thread fix is left without `unsafe`.
- **Proposal:** for a document the scanner reads (well-formed UTF-8, D14) and of at least
  4 MiB (a first guess), split the source into chunks, scan the chunks on several threads,
  then stitch their nodes into the one flat array of D2. Everything else stays: the nodes
  are the same, quick-xml still reads every document the scanner gives up on, and edits
  still re-read only the changed element (D9).
  - **Chunks:** more chunks than threads (about four per thread, up to 8 threads), taken
    from a shared counter, so a slow core (this laptop has efficiency cores) does not hold
    up the rest. Each boundary is moved forward to the next `<` followed by a name, the
    start of a start tag. A start tag at the boundary means no element whose content is one
    text can span two chunks, so folding (D11) stays inside a chunk.
  - **A chunk's scan** starts with an empty stack. It keeps its nodes in its own array, with
    parents inside the chunk as chunk-local ids. It also records the end tags that close
    elements opened in earlier chunks (name, position, and how many nodes came before),
    the elements still open at its end, and, for each node at its top level, how many outer
    elements were closed before it. Each chunk checks its own bytes for UTF-8; a boundary
    is always at an ASCII `<`, so that is the same check as today's.
  - **Checking a boundary:** the scan of a chunk must end exactly where the next chunk
    begins. When a boundary fell inside a comment, a CDATA section, a processing
    instruction or a quoted attribute value, the chunk before it runs past the boundary.
    Then the next chunk's nodes are thrown away and that chunk is scanned again from where
    the chunk before it ended, on the stitching thread. Such boundaries should be rare in
    real files.
  - **Stitching**, in order, on one thread: a global stack of open elements and their
    names. Each outer end tag must match the top of that stack, or the whole document goes
    to quick-xml as today. The end and subtree size of an outer element are set when its
    end tag comes. A node at a chunk's top level gets as parent the element left on the
    stack after the outer end tags before it. The element count and maximum depth are added
    up from the chunks. This is work per chunk and per outer element, not per node.
  - **Copying:** the final array is allocated once, at the total size. Each chunk's nodes
    are copied into their own part of it in parallel, adding the chunk's offset to the ids
    (`split_at_mut`, scoped threads, no `unsafe`).
  - **Also:** the line starts (82-110 ms) are found on a thread of their own while the
    index is built, since both only read the source. This is a smaller step that can come
    first.
- **Expected, not measured:** the searches and the second reads (about 300 ms) should
  shrink with the threads. Writing the nodes is memory traffic and gains less, and the
  copy adds some. A guess: index 463 ms to 150-200 ms, opening 700 ms to about 350 ms
  together with the line starts. At 10 MiB, the owner's typical size, the index (about
  20 ms) gains little that anyone can see.
- **Cost:**
  - Memory: while the index is built, every node exists twice, once in its chunk and once
    in the final array: about 225 MB more at 300 MiB, for well under a second. The index
    itself stays the same size.
  - Code: a second way to build the index, next to the one-thread scanner and quick-xml.
    It must give exactly their nodes.
- **Not chosen:**
  - A SIMD scanner like simdjson: needs `unsafe` (see Code style in `AGENTS.md`), and the
    node writes stay.
  - Two passes, the first counting each chunk's nodes so that the second writes straight
    into the final array: no doubled memory, but every chunk is scanned twice.
  - Boundaries at any `<`: an element whose only content is text could then span two
    chunks, and folding would have to remove a node across them.
  - Reading the file and indexing it at the same time: the read is about 130 ms and could
    overlap the index, but a stream changes the open path more. Possible later.
- **Decide by:** a prototype.
  - Its index must equal the one-thread scanner's on the three fixtures, on the corpus,
    and with tiny chunks (so that boundaries fall everywhere, inside comments, CDATA and
    quoted values too) on the scanner's edge cases and the corpus.
  - Measured with `xml-atlas-cli stats`, alternating against `main`, at 10, 100 and
    300 MiB, together with peak memory.
  - Kept if the index at 300 MiB takes at most half as long, and nothing is slower at
    10 MiB.
- **Prototype (2026-11-16, Windows, 8 threads):** `crates/xml/src/parallel.rs`, as proposed,
  with these details:
  - Cuts are at a start tag whose previous byte other than whitespace is `>`, so a cut
    inside a quoted value or text is rarer. A cut that still falls inside markup is
    noticed, and the next chunk is read again from where the chunk before ended. One case
    is refused instead (the document then goes to one thread): an element opened before a
    chunk whose end tag comes after exactly one node in the chunk that is not an element.
    Its text may need folding across chunks. A cut at a true start tag never gives this.
  - The final array is filled on a thread of its own while the chunks are read (sized by
    the builder's guess of one node per 40 bytes, then cut to size). Making it after the
    chunks took 90-110 ms at 300 MiB, writing every node twice; now the copy takes about
    10 ms.
  - Same index: the nodes, element count and depth equal the one-thread scanner's on the
    three fixtures, the corpus, and the scanner's edge cases. With chunks from 1 byte up,
    so that cuts fall inside comments, CDATA, processing instructions and quoted values,
    every reading equals it or is refused. Every document the scanner refuses is refused.
  - `xml-atlas-cli stats`, three alternating rounds against `main`, best of five each.
    The line starts are found alongside the index in the new build:

    | Fixture | Index, `main` | Index, D25 | Open, `main` | Open, D25 |
    |---|---|---|---|---|
    | 300 MiB | 336-367 ms | 142-168 ms | 496-543 ms | 250-274 ms |
    | 100 MiB | 115-121 ms | 43-54 ms | 171-185 ms | 76-86 ms |
    | 10 MiB | 10.8-12.1 ms | 7.0-10.3 ms | 16.3-18.4 ms | 10.9-13.8 ms |

    Finding the line starts alongside the index alone: 300 MiB open 530-549 to
    462-465 ms. In the new build the line starts take longer while the chunks are read
    (up to 126 ms at 300 MiB) but still end before the index.
  - Peak working set while opening, sampled every 5 ms: 300 MiB 573 to 814 MiB, 100 MiB 194
    to 275 MiB. That is the nodes held twice, as expected. The index afterwards is the
    same size.
  - Met: the index at 300 MiB takes 42-46 % of the time, and 10 MiB is faster, not slower.
- **Decided (owner, 2026-11-16):** kept as built.

## D26: Open files from Finder on macOS, with `unsafe` in one module

- **Problem:** the macOS app (PKG-20) cannot take a file from Finder. A double-click, *Open
  with*, a file dropped on the Dock icon and `open file.xml` in a terminal all reach the app
  as an Apple event ("open documents", `kAEOpenDocuments`), not as a command-line argument.
  winit 0.30.13, under eframe 0.37, does not pass that event on, so the app does not offer
  itself for XML files (PKG-4). Receiving the event takes calls into AppKit, which are
  `unsafe` in Rust, and `unsafe` is forbidden in the workspace (`AGENTS.md`).
- **What winit does (read in its source, 0.30.13):** it sets an application delegate of its
  own (`WinitApplicationDelegate`, `platform_impl/macos/event_loop.rs`), although its docs
  say it does not. It handles `applicationDidFinishLaunching:` and
  `applicationWillTerminate:`, and nothing for opening files. Replacing that delegate with
  ours would break its start-up. eframe 0.37 has no hook either; it only offers one before
  the event loop is built.
- **Proposal:** a handler for the Apple event itself, next to winit's delegate:
  - In `main`, before eframe starts, observe `NSApplicationWillFinishLaunchingNotification`.
    When it comes, install a handler for `kAEOpenDocuments` with `NSAppleEventManager`.
    AppKit installs its own handlers before that notification and dispatches the event
    for the files given at launch after it, so ours replaces AppKit's and still sees those
    files. To be confirmed by the prototype.
  - The handler is a small Objective-C class of our own (`define_class!` in objc2). It
    reads the file URLs from the event, turns them into paths and sends them over a channel
    to the app, and asks for a frame once the window exists. The app opens each path as a
    tab, as it does for a path on the command line (OPEN-5); a file already open is shown,
    not opened twice. Files that arrive before the first frame wait in the channel.
  - While the app runs, macOS sends the event to the running app instead of starting a
    second one, so a file opened from Finder becomes a new tab.
  - `Info.plist` declares XML documents (`public.xml`) with the role Viewer and the rank
    Alternate, so XML Atlas is listed in *Open with* without becoming the default (PKG-4).
- **How `unsafe` is contained:**
  - All of it lives in one file, `apps/xml-atlas/src/macos_open.rs`, compiled only on macOS
    (`#[cfg(target_os = "macos")]`), with `#![allow(unsafe_code)]` at its top. Each
    `unsafe` block says why it is sound in a `// SAFETY:` comment.
  - The workspace keeps `unsafe_code = "forbid"`. Only the `xml-atlas` crate sets
    `unsafe_code = "deny"` in its own `[lints]` table, because `forbid` cannot be allowed
    again for one module. Every other crate, including the engine and the parser, stays
    under `forbid`.
  - A test in the app's crate reads its own sources and fails when `allow(unsafe_code)`
    appears anywhere but `macos_open.rs`.
  - Crates: `objc2` 0.6, `objc2-foundation` and `objc2-app-kit` 0.3.2 and `block2` 0.6, all
    for macOS only. The same versions are in `Cargo.lock` already, through the clipboard and
    OpenGL crates, so nothing new is compiled; only a few of their features are turned on.
- **Cost:** about 100 to 150 lines, with perhaps ten `unsafe` blocks, that only build on
  macOS. Linux CI checks them with `cargo check --target aarch64-apple-darwin`; the macOS
  CI runner builds and runs them.
- **Not chosen:**
  - Our own application delegate in place of winit's: breaks winit's start-up handling.
  - Adding `application:openURLs:` to winit's delegate class at run time
    (`class_addMethod`): depends on a private class name and on winit's internals, and
    breaks silently when either changes.
  - An AppleScript applet as the app's main program that starts `xml-atlas` with the path:
    no `unsafe`, but every file opened from Finder would start another process instead of a
    new tab, and the Dock would show two icons.
  - Waiting for winit or eframe to pass on the event: nothing is known to be planned for the
    versions eframe uses, and the gap stays until then.
  - Leaving it as it is: the macOS app stays the only one that cannot be opened from the
    file manager.
- **Decide by:** a prototype, then:
  - On the CI's macOS runner, `open -a "XML Atlas.app" file.xml` (with `--env` for the log
    and `--stderr` to a file) starts the app with the file open, as the log shows; a second
    `open` with another file adds a tab in the same process. Paths with spaces and
    non-ASCII names.
  - On a real Mac, by the owner or someone at the company: a double-click after choosing
    XML Atlas as the default for `.xml`, *Open with*, a file dropped on the Dock icon, with
    the app running and not running.
  - Kept if all of these work and the unsafe code stays in the one file.
- **Owner's answers (2026-11-17):** `unsafe` is acceptable for this, contained as above. A
  Mac is available from 2026-11-18 for the checks on a real Mac. Next: the prototype.
- **Prototype (2026-11-17, #86):** built as proposed in `apps/xml-atlas/src/macos_open.rs`,
  with five `unsafe` uses. `setEventHandler:…` and `paramDescriptorForKeyword:` are called
  with `msg_send!`, because their generated bindings need `objc2-core-services`, which is not
  in the build. When OpenGL fails and the app starts again with wgpu (D21), files from
  Finder go to the new process as arguments. On the CI's macOS runner, `open -a` with
  `first file.xml` started the app with it open, and `zweite Größe.xml` sent while it ran
  opened as a second tab in the same process; macOS sent that name decomposed (`o` and a
  combining diaeresis), and it opened. Still to do: the checks on a real Mac.

## D27: An MCP server, on files first, then on the open window

- **What:** agents such as Claude Code use XML Atlas through the Model Context Protocol
  (MCP). The owner's order (2026-11-17): first a server that works on files without a
  window, then one that works on the documents open in the window, so the user sees what
  an agent does and can undo it.
- **Step 1, files:** `xml-atlas-cli mcp` speaks MCP over standard input and output, the
  transport local clients start a server with (`claude mcp add xml-atlas -- xml-atlas-cli
  mcp`). It keeps the documents it opened in memory, like tabs, until the session ends.
  Nothing is written to disk except by `save`.
- **Tools,** each a call into the engine (D4), the same as the CLI's commands. Elements are
  named by the paths the grid and CLI use (`/orders/order[2]/customer`):
  - `open` (a path; returns a document id and its stats: size, lines, elements, encoding,
    whether it is well-formed), `close`, `documents`.
  - Reading: `element` (name, attributes, text, line, and its children by name and count, as
    the grid shows them), `children` (a page of child rows with their columns), `text` (a
    range of lines of the source), `find` (matches with line, column and element path),
    `xpath` (an XPath 1.0 query, D35), `problems`.
  - Changing, each one undo step: `set_text`, `set_attribute`, `remove_attribute`, `rename`,
    `delete`, `duplicate`, `move`, `replace`, `format`, `compact`, `undo`, `redo`.
  - `save` (atomic, D13; keeps the file's encoding, OPEN-8) and `save_as`.
- **Answers stay small:** an agent reads the answer as text, and its context is short.
  Every list is a page with a limit (100 by default) and a note of how many there are;
  text and values are cut at 64 KiB, with the length given. Nothing returns a whole large
  file.
- **Crate:** `rmcp` 3.x, the official Rust SDK, with its `server` and `transport-io`
  features. It follows the protocol's versions, so we do not. It brings `tokio` into
  `xml-atlas-cli` only; the window, the engine and the parser do not get it. The engine
  stays synchronous: a tool call runs its engine call and answers.
- **Where the code lives:** a crate `crates/mcp` with the tools, over a trait for "the open
  documents". The CLI implements the trait with documents in memory (step 1); the app
  implements it with its tabs (step 2). So the tools are written once.
- **Step 2, the window:** the app listens on a local socket (a Unix socket, a named pipe on
  Windows) only when the user turns it on (a setting, off by default), and `xml-atlas-cli
  mcp --window` passes the agent's calls on to it. Calls act on the open tabs: an edit
  selects what it changed and is one undo step in the window, `open` opens a tab. Its
  details are a decision of their own, after step 1.
- **Cost:** `tokio` and `rmcp` in the CLI: a larger CLI and a longer clean build; the
  window is not affected. A new crate of about 500 to 800 lines with its tests.
- **Not chosen:**
  - An MCP implementation of our own over `serde_json`: no `tokio`, and the protocol is
    small, but we would have to follow every change of it ourselves.
  - The server in its own program (`xml-atlas-mcp`): one more executable in every package
    for little gain; a subcommand of the CLI is installed already.
  - Only step 2: an agent could not work without a window, as on a server or in CI.
- **Tests:** the tools against the engine in `crates/mcp`, with the corpus (a byte order
  mark, CRLF, UTF-16, non-ASCII names). Through the protocol: a test starts `xml-atlas-cli
  mcp`, initializes, lists the tools and calls `open`, `find`, `set_text`, `save` on a
  copy of a corpus file, and checks the saved bytes change only where asked (rule 7).
  Then once with Claude Code as the client.
- **Decided (owner, 2026-11-17):** as proposed, with `rmcp`; `tokio` in the CLI is no
  concern, as it costs build time and the CLI's size but nothing in the window, the engine
  or opening. An agent may save through `save` without being asked again; MCP clients ask
  their user before tool calls.
- **Step 1 built (2026-11-17):** `crates/mcp` holds the 22 tools over the `Documents`
  trait, with `Files` keeping the documents of one session; `apps/xml-atlas-cli/src/mcp.rs`
  serves them with `rmcp` 3.11 on a single-threaded `tokio` runtime. Results are JSON, both
  as text and as structured content; a refusal (a path that leads nowhere, an edit on a
  broken document, a change that would break one, which is undone) is a tool error with its
  reason. Element paths are those of the grid and `xml-atlas-cli`, and `children` builds
  each row's so that it leads back to the row. Measured: the stripped CLI grows from 2.9 to
  4.9 MiB; `xml-atlas-cli stats` on the 100 MiB fixture is unchanged (104.7-109.1 ms median
  for both in three alternating rounds). Tested on the corpus (a byte order mark and CRLF,
  Latin-1, a broken file) and through the protocol, by a test that starts `xml-atlas-cli
  mcp`; not yet with Claude Code as the client.

## D28: Agents in the open window (MCP, step 2)

- **What:** step 2 of D27. An agent works on the documents open in the window: the user
  sees each change, selected, and can undo it. The tools are those of step 1
  (`crates/mcp`); the window implements their `Documents` trait with its tabs.
- **How the agent reaches the window:** the agent's client starts `xml-atlas-cli mcp
  --window`, which speaks MCP as in step 1 and passes each tool call to the window.
  - The window listens on `127.0.0.1`, on a port the system picks, only while View > Allow
    AI agents (MCP) is on; the setting is remembered (WIN-3) and off by default.
  - It writes `agent.json` next to the settings file (WIN-3): the port, a secret of 32
    random bytes from the system (`getrandom`, already in the build) and its process id;
    only the user can read it. Every call must carry the secret, or the connection is
    closed. The file goes when the setting is turned off or the window closes.
  - Between them, one line of JSON per call and one per answer, over one connection:
    `{"secret", "tool", "arguments"}` and `{"result"}` or `{"error"}`.
  - The standard library has loopback TCP on every system; a Unix socket or a named pipe
    would need a crate and two kinds of code.
- **In the window:** a thread per connection reads the calls and hands them to the window,
  which runs them between two frames, on its own thread, where the documents are.
  - A change goes through the same code as the menus' (`Open::edit`): tree, grid and text
    follow, the selection and expanded rows stay, it is one undo step, and the element it
    changed is selected. The status bar names it: "Agent: Rename".
  - `open` opens a tab, or shows the one the file is open in. `documents` lists the tabs.
  - An agent may not discard the user's work: `close` is refused for a tab with unsaved
    changes. It may save (D27).
  - While an agent is connected the status bar says so.
  - Large files open on the window's thread while the agent waits, about a second per
    100 MiB: the window does not draw meanwhile. Opening in the background as File > Open
    does is a later step.
- **Decided (owner, 2026-11-17):** loopback TCP with a secret; a remembered setting, off
  by default; agents edit and save but do not discard.
- **Built (2026-11-17):** `crates/mcp/src/bridge.rs` holds both ends: `Listener` in the
  window, `WindowClient` in the CLI, and where `agent.json` is, the same rule as the
  settings file's, which moved there from the app. The `Documents` trait gained `change`,
  so that the window runs a change through `Open::edit`; `close` may refuse.
  `apps/xml-atlas/src/agent.rs` implements the trait over the tabs. Tested without a window
  (a listener, a loop that runs the calls, the client: the secret file is `0600`, a wrong
  secret closes the connection, the file goes when listening stops), on the tabs (an edit
  is one undo step, selected, its tab shown; `close` is refused with unsaved changes; the
  saved bytes change only where asked), and through the protocol (`mcp --window` without a
  window tells the agent to turn it on; with one, its calls arrive). The owner tried it with
  Claude Code in the running window on Linux, 2026-11-17: it worked.
- **Not chosen:** a Unix socket and a named pipe (a new crate, two code paths); the window
  speaking MCP itself (clients start a program to talk to, so the bridge is needed
  anyway); agents closing modified tabs.

## D29: Themes the user can change, and a native look

- **Problem:** WIN-2 gives Light, Dark and Follow system, with egui's own colours. The
  owner (2026-11-17): dark is good, but light is too light next to it, and users will want
  to change the colours. Themes should also let the program look more like the other
  programs on the system.
- **Why light is too light:** egui's light theme has panels and menus in grey 248 and the
  text and grid areas in pure white (255), with grey 80 text. Almost the whole window is
  close to white, so there is little to separate the views, and the light glare is much
  stronger than the dark theme's. Our syntax colours (`highlight.rs`) are dark enough; the
  backgrounds are the problem.
- **What a theme is:** one set of every colour the window uses, in one place:
  - the window's: panel and menu backgrounds, the text and grid backgrounds, text and weak
    text, separators, buttons in their states, the selection, links, warning and error;
  - the views': the syntax colours of the text (TEXT-2, now in `highlight.rs`), the marked
    element and the find matches (TEXT-6, TEXT-12), the grid's hatching (GRID-13), tree
    icons (TREE-11), the problem markers (OPEN-13);
  - and the shape: corner rounding and spacing.

  Today these come partly from egui's `Visuals` and partly from constants in four files.
  They move into one `Theme` that is turned into egui's `Visuals` and handed to the views
  once, when the theme changes. No work per frame is added.
- **Themes that come with the program:**
  - *Dark*: as today.
  - *Light*: softer than egui's: panels in a mid grey, text and grid areas off-white, not
    pure white, and darker separators, so the views stand apart. The owner picks it from
    two or three candidates shown in the running program.
  - *Native*, in a light and a dark variant for the system it runs on: colours, corner
    rounding and spacing after the system's own look (Windows 11, macOS, GNOME's Adwaita,
    KDE's Breeze), and the system's accent colour for the selection and focus. The colours
    are our own choice, taken from looking at the systems and their published design
    guidelines; no assets of theirs are used.
- **Two choices in View > Theme:**
  - *Mode*, as today (WIN-2): Follow system (the default), Light or Dark.
  - *Style*: XML Atlas (the default; the Dark and softer Light above), Native, or a theme
    file of the user's.

  Every style has a light and a dark variant, and the mode picks which one shows. So
  Native with Follow system switches with the system, and there is no "Native light" and
  "Native dark" to keep in step. Both choices are remembered (WIN-3).
- **The accent colour:**
  - Linux: the settings portal that already reports light or dark (`system_theme.rs`) has
    `accent-color` in the same `org.freedesktop.appearance` namespace (GNOME 47, KDE
    Plasma 6); the same thread reads it and listens for changes. Without it, a blue.
  - Windows: `AccentColor` in the registry (`HKCU\Software\Microsoft\Windows\DWM`), read at
    start and again when the window regains focus.
  - macOS: `NSColor.controlAccentColor` (`objc2-app-kit`, safe to call). Reading its red,
    green and blue may need `unsafe`; if so, it goes into the module D26 allows, or a
    decision of its own first (`AGENTS.md`). To check on a Mac.
- **Changing a theme:** with a text editor; no editor in the window (owner, 2026-11-17).
  A theme is a TOML file in `themes/` next to the settings file (WIN-3), readable and
  shareable, with a `[light]` and a `[dark]` part; the colours have names (`text_background`,
  `tag`, `attribute_name`, `selection`, …). A file there shows up under Style. View > Theme
  > *Save a copy of this theme* writes the current style as a file to start from, and *Open
  the themes folder* shows the folder; the window reads a changed file again when it gets
  the focus back. A colour that is missing or cannot be read, or a missing part, comes from
  the XML Atlas style, with a warning in the log, as WIN-3 does for settings. The help
  (WIN-4) describes the file.
- **What a native look cannot be in egui:** egui draws every widget itself. Menus, buttons,
  scroll bars and dialogs look like the system's only as far as colours, rounding, spacing
  and the font allow. The title bar and the file dialogs (`rfd`) are already the system's.
  The UI font follows TEXT-25's font choice; using the system's UI font by default
  (`fontique` names it) is part of *Native*. The menu bar at the top of the screen on macOS
  is wanted later (owner, 2026-11-17); it needs a crate (`muda`) and a decision of its own,
  after this plan.
- **Checks:** a test for every built-in theme that text, syntax colours, weak text and the
  selection have enough contrast on their backgrounds (4.5:1 for text, 3:1 for weak text,
  the WCAG levels); a test that a saved theme file reads back the same, with non-ASCII in
  its name; the startup time measured before and after (WIN-2's portal thread already runs).
  `xml-atlas-cli stats` does not touch the window, so its numbers do not move; they are
  reported anyway (rule 1).
- **Order of work,** each a pull request. Steps 1 and 2 come first, in a 1.x release; the
  rest later (owner, 2026-11-17):
  1. One `Theme` holding every colour, built from today's; the window looks the same.
  2. A softer *Light*, chosen by the owner from candidates in the running program.
  3. Style next to mode in View > Theme; theme files.
  4. *Native* for Windows, macOS, GNOME and KDE, with the accent colour.
- **Not chosen:**
  - Only a softer light theme: fixes the glare but not "users will want to change it".
  - Native widgets (through a toolkit such as GTK, or a web view): a second UI toolkit,
    against D1.
  - Themes as code (Rust constants per theme only): users could not share or change them.
  - An editor for themes in the window: a text editor is enough (owner).
  - *Native light* and *Native dark* as two themes: the mode already chooses light or dark.
  - Taking a theme from VS Code or another editor's format: tied to a format we do not
    control; a later import is possible from our own file.
- **Answered (owner, 2026-11-17):** theme files edited in a text editor are enough; the
  macOS menu bar is wanted later; steps 1 and 2 first; Native is one style with a light
  and a dark variant, chosen by the mode, instead of *Native light* and *Native dark*. Decided as
  above. The rules are WIN-7 to WIN-10 in `docs/spec/window.md`.

- **Built, step 1 (2026-11-18):** `apps/xml-atlas/src/theme.rs`. A `Theme` holds egui's
  `Visuals` and the colours the views paint themselves: the syntax colours (`Syntax`, which
  replaces `highlight.rs`'s `Palette`), the selected element, the marked item and its
  outline, the text selection and the hatching. `Theme::light()` and `Theme::dark()` are
  built from egui's visuals and the colours used before; tests check that each equals what
  the views computed before. `theme::install` hands both to egui once, at start;
  `theme::current` gives the views the one the mode shows, from egui's memory (one lookup
  a frame per view, and one per hatched grid cell). Spacing is not in the theme yet.

- **Built, step 2 (2026-11-18):** the owner looked at three candidates in the running
  program: a light grey, a mid grey and a warm paper tint, next to 1.0's Light. They liked
  both the light grey and 1.0's Light, and chose to offer both now. View > Light style picks
  *Soft grey* (the default) or *Classic, as in 1.0*; it is remembered as `light_style`.
  Measured, the analysis above was off in one point: egui fills every panel with the same
  grey 248, the views included; only text fields and scroll bar tracks are pure white. So
  the theme gained a view background for the tree, grid, cell pane and text, which in Dark
  and Classic equals the panels' fill. The contrast test (WIN-7) covers every built-in
  theme. Soft grey passes it; Dark's weak text (2.66:1) and punctuation (4.36:1), and
  Classic's weak text (2.89:1), miss it and are exempt: Dark stays as it is (owner,
  2026-11-18), and Classic is 1.0's Light.

## D30: Telling the user about a newer version (PKG-7)

- **Problem:** PKG-7 is planned but not built. Users do not learn that a newer version
  fixes their problem: 1.0.0 on Windows opens a console beside the window and shows boxes
  for six toolbar icons; both are fixed on `main` (#99, #100), and nobody running 1.0.0 will
  hear of it. Each release without the check is one more release whose users miss the next.
  1.0.0 cannot be helped; the next release can.
- **What PKG-7 already says:** at most once a day, ask GitHub for the latest release; when it
  is newer, say so once in the status bar with a link to the releases page; never download
  or install anything; send nothing but that request; a setting turns it off; silent without
  a network; in a Linux package or the Flatpak, say to update with the package manager.
- **The request:** `GET https://api.github.com/repos/glaure/xml-atlas-rs/releases/latest`.
  It returns the newest published release, never a draft (PKG-1, PKG-14) or a pre-release.
  Its `tag_name` (`v1.0.0`) is compared with the program's own version (`CARGO_PKG_VERSION`)
  as three numbers; `html_url` is the link. GitHub answers 60 requests an hour per address
  without a token, far above one a day. GitHub requires a `User-Agent`; it is
  `xml-atlas/<version>`, nothing about the user or the machine. No cookies, no token, no
  identifier: GitHub sees the address and the version, as for any download from it.
- **How it is built:**
  - On a thread of its own, started after the first frame, so start-up does not wait for
    the network (WIN-2's portal thread works the same way). The answer comes back through a
    channel and the status bar shows it on the next frame.
  - The HTTP client is `ureq` (3.x; version and features checked in the registry before it
    is added, `AGENTS.md` rule 5) with `rustls`: blocking, no async runtime in the app, pure
    Rust, the same on every platform. Certificates come from the system's store
    (`rustls-platform-verifier`), so a company proxy that inspects TLS with its own
    certificate still works. Only the app depends on it; the CLI and the engine do not.
  - `serde_json`, already in the tree, reads the answer; only `tag_name` and `html_url`.
  - Two values in the settings file (WIN-3): `update_check = true|false`, and the time of
    the last check, so the once a day holds across restarts. A version the notice has been
    shown for is remembered too, so it is shown once, not at every start.
  - A Linux package is recognised by the program running from `/usr/bin`, the Flatpak by
    `FLATPAK_ID`; their notice names the package manager instead of linking to the download.
  - Errors (no network, a proxy, a timeout of 10 s, an answer that cannot be read) go to the
    log and nowhere else.
- **Cost:** about 1 to 2 MB more in the app's binary, from TLS and the HTTP client (to be
  measured); a little build time. Nothing at start-up or per frame (`AGENTS.md` rule 1); the
  start-up time is measured before and after anyway.
- **Tests:** comparing versions (`v1.10.0` is newer than `v1.9.2`; `v1.0.0` is not newer
  than `1.0.0`; a tag that is not a version is ignored); reading a recorded answer from
  GitHub; the once a day and the once per version against the settings, with the clock
  passed in; the settings round trip. No test talks to the network.
- **Not chosen:**
  - Downloading or installing the update: PKG-7 rules it out (owner, 2026-11-15).
  - Calling the system's `curl`: Windows 10 has it only from build 17063 on, it may be
    missing on a minimal Linux, and its errors are text to parse.
  - The HTTP client that comes with the CLI's `tokio`: it would put an async runtime into the
    app for one request a day.
  - Our own update server or a version file on another site: one more thing to run; GitHub
    already publishes the releases.
- **Questions for the owner:**
  1. On or off by default? On means a fresh install contacts GitHub once a day without
     being asked, and users learn about fixes; off means almost nobody turns it on. PKG-7
     reads as on. Proposed: on, and the help (WIN-4) says what it sends.
  2. Is the extra 1 to 2 MB in the app worth it, or should the check wait for a reason that
     needs HTTP anyway?
  3. Where to turn it off: View menu next to the other remembered options, or only in the
     settings file? Proposed: Help > Check for updates (on/off), next to About.
  4. Should Help also have *Check now*, which asks at once and says "up to date" too?
     Proposed: yes; it is the same request.
- **Answered (owner, 2026-11-18):** on by default; the extra size is acceptable; Help gets
  *Check now*. Where to turn it off: in a Settings window, which the program does not have
  yet (D31).
- **Built (2026-11-19):** `apps/xml-atlas/src/update.rs`, as above, with `ureq` 3.5.2 and
  the features `rustls` and `platform-verifier`. Settings > Updates and Help > *Check for
  updates now*; the settings file has `[updates]` with `check`, `last_check` and `shown`.
  The time is noted when the request starts, so a check that fails also waits a day. The
  notice sits at the right of the status bar until it is closed; for a package or the
  Flatpak it has no link. Measured: `xml-atlas.exe` (Windows, release) grows from 19,020,288
  to 21,012,480 bytes, 1.9 MiB. Without the WebPKI root list that the `rustls` feature
  brings (unused with the platform verifier) it would be 58 KB smaller, but that needs
  ureq's `unversioned_rustls_crypto_provider`, outside its semver promise: not taken.
  Start-up (`tools/bench_windows.py open`, Windows 11, release builds, 5 runs each, two
  rounds, a fresh settings file per run so every run asks GitHub): ready 0.32 to 0.40 s
  median before, 0.37 to 0.42 s after, and 5 to 7 MiB more peak memory. With the check
  turned off in the settings file, no difference (before 0.35 to 0.40 s, after 0.35 to
  0.40 s; the same memory). So the cost is the check's own thread, the TLS handshake and the
  certificate check, which the benchmark counts because it waits for the process to go idle;
  it comes once a day.
- **Changed (owner, 2026-11-22): the packages get the link too.** The installed 1.2.0 `.deb`
  said "update it with your package manager", and apt knew of no 1.3.0: there is no apt or dnf
  repository, so every `.deb` and `.rpm` is a download. The notice now reads "XML Atlas 1.3.0
  is available: open the download page" for all of them and opens the release. Only the
  Flatpak names its own way to update, and running from `/usr/bin` no longer matters.

## D31: A Settings window

- **Problem:** the owner (2026-11-18) looks for *Settings*, *Preferences* or *Options* in the
  File menu, as in VS Code, or in a menu of its own, and finds none. Today the remembered
  settings (WIN-3) are spread out: the View menu has attribute rows in the tree, the cell
  pane, the toolbar, Allow AI agents, Font…, word wrap, the theme mode, the light style and
  the layout; Edit > Format options… has the format settings (FMT-10 to FMT-13); the find bar
  has its options. The update check (D30) needs a home too, and D29's style and theme files
  will add more. The owner is unsure which settings belong in such a window.
- **Proposed:** *File > Settings…* (Ctrl+,, as in VS Code; Cmd+, on macOS), a window with
  every remembered setting in one place, in sections:
  - *Appearance:* theme mode and light style (later D29's style and theme files), the font
    of text, tree and grid (TEXT-25), the toolbar.
  - *Views:* layout, attribute rows in the tree, the cell pane, word wrap for new tabs.
  - *Format:* the format settings, the same as Edit > Format options… (FMT-10 to FMT-13).
  - *Updates:* check for newer versions (D30), when it last checked, and *Check now*.
  - *AI agents:* Allow AI agents (MCP), with what it does and that it listens on
    `127.0.0.1` (D28).

  A change applies at once and is saved at once, as the View menu's options are today; there
  is no OK or Cancel. *Reset to defaults* per section. The window is an egui window over the
  main one, like the Format options dialog.
- **What stays in the menus:** what is switched while working stays where it is, as a
  shortcut to the same setting: word wrap (Alt+Z), the layout, the cell pane, attribute rows,
  the toolbar and the theme mode in View; Format options… in Edit; the find options in the
  find bar. Settings shows the same values; changing one place changes the other. Moved out
  of View into Settings only: *Allow AI agents*, which opens a port and is set once, not
  switched while working, and the light style, which is set once.
- **Not in Settings:** the recent files (File > Open Recent has *Clear list*), the window's
  size and the panes' sizes (egui keeps them; it does not: see D38), the find options (the find bar shows them
  where they act).
- **Not chosen:**
  - A menu of its own (*Settings* or *Tools > Options*): one more menu for one entry; the
    File menu is where the owner looked first, and where VS Code and many Windows programs
    have it.
  - Only the settings file, edited in a text editor: fine for theme files (D29), not for
    switching an update check off.
  - Moving everything out of the menus into Settings: the View menu's options are switched
    while working; hiding them in a window makes that slower.
- **Questions for the owner:**
  1. *File > Settings…* with Ctrl+, as proposed?
  2. The sections and what goes in them, as above?
  3. Move *Allow AI agents* and *Light style* out of the View menu into Settings only, and
     keep the rest in both places?
  4. Changes apply at once, without OK and Cancel?
- **Decided (owner, 2026-11-18):** as proposed. *File > Settings…* with Ctrl+,; the sections
  above; *Allow AI agents* and *Light style* leave the View menu for Settings, the rest stays
  in both places; changes apply and are saved at once, without OK and Cancel.

## D32: References a team defines, previewed resolved

- **Problem:** the owner's most important file (6 MiB, about 7,600 references) points from
  one part to another with conventions of its own team: an element that stands for elements
  found elsewhere by a path, and attribute values looked up the same way, in named contexts.
  The program that reads the file resolves them; in an editor they are opaque, and one that
  finds nothing goes unnoticed. The owner wants editor support and a preview of the resolved
  result, not XML Atlas resolving the file the way that program does.
- **Decided (owner, 2026-11-18):**
  - Generic rules in a file the user keeps, `xml-atlas-references.json` next to the document
    or in a folder above it (superseded by D41: in the app's configuration folder): which elements and attributes are references, where each named
    context is, which contexts exist only at run time (REF-1). XML Atlas holds no team's
    names or rules; the repository is public, and the conventions belong to their owners.
  - Go to target (F12), references that find nothing in the problems list, and the preview
    pane, together (REF-3 to REF-5); the CLI has the check and the preview (REF-6).
  - Where the program merges several sections before it resolves, the preview takes the
    nearest one instead (`nearest`, up to an ancestor) and says which it used (REF-7).
- **How:** `crates/engine/src/references.rs` evaluates the small part of XPath the owner's
  file uses (REF-2), on the index, on demand: nothing is built when a file opens without a
  rules file, and opening is as fast as before. With rules, the check runs when the file
  opens, after edits, and after a pause in typing. The preview copies the source bytes and
  splices the imported ones in, so it keeps line breaks, non-ASCII text and the original
  formatting, re-indented. Measured on the owner's file: all references checked in 37 ms;
  the preview of a board with 954 references (1 MiB of text) in 31 ms.
- **Not chosen:** a general XPath engine (the files use child steps, `*`, `@`, `text()` and
  one kind of condition; a crate would be large for that); resolving the document in place
  (the owner does not want it, and it would change the file); naming the owner's conventions
  in XML Atlas (see above).
- **Added (owner, 2026-11-18):** choosing exactly which specialization is resolved: the pane
  lists the sections of a `nearest` context around the selection, and a chosen one comes
  first, then the reference's own, and no other (REF-9). It needs no new rule: the sections
  are the elements the rule already names.
- **Revisit when:** a second user's conventions do not fit the rules file, or XInclude
  (`docs/spec/open.md`) is built: it could be one more kind of rule.

## D33: A tree of the resolved document

- **Problem:** the preview pane (REF-5) shows the selected element with its references
  resolved as text. A large resolved element is hard to take in as text; the owner
  (2026-11-19) wants to see it as a tree too, optionally, beside it.
- **Proposed and decided:** a pane of its own right of the preview, so the window reads
  *main tree | text | preview text | preview tree*. It shows the preview's text as a tree:
  - The preview's text is indexed as a document of its own, with the engine's usual index,
    once when the preview is made again (selection, edit or chosen section), never per frame.
    The preview stops at 4 MiB; the index takes a few milliseconds for that. A preview cut
    there is not well-formed at its end; the index keeps the elements before the cut, as it
    does for a broken file (OPEN-10).
  - The rows, icons, keys and expanding are those of the main tree (TREE-1 to TREE-14),
    drawn by the same code. It follows the main tree's settings: attribute rows (TREE-1) and
    the font (TEXT-25). It is read-only: no rename, delete or move.
  - The `XML Atlas:` notes of the preview (REF-5) are comment rows, as any comment is.
  - Clicking a row scrolls the preview text to that element and marks its lines, as a click
    in the main tree does in the text (TEXT-6). It does not select anything in the main tree
    or the file: an imported element has no place there, and taking an element that does
    back to the file is a later step.
  - Shown when View > *Preview tree* is on (remembered, WIN-3; in Settings > Views, WIN-11)
    and the preview is on; off by default. Like every side pane it is cut to its width and
    resizable (#72, #112).
- **Cost:** with the pane off, nothing. With it on: one index of at most 4 MiB per new
  preview, and the tree's rows on screen per frame (rule 2 of `AGENTS.md`).
- **Not chosen:**
  - *Text | Tree* tabs in the preview pane, or the tree above the text in it: the owner wants
    both side by side.
  - Selecting the element in the main tree from the resolved tree: later, for elements that
    are in the file; the preview does not yet record where an imported element came from.
  - Settings of its own for the resolved tree: it follows the main tree's.
- **Decided (owner, 2026-11-19):** a separate pane right of the preview text; a click
  scrolls the preview text only, at first; the main tree's settings. The rule is REF-10 in
  `docs/spec/references.md`.
- **Added (owner, 2026-11-19):** a right-click on a row of the resolved tree offers *Show in
  the main tree*, where possible. The preview now records the runs of its text it copied
  from the file (`Preview::copied`, `Preview::source_of`), so an element's start tag maps
  back to the element it was copied from, an imported one included; what the preview wrote
  itself maps to nothing. Shown means marked, as a click in the grid marks an element: the
  selection stays, so the preview does not switch to the element shown.

## D34: Repeated elements in the preview, and rules switched off in the file

- **Problem:** some programs read an element with a count attribute as that many copies of
  it. The owner's team (2026-11-19) writes `<AI Count="4">` for four channels `AI0` to `AI3`
  (their names for the attributes differ; the names here are examples). The preview (REF-5)
  shows the element once, as written, so it does not read as the program sees it.
- **What such a program does,** observed in the owner's team's code (not copied): after
  resolving imports, and before imports it resolves later,
  - an element with the count attribute (`N`) and optionally a start attribute (`S`, default
    0) is replaced by `N` copies named its name followed by `S` to `S+N-1`; `N` = 0 removes
    it. Count elements inside it are expanded first, with their own numbers;
  - inside each copy, placeholders in attribute values and in text are replaced by the
    copy's number, or by the number less `S` (its relative number); a text `name$(index+n)`
    or `name$(index-n)` becomes `name` followed by the computed number;
  - an element inside a copy with the suffix attribute (not `"false"`) gets the number
    appended to its name, or the relative number when it also has the relative attribute;
  - the count, start, suffix and relative attributes are removed from the copies.
- **Proposed and decided:**
  - A `repeat` rule in the rules file names the attributes and placeholders, so no team's
    names are in XML Atlas (D32):
    `"repeat": {"count": "Count", "start": "Start", "suffix": "Suffix",
    "relative": "Relative", "index": ["[Index]", "$Index"],
    "relative_index": ["[RelativeIndex]", "$RelativeIndex"]}`.
  - The engine expands the resolved preview in that order (after the imports), so the CLI's
    `preview` does too (`AGENTS.md` rule 4), with `--no-repeat` to leave it out.
  - On by default where the rules have a `repeat`. The preview pane has *Expand repeats*,
    shown only then, to switch it off and on for the session.
  - The preview tree (REF-10) shows the copies; *Show in the main tree* on a copy, or on
    anything inside one, goes to the element it was copied from.
  - The problems list (REF-4) is about the file as written: it does not expand.
  - **Every rule can be switched off in the file:** an object rule (an include, the
    attribute references, a context, `repeat`) with `"enabled": false` is read and left
    out, as if it were not there; `true` or no key keeps it. For `repeat` the key also sets
    where *Expand repeats* starts.
- **Cost:** only when the preview is made again; the copies count towards its 4 MiB limit.
- **Not chosen:**
  - The owner's team's attribute names built in: against D32.
  - Expanding in the problems list: it reports the file as written (owner).
  - Off by default: the preview is meant to read as the program reads (owner).
- **Decided (owner, 2026-11-19):** on by default; the rule as sketched; not in the problems
  list; every rule can be switched off with `"enabled": false`. The rule is REF-11 in
  `docs/spec/references.md`.

## D35: An XPath query panel

- **Problem:** Find (TEXT-12) searches text. It cannot ask "every `order` whose `status` is
  `open`", "how many `item`s", or "the `price` of the order with id 42". Those questions are
  XPath, which most people who work with XML already know from other tools. The roadmap has
  had the XPath query panel since D20; the references (D32) evaluate only a small part of it
  (REF-2).
- **What it should do:**
  - Type an expression; see what it selects as a list: elements, attributes and text, each
    with its path and line, or a value (`count(//item)` → a number; `string(...)`;
    `true`).
  - Click a result: the tree selects the element, the text view shows it, an attribute or
    text is marked as a grid cell is (TEXT-11). F3 and Shift+F3 step through the results,
    as through Find's matches.
  - The same in the CLI (`xml-atlas-cli xpath FILE EXPR`) and for agents (an MCP tool
    `xpath`), from one implementation in the engine (`AGENTS.md` rule 4).
- **Which XPath:** 1.0 (W3C, 1999): the paths, the 13 axes, predicates, and the 27
  functions (`count`, `contains`, `starts-with`, `normalize-space`, `sum`, `position`,
  `last`, ...). It covers what is asked of a single document. XPath 2.0 and 3.1 add types,
  sequences, `if`/`for` and many functions, mostly for XSLT and XQuery; not now.
- **How it is built:**
  - **Our own XPath 1.0 evaluator**, `crates/engine/src/xpath.rs` (owner, 2026-11-19): a
    parser for the whole grammar of XPath 1.0, and an evaluator that works on the index
    itself. It grows out of the small part the references use (REF-2), which can then use it too.
  - **On the index, without a second tree** (`AGENTS.md` rule 2): a node is a `NodeId`, or
    an element and the number of one of its attributes, or the text folded into an element
    (D11). Names, attribute values and text are decoded only when a step or a function
    needs them, as the views do. Nothing is built when a file opens.
  - **What the index gives for free:** nodes are stored in document order, and a node's
    subtree is the run of nodes right after it. So the `descendant` axis (`//`) is a range of
    `NodeId`s scanned in order, a name test compares the name's bytes in the source without
    decoding, results come out sorted, and document order is a comparison of two numbers.
    A long scan can be split over threads, as indexing is (D25), if measuring shows it is
    worth it.
  - **Namespaces:** prefixes in the expression mean what the root element declares them as
    (most files declare all their prefixes there). Names without a prefix also match
    elements in the document's default namespace, as XPath 2.0's "default element
    namespace" does (owner, 2026-11-19); the help says so. `local-name()` and
    `namespace-uri()` work as in XPath 1.0.
  - **On a thread, and it can be stopped.** A query on 300 MiB may take seconds. It runs off
    the frame, with a spinner and *Stop*; the evaluator checks a flag between nodes.
  - **After an edit** the results are marked out of date and the query runs again on Enter.
  - **Results list:** virtualized (`AGENTS.md` rule 2), so a million results are drawn as
    fast as ten. Each result is a few bytes until it is drawn.
- **The window:** *Edit > XPath…* (Ctrl+Shift+X; Ctrl+Shift+F is Find in element) opens a
  query bar under the find bar, with the last expressions in a list, and a results panel at
  the bottom, as the Problems panel (owner, 2026-11-19). A result is a row with its path, its
  line and its value; a click selects it, F3 and Shift+F3 step through the results. A value
  (a number, a string, a boolean) is shown in the bar. A wrong expression says where it is
  wrong ("expected `]` at 14").
- **Steps** (owner, 2026-11-19):
  1. The engine, the CLI (`xml-atlas-cli xpath FILE EXPR`) and the window.
  2. The MCP tool `xpath` for agents, on files and on the open window (D27, D28).
- **Tests:**
  - The examples of the XPath 1.0 recommendation (§2.5 and the functions of §4) on a small
    document, each with the nodes or the value it must give.
  - Each axis, node test, operator and function, with non-ASCII names and text, CDATA, folded
    text, a byte order mark and CRLF line endings (`AGENTS.md` rule 6).
  - Namespaces: prefixes, the default namespace, `local-name()`.
  - Wrong expressions: the message and the position.
  - Stopping a query.
  - The results checked by hand against `xmllint --xpath` (libxml2) on the fixtures; not in
    CI.
- **To measure** on `fixtures/orders.xml` (100 MiB) and `orders-300.xml`:
  - `//order[@id='20000']`, `count(//item)`, `//order[customer/name[contains(., 'Sons')]]`,
    `//*`, `sum(//price)`: time and memory of each;
  - the target: the first three in about a second at 300 MiB;
  - the open benchmark before and after; nothing should change at open.
- **Not chosen:**
  - The crate `xpath-eval` (0.4.0, MIT, no dependencies). It evaluates XPath 1.0 on any tree
    through its traits and passes the Jaxen test corpus, but it is young (0.2 to 0.4 in
    November 2026) and one person's project, and it walks axes node by node through
    iterators, so it cannot use the index's ranges (owner, 2026-11-19: our own).
  - `sxd-xpath`, `xee-xpath` (XPath 3.1), `xrust`: each evaluates only on its own tree. That
    tree would be built at every query or kept beside the index: several times the file's
    size in memory and seconds of building at 300 MiB, against `AGENTS.md` rules 1 and 2.
  - XPath as an option of Find: Find's matches are places in the text, and its options
    (match case, whole word) mean nothing to XPath; a value such as `count()` has no place.
  - The results in the grid, one row per element (owner, 2026-11-19: a results panel).
  - XPath 2.0 and later: types, sequences, `if` and `for`, mostly for XSLT and XQuery. 1.0
    is what most people know and type.
- **Decided (owner, 2026-11-19):** our own XPath 1.0 evaluator on the index; a query bar and
  a results panel; names without a prefix match the default namespace too; first the
  engine, the CLI and the window, then the MCP tool.
- **Built, step 1 (2026-11-20):** `crates/engine/src/xpath/` (the parser, the evaluator and
  the results), `xml-atlas-cli xpath`, and Edit > XPath… (`apps/xml-atlas/src/xpath_bar.rs`);
  the rules are XP-1 to XP-6 in `docs/spec/xpath.md`.
  - **A query on a thread needs the document while the window goes on.** `Document` now
    holds its source and index as `Arc`s; a query takes a reference to both. An edit
    changes them through `Arc::make_mut`, which copies only while a query still holds them;
    otherwise it changes them in place as before. The reparse job (D19) shares the source
    the same way instead of copying it.
  - **Results belong to one index.** They hold a `Weak` reference to it. An edit replaces
    the index or moves it, so the results know they are out of date and are not used.
  - **The index keeps no text that is only whitespace**, so XPath here sees none (XP-2).
  - **Measured** (Windows 11, release builds): the five queries above on 300 MiB in 284,
    83, 409, 98 and 170 ms; on 100 MiB in 92, 28, 145, 37 and 55 ms. The counts agree with
    counting the tags. Opening (`xml-atlas-cli stats fixtures/orders.xml --repeat 5`, two
    rounds): median 78 and 66 ms before, 75 and 70 ms after, index memory 96.6 MiB both; no
    difference beyond the noise.
- **Built, step 2 (2026-11-20):** the MCP tool `xpath` in `crates/mcp`, so agents have it
  on files and on the open tabs alike (XP-7). Nodes come in pages, as `find`'s matches do.
  In the window the tool runs on the window's thread, as every agent call does (D28), so a
  query is stopped after 5 seconds; the slowest of the five above takes 0.4 s at 300 MiB.

## D36: A diagram of the document's structure

- **Problem:** a user asks for a diagram view of the XML structure, "for getting a quick
  overview" (issue #121), with three additions:
  - collapsing it to a depth (#122);
  - Find's match highlighted in it (#123);
  - exporting it as PNG or SVG (#124).

  The tree shows every element: on the 10 MiB fixture that is 235,000 rows, and the shape of
  the document is hard to see in them. The grid shows one level at a time.
- **What it draws (owner, 2026-11-20): the structure, not every element.** One box per
  distinct path of element names, with how many elements have it. On
  `fixtures/orders-10.xml` (10 MiB) that is nine boxes:

  ```
  orders ── order ×21,183 ─┬─ customer ×21,183 ─┬─ name ×21,183
                           │                    └─ email ×21,183
                           ├─ items ×21,183 ── item ×63,543 ── price ×63,543
                           └─ note ×2,118
  ```

  - The count is that of the file, at any size. A box also says, in a smaller line, which
    attributes its elements have (`@id @date @status`) and whether they hold text.
  - Every element would be another diagram: 235,000 boxes on 10 MiB and about 7 million on
    300 MiB. It could not be laid out or read, and the tree already shows them.
- **The interaction (#121):**
  - **A click on a box** selects the first element with that path in the tree, grid and
    text. Clicking again, or F3, goes to the next one (as XPath's results do: the path is
    the query `/orders/order/items/item`).
  - **The other way round:** the box of the element selected in the tree, grid or text is
    marked, and the diagram scrolls it into view.
  - **Collapsing:** a box with children has a toggle. The diagram remembers per tab what is
    collapsed, while switching views and tabs.
- **Depth (#122):** a *Levels* control above the diagram: 1, 2, 3 … or all. It collapses
  every box below that depth; the toggles open single ones again.
- **Find and XPath (#123):** the box holding Find's outlined match is marked as the selected
  one is. XPath's results mark each box that has results, with their number (`item ×63,543,
  12 found`), so a query shows where in the structure its nodes are.
- **Export (#124):** *Export as SVG…* writes the whole diagram as it is collapsed, as text we
  write ourselves: no library, any size, sharp at any zoom. *Export as PNG…* is question 3.
- **How it is built:**
  - **The structure** comes from one walk of the index, on a thread, when the diagram is
    first shown, not when a file opens (`AGENTS.md` rule 1). Each element's path is its
    parent's path and its name: a table from (parent path, name) to path, with counts and
    attribute names. Names are compared as bytes, as XPath does (D35). Expected: well under
    a second at 300 MiB, to be measured. After an edit it is walked again after a pause, as
    the references are checked (REF-4).
  - **A recursive document** (sections in sections) has a path per depth. Past 2,000 paths
    the walk stops and the diagram says so; the boxes it has are right.
  - **Layout:** a tree drawn left to right, each box's children stacked to its right. It is
    laid out once per change and drawn with egui's painter: only the boxes on screen, with
    zoom (Ctrl+wheel, Ctrl+0) and panning (drag, wheel).
- **Not chosen:**
  - Every element as a box (owner, above).
  - Graphviz or another layout program: a C dependency or a process to start, for a layout
    that is a tree.
  - A diagram crate for egui: the layout of a tree is short, and what a box shows and does is
    ours.
- **Questions for the owner:**
  1. **Where does it show?**
     - (a) in place of the text, switched with *View > Diagram* and a key still to choose
       (Ctrl+D is Duplicate);
     - (b) in a pane of its own, right of the text, as the reference preview is (REF-5);
     - (c) in a window of its own, beside the main window;
     - (d) in place of the grid, so the text stays in view (the owner's, see below).

     Proposed: (a). The diagram needs width; the tree and grid stay.
  2. **What a box says:** the name and the count, and in a smaller line the attributes and
     whether it holds text, as proposed? Or the name and count only?
  3. **PNG:** (a) the whole diagram, drawn by our own code into an image (more code, any
     size); (b) only what is on screen, from a screenshot of the window (little code); or
     (c) SVG only (a browser or an image program makes a PNG from it). Proposed: (c) first,
     and (a) later if it is asked for.
  4. **Steps:**
     1. the diagram with clicks both ways and collapsing (#121);
     2. levels (#122) and the marks of Find and XPath (#123);
     3. the export (#124).

     Proposed: in this order, one pull request each.
  5. **Answering the issues:** a short comment on #121 that it is planned and how (the
     structure with counts, and why not every element), linking this decision? It goes out
     under your account.
- **Decided (owner, 2026-11-20):** in place of the grid, switched with *View > Diagram* (the
  key is chosen when it is built): the user wants the diagram instead of the grid while the XML
  stays visible. It takes the grid's place above or below the text, as the layout says (WIN-1),
  and shows there also when View > Grid has hidden the grid. (First answered "in place of the
  text"; changed by the owner the same day.) A box shows the name, the count, the attributes
  and whether the elements hold text; SVG export first, a PNG of the whole diagram later if it
  is asked for; a comment on #121 once this is on `main`. The steps were not asked separately
  and are taken as proposed: #121, then #122 and #123, then #124.
- **Built, step 1 (2026-11-20):** `crates/engine/src/structure.rs` (the walk, a path's
  elements one after another, the box of an element), `xml-atlas-cli structure`, and View >
  Diagram (`apps/xml-atlas/src/diagram.rs`) with a toolbar button.
  - **The key is Ctrl+Shift+D**, taken before Ctrl+D (Duplicate), which would match it too,
    as Ctrl+Shift+S is taken before Ctrl+S.
  - **F3** steps through the clicked box's elements while the find bar is closed and the
    XPath bar shows no results; those keep F3 while they are open.
  - **Going to the next element** does not use XPath: it walks the index from the
    selection, skipping every subtree whose path is not on the way to the box's path.
  - **The structure belongs to one index**, as XPath's results do (D35): it holds a `Weak`
    reference to it. After an edit the old boxes stay, marked out of date, and the walk
    runs again after 0.5 s without edits. Collapsed boxes are kept by their path of names,
    so they stay collapsed when the walk runs again.
  - **The layout** is a loop, not recursion: 2,000 paths deep would need a stack the
    window's thread does not have (D35's lesson).
  - **Measured** (Windows 11, release build, `xml-atlas-cli structure`, three runs): 8–10 ms
    on 10 MiB, 79–82 ms on 100 MiB, 238–243 ms on 300 MiB. About 60 % of it is reading the
    attribute names. Opening is unchanged: the walk runs only when the diagram is shown.

- **Built, step 2 (2026-11-21):** *Levels* and the marks of Find and XPath.
  - **Levels** is a drop-down above the diagram: All, 1, 2 … up to one less than the deepest
    path. Choosing a level fills the same set of collapsed boxes the toggles use, so a toggle
    afterwards opens or closes one box and the control says *custom*. The level is not kept
    for paths a later walk adds.
  - **Find's match** is the box of the innermost element holding the outlined match's first
    byte (`Structure::path_at`, a binary search over the nodes). It is the same outline the
    text view draws (`Open::mark`), so a marked grid row or tree row marks its box too.
    The selected element's box keeps its own look.
  - **XPath's results** are counted per box on the window's thread, once per query
    (`Structure::count_by_path`, one walk up per result), not per frame. A box with results
    is outlined and has a badge `N found`; the tooltip says it too. An attribute, a
    namespace or a folded text counts for its element, a text node, comment or instruction
    for its parent. Not measured on a result list of millions.

- **Built, step 3 (2026-11-21):** *Export as SVG…* in the diagram's header
  (`apps/xml-atlas/src/diagram_svg.rs`).
  - It writes the diagram as it is laid out and collapsed at that moment, in the window's
    colours, with no marks of the selection, Find or XPath. The file is chosen in a save
    dialog (`<file>-structure.svg`, in the document's folder) and written atomically.
  - The text is the window's measure: each piece (name, count, attributes, "text") is its
    own `<text>` with the width egui measured as `textLength`, so a viewer with another
    font keeps the boxes and the toggles in their places.
  - The PNG of the whole diagram stays unbuilt, as decided; a browser or an image program
    makes one from the SVG. Checked once by rendering a sample with Inkscape.

## D37: The menus, next to XML Marker 2.2's

- **Problem:** the owner asks how the menus compare with XML Marker 2's and how they could be
  easier to use. XML Marker's are known from what was observed (XM-61; XM-2 and XM-3 for
  1.1), ours from the code.
- **Compared:**

  | Menu | XML Marker 2.2 | XML Atlas | What follows |
  |---|---|---|---|
  | File | New in four kinds, Open, Save, Save As, with Encoding, Exit | New, Open, Open Recent, Save, Save As, Save as UTF-8, Close, Settings, Quit | Ours is complete for XML. *Save as UTF-8* comes and goes with the file; Quit has no key |
  | Edit | Undo, Redo, clipboard, Delete, Select All, Find, Replace | The same without Delete, and XPath, Format, Compact, Format options | Three jobs in one menu: editing, querying, reformatting |
  | View | 6 entries, all about what is shown | 16 entries and 2 headings, in no order by pane | The hardest menu to scan |
  | Options | Font, Indent, consolidation, wheel, Tabs, Parse as, Encoding | None; File > Settings (D31) | Ours is better: one window |
  | Navigate | Drill Up and Down, Locate in tree, Indent, Reparse, four bookmark commands | None | Drill Up and Down are on the toolbar and keys only |
  | Help | Documentation, What's new, updates, About | Help, updates, About | Enough |

- **What is hard to find or wrong today:**
  1. In no menu of the menu bar: Drill Up and Down, Find next and previous, the next and
     previous tab, Go to reference target, and Rename, Duplicate, Delete and Move of an
     element (right-click or keys only).
  2. Keys are typed into the labels with spaces, so they do not line up, and on macOS they
     say Ctrl where Cmd is meant.
  3. Grid and Diagram are two checkboxes for one place: with Diagram on, Grid is greyed out
     in View but not in Settings. The cell pane's checkbox differs in the same way.
  4. *Word wrap* in View may change one tab (TEXT-23), in Settings every tab, under one name.
- **Proposed:** six menus. `>` is a submenu, `|` a separator.

  ```text
  File      New  Open  Open Recent>  | Save  Save As  Save as UTF-8 | Close | Settings | Quit
  Edit      Undo  Redo | Cut  Copy  Paste  Select All
            | Find  Find next  Find previous  Find in element  Replace
            | Element> (Rename  Duplicate  Delete  Move up  Move down)
  View      Layout> | Grid  Diagram  Cell pane | Attributes in the tree  Collapse the tree
            | Word wrap | Toolbar  Font  Theme>
  Navigate  Drill Up  Drill Down | Go to reference target | Next tab  Previous tab
            (later: Go to line, bookmarks)
  Tools     XPath | Format  Compact  Format options
            | References> (Resolve references  Preview  Preview tree  Rules files)
  Help      XML Atlas Help | Check for updates now | About
  ```

  - An entry that does not apply is greyed out, with the reason on hover; it does not vanish.
  - Keys are set with `Button::shortcut_text` from the same constants that handle them, so
    they align and read Cmd on macOS.
  - Grid and Diagram exclude each other, in View and Settings alike; both off is no pane.
  - Two new keys: Ctrl+Q for Quit and Ctrl+I for Format (Indent in XM-3); both are free.
  - The toolbar and all existing keys stay.
- **Not taken from XML Marker:** an Options menu (D31); Indent under Navigate, where nobody
  looks for a command that changes the file; Locate in tree (WIN-5); Reparse (XM-72).
- **Order of work:** (1) aligned keys and the disagreements 3 and 4, with no entry moved;
  (2) Navigate and Edit > Element; (3) Tools and the shorter View; help and README with each.
- **Questions for the owner:**
  1. The six menus? Proposed: yes.
  2. References whole under Tools, instead of in View as now? Proposed: yes.
  3. Ctrl+Q and Ctrl+I? Proposed: yes.
- **Decided (owner, 2026-11-22):** as proposed. The six menus, References whole under
  Tools, Ctrl+Q and Ctrl+I, in the order of work above. Not built yet.
- **Built, step 1:** aligned keys (WIN-13), Ctrl+Q and Ctrl+I with them as they move no
  entry; Grid and Diagram exclude each other (WIN-1); *Word wrap in this tab* and *Word wrap
  in every tab* (TEXT-13). One rule added to the proposal: switching the diagram off brings
  back the grid it replaced, so that its key pressed twice changes nothing.
- **Built, steps 2 and 3:** the six menus (WIN-14), in `apps/xml-atlas/src/app/menus.rs`.
  Two entries are newer than the proposal: *Expand the tree on a click in the text* stands
  with the tree's options in View, and Go to line, which was to come to Navigate later,
  moved there from Edit, beside the bookmarks that were already there. Tested headless, with
  clicks and arrow keys on the drawn menus.

## D38: The layout options, next to XML Marker 2.2's

- **Problem:** the same comparison for the layout. What is known of XML Marker's: the four
  panes of 1.1 (XM-1) and 2.2's View menu and Stat panel (XM-61, XM-63). Whether 2.2 can
  move or remember panes was not observed.
- **Compared:**

  | | XML Marker | XML Atlas |
  |---|---|---|
  | Tree | Top left | Left, full height |
  | Clicked cell | Bottom left, under the tree, can be hidden | Right of the grid, taking 35 % of its width |
  | Text and grid | Text above grid | The same, or grid above text |
  | Errors | A tab beside the tree, with the count | A pane under the text; cannot be closed |
  | Diagram, preview | None | Diagram in the grid's place; preview and its tree right of the text |
  | Switches | Toolbar, status bar, line numbers, browser, Stat | Layout, grid, diagram, cell pane, preview, its tree, toolbar |

- **Measured** (the app's own layout run headless, default sizes, one problem listed when
  references are on), the text view in points:

  | Window | Plain | With preview | With preview and its tree |
  |---|---|---|---|
  | 1280 × 820 | 918 × 369 | 493 × 311 | 389 × 311 |
  | 1920 × 1080 | 1558 × 512 | 845 × 464 | 671 × 454 |

  With both reference panes the text keeps 42 % of its width.
- **A defect:** window and pane sizes are lost on exit. D31 says egui keeps them, but
  `eframe` is built without its `persistence` feature, so nothing does.
- **Proposed, most useful first:**
  1. **Remember sizes:** the window's size and maximized state, and each pane's share, in
     `settings.toml`, with *Reset layout*. Not through eframe's feature: it stores all of
     egui's memory, scroll areas included, and adds a second settings file.
  2. **Preview position:** *right* (as now) or *in the grid's place*, where it has the full
     width and the text keeps its own.
  3. **Text, Tree or Both** in the preview's header, instead of a second switch in a menu.
     Tree alone is not possible today.
  4. **Cell pane position:** beside the grid (as now) or under the tree, as in XML Marker.
  5. **Problems fold** to their header with the counts, remembered, so warnings do not cost
     text lines all the time.
  6. **Every optional pane has a close button,** which switches the same setting as View.
  7. **Maximize a pane** by a double-click on its header and back; F6 and Shift+F6 move the
     keyboard between tree, text and grid.
  8. **Pictures in Settings > Views** for the layouts, drawn as the themes' are (WIN-12).
- **Not chosen:** free docking (a new dependency, and many states to test, for a need these
  options cover); hiding the status bar (one line, and it carries the errors).
- **Questions for the owner:**
  1. 1 first, as a fix of its own? Proposed: yes.
  2. Which of 2 to 8? Proposed: 2, 3, 5 and 6 next; 4, 7 and 8 later.
- **Decided (owner, 2026-11-22):** as proposed. 1 first, as a fix of its own; then 2, 3, 5
  and 6; 4, 7 and 8 later.
- **Built, item 1 (2026-11-22, #146):** `apps/xml-atlas/src/sizes.rs`, WIN-3: the window's
  size and maximized state and each pane's share, with View > Reset layout. Tested headless;
  maximize at the start not yet seen on Wayland, X11 or Windows. Items 2 to 8 are not built.

## D39: Help with editing the rules file

- **Changed by D41 (owner, 2026-11-22):** the check and the editor stand. They work on the
  files in the configuration folder and are opened from Settings > References, not from
  *References > Edit rules*.
- **Problem:** the rules file (REF-1) is written by hand from the spec, and its mistakes are
  silent. Tried with `xml-atlas-cli references`:

  | Mistake | What happens today |
  |---|---|
  | `include` for `includes` | No error, 0 references |
  | An element name misspelt | No error, 0 references |
  | Some other JSON file | Accepted as rules, 0 references |
  | A key given twice | The last wins, no warning |
  | `enabeld` for `enabled` | Ignored, the rule stays on |
  | A byte order mark | "expected value at line 1 column 1" |
  | A trailing comma | An error with line and column: good |

- **Proposed:** help around the text of the file, in four steps, each a pull request.
  1. **A check in the engine** (`references::check`): syntax errors with line and column,
     unknown keys with the nearest known one, keys given twice, and per rule how many places
     in the document it matches, with those that match nothing marked. A byte order mark is
     accepted. `xml-atlas-cli references --check` prints the same (rule 4 of AGENTS.md).
  2. **Which rules are in use:** the preview's header names the file and the check's result.
     A change of the file on disk is read again (the watch of OPEN-14).
  3. **References > Edit rules:** a window with the file's text, the check beside it as one
     types, and Save (atomic; byte order mark and line endings kept; asks when the file
     changed on disk). Without a file it offers to create an empty one; the help gets an
     explained example, as JSON has no comments. *Insert* adds a template of each kind of
     rule at the cursor.
  4. **Rules from the selection:** on an element, *Make this an include* (asks for the
     attribute) and *Make this a context* (by path, or nearest up to a chosen ancestor). It
     inserts text and shows it before saving. A JSON Schema in the repository gives other
     editors completion.
- **Not chosen:**
  - A form that reads and writes the JSON: it would lose the user's layout and key order
    (rule 7 of AGENTS.md).
  - The file as a normal tab: tabs, tree and grid are for XML; a JSON document kind is a
    roadmap item of its own.
  - YAML or TOML for the sake of comments: a second format for one file.
- **Tests:** each mistake above gives its message, in a file with a byte order mark, CRLF
  and non-ASCII names; saving an unchanged file leaves its bytes; opening a document costs
  nothing more while references are off.
- **Questions for the owner:**
  1. The four steps, in this order? Proposed: yes; 1 and 2 already remove the silent cases.
  2. A window, as proposed, or a pane in the preview's place? Proposed: a window.
- **Decided (owner, 2026-11-22):** as proposed. The four steps in this order, the editor
  as a window.
- **Built, steps 1 to 3 (2026-11-22, #148):** `crates/engine/src/references/check.rs` and
  `apps/xml-atlas/src/rules_editor.rs`, REF-14 and REF-15, but for one part of step 2, the
  check's result in the preview's header.
- **Built, the rest of step 2 (2026-11-22):** `apps/xml-atlas/src/rules_status.rs`, REF-16.
  Step 4 is not built.

## D40: The rules file under another name

- **Superseded by D41 (owner, 2026-11-22), before it was built:** there is no search beside
  or above the document any more, so no name to look for and no `format` line. The defect
  below goes with the search.
- **Problem:** the owner wants rules named after their document, `catalog.json` for
  `catalog.xml`, instead of `xml-atlas-references.json`.
- **A defect to fix first:** the search walks the path as given. Opened as
  `../other/doc.xml` from a folder with rules, a document gets that folder's rules although
  they are neither beside nor above it (tried with the CLI). On Linux and macOS
  `std::path::absolute` keeps `..`. The search must start from the resolved path.
- **Proposed:** the first that exists wins.
  1. A file chosen for this document with *References > Rules file > Choose*, remembered
     for this user until *Use the automatic one*. Any name, any folder.
  2. In the document's folder, then each folder above, the nearest folder winning:
     1. `<name>.json`, the document's name with `.json` for its last extension, **only if**
        it contains `"format": "xml-atlas-references"`;
     2. `xml-atlas-references.json`, as today, which needs no such line.

  ```json
  {
    "format": "xml-atlas-references",
    "includes": [{"element": "Include", "attribute": "src"}]
  }
  ```

  - The line is needed because any JSON object passes as rules today: without it an
    unrelated `catalog.json` would silently replace the real rules.
  - A document of the same name in a subfolder finds the same file, as it does today.
  - Colleagues and the CLI need no setting. The engine does the search for both; `--rules`
    still names a file outright.
- **How the user changes the name:** *References > Rules file > Name after this document*
  shows old and new name, adds the `format` line and renames after a confirmation; it never
  replaces an existing file. By hand: add the line and rename. A 1.2 program ignores the
  line, but does not look for the new name.
- **Until it is built:** `ln -s catalog.json xml-atlas-references.json` in that folder works
  with 1.2 (tried), and `xml-atlas-cli references FILE --rules catalog.json` takes any name.
- **Order of work:** (1) the defect, with a test that fails without the fix; (2) the named
  file, in the engine, the CLI and the window; (3) Choose and Name after this document.
- **Not chosen:**
  - Any `<name>.json` without the line: see above.
  - A setting for the file's name: every colleague and the CLI would have to set it too.
  - `<name>.references.json`: needs no line, but is not the name asked for.
  - A processing instruction in the XML that names the rules: it changes the document.
- **Questions for the owner:**
  1. The required `format` line? Proposed: yes.
  2. For a document opened through a symbolic link, search from the link's target? Proposed:
     yes, as Open Recent already stores the target.
- **Decided (owner, 2026-11-22):** as proposed. The `format` line is required
  for a file named after its document; the search starts from a symbolic link's target.
  Not built: superseded by D41.

## D41: Rules files in the app's configuration folder

- **Problem:** the owner, later on 2026-11-22: finding rules beside or above the document is
  too implicit, and it means deploying JSON files into other repositories. They want the
  files in the app's configuration folder, a list of them in Settings, each switched on and
  off, and each editable there.
- **Measured** (the owner's rules on about 1,160 XML files of one source tree): of the five
  files with the document's name, four resolve and one, of another structure, gets only
  false warnings. A general `attribute_references` rule reports `[A1:B9]` and `[12:30]` in
  an unrelated document as broken references. So "every active file applies to every
  document" is not enough, and neither is the document's name alone.
- **Decided (owner, 2026-11-22):**
  1. **The folder:** `references/` beside `settings.toml` in the configuration folder, one
     JSON file per set of rules, in the format of REF-1.
  2. **The list:** Settings > References shows every file with a checkbox, *Use for*, its
     error if it has one, and *Edit*; and New, Import (a copy), Duplicate, Rename, Delete
     and Open folder.
  3. **Which file applies:** one that is switched on and whose *Use for* matches the
     document. An empty *Use for* matches nothing, `*` every document. Of several, the upper
     one wins, and the preview's header names the file in use. Files are never combined.
  4. **The search beside and above the document is removed,** without a fallback. The CLI
     uses the same list, or the file given with `--rules`. The release notes say to import
     the file once.
  5. **Editing:** the check and the editor window of D39.
- **How:**
  - *Use for* holds patterns separated by `;`. `*` stands for any characters, folders
    included, `?` for one. A pattern without `/` is compared with the file's name, one with
    `/` with the end of its resolved path from a folder's start: `catalog*.xml`,
    `*/XML/*catalog.xml`.
  - The switches, patterns and order are kept in `references.json` beside the folder, not
    in `settings.toml`, which an older XML Atlas rewrites without the keys it does not know,
    and not in the rules files, whose bytes are the user's.
  - The engine does the folder, the matching and the check; the window and the CLI call it.
  - A file that is new in the folder is off until the user switches it on. With references
    off (REF-12) the window reads none of this until Settings > References is shown.
- **Order of work, a pull request each:** (1) the folder, the list, the matching, import,
  and the end of the search; (2) the check and the editor.
- **Not chosen:**
  - Every active file for every document: see above.
  - A choice per document: each copy and checkout would need its own.
  - The old search as an option, or only to offer an import: a second mechanism to keep.
- **Cost accepted:** a team that shares its rules through git imports them on each machine,
  and again when they change.
- **Built (2026-11-22):** step 1 in #147 (`crates/engine/src/rules_folder.rs`, REF-13), step 2
  in #148 (D39). #145, a fix of the search that is removed, was closed unmerged.

## D42: A table of our own, virtualized sideways

- **Problem:** `egui_extras`' table makes a `Ui` for every column of every row on screen,
  those out of sight included: about 1 µs a cell, so 13 rows of 136 columns took 2 ms a frame
  where those in sight take 0.3 ms. That breaks rule 2 of AGENTS.md.
- **Decided (owner, 2026-11-22):** virtualize the grid's columns.
- **How:** `apps/xml-atlas/src/table.rs`, for the grid and the cell pane, in place of
  `egui_extras`, which nothing else used. Its layout, resizing and hover rules and its ids
  are adapted from that crate, so the views look and act as before; egui's MIT notice is in
  `licenses/egui-MIT.txt` and in the packages' third-party licenses.
  - Cells get their ids from row and column, so they stay when columns scroll.
  - Drawn besides those in sight: the first column, which sizes itself by its contents, the
    edited column, the one being scrolled to, one with an open menu, and a dragged border.
  - Drawn one frame longer: what was in sight, as egui finds hover and clicks by the last
    frame's rectangles.
  - A frame still adds up the widths of all columns, once: arithmetic, no widgets.
  - One difference: the cell pane's first frame measures only its first column, where
    `egui_extras` squeezed every column for that frame. So a pane scrolled sideways keeps its
    place when what it shows changes, where before it could jump back.
- **Not chosen:**
  - Spacer columns in `egui_extras`' table: its ids come from the position among the columns
    given, so menus, editors and clicks would move to other cells while scrolling.
  - A patched copy of `egui_extras`: a fork to carry through each egui update.
- **Cost accepted:** egui updates no longer bring the table's fixes; Tab and screen readers
  reach the columns in sight, as they already reached only the rows in sight.
- **Revisit when:** `egui_extras` virtualizes columns itself.

## D43: Download and install from the update notice (PKG-8)

- **Problem:** there is no apt or dnf repository and no updater, so every update is a visit
  to the releases page, a choice among five files and an install by hand.
- **Decided (owner, 2026-11-22):** a click in the notice downloads the right file and hands
  it to the system's installer. This replaces D30's "downloading or installing the update:
  PKG-7 rules it out" for that click only; the check still does neither.
- **Not chosen:** installing ourselves with `pkexec apt`, `dnf` or `msiexec` (a root process
  started on a downloaded file, code per distribution); an apt and dnf repository (a signing
  key and hosting, Linux only; still possible later).
- **How:** `apps/xml-atlas/src/installer.rs`.
  - The SHA-256 is the `digest` in GitHub's answer, computed with `ring`, which rustls
    already builds. It finds a damaged download, not a taken-over GitHub account: the files
    are unsigned until PKG-13 and PKG-21.
  - The package is held in memory (at most 100 MiB, today up to 22) until it is checked, so
    no unchecked file is ever on disk and none is left by a cancel or a crash.
  - The installer is found before downloading, from a list of known ones: File Roller and
    Ark register for `.deb` and `.rpm` too.
  - A download of our own has no mark of the web or quarantine, so SmartScreen and
    Gatekeeper would pass it unseen. The mark is written, and the MSI is opened through
    Explorer rather than `msiexec`, which does not look at the mark.
- **An MSI for all users gets the link only.** The MSI starts for the current user
  (`ALLUSERS=2`, `MSIINSTALLPERUSER=1`) and looks for older versions before any dialog, and
  Windows Installer upgrades within the same context only: opened, it would leave the copy
  for all users beside a new one. `ALLUSERS=1` needs `msiexec`, so no SmartScreen. The same
  applies to opening the MSI by hand, to be confirmed on Windows.
- **App Center:** builds before October 2026 greyed out *Install* for a newer local `.deb`
  (ubuntu/app-center#2190); later ones compare the versions.

## D44: Split view for the text (#182)

- **Problem:** the text view shows one place of a document at a time.
- **Decided (owner, 2026-11-26):**
  - Split Up, Down, Left and Right. Every view shows the tab's own document.
  - Up to 4 views; any view can be split again, in any direction.
  - The commands are a *Split* submenu under View and in the text's right-click menu. With
    more than one view, each has a close button. No new keys.
  - The tree, the grid, Find and Go to line act on the view last typed or clicked in, which
    is marked.
  - Not remembered after quitting; each tab keeps its split while the program runs.
- **Assumed, not yet asked:** the direction is where the new view appears.
- **Next:** rules in `docs/spec/`. Not built.

## D45: UI localization, English and German (#183)

- **Problem:** the window's texts are English only.
- **Decided (owner, 2026-11-26):**
  - English and German. Settings offers *System*, *English* and *Deutsch*; English is the
    default.
  - The built-in guide is translated too.
  - In German, keys have their German names (*Strg*, *Umschalt*, *Entf*); macOS keeps its
    symbols and Cmd.
  - The CLI, the MCP tools and the logs stay English.
  - For how translations are stored, the comparison favours a check at build time: a
    missing or misspelled text fails the build or the tests, with little or no new
    dependency and fast lookup. Adding a language may take a code change.
- **Later (owner, 2026-11-26):** French and Spanish. The method has to carry four
  languages; only English and German are in this step.
- **Size** (2026-11-26, a count of string literals with words outside the tests; rough, as
  it takes some that are never shown and misses others): about 480 in the window, 87 with
  placeholders; about 170 in the engine, 112 with placeholders; the guide, 187 lines.
- **Compared,** in the versions current on 2026-11-26. *New* counts crate names, over all
  platforms, that are not in our `Cargo.lock`. Checks are as read in each crate's README or
  source; none of the crates was built or tried.

  | Method | New | A missing or wrong text |
  |---|---|---|
  | Our own: a file per language, read at build | 0 expected | Missing: English. Wrong: the build fails. To be built and shown |
  | Our own: all languages side by side in a Rust macro | 0 | The build fails, except a placeholder left out in one language (tried with a prototype in one file) |
  | `i18n-embed-fl` 0.10.1 with `i18n-embed` and `rust-embed` (Fluent files) | 23 | Name and arguments checked at build against the fallback language only; German needs a test of ours |
  | `rust-i18n` 4.2.4 (YAML, JSON or TOML files) | 20 | At run time: English if there is a fallback, else the key's name; a tool lists untranslated texts |
  | `fluent-templates` 0.15.1 | 19 | Files built in; no check of the keys at their use found in its README |
  | `fluent-bundle` 0.16.0 and a layer of our own | 7 | At run time; the checks would be ours to write |
  | `tr` 0.1.11 (gettext) | 14, among them the C library gettext | At run time |

- **Method** (decided, see the answers below): a file per language in our own format on TOML,
  `apps/xml-atlas/lang/en.toml`, `de.toml`, later `fr.toml` and `es.toml`, which the
  program's build script turns into Rust: one method of `Lang` per text.

  ```toml
  # en.toml
  save = "Save"
  go_to_line = "Go to line {line}"
  rows = { one = "{count} row", other = "{count} rows" }

  # de.toml
  save = "Speichern"
  go_to_line = "Gehe zu Zeile {line}"
  rows = { one = "{count} Zeile", other = "{count} Zeilen" }

  # In a view: ui.button(lang.save())
  ```

  - `en.toml` says which texts there are and which placeholders each has. A text another
    file lacks is shown in English; a plural that lacks a form it needs counts as lacking.
    The build names these as a warning and does not fail. It does fail, naming the file and
    the text, when a file cannot be read, has a text that English has not (a misspelled
    name), or has other placeholders. A misspelled name or a missing argument at a use does
    not compile, as the method does not match.
  - Which plural form a number takes is a small function per language in our code. French
    takes "one" for 0 and 1, English, German and Spanish for 1 only. French and Spanish
    also have a form for whole millions (*1 000 000 de lignes*), which their files may
    give and which is else "other".
  - A text without placeholders is a `&'static str`: no file read, parsing or lookup by key
    at run time. One with placeholders allocates as `format!` does today.
  - The language is passed along from the settings, not kept in a global, so that a test
    can draw in any language.
  - A translator edits one file and no Rust. A new language is its file, its plural
    function, its entry in the setting and key names, and its guide.
  - The parser: `toml` 1.2.1 is already built for the app's build script, through
    `embed-resource`; it would be named there directly. Whether its features suffice is to
    check. The generator has not been built or tried. It and its checks are our own code to
    keep.
  - Chosen over the Rust macro, proposed first for two languages: with four, every entry
    would hold four texts, each translator would edit Rust and the same files, and a
    placeholder left out would pass. Over Fluent: it has rules for grammar that our texts
    are not known to need, for 23 new crates; German completeness, and later French and
    Spanish, would need our own test. Our files are no standard format, so translation
    tools may not read them as they are.
- **The engine stays English** and does not learn about languages (decision above: CLI, MCP
  and logs are English). Its `Display` texts are what those show. For the window, what is
  now only a `String` has to say what it is: the problems' messages (parse errors
  included), why a reference finds nothing, XPath's messages and the names of undo steps
  become values with their parts, with `Display` giving today's English. Errors that are
  enums already need no change: the window matches them. Texts from outside (the operating
  system, `regex`, the XML parser library) are shown as they come, after a translated lead.
- **Keys:** `keys.rs` (WIN-13) gets German names, through egui's `ModifierNames` for the
  modifiers and our own names for keys such as *Entf*. French and Spanish get theirs the
  same way; whether they too are shown in their own words is decided when they are added.
- **Guide:** one per language, `help.de.md` beside `help.md`. A test checks that all have
  the same number of sections; whether they say the same is for review. With four
  languages, a change to the guide is four changes.
- **System:** reading the system's language needs a source on each platform. `sys-locale`
  0.3.2 is in `Cargo.lock`; whether it is built on all three is still to check. A language
  for which there is no file gives English.
- **Not covered:** the file dialogs, which are the system's and follow its language, and
  egui's own texts, not yet looked for. German, French and Spanish are often longer than
  English, so each dialog needs a look in each. Whether egui's fonts have every character
  they need (such as *œ*, *¿* and French's narrow space) is to check.
- **Order of work:** (1) rules in `docs/spec/`; (2) the build script, the setting and the menus,
  toolbar and Settings, to see the approach in use; (3) the window's other texts; (4) the
  engine's messages made into values, a part at a time; (5) the guide. Until (4) is done,
  German shows some English messages. A test cannot find a text that was never put in a
  language file; review has to.
- **Decided (owner, 2026-11-26),** on the proposal above:
  1. A file per language of our own, checked at build.
  2. The engine's messages become values: English in the engine, translated in the window.
  3. The order of work above, with English left in German until step 4.
  4. A language need not be complete: where it lacks a text, the English one is shown. This
     replaces, for a missing text, the failed build or test that the comparison was to
     favour.
- **Rules:** `docs/spec/language.md`, LANG-1 to LANG-18.
  Found in writing them: `sys-locale` is built for Linux, Windows and macOS already, through
  egui's system fonts (`cargo tree`); windows whose id egui takes from their title need an
  id of their own, or a new language would forget their state.
- **Built, step 2:** `lang/en.toml` and `de.toml` (232 texts), the generator
  `lang/generate.rs`, run by the build script and tested, `src/lang.rs`, and the setting.
  `toml` and `sys-locale` are named as dependencies; `Cargo.lock` gains no crate. Found:
  the program's fonts have no ↑ and ↓, so *Bild auf* and *Bild ab* (LANG-9), and the
  German menu names do not all fit the theme examples, which draw those that do.
- **Changed (owner, 2026-11-26),** after seeing step 2: the language has a section of its
  own in Settings, *Localization*, instead of the first row of Appearance (LANG-6).
- **Built, step 3, first part:** the bars and the status bar. A tab keeps the window's
  language (`Open::lang`) for what a command says without drawing. What can stay on screen
  is kept as a value and worded when drawn: what a search said, why a query or a download
  failed. Found: in German the Find bar's row does not fit beside the tree in the program's
  own window size, so what a search says then goes under the bar.
- **Built, step 3, second part:** the dialogs. Why a font cannot be used is now a value,
  worded when shown and in English in the log. The font window has an id of its own, as its
  title changes with the language.
- **Built, step 3, third part:** the panes, the notices above the views, the diagram and its
  SVG export. A diagram's layout keeps its language, as it holds the widths of its texts,
  and is made again on a change. What the diagram and the update check could not do is kept
  as values. Counts of errors and warnings are one pair of texts. Found: the program's
  fonts lack the ℹ of the minified notice, in English too; and a run of spaces in the
  tooltip of *Expand repeats*, now gone.
- **Next:** step 4, the engine's messages.

## Open questions

None.

## Answered questions

About the language rules (owner, 2026-11-26), all as proposed: *Sie* in German; German
number separators in the window's own texts; the setting first in Settings > Appearance; the
reference preview's comments in English; the German words for the main terms
(`docs/spec/language.md`).

About D44 and D45, split view and localization (owner, 2026-11-26), asked one by one, all
as proposed: one document in at most 4 views, menus and close buttons without keys, the last
used view, no memory of the split; a language setting with English as the default, the guide
translated, German key names, the CLI, MCP and logs in English, translations checked at
build time. Then, with French and Spanish wanted later, on the storage proposal in D45: a
file per language, the engine's messages as values, the proposed order of work, all as
proposed; and against the proposal, no language has to be complete: English stands in.

About D41 (owner, 2026-11-22), replacing D40: rules files in the configuration folder, each
switched on and off, with a *Use for* pattern; the search beside and above the document is
removed; the folder and list first, then the editor.

About D37 to D40 (owner, 2026-11-22), all as proposed: six menus with References under Tools,
Ctrl+Q and Ctrl+I; remembered sizes first, then the preview's position, Text/Tree/Both,
folding problems and close buttons; a check of the rules file before its editor window; a
rules file named after its document needs a `format` line, and the search starts from a
link's target.

About D36, the structure diagram (owner, 2026-11-20): one box per path with counts, not one
per element; in place of the grid, the text staying in view; name, count, attributes and text in a box; SVG export
first; a comment on issue #121 once the decision is on `main`.

About D35, the XPath query panel (owner, 2026-11-19): our own XPath 1.0 evaluator on the
index, not the crate `xpath-eval`; a query bar and a results panel; names without a prefix
match the default namespace too; first the engine, the CLI and the window, then the MCP tool.

About D32, references (owner, 2026-11-18): generic rules in a rules file the user keeps;
go to target, the problems and the preview pane in one step; the nearest section stands in
for the program's merge.

About D31, a Settings window (owner, 2026-11-18): File > Settings… with Ctrl+,; the sections
as proposed; Allow AI agents and Light style move from the View menu to Settings, the rest
stays in both; changes apply at once, without OK and Cancel.

About D30, the notice of a newer version (owner, 2026-11-18): on by default; 1 to 2 MB more
in the app is acceptable; Help gets *Check now*; it is turned off in a Settings window
(D31).

About D29, themes (owner, 2026-11-17): an editor in the window is not needed, theme files
are enough; the macOS menu bar at the top of the screen is wanted later, as its own
decision; steps 1 and 2 (a softer Light) come first, the rest later. Native is one style
next to the light/dark mode, not two themes.

The five questions asked before the review: index updates in D9, rope or not in D10, memory
mapping in D11, roadmap order in D15, slow spots in the review notes and D16.

Asked later:

1. Does the tokenizer of D14 pay for itself? Answered: yes for well-formed UTF-8, 17 % off
   opening 300 MiB as built (D14).
2. How should node identity survive a full re-index (after Format, or when a splice fails)?
   Answered: by paths of child positions, as built for Format (FMT-8) and the structured
   edits (EDIT-5 to EDIT-10). `Index::element_position` gives an element's position among
   its element siblings at each level; comments and text do not count, so Remove comments
   keeps them. Before a change the app notes the selection, the marked element and the
   expanded tree rows that way, and finds them again after it (`Open::edit_with`). A
   structured edit says how it moves elements (`Shift` in `apps/xml-atlas/src/app.rs`), so
   a moved, inserted or deleted element is followed.
3. Which encodings do the owner's real files use? Answered: UTF-8. Other encodings are in
   1.0 anyway (D23).
