# What the reference programs do

Observations of XML Marker 1.1 and of the Qt XML Atlas. The area files (`open.md`, `tree.md`,
`grid.md`, `text.md`, `format.md`) refer to the ids here. This file records what was seen. It
does not decide what XML Atlas should do; that is the job of the rules in the area files.

## How this was observed

- **XML Marker 1.1** (ids `XM-…`): run on 2026-11-14 under wine 10.0 on Linux, driven with
  synthetic mouse and keyboard input, with small hand-written probe files. Only what appeared
  on screen is recorded. Its help file and its web site were not read. Nothing from the program
  is kept in this repository.
- **The owner's experience** with XML Marker on Windows fills some gaps. Those items say
  "Reported by the owner".
- **Qt XML Atlas** (ids `QT-…`): read from its source (`glaure/xml-atlas`, commit `9c8024f`).
  It was not run.

Limits:

- Everything about XML Marker is from one session under wine with a Western European code page.
  Character handling (XM-5 to XM-8) may differ on Windows.
- Each item was seen once or twice on small files. Nothing here was checked on a large file.
- The probe files had no namespaces, DOCTYPE or entities other than `&amp;`.

## XML Marker: window

- **XM-1** Four panes. Top left: the tree, sharing its place with an error list (two tabs; the
  second tab's label shows the warning and error counts). Bottom left: "Table Selection
  Browser". Top right: the text. Bottom right: "Tree Selection Browser".
- **XM-2** Menus: File (New, New DTD, Open, Save, Save As, nine recent files, Exit), Edit
  (Undo, Redo, Cut, Copy, Paste, Delete, Select All, Indent, Find, Replace), View (Toolbar,
  Status Bar, Table Selection Browser), Options (Font, Indent Options, Tabs, Unique
  consolidation, Text highlighting options, Parse this document as DTD), Navigate (Locate in
  tree, Drill Down, Drill Up), Help.
- **XM-3** Shortcuts shown in the menus: Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Z, Ctrl+Y, Ctrl+X,
  Ctrl+C, Ctrl+V, Del, Ctrl+A, Ctrl+I (Indent), Ctrl+F, Ctrl+H, Ctrl+L (Locate in tree),
  Alt+Down (Drill Down), Alt+Up (Drill Up), Ctrl+E (Editor Select), Ctrl+T (Insert Tag),
  Ctrl+R (Insert Attribute), Ctrl+U (Comment Out Selection), Ctrl+M (Mark Selection With Tag),
  F5 (minimize empty columns for visible rows).
- **XM-58** A toolbar under the menu, observed by the owner (2026-11-17), in this order with
  separators: New, Open, Save | Cut, Copy, Paste | Undo, Redo, Find | Locate in tree, Drill
  Up, Drill Down, Indent. View > Toolbar hides it; it can be moved.
- **XM-4** The status bar shows "Ready" and the cursor as position, line and column. No file
  size, element count or timing.

## XML Marker: opening

- **XM-5** One document at a time. Started with two file arguments, it opened the first and
  ignored the second. One window.
- **XM-6** A UTF-8 file is shown byte by byte in the system code page: `ü` appears as two wrong
  characters, in the text, the tree and the grid. A UTF-8 byte order mark appears as three
  stray characters before the XML declaration. The file still parses.
- **XM-7** A Latin-1 file (declared ISO-8859-1) is shown correctly.
- **XM-8** A UTF-16 file with a byte order mark opens and parses. Characters that exist in the
  code page are right; Japanese characters came out as unrelated characters.
- **XM-9** After opening, the root element is in the tree, collapsed. Nothing is selected and
  both browsers say they are empty.
- **XM-10** A file that is not well-formed opens without a dialog. The left pane switches to
  the error list. An error is marked in the text at its position with a red marker and a
  short message. Selecting an error shows its description, its location in words and what was
  found instead.
- **XM-11** `<a><b>one</b><c>two</a>` gives one warning (start and end tag do not match) and
  one error (tag not closed), and the tree is empty. The same for a longer file with an
  unclosed tag followed by a second root element.
- **XM-12** A stray `<` typed inside a start tag gave one error, but the tree still showed the
  whole document, including elements after the error.
- **XM-13** The title bar shows the file name, with `*` after an unsaved change. Closing the
  window with unsaved changes asks whether to save (yes, no, cancel).
- **XM-14** It does not notice when the open file changes on disk. (Reported by the owner.)

## XML Marker: tree

- **XM-20** Tree rows are elements, attributes and text. Under an element come first its
  attributes, one row each as `name = "value"`, then its children in document order. The text
  of an element is a row of its own (seen for an element with one attribute and text).
  Comments and processing instructions are not in the tree.
- **XM-21** An element row shows only the name. There is no value hint beside it. Its icon
  differs by kind: with child elements, with text, with text and attributes, empty.
- **XM-22** Selecting a tree row at once highlights its whole extent in the text (tinted
  background with an outline, scrolled into view) and fills the Tree Selection Browser.
  Selecting an attribute row highlights just `name="value"` in the text.
- **XM-23** The right-click menu of a tree row has one entry: Editor Select (Ctrl+E). It
  selects the element's whole text range in the text, ready to copy. The tree offers
  navigation only; elements cannot be renamed or moved there. (Reported by the owner.)
- **XM-24** Nothing in the tree marks an error. Errors are only in the error list and the
  text.

## XML Marker: grid (the two "selection browsers")

- **XM-30** The Tree Selection Browser shows the element selected in the tree: a heading with
  its name, a two-column table of its attributes (name, value), then a table of its child
  elements ("subtags") with a count above each table. An element without attributes says so
  in a line of text.
- **XM-31** The child table has one row per child element, in document order, whatever their
  names. Children with different names share the one table.
- **XM-32** Columns, left to right: the child's tag name; a text column when some child has
  text; one column for each attribute name found on the children, in order of first
  appearance; then columns for the children's own child elements (grandchildren of the
  selection), named after them.
