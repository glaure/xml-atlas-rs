# Roadmap

The order follows `docs/decisions.md`, D15. Version 1.0 is everything here except the items
marked *after 1.0* (D20): all of 0.4 and the macOS package.

## 0.1: Viewer you can trust on real files

- [x] Flat span index, partial tree on parse errors
- [x] Background open, virtualized tree, grid and text views
- [x] Format that changes only layout whitespace (D12), atomic save (D13)
- [x] CLI stats and format, fixture generator
- [x] Baseline: XML Marker under wine on the same files (`docs/benchmarks.md`)
- [x] Baseline for interaction: scrolling, selecting, searching, against XML Marker on
      Windows (`docs/benchmarks.md`). No baseline for the Qt XML Atlas: it was much slower
      than XML Marker, so XML Marker is the one to beat (owner, 2026-11-16)
- [x] Start-up: 0.59 s and 193 MiB with a tiny file, slower and heavier than XML Marker.
      Now 0.30-0.33 s and 132 MiB with OpenGL instead of wgpu (D21)
- [x] Corpus of small real-world files as tests: byte order mark, CRLF, UTF-16, Latin-1, one
      huge line, deep nesting, namespaces, DOCTYPE (`crates/engine/tests/corpus/`)
- [x] Behaviour spec in `docs/spec/`: XML Marker and the Qt XML Atlas observed
      (`docs/spec/reference.md`); the owner's questions answered, none open
- [x] Find: match case, whole word, direction, F3 (TEXT-12); also `xml-atlas-cli find`
- [x] Error list with markers in the text (OPEN-13)
- [x] Tree: attribute and text rows, with a setting to hide attribute rows (TREE-1)
- [x] Tree: icons per kind of row (TREE-11)
- [x] Grid: attribute table of the selected element (GRID-11), nested children by name and
      count (GRID-12), absent and empty values told apart (GRID-13)
- [x] Grid: a column for every attribute and child of every row, not only of the first 1,000
      (GRID-3)
- [x] Text: highlight the element's exact extent (TEXT-6) and the clicked grid item (TEXT-11)
- [x] Encodings other than UTF-8: converted at open, saved back in their own (OPEN-8, D23)
- [x] Tokenizer prototype and decision (D14): a `memchr` scanner for well-formed UTF-8,
      quick-xml for the rest; opening 300 MiB 17 % faster
- [x] Error recovery after the first parse error (OPEN-10)
- [x] Integer scrolling for very tall views (D16): the text view; tree and grid stay within
      a float's precision at 1.0's sizes
- [x] Keyboard navigation in tree and grid (TREE-8, GRID-17); Drill Up and Drill Down (TREE-10)
- [x] Ask before closing with unsaved changes (OPEN-11)
- [x] CI: fmt and clippy on Linux; tests on Linux, Windows and macOS; build with Rust 1.95

## 0.2: Structured editing

- [x] Smaller index: 32-byte nodes (D11)
- [x] Smaller index: fold single text children (D11)
- [x] Edits on the one buffer, with undo and redo (D10, EDIT-1 to EDIT-3); Format and Compact
      keep the selection and expanded rows (FMT-8, FMT-9)
- [x] Index update of the changed element only (D9, EDIT-4); a worker thread when 1 GiB files
      are edited
- [x] Edit cells in the grid; rename, delete, duplicate and move elements in the tree
      (GRID-10, TREE-9)
- [x] Replace and Replace All (TEXT-14)
- [x] The edits in the engine and the CLI: set text, set and remove attributes, rename,
      delete, duplicate, move (EDIT-5 to EDIT-10)
- [x] Recent files (OPEN-12)
- [x] Light and dark theme, following the system by default (WIN-2)
- [x] Remember settings between sessions: layout, attribute rows, Format options, Find options
      (WIN-3)
- [x] Several documents at once, as tabs (OPEN-5)
- [x] Notice when the file changes on disk (OPEN-14)
- [x] Format settings: indent size, tabs or spaces, attributes per tag line (FMT-10, FMT-11);
      settings read from the document's own settings comment (FMT-15)
- [x] Format: remove comments, trim text (FMT-12, FMT-13); compact (FMT-14)
- [x] Grid: sort by column (GRID-14), hide columns (GRID-15)
- [x] Grid: second pane for the clicked cell, as an experiment to try (GRID-16)

## 0.3: Text editing

- [x] Editable text view: cursor, selection, clipboard, input methods; the tree follows a
      click but not typing (TREE-12). Rules TEXT-16 to TEXT-22, design and order of work in
      `docs/decisions.md`, D19; also integer scrolling (TEXT-8) and long lines (TEXT-5)
