# XML Atlas Help

XML Atlas opens large XML files quickly and shows each as a tree, a grid and the source text.
All three show the same document, and a change in one shows in the others.

## Opening files

- File > New (Ctrl+N) starts an untitled document; saving asks for its name.
- File > Open (Ctrl+O), drop files onto the window, or name them on the command line. On
  macOS, double-click them in Finder or drop them on the Dock icon.
- Each file opens in a tab of its own. Ctrl+Tab and Ctrl+Shift+Tab switch tabs, Ctrl+W
  closes one. File > Open Recent lists the last files.
- Files in other encodings than UTF-8 (Latin-1, UTF-16 and others) open as they are and are
  saved in their own encoding.
- A file that is not well-formed still opens. The problems list under the text names each
  error; click one to go there.
- When another program changes the open file, a notice above the views offers to reload it.
- A minified file, its elements on one long line, opens with word wrap on and a notice that
  offers to format it.

## The toolbar

Under the menu: New, Open, Save, Cut, Copy, Paste, Undo, Redo, Find, XPath, Drill Up, Drill Down,
Format and Diagram. Resting the pointer on a button names it. View > Toolbar hides it.

## The tree

- Shows the elements, with attributes and text as rows of their own (View > Attributes in the
  tree turns attribute rows off).
- Click an element to select it: the grid shows its children, the text highlights it.
- Arrow keys move and expand; Home, End, Page Up and Page Down jump. Alt+Up and Alt+Down
  drill up to the parent and down into the selection.
- Right-click an element for Rename (F2), Delete (Del), Duplicate (Ctrl+D), and Move up and
  down (Alt+Shift+Up and Alt+Shift+Down).

## The grid

- Shows the selected element's children as rows. Their attributes (`@name`) and child
  elements are the columns. A cell that stands for child elements shows their name and count.
- An empty cell is an empty value; a hatched cell has no such attribute or child.
- Click a header to sort by that column; click again to reverse; `#` returns to document
  order. Right-click a header to hide columns.
- Double-click a row, or press Enter, to open it. Backspace goes back to the parent.
- Click a cell and press F2 to edit its value. The cell pane beside the grid shows the
  clicked cell in full (View turns it off).

## The diagram

- View > Diagram (Ctrl+Shift+D) shows the structure of the document in the grid's place: one
  box per path of element names, such as `orders/order/items/item`, with how many elements
  have it, the names of their attributes, and `text` when they hold text. It is read when you
  first show it, and again shortly after an edit.
- Click a box to select the first element with that path; click it again, or press F3, for
  the next one. The box of the element selected in the tree, grid or text is marked.
- The small circle right of a box collapses its children, and opens them again. Each tab
  remembers what is collapsed.
- Drag or use the wheel to move around; Ctrl+wheel zooms, Ctrl+0 goes back to 100 %.
- A document that nests one element in itself (sections in sections) has a path per depth;
  the diagram shows the first 2,000 paths and says when there are more.

## The text

- The source, with colours. Type in it as in any editor; tree and grid follow.
- Clicking a line selects its element in the tree.
- Alt+Z turns word wrap on and off. Very long lines can be read to the end either way.
- View > Font… chooses the font, style and size of the text, the tree and the grid. Only
  monospaced fonts are listed.

## References

- For files with references of their own, described in rules files. Off until you turn on
  View > Resolve references.
- File > Settings > References lists the rules files. Import one or make a new one, switch it
  on and say in Use for which documents it is for: `catalog*.xml` by name, `*/XML/*catalog.xml`
  by path, `*` for every document. Of several for one document the upper one is used.
- A rules file lying beside an XML file is not used: import it.
- Then F12 goes to what the selected reference finds, and references that find nothing are
  listed as problems.
- View > Preview with references resolved shows the selected element resolved; Preview tree
  shows that as a tree. The file is never changed.

## Find and Replace

- Ctrl+F finds, Ctrl+H finds and replaces. Enter or F3 goes to the next match, Shift+Enter or
  Shift+F3 to the previous one. Match case and whole word are in the find bar.