- **XM-33** A grandchild name that occurs under only one row does not get its own column. Such
  names share one column, "Unique Subtags". In a row with one such grandchild the cell shows
  its name; with several different ones it shows a count ("3 unique subtags"). The setting
  for this (Options > Unique consolidation) is a number, default 2: consolidate only if that
  removes at least so many columns. With one unique name the column kept its own name.
- **XM-34** What a grandchild cell shows: the text, if the grandchild has only text; the
  element's name as a link-like entry, if it has attributes or children of its own; the name
  and a count ("note (2 occurrences)"), if the row has several grandchildren of that name.
- **XM-35** A cell with no value is drawn hatched, so "absent" and "empty" look different.
- **XM-36** Cells show the raw source: `&amp;` stays `&amp;`, and a CDATA section shows with
  its `<![CDATA[` and `]]>`. Mixed content shows as "Text (2 occurrences)".
- **XM-37** Clicking a cell or a row name does not change the tree selection. It turns the
  cell yellow, outlines the matching range in the text (a second highlight inside the first),
  and shows that item in the Table Selection Browser: an element with its attributes and text
  or children, a text value as plain text, a group of occurrences as a table of its own.
  Double-clicking does the same as clicking.
- **XM-38** Right-click menu in the grid: Cut, Copy, Paste, In-place edit, Edit using editor,
  Editor Select, Copy grid to clipboard, and "minimize empty columns" (for this row, for this
  row in addition, for the visible rows, all, restore all). The two edit entries were disabled
  for the cell tried. Selecting several rows is not possible. (Reported by the owner.)
- **XM-39** Clicking a column header did not reorder the rows. A small drop-down button
  appeared in the header; it sets the sort direction of that column. (Reported by the owner.)

## XML Marker: text and navigation

- **XM-40** The text is editable, coloured (tag names, attribute names, attribute values and
  comments differ), in a proportional font by default, without line numbers. A one-line file
  ran off to the right; it was not wrapped.
- **XM-41** Clicking in the text moves only the cursor. Tree and grid do not follow.
- **XM-42** Locate in tree (Ctrl+L) selects the innermost tree row that contains the cursor,
  expanding its ancestors. With the cursor in an attribute value it selected the attribute
  row.
- **XM-43** Drill Up (Alt+Up) selects the parent of the selected row. Drill Down (Alt+Down)
  then went back down to the row that contains the cursor. Seen for one step each.
- **XM-44** Typing in the text re-parses at once: 0.15 s after three keystrokes the tree was
  already rebuilt. The rebuilt tree was collapsed to the root and the selection was gone.
- **XM-45** Right-click menu in the text: Cut, Copy, Paste, Insert Tag, Insert Attribute,
  Comment Out Selection, Mark Selection With Tag. Insert Tag and Insert Attribute open a small
  dialog for name and value. (Reported by the owner.)
- **XM-46** Find: text to find (with history), match whole word, match case, direction up or
  down. Replace: the same two options, replace in selection or whole file, Find Next, Replace,
  Replace All. No regular expressions. No search in tree or grid.
