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
  piece table, or keep the index in chunks. **Resolved by D9 and D10.**
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
- **Must change first:**
  - Tree expansion is a `Vec<bool>` indexed by node id. Ids move on every re-index, so
    expansion and selection need to be remapped (ids before the splice stay, ids after it
    shift by a constant).
  - Every accessor takes the source as one contiguous `&[u8]`. With D10 a node's bytes can
    span two pieces, so accessors need a source type that can return borrowed or copied bytes.
- **Revisit when:** background updates lag visibly at 1 GiB. Then store offsets relative to
  the parent and links as relative distances, so an edit touches only the ancestors and their
  following siblings.

## D10: Piece table instead of a rope

- **What:** the text buffer for editing is a piece table: the bytes read from the file stay
  untouched, edits go to an append-only second buffer, and a list of pieces says which ranges
  make up the document. This supersedes the roadmap's plan to use `ropey`.
- **Why:**
  - Opening stays zero-copy. A rope copies the whole file into chunks.
  - Untouched regions are saved byte for byte, including the byte order mark, CRLF line
    endings and bytes that are not valid UTF-8. `ropey` requires valid UTF-8.
  - Index offsets into the original buffer stay meaningful.
  - A snapshot for the worker thread (D9) is cheap: both buffers are append-only.
  - Undo is a list of piece changes.
- **Risk:** we write and test it ourselves. Keep it small: pieces in a `Vec` first, with a
  newline count per piece for the line index; a tree of pieces only if edit counts demand it.

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

## Open questions

The five questions asked before the review are answered: index updates in D9, rope or not in
D10, memory mapping in D11, roadmap order in D15, slow spots in the review notes and D16.

New ones:

1. Does the tokenizer of D14 pay for itself?
2. How should node identity survive a full re-index (after Format, or when a splice fails)?
   Paths of child positions are one option.
3. Which encodings do the owner's real files use? That sets the priority of UTF-16.
