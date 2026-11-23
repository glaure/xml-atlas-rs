# Tree

The tree shows the structure of the document and is the main way to move around it.

## Rules

- **TREE-1** `[done]` The tree shows elements, their attributes and their text. Under an
  element come first its attributes, one row each as `name = "value"`, then its text and child
  elements in document order, as in XML Marker (XM-20). View > Attributes in the tree hides the
  attribute rows; they are shown by default, as in XML Marker. Comments, processing
  instructions and anything outside the root element are not tree rows. An element with
  attributes or text can be expanded, even if it has no child elements.
- **TREE-2** `[built]` A row shows the element name, then a grey hint: the text of an element
  that contains only text (up to 60 characters), otherwise its first attribute as
  `name="value"` (value up to 40 characters).
- **TREE-3** `[built]` After opening, the root element is expanded and everything below it is
  collapsed.
- **TREE-4** `[built]` Clicking the arrow, or double-clicking the row, expands or collapses it.
- **TREE-5** `[built]` Clicking a row selects the element: the grid shows its children, and the
  text view scrolls to it and highlights its lines. Clicking an attribute or text row selects
  its element for the grid and stays marked in the tree; the text or the attribute's
  `name="value"` gets the second highlight in the text view (TEXT-11).
- **TREE-6** `[built]` When an element is selected from the grid or the text view, the tree
  expands its ancestors and scrolls to it.
- **TREE-7** `[built]` The tree stays responsive with hundreds of thousands of rows expanded,
  with thousands of attributes on an element and with values and texts of many megabytes.
  What a row shows is worked out once when it comes on screen, and again only when the
  document's nodes or View > Attributes in the tree change; a frame in which nothing changed
  reads nothing from the document. Working a row out reads a value as far as the row shows it
  (but a reference in that part needs a look at all the value's references: one that is not
  known leaves the whole value as written), and walks a start tag once for all its attribute
  rows.
- **TREE-8** `[done]` Keyboard, after a click in the tree: Up and Down move the selection,
  Right expands (on an expanded element it moves to the first row inside), Left collapses (on
  a collapsed element, an attribute or a text row it moves to the parent element), Home and End
  go to the first and last row, Page Up and Page Down move by a screen. The tree scrolls only
  when the selection would leave the screen. Keys are not taken while a text field, such as
  Find, has the keyboard.
- **TREE-9** `[done]` Editing from the tree: rename, delete, duplicate and move an element
  (EDIT-7 to EDIT-10), from the right-click menu of a row or with keys after a click in the
  tree: F2 renames in place (Enter saves, Escape or a click elsewhere cancels), Delete
  deletes, Ctrl+D duplicates, Alt+Shift+Up and Alt+Shift+Down move (Alt+Up and Alt+Down are
  Drill Up and Down, TREE-10). On an attribute row Delete removes the attribute; on a text
  row it deletes the text. The root element can only be renamed. The selection follows a
  moved element, goes to a copy, and after a delete to the element that took its place, else
  the one before, else the parent; expanded rows stay with their elements. A refused edit
  says why in the status bar. Each edit is one undo step.
  XML Marker's tree cannot do this (XM-23); renaming is the one the owner has missed most.
- **TREE-10** `[done]` Drill Up (Alt+Up), from anywhere, selects the parent of the selected
  element and outlines the element it came from. Drill Down (Alt+Down) selects the child that
  contains the outlined place, or the first child element when nothing inside is outlined; so
  Drill Up then Drill Down returns to where it started. XML Marker drills towards the text
  cursor (XM-43); the text here is not editable yet, so the outline stands in for the cursor.
- **TREE-11** `[done]` Each row has an icon for its kind, between the arrow and the name:
  element with child elements, element with only text, empty element (attributes do not
  count), attribute, text. The icons are our own shapes, drawn in the colours of the text
  view's palette, so they follow the light and dark theme.
