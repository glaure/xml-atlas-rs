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

## Questions

1. When does XML Marker show a table: for any selected element, or only when its children
   repeat the same name?
2. If the children have different names, what happens? One table, several, or none?
3. Which columns does it make? Attributes, child elements, both? In what order?
4. How does it show a child element that itself has children?
5. How does it show a row that has the same child twice?
6. Does clicking a column header sort? Can columns be reordered or hidden?
7. Can you edit a cell? What happens in the text view when you do?
8. The Qt XML Atlas has "Table Selection Browser" and "Unique consolidation" ("reduce the
   number of unique columns by merging them to one"). What are they for?
9. What does selecting several rows do?
10. Is there anything you wish XML Marker's grid did differently?
