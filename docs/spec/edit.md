# Editing

Changes to the document, how they are undone, and when the document counts as changed. The
edits themselves (grid cells, tree rename, delete, move) come with their own rules as they are
built.

## Rules

- **EDIT-1** `[done]` Edit > Undo and Edit > Redo, also Ctrl+Z, and Ctrl+Y or Ctrl+Shift+Z.
  The menu names what would be undone or redone (*Undo Format*); with nothing to undo the item
  is greyed out. Each command that changes the document is one step, however many places it
  changes. Undo puts back exactly the bytes that were there, including a byte order mark,
  CRLF line endings and bytes that are not UTF-8. A new change after undoing drops the steps
  that could have been redone. While the Find field has the keyboard, Ctrl+Z undoes typing in
  the field, not the document.
- **EDIT-2** `[done]` The document counts as modified (title in the status bar, the question
  before closing) when it differs from the file it was opened from or last saved to. Undoing
  back to that state makes it unmodified again.
- **EDIT-3** `[done]` Undo keeps up to 512 MiB of old text; beyond that the oldest steps are
  dropped. The newest step is always kept, so the last Format of a large file can be undone.
- **EDIT-4** `[gap]` Undo, redo and each edit take about as long as opening the file, because
  the whole file is read again (0.4 s for 100 MiB, 40 ms for 10 MiB). They should take time
  in proportion to what changed (`docs/decisions.md`, D9).

## Acceptance tests

- Format a file, then Ctrl+Z: the file is byte for byte as before, and not marked modified.
  Ctrl+Y: formatted again, modified.
- Format, save, Ctrl+Z: modified (the file on disk is the formatted one).
- Type in the Find field, press Ctrl+Z: the field's typing is undone, the document is not.

## Seen in the reference programs

- XML Marker has Edit > Undo and Redo (XM-1), but Indent cannot be undone (XM-57).

## Questions

1. Should the undo history survive closing the file? Today it does not.
2. Is 512 MiB the right limit, or should it follow the file size?
