# References

Some XML files point from one part to another with conventions of their own: an element that
stands for elements found elsewhere, or an attribute whose value is looked up somewhere else.
The program that reads the file resolves them; XML Atlas shows where they go and how the
result reads, without changing the file (`docs/decisions.md`, D32). The examples below use
made-up names.

```xml
<Doc>
  <Shared>
    <Parts Kind="a"><P1/><P2/></Parts>
  </Shared>
  <Unit>
    <General>
      <Var><Ch><Count>8</Count></Ch></Var>
      <Box N="[Var:Ch/Count]">
        <Include src="Shared:Parts[@Kind='a']/*"/>
      </Box>
    </General>
  </Unit>
</Doc>
```

Resolved, `Box` reads `<Box N="8">` with `<P1/>` and `<P2/>` in place of the `Include`.

## Rules

- **REF-1** `[done]` A rules file says what is a reference. It is one of the files in the
  configuration folder (REF-13); without one for the document, nothing in this file applies.
  A rules file that cannot be read is named in the preview pane with the reason. The rules
  stay outside XML Atlas, so a team's conventions stay its own.

  ```json
  {
    "includes": [{"element": "Include", "attribute": "src", "fallback": "default-src",
                  "skip_when": {"Later": "true"}}],
    "attribute_references": {"skip_names_with": "-Later"},
    "contexts": {
      "Shared": {"path": "/Doc/Shared"},
      "Var": {"nearest": "Var", "up_to": "Unit"}
    },
    "runtime_contexts": ["Live"],
    "variables": {"$PARENT": "parent_name"},
    "repeat": {"count": "Count", "start": "Start", "suffix": "Suffix", "relative": "Relative",
               "index": ["[Index]", "$Index"],
               "relative_index": ["[RelativeIndex]", "$RelativeIndex"]}
  }
  ```

  - `includes`: an element called `element` stands for what its `attribute` refers to; when
    that finds nothing, `fallback` is tried. `skip_when` leaves it as it is when an attribute
    has that value (case ignored).
  - `attribute_references`: an attribute value written `[Context:path]` (or `${Context:path}`,
    or only a variable) stands for the value found there: the text of the elements found, or
    the attribute value. Attributes whose name contains `skip_names_with` are left as they
    are.
  - `contexts`: a reference is `Context:path`, the context ending at the first colon. A
    context is either one element at an absolute `path`, or `nearest`: the nearest element of
    that name, a child of an ancestor of the reference, up to the ancestor called `up_to`,
    whose other descendants of that name are tried last. Without a context, a path starts at
    the reference itself (`..` is its parent).
  - `runtime_contexts`: contexts that exist only when the program reading the file runs;
    references to them are shown as such, not as problems.
  - `variables`: `$NAME` in a path, or as a whole attribute value; `parent_name` is the name
    of the reference's parent element. Any other `$NAME` is known only at run time.
  - `repeat`: repeated elements in the preview (REF-11). `count` is required; the others are
    optional.
  - `"enabled": false` in any rule object (an include, `attribute_references`, a context,
    `repeat`) leaves that rule out; for `repeat`, *Expand repeats* then starts off.
- **REF-2** `[done]` A path is a small part of XPath: child names, `*`, `.`, `..`, `//`, a
  leading `/`, and last `@name`, `@*` or `text()`; the conditions `[@a='v']`, `[@a]` and
  `[n]`. Anything else is a reference the preview cannot read, said as such.
- **REF-3** `[done]` Go to reference target (F12, or the tree's right-click menu of a
  reference): selects the first element the selected reference finds, so tree, grid and text
  follow; the status bar says where it went and how many it found. On an attribute row whose
  value is a reference it says the value. On anything that does not resolve it says why.
- **REF-4** `[done]` References that find nothing are warnings in the problems list (OPEN-13),
  at the reference. Skipped references, those known only at run time and those whose `nearest`
  context has nothing around them (they resolve where they are imported) are not. After
  typing, the check waits for a pause (TEXT-22); after any other edit it runs at once.
