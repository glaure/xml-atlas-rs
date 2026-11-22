# Window

How the views are laid out, and how the window looks.

## Rules

- **WIN-1** `[done]` Below the menu, the tabs of the open documents (OPEN-5). The tree is on
  the left. View > Layout chooses what is on the right:
  *Text above grid* (the default), as in XML Marker (XM-1), or *Grid above text*. The change is
  immediate and keeps the open file. The find bar sits directly above the text when open
  (TEXT-12), and the problems list directly under it when there are problems (OPEN-13). The
  borders between the views can be dragged; each layout keeps its own grid height. The choice
  is remembered (WIN-3). View > Grid hides the grid, and its cell pane, so that the text and the
  reference preview (REF-5) have the room; the arrow keys then go to the tree. Remembered
  too. View > Diagram (Ctrl+Shift+D) shows the structure diagram in the grid's place, with a
  height of its own per layout, also while the grid is hidden (D36); remembered too. Its
  *Levels* control collapses it below a depth, and Find's match and XPath's results mark
  their boxes. *Export as SVG…* writes it as an image.
- **WIN-3** `[done]` Settings are remembered between sessions: the layout (WIN-1), attribute
  rows in the tree (TREE-1), whether the grid is shown (WIN-1), the Format options (FMT-10 to FMT-13), and Find's match case and
  whole word (TEXT-12), and the recent files (OPEN-12). They are kept in `xml-atlas/settings.toml` in the platform's
  configuration folder (`~/.config` on Linux, `%APPDATA%` on Windows, `~/Library/Application
  Support` on macOS); `XML_ATLAS_SETTINGS` can name another file. The file is written, atomically,
  whenever a setting changes, and not before; it is readable TOML. A missing file gives the
  defaults; a line that cannot be read is skipped, with a warning in the log, and the others
  still count.

  Also remembered (D38): the window's size and whether it was maximized (on macOS the size
  only; never its position, which Wayland does not tell), and each pane's share of the room
  it has, per layout for the grid and the diagram. A share changes only when its border is
  dragged, so neither what a pane holds nor a smaller window changes it, and a pane keeps its
  share when the window changes its size. The tree is 320 points wide until it is dragged.
  The problems and XPath results are never higher than their rows need. View > *Reset layout*,
  and Settings > Views > *Reset to defaults*, give the panes their first sizes again and
  leave the window as it is. A change of the window is written a second after it stops, or
  at exit.
- **WIN-2** `[done]` Light and dark theme. View > Theme chooses *Follow system* (the default),
  *Light* or *Dark*; the choice is remembered (WIN-3) and takes effect at once. Text colours
  (TEXT-2), tree icons (TREE-11), highlights, grid hatching and problem markers follow it.
  Following the system: on Linux the program asks the desktop's settings portal
  (`org.freedesktop.appearance`, `color-scheme`, answered by GNOME, KDE and others) and
  follows it while running, because the window library does not report the theme there; with
  no portal it falls back to dark and says why in the log. On Windows and macOS the window
  library reports the system theme. The Qt XML Atlas has the same choice (QT-12).
