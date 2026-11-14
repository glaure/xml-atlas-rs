# Tree

The tree shows the structure of the document and is the main way to move around it.

## Rules

- **TREE-1** `[built]` The tree shows elements only. Text, comments, attributes and processing
  instructions are not tree rows.
- **TREE-2** `[built]` A row shows the element name, then a grey hint: the text of an element
  that contains only text (up to 60 characters), otherwise its first attribute as
  `name="value"` (value up to 40 characters).
- **TREE-3** `[built]` After opening, the root element is expanded and everything below it is
  collapsed.
- **TREE-4** `[built]` Clicking the arrow, or double-clicking the row, expands or collapses it.
- **TREE-5** `[built]` Clicking a row selects the element: the grid shows its children, and the
  text view scrolls to it and highlights its lines.
- **TREE-6** `[built]` When an element is selected from the grid or the text view, the tree
  expands its ancestors and scrolls to it.
- **TREE-7** `[built]` The tree stays responsive with hundreds of thousands of rows expanded.
- **TREE-8** `[planned]` Keyboard: up and down move the selection, right expands, left
  collapses or goes to the parent.
- **TREE-9** `[planned]` Editing from the tree: rename, delete, duplicate and move an element.

## Acceptance tests

- In the 100 MiB fixture, expand `orders`: 211,383 rows, scrolling stays smooth.
- Click a line deep in the text view: the tree expands down to that element and shows it.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker's tree has rows for elements, attributes and text, not for comments or
  processing instructions (XM-20). The Qt XML Atlas has elements and attributes (QT-2).
  TREE-1 has elements only.
- XML Marker shows only the name on an element row and tells kinds of element apart by icon
  (XM-21). The grey hint of TREE-2 is ours.
- In XML Marker, selecting a tree row highlights the element in the text and fills the grid
  at once (XM-22). TREE-5 matches.
- XML Marker's tree does not follow the text cursor. "Locate in tree" (Ctrl+L) selects the
  innermost row that contains the cursor, down to an attribute; "Drill Up" (Alt+Up) goes to
  the parent, "Drill Down" (Alt+Down) back towards the cursor (XM-41 to XM-43). In the Qt XML
  Atlas only "Locate in tree" works; the drill entries are placeholders (QT-6, QT-7). TREE-6
  follows on every click instead.
- XML Marker starts with the root collapsed (XM-9) and collapses the tree again after every
  edit and after Indent (XM-44, XM-56). TREE-3 and FMT-8 differ on purpose.
- The right-click menu of a tree row in XML Marker has one entry, Editor Select (XM-23).
- Nothing in XML Marker's tree marks an error (XM-24). The Qt XML Atlas adds an error row
  (QT-3).

## Decisions needed

1. Attributes as tree rows, as in both reference programs? It makes an attribute selectable
   and, later, editable from the tree, but multiplies the row count: the 100 MiB fixture has
   3.4 million nodes before attributes. An alternative is to keep TREE-1 and show attributes
   only in the grid.
2. Text as tree rows, as in XML Marker? The hint of TREE-2 shows the same information without
   a row.
3. Keep following the text on every click (TREE-6), or follow only on request as XML Marker
   does (Ctrl+L)? Once the text is editable (TEXT-9), following every cursor move may be
   distracting; a middle way is to follow on click but not while typing.
4. Add Drill Up and Drill Down with Alt+Up and Alt+Down? They would join TREE-8.
5. Different icons per kind of element, as in XML Marker?

## Questions

1. What does Editor Select do in XML Marker? Not tried yet.
2. Do you use the tree to edit in XML Marker, and how?