- **TREE-12** `[built]` A click in the text moves the tree to that element (TREE-6, TEXT-7),
  expanding what is closed above it. A setting turns the expanding off (View > Expand the
  tree on a click in the text, Settings > Views, a toolbar toggle; on by default): the tree
  then selects the closest element it shows, the topmost collapsed ancestor, and opens
  nothing; the grid and the text still follow the clicked element.
  Moving the cursor while typing does not. Locate in tree (Ctrl+L) moves it on request
  (XM-42).
- **TREE-13** `[planned]` Editor Select, from the tree's right-click menu (and Ctrl+E): selects
  the element's whole text in the text view, ready to copy (XM-23). It needs a text selection,
  so it comes with the editable text view (TEXT-9).
- **TREE-14** `[done]` Collapse all, from the right-click menu of any row and View > Collapse
  the tree: every element is collapsed except the root, as after opening (TREE-3), and the
  tree scrolls to the top. The selection and the grid stay; the next key in the tree starts
  again at the first row. It changes nothing in the document and is not an undo step.
  Asked for in issue #103.
- **TREE-15** `[built]` Bookmarks mark elements to come back to. Navigate > Toggle Bookmark
  or Ctrl+F2 marks the grid's marked row, else the selected element, or takes the mark off
  (XM-61). Navigate > Next
  Bookmark and Last Bookmark select the next and the previous marked element in document
  order, going round at the ends, and say in the status bar which one of how many (`bookmark
  2 of 5`), or that there are none. In the text view F2 is Next Bookmark and Shift+F2 Last
  Bookmark; in the tree and the grid F2 keeps renaming and editing (TREE-8, GRID-10), and the
  menu entries carry other keys (F4 and Shift+F4). Clear all Bookmarks takes every mark off.
  - A marked element shows its line number on a tinted ground in the text's gutter, at the
    line of its start tag, and a bar at the left edge of its row in the tree and in the grid.
  - A bookmark follows its element through edits, undo and redo, and Format and Compact; it
    is gone when its element is deleted or replaced; a Move up or down keeps the marks of the
    elements it moves. A bookmark is not part of the
    document: it is not an undo step, does not make the document modified, and is not
    saved. Closing the tab or reloading the file from disk ends the bookmarks; they are not
    remembered between runs.
  - One list for all views of a tab, kept per tab.

## Acceptance tests

- In the 100 MiB fixture, expand `orders`: 211,383 rows, scrolling stays smooth.
- Expand one `order`: rows `id`, `date` and `status` as `name = "value"`, then `customer` and
  `items`. With View > Attributes in the tree off, only `customer` and `items`.
- Expand an element that holds only text, like `name` under `customer`: one text row.
- Click a line deep in the text view: the tree expands down to that element and shows it.
- In the 100 MiB fixture, click `orders` and press Page Down twice: about 80 orders down, the
  selection stays on screen and the grid follows.
- Select an `order`, press Alt+Down (its `customer`), then Alt+Up: the `order` again, with
  `customer` outlined; Alt+Down returns to `customer`.
- In a CRLF file with a byte order mark: select the second `order`, F2, rename it
  `bestellung`, Enter; Ctrl+D; Alt+Shift+Down; right-click the next `order` > Delete. Both
  tags are renamed, the copy has the same indentation and line endings, the deleted lines
  are gone. Four Ctrl+Z restore the file byte for byte.
- Expand several `order` elements and their `customer`, select one, right-click > Collapse
  all: only `orders` is expanded, the tree shows its top, the grid still shows the selection.
- F2, type `1x`, Enter: refused, "not a valid XML name"; the document is unchanged.

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

- Attributes and text as tree rows, with a setting to hide attribute rows (TREE-1). The
  attribute rows cost nothing until their element is expanded, so they are on by default.
- Follow a click in the text, not the cursor while typing (TREE-12).
- Drill Up and Drill Down (TREE-10).
- Icons per kind of row, from the theme (TREE-11).
- The text hint of TREE-2 stays next to the text row, for now. (Owner.)
- The attribute rows setting is remembered between sessions (WIN-3). (Owner.)
- Editor Select in the tree (TREE-13). (Owner.)
- Collapse all, back to the root (TREE-14). (Owner, issue #103.)

## Questions

None open.
