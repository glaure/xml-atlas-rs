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

## Known gaps to decide

- A row with two child elements of the same name shows only the first in that column.
- A child element that has children of its own gets no column; nothing shows it is there.
- Children with different names share one table.

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
  table. XML Atlas has no attribute table for the selected element.
- Columns in XML Marker: tag name, text, the children's attributes, then the children's own
  child elements (XM-32). GRID-2 has the same set in another order, with `@` before attribute
  names instead of an icon.
- A child element that has attributes or children of its own gets a column in XML Marker; the
  cell shows its name, not its text (XM-34). In XML Atlas it gets no column (first known gap).
- The same child twice in one row: XML Marker shows the name with a count (XM-34). XML Atlas
  shows the first (second known gap).
- Child names that occur in one row only share one "Unique Subtags" column in XML Marker,
  controlled by a threshold (XM-33). The Qt XML Atlas copies this and adds "Unique
  Attributes" (QT-5). XML Atlas makes a column for each.
- XML Marker draws a cell with no value differently from an empty value (XM-35). XML Atlas
  leaves both blank.
- XML Marker's cells show raw source, `&amp;` included (XM-36). GRID-4 decodes on purpose.
- Clicking a cell or row in XML Marker never moves the tree selection. It highlights the range
  in the text and shows the item in a second pane, the Table Selection Browser; a cell that
  stands for several elements opens as a table there (XM-37). Double-click does the same.
  GRID-6 matches for the text highlight. GRID-7 differs: double-click moves the selection.
- Clicking a header in XML Marker does not sort (XM-39). Its grid can hide empty columns per
  row or for the visible rows, and copy the grid to the clipboard (XM-38).
- "Table Selection Browser" in the Qt XML Atlas is a menu entry without code (QT-6).

## Decisions needed

1. A table of the selected element's own attributes above the child table, as in XML Marker?
2. Nested children: adopt XML Marker's cells (name, or name with count) so that nothing is
   hidden? This would close the first two known gaps.
3. A second pane for what is clicked in the grid, or keep one grid and drill in with
   double-click (GRID-7)? The second pane lets you look inside a row without losing your
   place; the double-click is simpler.
4. Unique consolidation: copy it, or leave wide tables wide and add column hiding instead?
5. Mark absent values differently from empty ones?
6. Keep decoding entities in cells (GRID-4)? It reads better; the raw form matters when
   editing a cell (GRID-10).
7. Sorting by a column. XML Marker has none; it would be an addition.

## Questions

1. Is there anything you wish XML Marker's grid did differently?
2. Which of its grid commands do you use: in-place edit, copy grid to clipboard, minimize
   empty columns?
3. What does the drop-down in a column header offer? Not opened yet.
4. What does selecting several rows do? Not tried yet.
