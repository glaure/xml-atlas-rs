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

## Questions

1. What exactly does XML Marker put in its tree: elements only, or also attributes, text and
   comments?
2. What does a tree row show besides the name? Is there a value or hint next to it?
3. The Qt XML Atlas has "Drill down", "Drill up" and "Locate in tree". What do they do, and
   which XML Marker behaviour are they modelled on?
4. Does selecting in the tree move the text view at once, or only on request?
5. Is there a right-click menu on tree rows? What is in it?
6. Does anything in the tree show that the document has an error?
