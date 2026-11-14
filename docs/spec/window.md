# Window

How the views are laid out, and how the window looks.

## Rules

- **WIN-1** `[built]` The tree is on the left. On the right, the grid is above the text; the
  find bar sits between them when open (TEXT-12), and the problems list under the text when
  there are problems (OPEN-13). The border between tree and the rest, and the one between grid
  and text, can be dragged. XML Marker has the text above its table (XM-1); see the decision
  below.
- **WIN-2** `[planned]` Light and dark theme. A setting chooses light, dark, or following the
  system, which is the default. Text colours (TEXT-2), tree icons (TREE-11), highlights and
  problem markers follow the theme. The Qt XML Atlas has the same choice (QT-12). Today the
  window does not follow the system theme (seen by the owner on 2026-11-14).

## Acceptance tests

- With the system in light mode, start XML Atlas: it is light. Switch the setting to dark: the
  whole window turns dark at once, without reopening the file.

## Seen in the reference programs

- XML Marker: tree top left with the error list as a second tab, its Table Selection Browser
  bottom left, the text top right and its Tree Selection Browser (the grid) bottom right
  (XM-1). There is no theme setting.
- Qt XML Atlas: tree docked left, text in the middle, table docked below; a light and dark
  theme chosen in a dialog, by default following the system (QT-12).

## Decided

- Theme support: light, dark, and following the system (WIN-2). Planned for 0.2, with the
  other settings. (Owner, 2026-11-14.)

## Decisions needed

1. Swap grid and text so that the text is on top, as in XML Marker? The owner noticed the
   difference on first use.
