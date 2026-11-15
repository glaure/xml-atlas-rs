# Behaviour spec (clean room)

What XML Atlas should do, in our own words. It is written from using XML Marker, from the Qt
XML Atlas and from the XML specification. It never includes copied text, screenshots or assets
from XML Marker.

## How to read it

One file per area. Every rule has an id and a status:

- `[done]`: built, and confirmed by an automated test or a measurement.
- `[built]`: built, but known only from reading the code. Nobody has confirmed it on screen.
  Check it in the running program, then change it to `[done]` or `[gap]`.
- `[gap]`: the code does something else or something poorer. The rule says what is right.
- `[planned]`: decided, not built.

Each area file ends with three lists:

- **Seen in the reference programs**: what XML Marker and the Qt XML Atlas do, with ids that
  point into `reference.md`, and how our rules compare.
- **Decided**: what the owner chose where XML Atlas could follow the reference or differ.
  Each decision is also a rule above, or a note that we differ and why. Open choices wait in
  **Decisions needed** until the owner has answered.
- **Questions**: things nobody has looked at yet, or that only the owner knows.

## How to change it

- Describe what the user sees and does, not how the code works.
- A rule about XML Marker's behaviour goes in only after someone has seen it. Do not guess.
- Where XML Atlas should differ on purpose, say so in the rule.
- When a rule's status changes, change it in the same pull request as the code.

## Files

| File           | Area                                                        |
|----------------|-------------------------------------------------------------|
| `open.md`      | Opening files, encodings, malformed input, very large files |
| `tree.md`      | Tree contents, expansion, selection, navigation             |
| `grid.md`      | Which rows and columns appear, cells, sorting, editing      |
| `text.md`      | Source view, its link to tree and grid, editing             |
| `format.md`    | Indentation rules, what whitespace is kept                  |
| `edit.md`      | Undo and redo, unsaved changes, structured edits            |
| `install.md`   | Installers and packages for Windows, macOS and Linux        |
| `window.md`    | Layout of the views, light and dark theme                   |
| `reference.md` | What XML Marker and the Qt XML Atlas do, as observed        |

Not written yet: find and replace, the edits themselves, settings, keyboard shortcuts.
