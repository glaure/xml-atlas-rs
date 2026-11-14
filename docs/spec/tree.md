# Tree

The tree shows the structure of the document and is the main way to move around it.

## Rules

- **TREE-1** `[gap]` The tree shows elements, their attributes and their text. Under an
  element come first its attributes, one row each as `name = "value"`, then its text and child
  elements in document order, as in XML Marker (XM-20). A setting hides the attribute rows.
  Comments and processing instructions are not tree rows. Today the tree shows elements only.
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
  XML Marker's tree cannot do this (XM-23); renaming is the one the owner has missed most.
- **TREE-10** `[planned]` Drill Up (Alt+Up) selects the parent of the selected row. Drill Down
  (Alt+Down) selects the child that leads towards the text cursor (XM-43).
- **TREE-11** `[planned]` Each row has an icon for its kind: element with children, element
  with text, empty element, attribute, text. The icons come from the theme.
- **TREE-12** `[planned]` A click in the text moves the tree to that element (TREE-6, TEXT-7).
  Moving the cursor while typing does not. Locate in tree (Ctrl+L) moves it on request
  (XM-42).

## Acceptance tests

- In the 100 MiB fixture, expand `orders`: 211,383 rows, scrolling stays smooth.
- Click a line deep in the text view: the tree expands down to that element and shows it.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker's tree has rows for elements, attributes and text, not for comments or
  processing instructions (XM-20). The Qt XML Atlas has elements and attributes (QT-2).
  TREE-1 follows XML Marker.
- XML Marker shows only the name on an element row and tells kinds of element apart by icon
  (XM-21). The grey hint of TREE-2 is ours; TREE-11 adds the icons.
- In XML Marker, selecting a tree row highlights the element in the text and fills the grid
  at once (XM-22). TREE-5 matches.
- XML Marker's tree does not follow the text cursor. "Locate in tree" (Ctrl+L) selects the
  innermost row that contains the cursor, down to an attribute; "Drill Up" (Alt+Up) goes to
  the parent, "Drill Down" (Alt+Down) back towards the cursor (XM-41 to XM-43). In the Qt XML
  Atlas only "Locate in tree" works; the drill entries are placeholders (QT-6, QT-7). TREE-10
  and TREE-12 take both.
- XML Marker starts with the root collapsed (XM-9) and collapses the tree again after every
  edit and after Indent (XM-44, XM-56). TREE-3 and FMT-8 differ on purpose.
- The right-click menu of a tree row in XML Marker has one entry, Editor Select: it selects
  the element's text (XM-23).
- Nothing in XML Marker's tree marks an error (XM-24). The Qt XML Atlas adds an error row
  (QT-3).

## Decided

- Attributes and text as tree rows, with a setting to hide attribute rows (TREE-1).
- Follow a click in the text, not the cursor while typing (TREE-12).
- Drill Up and Drill Down (TREE-10).
- Icons per kind of row, from the theme (TREE-11).

## Questions

1. With text as its own row (TREE-1), the text hint of TREE-2 repeats it. Keep the hint only
   for the first attribute, keep both, or drop the hint?
2. Attribute rows multiply the row count: the 100 MiB fixture has 3.4 million nodes before
   attributes. Should the setting that hides them be on or off by default?
3. Should the tree get Editor Select (select the element's text, ready to copy)?
