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

- [ ] XPath query panel
- [ ] Schema (XSD) validation
- [ ] Import other XML files: follow XInclude references (`docs/spec/open.md`)
- [ ] Memory-mapped open for files larger than RAM (D11)
- [ ] Grid: move rows in the file, copy and paste elements, copy the grid for a spreadsheet,
      reorder columns (GRID-21 to GRID-24)
- [ ] Text: a formatted view of a minified file that leaves the file as it is (TEXT-24)

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
- [ ] Notice when a newer version is released (PKG-7)
- [ ] JSON control channel and MCP server over the engine commands
- [ ] Web build (WebAssembly)
