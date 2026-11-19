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

- **REF-1** `[done]` A rules file, `xml-atlas-references.json`, next to the document or in a
  folder above it (the nearest wins), says what is a reference. Without one, nothing in this
  file applies. It is read when the document opens; open it again after changing the rules.
  A rules file that cannot be read is named in the preview pane with the reason. The rules
  stay outside XML Atlas, so a team keeps its conventions in its own repository.

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
    "variables": {"$PARENT": "parent_name"}
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
- **REF-5** `[built]` View > Preview with references resolved: a read-only pane beside the
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
  PATH resolved, up to 16 MiB, with the sections chosen as in REF-9.
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

## Acceptance tests

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

## Questions

None open.
