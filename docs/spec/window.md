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

## Seen in the reference programs

- XML Marker: tree top left with the error list as a second tab, its Table Selection Browser
  bottom left, the text top right and its Tree Selection Browser (the grid) bottom right
  (XM-1). There is no theme setting.
- Qt XML Atlas: tree docked left, text in the middle, table docked below; a light and dark
  theme chosen in a dialog, by default following the system (QT-12).

## Decided

- Both layouts, switchable: today's and XML Marker's (WIN-1). (Owner, 2026-11-14.)
- Theme support: light, dark, and following the system (WIN-2). (Owner, 2026-11-14.)

## Questions

1. Which layout should be the default once settings are remembered?