- **WIN-4** `[built]` A Help menu. *XML Atlas Help* (also F1) opens a window with a short
  guide: opening files, the tree, the grid, the text, Find and Replace, formatting, undo and
  saving, the keys, and where the settings are. The guide is built into the program, so it
  works offline; its text is `apps/xml-atlas/assets/help.md`, kept up to date with the
  features. *About XML Atlas* shows the version, what the program is, its licenses (and
  where the libraries' licenses are), and links to the project page and the releases.
  *Check for updates now* asks for a newer version at once (PKG-7, D30).
- **WIN-5** `[built]` A toolbar under the menu, after XML Marker's (XM-58): New, Open, Save |
  Cut, Copy, Paste | Undo, Redo, Find, XPath | Drill Up, Drill Down, Format | Diagram (D36,
  switches View > Diagram). Each button does
  what its menu entry or key does and is greyed out when that is: Save without changes, Cut and
  Copy without a text selection, Undo and Redo with nothing to undo or redo, the rest without
  an open document (except New and Open). Resting the pointer on a button names it and its key.
  The icons are drawn as shapes in the button's text colour, as the tree's are (TREE-11), no
  images and no font symbols: egui 0.37 takes symbols missing from its own fonts from the
  system's, and Windows has no font it finds for New, Open, Copy, Drill Up, Drill Down and
  Format, so 1.0.0 showed boxes there (reported 2026-11-18). View > Toolbar shows or hides it,
  remembered (WIN-3). Not like XML Marker: no Locate in tree, as the tree follows the selection
  by itself, and the toolbar stays under the menu; egui cannot move it.
- **WIN-6** `[built]` *Allow AI agents (MCP)* in Settings (WIN-11; in the View menu until
  2026-11-18), off by default and remembered (WIN-3),
  lets an agent such as Claude Code work in the window through `xml-atlas-cli mcp --window`
  (`docs/decisions.md`, D28). While it is on, the window listens on `127.0.0.1` and writes
  the port and a secret to `agent.json` next to the settings file, readable only by the
  user; turning it off or closing the window removes the file. An agent's call runs between
  two frames: a change is one undo step, its tab is shown and the element it changed
  selected, and the status bar names it ("Agent: rename /r/b"); a refusal is shown as a
  warning. The status bar says "Agent connected" while one is. An agent may open files as
  tabs and save, but not close a tab with unsaved changes. Tried by the owner with Claude
  Code on Linux (2026-11-17).
- **WIN-7** `[planned]` Every colour of the window is part of the theme: panels, menus, the
  text and grid backgrounds, text and weak text, separators, buttons, the selection, links,
  warnings and errors, the syntax colours (TEXT-2), the marked element and find matches
  (TEXT-6, TEXT-12), the grid's hatching (GRID-13), tree icons (TREE-11) and problem markers
  (OPEN-13); also corner rounding and spacing. Changing the theme changes all of them at
  once and costs nothing per frame. Every built-in theme passes a contrast check: 4.5:1 for
  text and syntax colours, 3:1 for weak text, on their backgrounds (`docs/decisions.md`, D29).
  So far (D29 steps 1 and 2): the colours are in one `Theme` in
  `apps/xml-atlas/src/theme.rs`, and a test checks the contrast of every built-in theme.
  Dark's weak text (2.66:1) and punctuation (4.36:1) and Classic Light's weak text (2.89:1)
  miss it and are kept as in 1.0 (owner, 2026-11-18). Spacing is still to come.
- **WIN-8** `[built]` A softer Light: menus, toolbars and the status bar in a light grey
  (232), the tree, grid, cell pane and text a little lighter (247), darker separators (182)
  and text (45), so the views stand apart and the window glares less. *Light style* in Settings (WIN-11)
  chooses *Soft grey* (the default) or *Classic, as in 1.0*; remembered (WIN-3). The owner
  chose these from three candidates in the running program (2026-11-18): a light grey, a
  mid grey (220 and 244) and a warm paper tint. Dark stays as it is.
- **WIN-9** `[planned]` View > Theme has two choices, both remembered (WIN-3): the *mode*
  of WIN-2 (Follow system, Light, Dark) and the *style*: XML Atlas (the default), Native, or
  a theme file. Every style has a light and a dark variant; the mode picks which one shows.
  *Native* looks like the system it runs on (Windows 11, macOS, GNOME, KDE) as far as egui
  allows: colours, rounding, spacing, the system's UI font, and the system's accent colour
  for the selection and focus, followed while running. Its colours are our own choice;
  nothing of the systems' is copied.
- **WIN-10** `[planned]` Theme files: TOML in `themes/` next to the settings file, with a
  `[light]` and a `[dark]` part and named colours, edited in a text editor (no editor in
  the window). Each file there is a style in View > Theme. *Save a copy of this theme*
  writes the current style as a file to start from; *Open the themes folder* shows the
  folder. A changed file is read again when the window gets the focus back. A missing or
  unreadable colour or part comes from the XML Atlas style, with a warning in the log.
- **WIN-11** `[built]` *File > Settings…* (Ctrl+,; Cmd+, on macOS) opens a window with
  every remembered setting (WIN-3), in sections: *Appearance* (theme mode, light style, the
  font of text, tree and grid, the toolbar), *Views* (layout, attribute rows in the tree,
  the grid, the cell pane, word wrap for the tabs not switched with Alt+Z), *References* (REF-12, with the preview and its tree, and the rules files, REF-13), *Format* (as Edit > Format
  options…), *Updates* (PKG-7: on or off, when it last checked, *Check now*) and *AI agents* (WIN-6). The font's *Choose…* opens the font dialog (TEXT-25). A
  change applies and is saved at once, without OK and Cancel; each section has *Reset to
  defaults*. A test checks that every setting but the recent files and the find options is
  in a section, so a new setting cannot be left out.
  The View menu keeps word wrap, the layout, the grid, the cell pane, references, their
  preview and its tree,
  attribute rows, the toolbar and the theme mode as shortcuts to the same settings; *Allow AI agents* and *Light style* are
  only in Settings. Not in Settings: the recent files, window and pane sizes, the find
  options (`docs/decisions.md`, D31).
- **WIN-12** `[built]` Settings > Appearance shows how each theme looks: above each of
  *Follow system*, *Light*, *Dark*, *Soft grey* and *Classic* a small example window with
  menu, toolbar, tree, coloured text, the selected element, grid and status bar, in that
  theme's colours. *Follow system* is half light, half dark; it and *Light* show the chosen
  light style. A click on an example chooses it. The examples are drawn from the themes,
  not stored as images. On a screen too low for the section, it scrolls.

## Acceptance tests

- Choose Text above grid, turn off View > Attributes in the tree, choose Tabs in Edit > Format
  options, quit, start again: all three are as chosen.
- Drag the tree's, the grid's and the preview's borders, resize the window, quit, start again:
  all four are as left. Maximize, quit, start again: maximized, and un-maximizing gives the
  size from before. View > Reset layout: the panes' first sizes, the window unchanged.
- Open a file with problems, choose View > Layout > Text above grid: text on top, the
  problems list under it, the grid at the bottom; the selection is unchanged. Choose Grid above
  text: back as before.
- With the system in light mode, start XML Atlas: it is light. Switch View > Theme to Dark: the
  whole window turns dark at once, without reopening the file.
- With Follow system, switch the desktop between light and dark while XML Atlas runs: the
  window follows within a moment.
- In Settings > Appearance click the picture of Dark: the window turns dark. Click the
  picture of Classic: the pictures of Light and of Follow system's left half change with it.
- Press F1: the help window opens, its text wraps with the window's width and scrolls. Help >
  About XML Atlas shows the version the package was built as; its links open in the browser.
- Turn on *Allow AI agents (MCP)* in Settings and connect Claude Code with `xml-atlas-cli mcp
  --window`; ask it to rename an element in the open file: the tab shows it renamed and
  selected, Ctrl+Z undoes it. Ask it to close the tab: refused while there are unsaved
  changes. Turn the setting off: `agent.json` is gone and the agent is told to turn it on.
- Turn off View > Toolbar, quit and start again: no toolbar. Turn it on: click a grid row,
  then Drill Up and Drill Down on the toolbar: the selection goes to the parent and back.
  Select text, Copy and Paste on the toolbar: pasted at the cursor.
- Choose Style Native with Follow system, switch the desktop between light and dark: the
  window follows, in Native's variants. Change the system's accent colour: the selection
  takes it.
- Save a copy of the theme, change `tag` in its `[light]` part in a text editor, switch
  back to XML Atlas: the tags have the new colour. Write `tag = "nonsense"`: the tags have
  the XML Atlas colour and the log says why.

## Seen in the reference programs

- XML Marker: tree top left with the error list as a second tab, its Table Selection Browser
  bottom left, the text top right and its Tree Selection Browser (the grid) bottom right
  (XM-1). There is no theme setting.
- Qt XML Atlas: tree docked left, text in the middle, table docked below; a light and dark
  theme chosen in a dialog, by default following the system (QT-12).

## Decided

- Both layouts, switchable: today's and XML Marker's (WIN-1). (Owner, 2026-11-14.)
- Theme support: light, dark, and following the system (WIN-2). (Owner, 2026-11-14.)
- The default layout, until the user changes it: tree on the left, text at the top right,
  the grid below the text. (Owner, 2026-11-16.)
- Help holds a built-in guide and About, and later the check for a newer version (WIN-4,
  PKG-7). (Owner, 2026-11-17.)
- A toolbar after XML Marker's, without Locate in tree, fixed under the menu; File > New
  added for it (WIN-5, OPEN-22). (Owner, 2026-11-17.)
- Themes (D29): dark is good, light is too light next to it, users will want to change the
  colours, and a native look. A softer Light first; then a style next to the light/dark
  mode, Native as one style, theme files edited in a text editor (WIN-7 to WIN-10). The
  macOS menu bar at the top of the screen later, as its own decision. (Owner, 2026-11-17.)

## Questions

None open.
