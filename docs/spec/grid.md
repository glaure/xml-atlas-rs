# Grid

The grid shows repeating data as a table. It is the feature that sets XML Marker apart, so
this file needs the most observation.

## Rules

- **GRID-1** `[built]` The grid shows the child elements of the selected element, one row
  each, in document order.
- **GRID-2** `[done]` The columns are: the row number, the element name, then one column for
  each attribute (`@name`), one for each child element name (its name), and `text()` for the
  row's own text. Columns appear in the order they are first met.
- **GRID-3** `[done]` Every attribute and child of every row has a column. The first 1,000
  rows are examined when the grid is shown, the rest over the next frames, about 6 ms each, so
  a column met only far down appears at the end shortly after. Examining reads names only:
  about 130 ns a row on one thread, so the 211,383 orders of the 100 MiB fixture take about
  27 ms spread over some frames. The rows of an element with at least 10,000 nodes below it
  are examined on up to 8 threads: the 633,506 orders of the 300 MiB fixture in 22 to 25 ms
  instead of 80 ms. While typing in a list of more than 10,000 rows, only the changed rows are
  examined: a new column is added at the end, and one no row has any more stays until the grid
  is shown again.
- **GRID-4** `[built]` A cell shows the attribute value or the text with entities decoded, cut
  at 200 characters.
- **GRID-5** `[built]` If the selected element has no child elements, the grid shows that
  element itself as a single row.
- **GRID-6** `[built]` Clicking a row highlights that element in the text view. Clicking a
  cell that stands for a child element (GRID-12) highlights that child. The grid does not
  change, and the row stays marked.
- **GRID-7** `[built]` Double-clicking a row selects that element, so the grid then shows its
  children. Double-clicking a cell that stands for a child element selects that child.
- **GRID-8** `[built]` Above the table: the path of the selected element and the row count.
- **GRID-9** `[built]` The grid stays responsive with hundreds of thousands of rows, and
  with rows that have hundreds of thousands of children: a frame costs what is on screen. The
  cells of a row are worked out once when it comes on screen, with one walk over its children
  for all columns, and again only when the document's nodes or the columns change. The same
  in the cell pane (GRID-16).
- **GRID-10** `[done]` Editing a cell changes the attribute or text in the document
  (EDIT-5, EDIT-6). Clicking a cell marks it; Left and Right move the mark along the row (left
  of the first column the whole row is marked again). F2 or Enter on the marked cell, or
  right-click > Edit, edits it in place: Enter saves, Escape or a click elsewhere cancels.
  Editable are attribute cells (a hatched one adds the attribute), `text()` cells and child
  cells that show text, when the element holds only text or nothing; the editor starts with
  the whole value, not the 200 characters the cell shows. Enter on a cell that stands for
  child elements opens it; F2 on a cell that cannot be edited says why in the status bar.
  Saving an unchanged value, or nothing into a hatched cell, changes nothing. Each edit is
  one undo step, and the row and cell stay marked.
- **GRID-11** `[done]` Above the child table, a table of the selected element's own
  attributes: name and value (XM-30). It folds away, and scrolls beyond six attributes.
- **GRID-12** `[done]` A child element that has attributes or children of its own gets a
  column too. Its cell shows its name as `<name>`. When a row has several children of the same
  name, the cell shows the name and how many: `<note> ×2` (XM-34).
- **GRID-13** `[done]` A cell with no value is hatched; a cell with an empty value is blank
  (XM-35). `<gift/>` counts as an empty value.
- **GRID-14** `[done]` Clicking a header, or its menu, sorts the rows by that column,
  ascending or descending (XM-39); the header shows an arrow. Clicking again reverses the
  direction; clicking `#`, or Document order in the menu, restores document order. `element`
  sorts by element name. Numbers compare as numbers and come before text; text compares
  ignoring case, letter by code point (`ä` after `z`); cells that stand for child elements
  come after values, by count; absent cells come last in either direction. Equal values keep
  document order. The row number stays the document position.
- **GRID-15** `[done]` Columns can be hidden and shown again from the header menu: Hide
  column, Hide empty columns (absent or empty in every row), Show column > a column or Show
  all. Above the table, how many columns are hidden. Left and Right skip hidden columns.
  This replaces XML Marker's unique consolidation (XM-33).
- **GRID-19** `[done]` Sort and hidden columns stay when an edit, undo or redo rebuilds the
  grid of the same element. Selecting another element starts in document order with every
  column shown.
- **GRID-16** `[done]` A pane beside the grid shows what the clicked row or cell stands for
  (XM-37). The grid does not change. A row, or a cell that stands for one child element
  (`<total>`), shows that element as the grid would: its path, its attributes and a table of
  its children. A cell that stands for several (`<note> ×2`) shows a table of just those. A
  value cell shows its name and the whole value, not cut at 200 characters, with entities
  decoded; it can be selected and copied, not edited. An absent cell says so. Before a click
  the pane says to click. A double-click on a row in the pane selects that element. The
  pane's width can be dragged. View > Cell pane beside the grid turns it off, remembered with
  the other settings (WIN-3). An experiment: kept or removed once the owner has tried it.
- **GRID-17** `[done]` Keyboard, after a click in the grid: Up and Down move the marked row,
  Home and End go to the first and last, Page Up and Page Down move by 20 rows; the text
  follows and the grid scrolls the row into view. The marked column stays. Enter opens the
  marked row, as a double-click does (GRID-7), when no cell is marked (GRID-10). Backspace
  goes back to the parent (Drill Up, TREE-10).
