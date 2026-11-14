# AGENTS.md

Guidance for people and coding agents working on XML Atlas.

## Rules

1. **Speed is the product.** A change to the open path, the index or a view must not slow the
   benchmark (`xml-atlas-cli stats fixtures/orders.xml --repeat 5`). Report before and after
   numbers in the pull request.
2. **Views are virtualized.** Per-frame work scales with what is on screen, never with the file.
   No per-node allocation when opening a file; decode names, attributes and text on demand.
3. **Clean room.** Build from `docs/spec/` and observed behaviour only. Running XML Marker to
   observe or time it is fine. Never copy, disassemble or decompile it, and never use its code,
   icons, screenshots or other assets.
4. **One implementation per command.** User-visible operations live in `crates/engine`; the GUI,
   CLI and (later) MCP server call them. Test them there.
5. **Verify, don't guess.** Open a definition before using it; check crate APIs in the
   registry source for the pinned version.
6. **Narrow, tested changes.** A bug fix comes with a test that fails without it. Test with
   real-world input, not only ASCII: non-ASCII text, a byte order mark, CRLF line endings.
7. **Never change what the user did not ask to change.** Anything that rewrites the document
   (format, edits, save) keeps every other byte as it was, and saves atomically.
8. **Report only what ran.** Say what you built, tested and measured, and what you could not.
9. **`main` changes through pull requests.**

## Code style

- Rust 2024, `cargo fmt`, `cargo clippy --workspace --all-targets` clean.
- No `println!` / `eprintln!` outside the CLI, xtask and tests; use `log`.
- Errors: `thiserror` in libraries, `anyhow` in binaries.
- `unsafe` is forbidden workspace-wide. Lifting that for a SIMD tokenizer needs a decision
  recorded in `docs/decisions.md`.
- Files read top to bottom: constants, types, public functions, private helpers.
