# Roadmap

The order follows `docs/decisions.md`, D15.

## 0.1: Viewer you can trust on real files (current)

- [x] Flat span index, partial tree on parse errors
- [x] Background open, virtualized tree, grid and text views
- [x] Format that changes only layout whitespace (D12), atomic save (D13)
- [x] CLI stats and format, fixture generator
- [x] Baseline: XML Marker under wine on the same files (`docs/benchmarks.md`)
- [ ] Baseline for the Qt XML Atlas (it needs a file argument first) and for interaction:
      scrolling, selecting, searching
- [ ] Start-up: 0.59 s and 193 MiB with a tiny file, slower and heavier than XML Marker.
      Measure where it goes (window, GPU renderer, fonts) and cut it
- [x] Corpus of small real-world files as tests: byte order mark, CRLF, UTF-16, Latin-1, one
      huge line, deep nesting, namespaces, DOCTYPE (`crates/engine/tests/corpus/`)
- [ ] Behaviour spec in `docs/spec/`: drafts written, questions for the owner open
- [ ] Find (text, element name, attribute value)
- [ ] Encodings other than UTF-8 (Latin-1 and Windows-1252 from the declaration, UTF-16)
- [ ] Tokenizer prototype and decision (D14); error recovery after the first parse error
- [ ] Integer scrolling for very tall views (D16)
- [ ] Keyboard navigation in tree and grid
- [ ] Ask before closing with unsaved changes
- [x] CI: fmt, clippy and tests on Linux, Windows and macOS; build with Rust 1.95

## 0.2: Structured editing

- [ ] Smaller index: fold single text children, 32-byte nodes (D11)
- [ ] Source type that hides the buffer layout from the views (D9)
- [ ] Piece table with undo and redo (D10)
- [ ] Index update on a worker thread, with ids remapped for selection and expansion (D9)
- [ ] Edit cells in the grid; rename, delete, duplicate and move elements in the tree
- [ ] The same edits from the CLI
- [ ] Recent files, settings

## 0.3: Text editing

- [ ] Editable text view: cursor, selection, clipboard, input methods
- [ ] Highlighting across lines, driven by the index (it knows comment and CDATA spans)
- [ ] Wrap or page very long lines; minified-file mode

## 0.4: Power features

- [ ] XPath query panel
- [ ] Schema (XSD) validation
- [ ] Memory-mapped open for files larger than RAM (D11)

## Distribution and automation

- [ ] Packages: MSI, DEB, Flatpak, DMG via `cargo xtask package`
- [ ] JSON control channel and MCP server over the engine commands
- [ ] Web build (WebAssembly)