- [x] Highlighting across lines, driven by the index (it knows comment and CDATA spans) (TEXT-3)
- [x] Page very long lines (TEXT-5)
- [x] Word wrap (TEXT-13)
- [x] Minified-file mode: opens wrapped, a notice offers Format (TEXT-23)

## 0.4: Power features (after 1.0, D20)

- [x] XPath query panel: our own XPath 1.0 on the index, the window and the CLI's `xpath`
      (D35, XP-1 to XP-6)
- [x] XPath for agents: an MCP tool `xpath` (D35, XP-7)
- [ ] Schema (XSD) validation
- [ ] Import other XML files: follow XInclude references (`docs/spec/open.md`)
- [x] References a rules file defines: go to target (F12), the ones that find nothing as
      problems, a preview with them resolved, and the CLI's `references` and `preview` (D32)
- [x] The resolved preview as a tree, in a pane right of the preview text (REF-10, D33)
- [x] References switched on and off as a whole, off by default (REF-12)
- [x] Repeated elements expanded in the preview; rules switched off in the file (REF-11, D34)
- [x] A diagram of the document's structure: one box per path, with counts, clicks both
      ways and collapsing (D36, step 1; issue #121)
- [x] The diagram's levels and the marks of Find and XPath (D36, step 2; issues #122, #123)
- [x] The diagram as an SVG file (D36, step 3; issue #124)
- [ ] Diff between two files: side by side, with the differences marked in the text and the
      tree; needs a spec ID and a decision on text or structure diff (whitespace-only changes,
      attribute order) in `docs/decisions.md` first
- [ ] Diff between git revisions of one file: the same view, with the revisions picked from
      the file's history; the engine calls `git`, the GUI and CLI use the engine (rule 4)
- [ ] Memory-mapped open for files larger than RAM (D11)
- [ ] Grid: move rows in the file, copy and paste elements, copy the grid for a spreadsheet,
      reorder columns (GRID-21 to GRID-24)
- [ ] Grid: remember the column widths, also in the cell pane; they are lost on exit. Needs a
      spec ID, and a decision on what a width is kept by (today the parent element, for the
      session only)
- [x] Bug: a tree row's cost per frame followed the file (TREE-7): 4 s a frame for the
      attribute rows of an element with a 50 MiB value. Now rows are worked out when they
      come on screen or change, and read only as much of a value as they show
- [ ] Bug: with word wrap on, a new width of the text view breaks long lines anew over all
      their length (TEXT-13): 130-140 ms a frame while resizing with one line of 50 MiB, 3 ms
      with 1 MiB, 0.2 ms with wrap off. A minified file opens with wrap on (TEXT-23)
- [ ] Bug: the grid's cost follows the number of columns (GRID-9): about 130 ms for the frame
      after an edit and 3 ms for each other one with 100,000 attributes on one element. All
      cells of a row are worked out, also of columns out of sight (about 40 ms a row); the
      rest is in the grid as well and not profiled yet. Low priority: far from real files
- [ ] Text: a formatted view of a minified file that leaves the file as it is (TEXT-24)
- [x] Help menu: a built-in guide (F1) and About with the version (WIN-4)
- [x] Toolbar after XML Marker's (WIN-5), and File > New (OPEN-22)
- [x] Font, style and size of the text, tree and grid (TEXT-25)
- [x] Themes, step 1 and 2: every colour in one theme; a softer Light, with 1.0's Light kept
      as *Classic* (D29, WIN-8)
- [x] Themes: an example of each in Settings > Appearance (WIN-12)
- [ ] Themes: style next to mode, theme files the user can change and share, a native look
      with the system's accent colour (D29, WIN-9, WIN-10)

Decided 2026-11-22 (D37 to D41 in `docs/decisions.md`):

- [x] Bug: window and pane sizes were lost on exit; now remembered, with Reset layout
      (D38, WIN-3; #146)
- [ ] Menus: aligned keys, Navigate and Tools menus, a shorter View (D37)
- [ ] Layout: the preview's position, Text/Tree/Both in its header, folding problems, close
      buttons on panes (D38)
- [ ] Layout, later: the cell pane's position, maximize a pane and F6, pictures of the
      layouts in Settings (D38)
- [x] References: rules files in the configuration folder, listed in Settings, each switched
      on and off, with a pattern for its documents; no search beside the document (D41,
      REF-13; #147)
- [x] References: a check of the rules files and an editor for them (D39, D41, REF-14, REF-15;
      #148)
- [x] References: the check's result in the preview's header, beside the file's name (D39,
      the rest of step 2; REF-16)
- [ ] References, later: rules from the selection and a JSON Schema (D39, step 4)

From XML Marker's 2.x change log (observe it, write the rules in `docs/spec/` first; each
needs a spec ID before work starts). Checked against the menus of XML Marker 2.2.0 (the
latest release, 2013) on 2026-11-21; the entries it adds to 1.x are below:

- [ ] Bookmarks in text, tree and grid: set (Ctrl+F2), cycle (F2); they follow edits. Navigate
      menu: Toggle, Last, Next, Clear all bookmarks
- [ ] New Text and New Json in File > New, next to New DTD (OPEN-22 has New XML; XM-69)
- [ ] Save as with Encoding: a Save As that asks for the encoding, with and without BOM
      (OPEN-8, D23; XM-64 to XM-67). Do better than XML Marker: ask before characters are
      lost, and add no line break
- [ ] View > Line ending: show line breaks as marks in the text (CR, LF, CR LF, mixed); a
      display option only (XM-62)
- [ ] View > Stat: a panel with the time to start, load and parse (XM-63, XM-68); compare
      with `xml-atlas-cli stats`
- [x] Go to line (Ctrl+G) in the text view; also with the line of an error (TEXT-27)
- [ ] Branch selector: a box that shows the selected element as a path and selects another
      element when the path is edited (overlaps XPath; decide whether XPath answers it)
- [ ] Reparse all (F7): the manual form of the index rebuild after text edits (D19). In 2.2
      typing re-parses at once and Reparse showed no change (XM-72): find out what it is for
      before building it
- [ ] Option: the mouse wheel scrolls the tree instead of changing the selection (Options >
      Scroll wheel changes tree selection, on by default in 2.2)
- [ ] Parse this document as DTD (Options, XM-2): observe, then decide whether it is wanted
- [ ] Manual character encoding choice (Options > Character encoding) next to detection
      (OPEN-8); check that the saved file keeps its encoding. In 2.2 the choice changed
      nothing visible (XM-71): find out whether it re-reads or sets the save encoding
- [ ] Grid: a cell with attributes and one text shows the text, not the tag name; check
      against GRID-11 to GRID-13
- [ ] Test cases from 2.1's crash fixes: very large paste, typing in the tree with an
      empty document, deeply nested files, sorting numbers that start with 9
- [ ] Test case from 2.2: Format must never delete comments
- [ ] JSON: browse and edit JSON files, as a second document kind (needs a decision in
      `docs/decisions.md`: own index or convert; one root only in XML Marker). 2.2 shows a
      tree of name = value with every value as a string, errors with a marker, and Indent for
      JSON (XM-70)
- [ ] Plain text mode: the editor without highlighting or navigation (low priority; XM-69:
      no tree, no checks)

## Distribution and automation

- [x] Linux `.deb` (Ubuntu, Debian) and `.rpm` (RHEL, Fedora): `cargo xtask package`, built
      against glibc 2.34 (D22), installed and started on Ubuntu 22.04/24.04, Debian 12,
      Rocky Linux 9 and Fedora 43 by `packaging/linux/install-test.sh`
- [x] Windows MSI and zip: `cargo xtask package-windows` (PKG-10 to PKG-12) with WiX 5.0.2,
      installed for the current user and for all users on Windows 11, and 0.1.1 over 0.1.0
      (PKG-5)
- [x] Packages built by CI from a version tag into a draft release (`docs/spec/install.md`,
      PKG-1): first run with `v0.1.3`
- [ ] Windows signing with the employer's certificate, a manual step (PKG-13, PKG-14)
      (*after 1.0*; owner, 2026-11-17)
- [ ] Flatpak (PKG-32), after the `.deb` and `.rpm`
- [ ] macOS DMG (universal, Homebrew cask) with a manual signing and notarization step
      (*after 1.0*, D20): unsigned DMG built by the release workflow (PKG-20); left: opening
      files from Finder, signing (PKG-21), Homebrew cask (PKG-22)
- [x] Settings window: File > Settings… (WIN-11, D31), before the update notice
- [x] Notice when a newer version is released, on by default, with Help > Check now (PKG-7,
      D30)
- [x] Download and install from that notice, through the system's installer (PKG-8, D43);
      still to try by hand: App Center, GNOME Software, Discover, Windows, macOS
- [ ] Bug, to confirm on Windows: the MSI opened over a copy installed for all users probably
      installs a second copy for the current user instead of replacing it (PKG-5, D43). Its
      *Install* button installs per user (`ALLUSERS=2`, `MSIINSTALLPERUSER=1`), and Windows
      Installer upgrades only within the same context. All users under *Advanced* may not
      help: the MSI looks for older versions before that dialog. Until fixed, *Download and
      install* gives such installs the link only (PKG-8)
- [x] MCP server on files: `xml-atlas-cli mcp` (D27, step 1)
- [x] MCP on the documents open in the window: `xml-atlas-cli mcp --window` (D28)
- [ ] Web build (WebAssembly)
