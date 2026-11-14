# Grid

The grid shows repeating data as a table. It is the feature that sets XML Marker apart, so
this file needs the most observation.

## Rules

- **GRID-1** `[built]` The grid shows the child elements of the selected element, one row
  each, in document order.
- **GRID-2** `[built]` The columns are: the row number, the element name, then one column for
  each attribute (`@name`), one for each child element that has no child elements of its own
  (its name), and `text()` for the row's own text. Columns appear in the order they are first
  met.
- **GRID-3** `[gap]` Every attribute and leaf child of every row has a column. Today only the
  first 1,000 rows are examined, so a column that first appears later is missing.
- **GRID-4** `[built]` A cell shows the attribute value or the text with entities decoded, cut
  at 200 characters.
- **GRID-5** `[built]` If the selected element has no child elements, the grid shows that
  element itself as a single row.
- **GRID-6** `[built]` Clicking a row highlights that element in the text view. The grid does
  not change.
- **GRID-7** `[built]` Double-clicking a row selects that element, so the grid then shows its
  children.
- **GRID-8** `[built]` Above the table: the path of the selected element and the row count.
- **GRID-9** `[built]` The grid stays responsive with hundreds of thousands of rows.
- **GRID-10** `[planned]` Editing a cell changes the attribute or text in the document.
- **GRID-11** `[planned]` Above the child table, a table of the selected element's own
  attributes: name and value (XM-30).
- **GRID-12** `[planned]` A child element that has attributes or children of its own gets a
  column too. Its cell shows its name. When a row has several children of the same name, the
  cell shows the name and how many (XM-34).
- **GRID-13** `[planned]` A cell with no value looks different from a cell with an empty value
  (XM-35).
- **GRID-14** `[planned]` Clicking a header, or its menu, sorts the rows by that column,
  ascending or descending (XM-39). Clicking again reverses the direction.
- **GRID-15** `[planned]` Columns can be hidden and shown again. A command hides the columns
  that are empty in every row. This replaces XML Marker's unique consolidation (XM-33).
- **GRID-16** `[planned]` Clicking a cell shows what it stands for in a second pane: an element
  with its attributes and children, or a table when the cell stands for several elements
  (XM-37). The grid does not change. To be tried before it is kept.

## Known gaps

- A row with two child elements of the same name shows only the first in that column
  (GRID-12).
- A child element that has children of its own gets no column; nothing shows it is there
  (GRID-12).
- Children with different names share one table. XML Marker does the same (XM-31); kept.

## Acceptance tests

- Select `orders` in the fixture: one row per order, columns `@id`, `@date`, `@status` and
  `note`.
- Select one `items` element: one row per item, columns `@line`, `@sku`, `@qty`, `price`.
- Select a `price` element: one row, columns `@currency` and `text()`.

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
  to use in XML Marker.
- Column hiding (GRID-15) instead of unique consolidation, which is harder to understand.
- Absent and empty values look different (GRID-13).
- Cells keep decoding entities (GRID-4).
- Sorting by column (GRID-14).

## Questions

1. Is there anything you wish XML Marker's grid did differently? Open for now.
2. The owner mostly reads the grid. Copy grid to clipboard and in-place editing (GRID-10)
   could be handy. Which comes first?