- **REF-5** `[built]` View > Preview with references resolved (under *Resolve references*,
  REF-12): a read-only pane beside the
  text shows the selected element as it reads with its references resolved. An import is
  replaced by the elements it finds, re-indented to where it stands, or by the value; an
  attribute reference by its value. Imports inside imported elements are resolved too, where
  they were imported, up to 10 deep; one that imports an element it is already inside is a
  loop and is left. What does not resolve stays and is followed by a comment starting
  `XML Atlas:` that says why; a `nearest` context says where it was taken from. Imported lines
  are tinted, lines with a note tinted as a warning. The pane is made again when the
  selection or the document changes, shows only the lines on screen, and stops after 4 MiB.
  *Copy* copies its text. The element's path and a section's label (REF-9) are cut to the
  pane's width and shown whole on hover, so a long one does not widen the pane over the text
  view; until 2026-11-19 it did, and the pane's border snapped back when dragged. The lines
  fill the pane's height; until then a tall pane left a band empty below them.
- **REF-6** `[done]` The CLI does the same: `xml-atlas-cli references FILE [--rules PATH]
  [--limit N]` counts the references and lists those that find nothing;
  `xml-atlas-cli preview FILE PATH [--rules PATH] [--section PATH]...` prints the element at
  PATH resolved, up to 16 MiB, with the sections chosen as in REF-9. Without `--rules` both
  take the file REF-13 gives for FILE, and say when there is none.
- **REF-7** `[done]` A preview, not the program's own resolution: contexts known only at run
  time stay, `nearest` stands in for whatever the program merges, and nothing is cached or
  resolved in a second pass. The file never changes.
- **REF-8** `[planned]` Agents (MCP) get the same check and preview.
- **REF-9** `[built]` Choosing the section: where a `nearest` context has more than one
  element to come from around the selection (several `Var` in one `Unit`), the preview pane
  shows a list per context: *the nearest that has it* (REF-1, the default) or one of them,
  each named by its parent's start tag (`Sub Version="2"`). With one chosen, a reference
  inside that `Unit` takes the context from it first, then from its own nearest one, and from
  no other: as the program does when it merges the general section with one specialization.
  The choice applies to the preview and to F12, not to the problems list (REF-4), which
  still asks whether any section has it. It is kept per tab, follows edits as the selection
  does, and goes when its section is deleted.
- **REF-10** `[built]` The preview as a tree: View > *Preview tree* (off by default,
  remembered, also in Settings > Views) shows a pane right of the preview, so the window
  reads main tree | text | preview text | preview tree. It is the preview's text indexed as
  a document of its own when the preview is made again, drawn with the main tree's rows,
  icons and keys and its settings (attribute rows, font), read-only. A preview cut at 4 MiB
  shows the elements before the cut. A click on a row scrolls the preview text to that
  element and marks it; it selects nothing in the main tree (`docs/decisions.md`, D33).
  Right-click > *Show in the main tree* expands the main tree to the element the row was
  copied from, scrolls there and marks it there and in the text, as a click in the grid
  marks one; the selection and the preview stay. An imported element is shown where it
  stands in the file. Greyed out for what the preview wrote itself (a note). The engine's
  preview records which runs of its text were copied from where (`Preview::source_of`).
  The tree is made when its pane first shows a preview, not before; with the pane off it
  costs nothing. The main tree and this one are drawn by the same code (`tree::draw`).
