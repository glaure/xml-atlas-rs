# XML Atlas

A fast, cross-platform XML viewer and editor in pure Rust. It succeeds the
[Qt/C++ XML Atlas](https://github.com/glaure/xml-atlas).

XML Atlas is an independent project. Its behaviour is inspired by XML Marker
(symbolclick.com). It uses none of XML Marker's code or assets.

## Status

Version 1.0, for Windows, Linux and macOS. What it does:

- Opens large files on background threads: a 100 MiB file with 2.3 M elements is indexed in
  about 0.1 s (`xml-atlas-cli stats`). Views are virtualized: their work per frame depends on
  what is on screen, not on the size of the file.
- Tree, grid and text views of the same document. The grid shows the selected element's
  children as rows, with their attributes and children as columns; it sorts, hides columns
  and edits cells.
- Editing: the text as in any editor, with undo and input methods; in the tree and grid,
  rename, delete, duplicate, move and set values. Tree and grid follow typing.
- Find and Replace, an error list with markers, and files that are not well-formed open
  up to the error and past it.
- Format, Compact, remove comments and trim text change only whitespace and what was asked
  for; the byte order mark and line endings are kept.
- Files in other encodings (Latin-1, UTF-16 and others) open and are saved in their own.
- Word wrap, very long lines, and minified files, which open wrapped with an offer to format.
- Several documents as tabs, recent files, light and dark theme, a notice when a file changes
  on disk.
- Save is atomic: a crash while saving leaves the old file intact.

See [ROADMAP.md](ROADMAP.md) for what comes after 1.0: XPath, schema validation, signed
packages.

## Install

Download a package from the [releases page](https://github.com/glaure/xml-atlas-rs/releases):

- Windows 10 and 11: the `.msi`, or the `-windows-x64.zip` to run without installing. The
  files are not signed yet, so SmartScreen may warn: *More info*, then *Run anyway*.
- Ubuntu 22.04 and later, Debian 12 and later: `sudo apt install ./xml-atlas_….deb`
- RHEL 9 and its rebuilds, Fedora: `sudo dnf install ./xml-atlas-….rpm`
- macOS 12 and later, Apple silicon and Intel: the `-macos-universal.dmg`; drag XML Atlas
  into Applications. The app is not signed yet, so the first start is refused: System
  Settings > Privacy & Security, *Open Anyway*. It cannot open a file from Finder yet; use
  File > Open or drop the file onto the window.

`SHA256SUMS` on the same page holds each file's checksum.

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

## References

A file can point from one part to another with conventions of its own, such as
`<Include src="Shared:Parts/*"/>`. Put `xml-atlas-references.json` next to it, or in a folder
above it, to say what is a reference (`docs/spec/references.md`). Then F12 goes to what the
selected reference finds, the ones that find nothing are listed as problems, and View >
Preview with references resolved shows the selected element with them resolved. The file is
never changed.

```sh
cargo run --release -p xml-atlas-cli -- references file.xml          # check them all
cargo run --release -p xml-atlas-cli -- preview file.xml /Doc/Unit   # one element resolved
```

## For AI agents (MCP)

`xml-atlas-cli mcp` serves XML Atlas's tools over the Model Context Protocol: open a file,
read elements, child rows, text and problems, find and replace, edit elements, format, undo
and save. Changes stay in memory until `save`, which changes nothing else in the file. For
Claude Code:

```sh
claude mcp add xml-atlas -- xml-atlas-cli mcp
```

To let an agent work in the open window instead, on its tabs, turn on *Allow AI agents
(MCP)* in File > Settings and add `xml-atlas-cli mcp --window`. Each change is selected in the window and can be
undone there.

## Benchmarks

```sh
cargo xtask gen-fixture --mib 100          # writes fixtures/orders.xml
cargo run --release -p xml-atlas-cli -- stats fixtures/orders.xml --repeat 5
```

## Linux packages

```sh
cargo install cargo-zigbuild    # also needs zig, and dpkg-deb
cargo xtask package             # writes target/dist/*.deb and *.rpm
packaging/linux/install-test.sh # installs them in fresh containers (needs Docker)
```

The executables are built against glibc 2.34, so one package runs on RHEL 9, Ubuntu 22.04,
Debian 12 and later (`docs/decisions.md`, D22).

## Windows packages

On Windows, with Rust (MSVC), the .NET SDK and WiX 5.0.2 (WiX 7 needs its maintenance-fee
EULA accepted first, so the version is pinned):

```sh
dotnet tool install --global wix --version 5.0.2
wix extension add --global WixToolset.UI.wixext/5.0.2
cargo xtask package-windows    # writes target/dist/*.msi and *-windows-x64.zip
```

## macOS package

On macOS, with both Rust targets:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo xtask package-macos      # writes target/dist/*-macos-universal.dmg
```

## Layout

| Path                | Purpose                                                        |
|---------------------|----------------------------------------------------------------|
| `crates/xml`        | Tokenizer → flat span-based node index (no DOM, lazy decoding) |
| `crates/engine`     | Document, line index and commands shared by every front end    |
| `crates/mcp`        | The MCP tools over the engine, for `xml-atlas-cli mcp`         |
| `apps/xml-atlas`    | egui desktop app                                               |
| `apps/xml-atlas-cli`| Command-line front end and benchmark tool                      |
| `xtask`             | Project tasks: fixtures, Linux, Windows and macOS packages     |
| `docs/spec`         | Clean-room behaviour spec                                      |
| `packaging/linux`   | Desktop entry, icon and metainfo the Linux packages install    |
| `packaging/windows` | Installer source (WiX) and icon of the Windows packages        |
| `packaging/macos`   | Icon of the macOS app                                          |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
