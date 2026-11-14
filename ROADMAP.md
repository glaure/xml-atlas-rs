# Roadmap

The order follows `docs/decisions.md`, D15.

## 0.1: Viewer you can trust on real files (current)

- [x] Flat span index, partial tree on parse errors
- [x] Background open, virtualized tree, grid and text views
- [x] Format that changes only layout whitespace (D12), atomic save (D13)
- [x] CLI stats and format, fixture generator
- [ ] Baseline: time XML Marker under wine and the Qt XML Atlas on the same files
      (record in `docs/benchmarks.md`)
- [ ] Corpus of small real-world files as tests: byte order mark, CRLF, UTF-16, Latin-1, one
      huge line, deep nesting, namespaces, DOCTYPE
- [ ] Behaviour spec in `docs/spec/`
- [ ] Find (text, element name, attribute value)
- [ ] Encodings other than UTF-8 (Latin-1 and Windows-1252 from the declaration, UTF-16)
- [ ] Tokenizer prototype and decision (D14); error recovery after the first parse error
- [ ] Integer scrolling for very tall views (D16)
- [ ] Keyboard navigation in tree and grid
- [ ] Ask before closing with unsaved changes
- [ ] CI: fmt, clippy and tests on Linux, Windows and macOS

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
