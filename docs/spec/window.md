# Window

How the views are laid out, and how the window looks.

## Rules

- **WIN-1** `[done]` The tree is on the left. View > Layout chooses what is on the right:
  *Grid above text* (the default) or *Text above grid*, as in XML Marker (XM-1). The change is
  immediate and keeps the open file. The find bar sits directly above the text when open
  (TEXT-12), and the problems list directly under it when there are problems (OPEN-13). The
  borders between the views can be dragged; each layout keeps its own grid height. The choice
  is remembered (WIN-3).
- **WIN-3** `[done]` Settings are remembered between sessions: the layout (WIN-1), attribute
  rows in the tree (TREE-1), the Format options (FMT-10 to FMT-13), and Find's match case and
  whole word (TEXT-12). They are kept in `xml-atlas/settings.toml` in the platform's
  configuration folder (`~/.config` on Linux, `%APPDATA%` on Windows, `~/Library/Application
  Support` on macOS); `XML_ATLAS_SETTINGS` can name another file. The file is written, atomically,
  whenever a setting changes, and not before; it is readable TOML. A missing file gives the
  defaults; a line that cannot be read is skipped, with a warning in the log, and the others
  still count.
- **WIN-2** `[planned]` Light and dark theme. A setting chooses light, dark, or following the
  system, which is the default. Text colours (TEXT-2), tree icons (TREE-11), highlights and
  problem markers follow the theme. The Qt XML Atlas has the same choice (QT-12). Today the
  window does not follow the system theme (seen by the owner on 2026-11-14).

## Acceptance tests

- Choose Text above grid, turn off View > Attributes in the tree, choose Tabs in Edit > Format
  options, quit, start again: all three are as chosen.
- Open a file with problems, choose View > Layout > Text above grid: text on top, the
  problems list under it, the grid at the bottom; the selection is unchanged. Choose Grid above
  text: back as before.
- With the system in light mode, start XML Atlas: it is light. Switch the setting to dark: the
  whole window turns dark at once, without reopening the file.

## Seen in the reference programs

- XML Marker: tree top left with the error list as a second tab, its Table Selection Browser
  bottom left, the text top right and its Tree Selection Browser (the grid) bottom right
  (XM-1). There is no theme setting.
- Qt XML Atlas: tree docked left, text in the middle, table docked below; a light and dark
  theme chosen in a dialog, by default following the system (QT-12).

## Decided

- Both layouts, switchable: today's and XML Marker's (WIN-1). (Owner, 2026-11-14.)
- Theme support: light, dark, and following the system (WIN-2). Planned for 0.2, with the
  other settings. (Owner, 2026-11-14.)

## Questions

1. Which layout should be the default once settings are remembered?
