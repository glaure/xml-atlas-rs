# XML Atlas

A fast, cross-platform XML viewer and editor in pure Rust. It succeeds the
[Qt/C++ XML Atlas](https://github.com/glaure/xml-atlas).

XML Atlas is an independent project. Its behaviour is inspired by XML Marker
(symbolclick.com). It uses none of XML Marker's code or assets.

![XML Atlas with a 10 MiB file: the tree on the left, the text above and the grid of the
selected element's children below](docs/images/xml-atlas.png)

## Fast on big files

![A 300 MiB file: jumping through the tree, sorting the grid and running an XPath query
that counts 211,145 orders in 453 ms](docs/images/demo.gif)

A 300 MiB file with 7 million elements opens in under a second, where XML Marker needs a
minute. The tree, grid and text views only draw what is on screen, so scrolling costs the
same in a small file and a huge one.

| Open (Windows 11) | XML Marker 1.1 | XML Atlas 1.2 |
|-------------------|----------------|---------------|
| 10 MiB            | 0.91 s         | 0.56 s        |
| 100 MiB           | 10.6 s         | 0.59 s        |
| 300 MiB           | 56.5 s         | 0.82 s        |

| Find the last order (Windows 11) | XML Marker 1.1 | XML Atlas 1.2 |
|----------------------------------|----------------|---------------|
| 100 MiB                          | 2.2 s          | 34 ms         |
| 300 MiB                          | 6.8 s          | 67 ms         |

Same machine, median of three runs. XML Marker starts sooner and uses less memory with a tiny
file, and it is a little quicker in the tree on End, Home and Page Down. Method, caveats and
the Linux numbers are in [docs/benchmarks.md](docs/benchmarks.md).

Things to try:

1. Make a big file: `cargo xtask gen-fixture --mib 300`, then open it and drag the tree's
   scroll bar to the end.
2. Select an element with many children. The grid shows them as rows, with attributes and
   child elements as columns; click a header to sort, edit a cell, and the text follows.
3. Edit > XPath: `//order[@status='shipped']`. The query is checked as you type and the
   matches are a list to step through. Try `count(//order)`.
4. Delete a closing tag. The file still opens up to the error and past it, with markers and
   a problems list.
5. Rename an element and save, then compare with `cmp` or `git diff`: only what you changed
   differs. Byte order mark, line endings and encoding stay as they were, and saving is
   atomic.
6. `claude mcp add xml-atlas -- xml-atlas-cli mcp`, then ask Claude Code to change
   something in a file (see [For AI agents](#for-ai-agents-mcp)).

## Status

Version 1.2, for Windows, Linux and macOS. What it does:

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
- XPath 1.0 queries (Edit > XPath, and `xml-atlas-cli xpath`): checked as you type, the
  nodes in a list to step through, or a count, sum or text; well under a second on a
  300 MiB file. Agents have them too.
- A diagram of the document's structure (View > Diagram): one box per path of element
  names with how many there are, their attributes and whether they hold text; a click goes
  to the elements. A *Levels* control collapses it to a depth, and Find's match and XPath's
  results mark their boxes; *Export as SVG…* saves it as an image. Also `xml-atlas-cli structure`.
- References your team defines in rules files you manage in File > Settings: go to a
  reference's target (F12), the ones that find nothing in the problems list, and a preview
  of the selected element with them resolved, as text and as a tree, with repeated elements
  expanded.
- AI agents such as Claude Code can read and edit files, or the documents open in the window,
  through `xml-atlas-cli mcp` (MCP).
- File > Settings… with every setting; a softer light theme; a font of your choice for text,
  tree and grid; a toolbar; a built-in guide (F1).
- A notice in the status bar when a newer version is released: once a day the program asks
  GitHub for the latest release, sending nothing but its version. Never a download; turn it
  off in File > Settings > Updates.

See [ROADMAP.md](ROADMAP.md) for what comes next: schema validation, signed packages.

## Install

Download a package from the [releases page](https://github.com/glaure/xml-atlas-rs/releases):

- Windows 10 and 11: the `.msi`, or the `-windows-x64.zip` to run without installing. The
  files are not signed yet, so SmartScreen may warn: *More info*, then *Run anyway*.
- Ubuntu 22.04 and later, Debian 12 and later: `sudo apt install ./xml-atlas_….deb`
- RHEL 9 and its rebuilds, Fedora: `sudo dnf install ./xml-atlas-….rpm`
- macOS 12 and later, Apple silicon and Intel: the `-macos-universal.dmg`; drag XML Atlas
  into Applications. The app is not signed yet, so the first start is refused: System
  Settings > Privacy & Security, *Open Anyway*. Since 1.1 it opens files from Finder
  (double-click, *Open with*) and from its Dock icon.

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
`<Include src="Shared:Parts/*"/>`. A rules file says what is a reference
(`docs/spec/references.md`). In File > Settings > References import one or make a new one,
switch it on, fill in *Use for* with the documents it is for (`catalog*.xml`), and turn on
Resolve references (off by default). Rules files are kept in the configuration folder; one
lying beside an XML file is not used (until 1.2 it was: import it once). Then F12 goes to what the selected reference finds, the ones
that find nothing are listed as problems, and View > Preview with references resolved shows
the selected element with them resolved. The file is never changed.

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