- **GRID-18** `[done]` The row-number column is as wide as the largest row number.
- **GRID-20** `[done]` Columns that do not fit scroll sideways, with the header; so do those
  of the cell pane (GRID-16). Left and Right scroll the marked cell into view, and back to
  the start of the row when the whole row is marked again.
- **GRID-21** `[planned]` (after 1.0) Move rows: Alt+Shift+Up and Alt+Shift+Down on a marked
  row move its element before the previous or after the next sibling in the file, as in the
  tree (TREE-9, EDIT-10); the row stays marked. While the grid is sorted (GRID-14) the rows
  cannot be moved, and the status bar says why.
- **GRID-22** `[planned]` (after 1.0) Copy and paste elements: Ctrl+C on a marked row copies
  its element's source text; Ctrl+V on a marked row inserts the copied element after it, as
  a new element on its own lines with the row's indentation, like Duplicate (EDIT-9). The
  grid gets a row for it. What is pasted must be well-formed XML; otherwise nothing changes
  and the status bar says why. One undo step.
- **GRID-23** `[planned]` (after 1.0) Copy the grid for a spreadsheet: the shown rows and
  columns, as tab-separated text with the header as the first line, in the shown order and
  without hidden columns (GRID-15). Pasted into Excel, each cell lands in its own cell.
- **GRID-24** `[planned]` (after 1.0) Reorder columns by dragging a header; the order is kept
  with the hidden columns. Optionally, applying the shown order to the file: the sort order
  (GRID-14) reorders the elements, the column order reorders each row's attributes or child
  elements. Changes only the order, keeps every byte inside each item, and is one undo step.
  Whether it is offered is decided when it is built.

## Known gaps

- Children with different names share one table. XML Marker does the same (XM-31); kept.

## Acceptance tests

- Select `orders` in the fixture: one row per order, columns `@id`, `@date`, `@status`,
  `customer`, `items` and `note`. `customer` and `items` show `<customer>` and `<items>`;
  `note` is hatched in orders without one.
- Select one `items` element: one row per item, columns `@line`, `@sku`, `@qty` and `price`,
  which shows `<price>` because it has an attribute.
- Select a `price` element: one row, the attribute table shows `currency`, the columns are
  `@currency` and `text()`.
- Open a list of 3,000 rows with a byte order mark and CRLF where only row 2,008 has
  `größe="groß"` and a `<später>` child: the grid gets `@größe` and `später` columns, and
  that row shows `groß` and `ä`.
- Select `orders` in the fixture, click a row, press End: row 211,383 is marked and shown, its
  number on one line.
- Select `orders` in a CRLF file with a byte order mark, click an order's `<total>` cell: the
  pane shows the `total` element with its `currency` attribute. Click its `<note> ×2` cell: a
  table of the two notes. Click a `customer` cell holding `Günther &amp; Söhne`: the pane
  shows `Günther & Söhne`.
- In a CRLF file with a byte order mark, click the hatched `@status` cell of an order, press
  F2, type `new`, Enter: only ` status="new"` is added to that start tag. Click a `customer`
  cell holding `Günther &amp; Söhne`, press Enter, then Escape: nothing changes. Ctrl+Z undoes
  the edit, and the file saves byte for byte as before.

## Seen in the reference programs

Details and limits in `reference.md`. This is where XML Atlas differs most from XML Marker
today.

- XML Marker shows, for the selected element, a table of its attributes and then one table
  of its child elements, whatever their names (XM-30, XM-31). GRID-1 matches for the child
  table; GRID-11 adds the attribute table.
- Columns in XML Marker: tag name, text, the children's attributes, then the children's own
  child elements (XM-32). GRID-2 has the same set in another order, with `@` before attribute
  names instead of an icon.
- A child element that has attributes or children of its own gets a column in XML Marker; the
  cell shows its name, not its text. The same child twice in one row shows the name with a
  count (XM-34). GRID-12 follows.
- Child names that occur in one row only share one "Unique Subtags" column in XML Marker,
  controlled by a threshold (XM-33). The Qt XML Atlas copies this and adds "Unique
  Attributes" (QT-5). GRID-15 hides columns instead.
- XML Marker draws a cell with no value differently from an empty value (XM-35). GRID-13
  follows.
- XML Marker's cells show raw source, `&amp;` included (XM-36). GRID-4 decodes on purpose.
- Clicking a cell or row in XML Marker never moves the tree selection. It highlights the range
  in the text and shows the item in a second pane, the Table Selection Browser; a cell that
  stands for several elements opens as a table there (XM-37). Double-click does the same.
  GRID-6 and GRID-16 follow. GRID-7 differs: double-click moves the selection.
- XML Marker sorts from a drop-down in the column header (XM-39). Its grid can hide empty
  columns per row or for the visible rows, and copy the grid to the clipboard. Several rows
  cannot be selected (XM-38).
- "Table Selection Browser" in the Qt XML Atlas is a menu entry without code (QT-6).

## Decided

- An attribute table for the selected element (GRID-11).
- Nested children shown by name and count (GRID-12).
- A second pane for the clicked cell (GRID-16), to be tried: the owner is not sure it is easy
  to use in XML Marker. Built as an experiment that View turns off.
- Column hiding (GRID-15) instead of unique consolidation, which is harder to understand.
- Absent and empty values look different (GRID-13).
- Cells keep decoding entities (GRID-4).
- Sorting by column (GRID-14).
- After 1.0, not in it: moving rows in the file (GRID-21), copy and paste of elements
  (GRID-22), copying the grid for a spreadsheet (GRID-23) and reordering columns, perhaps
  also in the file (GRID-24). Copy and paste is mostly wanted in the document itself, and
  pasting adds new elements. (Owner, 2026-11-17.)

## Questions

None open.