- Replace inserts the replacement as typed, in tags and names too. When that breaks the
  document, the status bar says so and the problems list shows where; Ctrl+Z undoes it.
  Replace All is one undo step.

## XPath

- Edit > XPath (Ctrl+Shift+X) opens a query bar for an XPath 1.0 expression, such as
  `//order[@status='open']`, `count(//item)` or `//order[@id='42']/customer/name`. Enter
  runs it; the list button holds the expressions run before. In the empty field, Tab takes
  the example it shows. While you type, the bar says
  whether the expression is valid; a wrong one is framed in red, the place where it goes
  wrong is underlined, and the message says what is expected there.
- The nodes it selects are listed at the bottom with their line, path and value. Click one to
  go to it; F3 and Shift+F3 step through them while the bar is open. A number, text or
  true/false is shown in the bar.
- A long query shows a spinner; Stop ends it. After an edit, Enter runs the query again.
- Names without a prefix also match elements in the default namespace of the root element.
  Text that is only whitespace between elements is not a node.

## Formatting

- Edit > Format indents the document; it changes only whitespace between tags. Text, mixed
  content and `xml:space="preserve"` stay as they are.
- Edit > Compact puts the document on one line.
- Edit > Format options sets the indent, tabs, attributes per line, and whether to remove
  comments and trim text.
- Each of these is one undo step.

## Undo and saving

- Ctrl+Z undoes, Ctrl+Y or Ctrl+Shift+Z redoes, also after Format.
- Ctrl+S saves, Ctrl+Shift+S saves under another name. Saving is atomic: if it fails, the
  old file is still whole. Only what you changed is changed; the byte order mark and line
  endings stay.
- Closing a tab or the window with unsaved changes asks first.

## Keyboard

- Ctrl+N new, Ctrl+O open, Ctrl+S save, Ctrl+Shift+S save as, Ctrl+W close tab
- Ctrl+Tab and Ctrl+Shift+Tab next and previous tab
- Ctrl+Z undo, Ctrl+Y or Ctrl+Shift+Z redo
- Ctrl+F find, Ctrl+H replace, F3 and Shift+F3 next and previous match
- Ctrl+Shift+X XPath; with its bar open, F3 and Shift+F3 step through its results
- Ctrl+Shift+D diagram; after a click on a box, F3 goes to its next element
- Alt+Up and Alt+Down drill up and down; Alt+Shift+Up and Alt+Shift+Down move an element
- F2 rename (tree) or edit a cell (grid); Del delete; Ctrl+D duplicate
- F12 go to reference target; Alt+Z word wrap; F1 this help

On macOS, Cmd takes the place of Ctrl.

## AI agents

An agent such as Claude Code can work with XML files through XML Atlas, over the Model
Context Protocol (MCP):

- On files, without a window: `claude mcp add xml-atlas -- xml-atlas-cli mcp`.
- In this window, on the open tabs: turn on Allow AI agents (MCP) in File > Settings, then
  `claude mcp add xml-atlas-window -- xml-atlas-cli mcp --window`. Each change an agent
  makes is selected and named in the status bar, and Ctrl+Z undoes it. An agent cannot close
  a tab with unsaved changes.

Agents read elements, tables of children, lines of text, Find matches, problems and XPath
results; they change text, attributes and names, delete, duplicate, move, replace, format,
undo and save.

## Settings

File > Settings… (Ctrl+,) shows every setting in one window: appearance, views, references,
format options, updates and AI agents. A change applies at once. Appearance shows an example of
each theme; click one to use it.

## Updates

Once a day XML Atlas asks GitHub for its latest release. When it is newer, the status bar
says so once, with a link to the release; a `.deb`, `.rpm` or Flatpak is updated with the
package manager instead. The request carries the program's version and nothing else, and
nothing is downloaded or installed. Without a network nothing is shown. Help > Check for
updates now asks at once and also says when you have the latest version. Turn the daily
check off in File > Settings > Updates.

The layout, theme, view options, text font, format options and recent files are remembered in
`xml-atlas/settings.toml` in the system's configuration folder. So are the sizes of the
window and of the panes; View > Reset layout gives the panes their first sizes again.
