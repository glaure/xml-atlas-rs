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

Under the menu: New, Open, Save, Cut, Copy, Paste, Undo, Redo, Find, Drill Up, Drill Down
and Format. Resting the pointer on a button names it. View > Toolbar hides it.

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

## The text

- The source, with colours. Type in it as in any editor; tree and grid follow.
- Clicking a line selects its element in the tree.
- Alt+Z turns word wrap on and off. Very long lines can be read to the end either way.

## Find and Replace

- Ctrl+F finds, Ctrl+H finds and replaces. Enter or F3 goes to the next match, Shift+Enter or
  Shift+F3 to the previous one. Match case and whole word are in the find bar.
- Replace inserts the replacement as typed, in tags and names too. When that breaks the
  document, the status bar says so and the problems list shows where; Ctrl+Z undoes it.
  Replace All is one undo step.

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
- Alt+Up and Alt+Down drill up and down; Alt+Shift+Up and Alt+Shift+Down move an element
- F2 rename (tree) or edit a cell (grid); Del delete; Ctrl+D duplicate
- Alt+Z word wrap; F1 this help

On macOS, Cmd takes the place of Ctrl.

## Settings

The layout, theme, view options, format options and recent files are remembered in
`xml-atlas/settings.toml` in the system's configuration folder.