- **XM-47** Options > Tabs: tab size (4), keep tabs or insert spaces. Options > Text
  highlighting options: solid or dotted outline for the highlighted range.

## XML Marker: Indent

Seen with the default options on a one-line file.

- **XM-50** Options: indent size (4), maximum number of attributes in a tag line (6), remove
  comments (off), show this dialog before indenting (on).
- **XM-51** Each element goes on its own line, indented four spaces per level. An element with
  only text stays on one line.
- **XM-52** Text is changed: `<t>  padded  </t>` became `<t>padded</t>`, and mixed content
  `<p>Hello <b>big</b> world</p>` was split over five lines, which adds whitespace to the
  text.
- **XM-53** Attributes are rewritten as `name = "value"`, with spaces around the equals sign,
  even when there is only one. A tag with eight attributes got one attribute per line.
- **XM-54** `xml:space="preserve"` was not respected: the content was re-indented.
- **XM-55** A comment lost the space after `<!--`. A comment that followed an end tag stayed
  on that tag's line.
- **XM-56** After Indent the tree was collapsed to the root.
- **XM-57** Indent cannot be undone. (Reported by the owner.)

## Qt XML Atlas

- **QT-1** One document, one window. No file argument on the command line. Opening reads the
  whole file, parses it with pugixml and puts the text in a `QPlainTextEdit`. Up to ten recent
  files.
- **QT-2** The tree shows elements and, under each, its attributes as `name = "value"`. No
  text rows. After opening, only the root is expanded.
- **QT-3** A parse error adds one row to the tree with the parser's message. Everything parsed
  before the error stays in the tree.
- **QT-4** Selecting a tree row highlights the element (or the attribute) in the text, moves
  the cursor there and fills the table view.
- **QT-5** The table view follows XM-30 to XM-34: attribute table, then a child table with tag
  name, attribute columns, grandchild columns, and "Unique Attributes" / "Unique Subtags"
  columns. The consolidation setting ("Unique columns", 1 to 64, default 2) is compared with
  the number of names that occur once. Unlike XML Marker, text and CDATA children of the
  selected element get rows of their own.
- **QT-6** "Table Selection Browser" (View menu) and "Drill down" / "Drill up" are menu
  entries without code behind them, as are New DTD, Delete, Select All, Replace, Toolbar,
  Status Bar, Tabs, Text highlighting options and Parse this document as DTD. They are
  placeholders named after XML Marker's entries (XM-2).
- **QT-7** "Locate in tree" (Ctrl+L) selects and expands the element that contains the text
  cursor. It never selects an attribute row.
- **QT-8** Every change to the text re-parses the whole document and rebuilds the tree, fully
  expanded.
- **QT-9** Find takes plain text only, F3 and Shift+F3 repeat it. Matches are marked yellow.
- **QT-10** Indent rewrites the whole document from the parsed tree: indent size (default 4),
  spaces. More attributes than the maximum (default 6): one per line. Attributes are written
  as `name = "value"`. Text is trimmed; text of several lines is re-indented line by line.
  Comments are kept; "Remove comments" and "Show this dialog before indenting" are in the
  dialog but not connected. An XML declaration is added if the document has none. Entities
  and attribute values are written back unescaped, so `&amp;` becomes a bare `&`.
- **QT-11** Save writes the editor text to the file directly, not atomically. No prompt for
  unsaved changes.
- **QT-12** Light and dark theme, editor font choice, line numbers in the editor.
- **QT-13** Packaging (read from `CMakeLists.txt` and the CI workflow): a Windows MSI built
  with CPack and WiX, with the minimal installer UI, a license page, a product icon, a Start
  menu shortcut "XMLAtlas" and a fixed upgrade code, so a newer MSI replaces an older one.
  Qt's libraries are copied next to the program (`windeployqt`). A Flatpak section exists
  with placeholder values (`com.example.XMLAtlas`) and is not used. CI builds on Ubuntu and
  Windows; no Linux or macOS package is produced. Version 0.2.0.

## Not observed yet

- XML Marker on a large file: scrolling, selecting, how long a re-parse after a keystroke
  takes.
- What In-place edit, Edit using editor, Comment Out Selection, Mark Selection With Tag, New
  DTD and Parse this document as DTD do. The owner does not use the DTD functions.
- Undo granularity in the text.
- Namespaces, DOCTYPE, entities, very long lines, very deep nesting.
