# XML Atlas

A fast, cross-platform XML viewer and editor in pure Rust. It succeeds the
[Qt/C++ XML Atlas](https://github.com/glaure/xml-atlas).

XML Atlas is an independent project. Its behaviour is inspired by XML Marker
(symbolclick.com). It uses none of XML Marker's code or assets.

## Status

Early. What works today:

- Opens large files on a background thread. A 100 MiB file with 3.4 M nodes opens in about 0.4 s.
- Element tree, virtualized; it expands and scrolls to the selection.
- Grid: the child elements of the selection are rows, and their attributes and leaf
  children are columns. Click a row to show it in the text; double-click to open it.
- Source view with line numbers and syntax colouring. Click a line to select its element.
- Malformed files open up to the error. The status bar links to the error position.
- Format (re-indent). It changes only whitespace between tags; text, mixed content,
  `xml:space="preserve"`, the byte order mark and line endings are kept.
- Save is atomic: a crash while saving leaves the old file intact.

Not yet: editing, find, and files that are not UTF-8 (Latin-1, UTF-16).

See [ROADMAP.md](ROADMAP.md) for what comes next.

## Build and run

Requires Rust 1.95 or later.

```sh
cargo run --release -p xml-atlas -- path/to/file.xml
```

## Command line

```sh
cargo run --release -p xml-atlas-cli -- stats file.xml --repeat 5   # timings, memory
cargo run --release -p xml-atlas-cli -- format file.xml -o out.xml
```

## Benchmarks

```sh
cargo xtask gen-fixture --mib 100          # writes fixtures/orders.xml
cargo run --release -p xml-atlas-cli -- stats fixtures/orders.xml --repeat 5
```

## Linux packages

```sh
cargo install cargo-zigbuild    # also needs zig, and dpkg-deb
cargo xtask package             # writes target/dist/*.deb and *.rpm
```

The executables are built against glibc 2.34, so one package runs on RHEL 9, Ubuntu 22.04,
Debian 12 and later (`docs/decisions.md`, D22).

## Layout

| Path                | Purpose                                                        |
|---------------------|----------------------------------------------------------------|
| `crates/xml`        | Tokenizer → flat span-based node index (no DOM, lazy decoding) |
| `crates/engine`     | Document, line index and commands shared by every front end    |
| `apps/xml-atlas`    | egui desktop app                                               |
| `apps/xml-atlas-cli`| Command-line front end and benchmark tool                      |
| `xtask`             | Project tasks: fixtures, Linux packages                        |
| `docs/spec`         | Clean-room behaviour spec                                      |
| `packaging/linux`   | Desktop entry, icon and metainfo the Linux packages install    |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