- **REF-11** `[built]` Repeated elements: with a `repeat` rule, the preview (REF-5, REF-10,
  the CLI's `preview`) shows an element with the rule's count attribute as that many copies,
  named its name and their number from the start attribute on; placeholders in attribute
  values and text become the copy's number or relative number, an element with the suffix
  attribute gets the number appended, and the rule's attributes are removed. Inner counts
  are expanded first. Done after the imports. On by default; the pane's *Expand repeats*
  and `preview --no-repeat` leave it out. *Show in the main tree* on a copy goes to the
  element it was copied from. Not in the problems list (REF-4). Any rule in the file with
  `"enabled": false` is left out (`docs/decisions.md`, D34). The engine writes the resolved
  preview again (`repeat::expand`), each copy on a line of its own at the element's
  indentation; copied runs keep their mapping to the file and their tints. Tried on a
  5.9 MiB file of the owner's: three elements of count 8 became copies 0 to 7, those of
  count 0 went, and no count attribute was left. The owner checked it in the window with
  their rules (2026-11-19).
- **REF-12** `[built]` View > *Resolve references*, also in Settings > References, switches
  references on and off for every tab; off by default and remembered (WIN-3). Off, no rules
  file is looked for, read or checked: no reference warnings (REF-4), no *Go to reference
  target* in the tree's menu, F12 says where to turn them on, and neither preview pane shows.
  The two preview switches stand indented under it, greyed out while it is off, and keep
  their choice. Turned on, each tab gets its rules (REF-13) and is checked at once. Turned off,
  the sections chosen in the preview (REF-9) are forgotten. Settings saved before the switch
  existed count as off, also with the preview on. An agent's tab follows the switch; the
  CLI's `references` and `preview` do not, as they are asked for by name.
- **REF-13** `[built]` The rules files are the JSON files in `references/` beside the settings
  file (WIN-3). Nothing is looked for beside or above a document (D41). Settings > References
  lists them, each with:
  - a checkbox: off, the file is not used. A new, imported, copied or hand-placed file is off;
  - *Use for*: patterns separated by `;`. `*` stands for any characters, folders included,
    `?` for one. A pattern without `/` is compared with the document's file name, one with
    `/` with the end of its resolved path (links and `..` resolved) from a folder's start;
    one starting with `/` or `*` with the whole path. Empty names no document, `*` every
    one, also one without a file yet. Case does not count on Windows and macOS. *+ shown*
    adds the name of the document shown. Typing is written after a pause or when the field
    is left;
  - its error when it cannot be read, and whether it is in use for the document shown;
  - Up and Down, Duplicate, Rename and Delete (asks first).

  Under the list are New, Import (a byte-for-byte copy; the original stays, and an existing
  file is never replaced) and Open folder. Of the files that are on and name a document, the
  upper one is used, alone, also when it cannot be read: then it says so and the next does
  not step in. The preview's first line names the file in use. A change in the folder is
  taken within about a second. The switches, patterns and order are in `references.json`
  beside the folder; when that cannot be read, no file is on and it is not written over.

## Acceptance tests

- Import a rules file in Settings > References: it is off. Switch it on and click *+ shown*:
  the preview names it and resolves. Switch it off: preview and warnings go, in every tab.
  A document beside which an `xml-atlas-references.json` lies gets no rules from it.
- With a fresh settings file open a document that has rules: no reference warnings, no
  preview, F12 says references are off. Turn on View > Resolve references: the warnings
  appear and F12 works; turn on the preview and its tree, then Resolve references off: both
  panes and the warnings go. On again: both panes are back.
- A `Unit` with `General/Var` and two `Sub` sections whose `Var` differ: choosing
  `Sub Version="2"` in the pane changes the preview's values to that section's and F12 goes
  there; deleting the first `Sub` keeps the choice on the same section.
- With the rules above next to the example: F12 on the `Include` selects `P1` and says it
  finds 2 elements; the problems list is empty; the preview of `Box` reads `<Box N="8">`
  with `<P1/>` and `<P2/>` indented where the `Include` was.
- Change `Kind='a'` to `Kind='c'` in the `Include` and pause: the problems list has one
  warning at it, "finds nothing"; the preview keeps the `Include` with a comment.
- In a file with a byte order mark and CRLF, the preview keeps CRLF and non-ASCII names.
- A 6 MiB file with 7,600 references (the owner's): all are checked in about 40 ms; the
  preview of an element with 950 references and 1 MiB of text in about 30 ms.

## Seen in the reference programs

Neither XML Marker nor the Qt XML Atlas knows about references.

## Decided

- Generic rules in a file the user keeps, nothing of one team's conventions in XML Atlas;
  go to target, the problems and the preview pane together; `nearest` as the stand-in for a
  merge. (Owner, 2026-11-18, D32.)
- Choosing the specialization to resolve against, in the pane (REF-9). (Owner, 2026-11-18.)
- The preview as a tree in a pane of its own right of the preview text; a click scrolls the
  preview text only, at first; the main tree's settings (REF-10). (Owner, 2026-11-19, D33.)
- Repeated elements expanded in the preview, on by default, from a `repeat` rule; not in the
  problems list; any rule switched off with `"enabled": false` (REF-11). (Owner, 2026-11-19,
  D34.)

- A switch for references as a whole, off by default (REF-12). (Owner, 2026-11-22.)
- Rules files in the configuration folder, each switched on and off and with the documents
  it is for; no search beside or above the document (REF-13). (Owner, 2026-11-22, D41.)

## Questions

None open.
