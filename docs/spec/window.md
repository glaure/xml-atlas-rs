# Window

How the views are laid out, and how the window looks.

## Rules

- **WIN-1** `[done]` Below the menu, the tabs of the open documents (OPEN-5). The tree is on
  the left. View > Layout chooses what is on the right:
  *Text above grid* (the default), as in XML Marker (XM-1), or *Grid above text*. The change is
  immediate and keeps the open file. The find bar sits directly above the text when open
  (TEXT-12), and the problems list directly under it when there are problems (OPEN-13). The
  borders between the views can be dragged; each layout keeps its own grid height. The choice
  is remembered (WIN-3).
- **WIN-3** `[done]` Settings are remembered between sessions: the layout (WIN-1), attribute
  rows in the tree (TREE-1), the Format options (FMT-10 to FMT-13), and Find's match case and
  whole word (TEXT-12), and the recent files (OPEN-12). They are kept in `xml-atlas/settings.toml` in the platform's
  configuration folder (`~/.config` on Linux, `%APPDATA%` on Windows, `~/Library/Application
  Support` on macOS); `XML_ATLAS_SETTINGS` can name another file. The file is written, atomically,
  whenever a setting changes, and not before; it is readable TOML. A missing file gives the
  defaults; a line that cannot be read is skipped, with a warning in the log, and the others
  still count.
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
  Checking for a newer version (PKG-7) is to come in this menu.
- **WIN-5** `[built]` A toolbar under the menu, after XML Marker's (XM-58): New, Open, Save |
  Cut, Copy, Paste | Undo, Redo, Find | Drill Up, Drill Down, Format. Each button does what
  its menu entry or key does and is greyed out when that is: Save without changes, Cut and
  Copy without a text selection, Undo and Redo with nothing to undo or redo, the rest
  without an open document (except New and Open). Resting the pointer on a button names it
  and its key. The icons are symbols from egui's own fonts, no images. View > Toolbar shows
  or hides it, remembered (WIN-3). Not like XML Marker: no Locate in tree, as the tree
  follows the selection by itself, and the toolbar stays under the menu; egui cannot move it.
- **WIN-6** `[built]` View > Allow AI agents (MCP), off by default and remembered (WIN-3),
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
  So far (D29 step 1): the colours are in one `Theme` in `apps/xml-atlas/src/theme.rs`,
  built from the ones before, so the window looks the same. Spacing and the contrast check
  are still to come.
- **WIN-8** `[planned]` A softer Light: panels in a mid grey, the text and grid areas
  off-white rather than pure white, darker separators, so the views stand apart and the
  window glares less. Chosen by the owner from candidates in the running program. Dark
  stays as it is.
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

## Acceptance tests

- Choose Text above grid, turn off View > Attributes in the tree, choose Tabs in Edit > Format
  options, quit, start again: all three are as chosen.
- Open a file with problems, choose View > Layout > Text above grid: text on top, the
  problems list under it, the grid at the bottom; the selection is unchanged. Choose Grid above
  text: back as before.
- With the system in light mode, start XML Atlas: it is light. Switch View > Theme to Dark: the
  whole window turns dark at once, without reopening the file.
- With Follow system, switch the desktop between light and dark while XML Atlas runs: the
  window follows within a moment.
- Press F1: the help window opens, its text wraps with the window's width and scrolls. Help >
  About XML Atlas shows the version the package was built as; its links open in the browser.
- Turn on View > Allow AI agents (MCP) and connect Claude Code with `xml-atlas-cli mcp
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
