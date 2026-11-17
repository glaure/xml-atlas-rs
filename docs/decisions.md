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

## Open questions

None.

## Answered questions

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
