# Editing

Changes to the document, how they are undone, and when the document counts as changed; and
the structured edits behind grid cells (GRID-10), the tree (TREE-9) and the CLI.

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
- **EDIT-4** `[done]` An edit inside one element, its undo and its redo read again only that
  element: 10 to 17 ms on a 100 MiB file, under 1 ms on 10 MiB (`docs/decisions.md`, D9).
  Format, Compact, and changes that leave the document not well-formed read the whole file
  again (0.4 s for 100 MiB), which also finds and reports the error.

- **EDIT-5** `[done]` Set text: the text of an element that holds only text, or nothing,
  is replaced. `<a/>` becomes `<a>text</a>`. `&`, `<` and `>` are written as entities. A CDATA
  section stays one when the new text allows it. An element with child elements, comments or
  processing instructions is refused, with a message.
- **EDIT-6** `[done]` Set attribute: the value is replaced inside its quotes, keeping the
  quote character; a new attribute goes after the last one, in double quotes. Values are
  escaped for their quotes; tabs and line breaks become character references so they survive.
  Remove attribute takes the whitespace before it along.
- **EDIT-7** `[done]` Rename: the start tag and the end tag. A name that is not a valid XML
  name is refused.
- **EDIT-8** `[done]` Delete: an element (or comment, text) is removed; when it stands on
  lines of its own, those lines go too, so no empty line is left. The root element cannot be
  deleted.
- **EDIT-9** `[done]` Duplicate: a copy goes right after the element, on its own lines with
  the same indentation when the element stands on lines of its own.
- **EDIT-10** `[done]` Move up and down: the element swaps places with the previous or next
  element among its siblings. Whitespace, comments and text between them stay where they are.
- EDIT-5 to EDIT-10 change only the bytes they are about: indentation, quotes, line endings,
  the byte order mark and everything else stay as they were. Each is one undo step. Today
  they are in the engine and the CLI (`xml-atlas-cli set-text|set-attribute|remove-attribute|
  rename|delete|duplicate|move FILE PATH ...`, where PATH is the path the grid shows, such
  as `/orders/order[2]/customer`); the grid and tree come next (GRID-10, TREE-9). On a
  100 MiB file each takes 15 to 33 ms, its undo as long.

## Acceptance tests

- Format a file, then Ctrl+Z: the file is byte for byte as before, and not marked modified.
  Ctrl+Y: formatted again, modified.
- Format, save, Ctrl+Z: modified (the file on disk is the formatted one).
- `xml-atlas-cli rename shop.xml /shop/owner Eigentümer -o shop.xml` on a CRLF file with a
  byte order mark: only the two tag names change.
- `xml-atlas-cli delete orders.xml /orders/order` refuses: the path matches several elements.
- Type in the Find field, press Ctrl+Z: the field's typing is undone, the document is not.

## Seen in the reference programs

- XML Marker has Edit > Undo and Redo (XM-1), but Indent cannot be undone (XM-57).

## Questions

1. Should the undo history survive closing the file? Today it does not.
2. Is 512 MiB the right limit, or should it follow the file size?
