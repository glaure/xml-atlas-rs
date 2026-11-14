# Format (re-indent)

Format makes the nesting visible without changing what the document means.

## Rules

- **FMT-1** `[done]` Format changes only whitespace between tags, and only inside an element
  whose children are all elements, comments or processing instructions. Each such child goes
  on its own line, indented as FMT-10 says.
- **FMT-2** `[done]` An element that contains text or CDATA is copied exactly, including the
  spacing of mixed content such as `<p>Hello <b>big</b> world</p>`.
- **FMT-3** `[done]` An element with `xml:space="preserve"`, and everything inside it, is
  copied exactly.
- **FMT-4** `[done]` Start tags, comments, processing instructions, the XML declaration and
  the DOCTYPE are copied exactly, except where FMT-11 breaks a long start tag over lines.
  Entity references stay as written.
- **FMT-5** `[done]` The byte order mark and the line-ending style (LF or CRLF) are kept.
- **FMT-6** `[done]` Formatting a formatted document changes nothing.
- **FMT-7** `[done]` A document that is not well-formed is not formatted; the status bar says
  why.
- **FMT-8** `[gap]` After Format, the selection and the expanded tree rows are as before.
  Today the selection returns to the root element.
- **FMT-9** `[planned]` Format can be undone. XML Marker cannot undo Indent (XM-57).
- **FMT-10** `[done]` Indent size and tabs or spaces are settings (Edit > Format options).
  The default is four spaces, as in XML Marker (XM-50). With tabs, each level is one tab. The
  status bar says which options Format used.
- **FMT-11** `[done]` A setting "maximum attributes in a tag line", default 6, as in XML
  Marker; 0 never wraps. A start tag with more attributes than that gets each attribute on its
  own line, indented one level deeper than the tag, and its `>` or `/>` right after the last
  attribute. Only the whitespace between attributes changes; names, values, quotes and the
  spacing around `=` stay as written (FMT-4). This applies to empty elements and to elements
  with text too (their content stays exact), but not inside `xml:space="preserve"` (FMT-3).
- **FMT-12** `[done]` A setting "remove comments", off by default (Edit > Format options,
  `--remove-comments`). It removes comments where Format controls the layout: outside the root
  element and between elements. It keeps the settings comment (FMT-15), so the file still says
  how to format it, and comments inside mixed content or `xml:space="preserve"`, which are
  copied exactly (FMT-2, FMT-3). Removing comments is not a layout change.
- **FMT-13** `[done]` A setting "trim text", off by default (`--trim-text`). In an element that
  holds only text, it removes whitespace at the start and end of that text:
  `<t>  padded  </t>` becomes `<t>padded</t>`, an element of only whitespace becomes empty.
  Mixed content (`Hello <b>big</b> world`), CDATA and `xml:space="preserve"` are never
  trimmed: that would join words or change literal text. Entities stay as written.
- **FMT-14** `[done]` Compact (Edit > Compact, `xml-atlas-cli compact`): the opposite of Format.
  It leaves out the whitespace Format would add or change, so the document is one line,
  ending with one line break in the file's style. A start tag that spans lines is joined with
  one space between attributes. Everything FMT-2 to FMT-5 protect is kept. Remove comments
  and trim text apply. Formatting a compacted document gives what formatting the original
  gives.
- **FMT-15** `[done]` A document can carry its own settings in a comment before the root
  element. The comment starts with `Indentation Settings`; each following line has the form
  `XML Marker: <setting> = <value>`:

  ```xml
  <!--Indentation Settings
  XML Marker: Indent Size = 4
  XML Marker: Maximum attributes in tag line = 6
  XML Marker: Tab size = 4 as spaces
  -->
  ```

  When there is such a comment, Format uses its values for that document instead of the
  saved settings. A setting the comment does not name comes from the saved settings, and a
  line that cannot be read is ignored. Without the comment, Format uses the saved settings.
  Format leaves the comment as it is. `Tab size = N as tabs` indents with tabs, `as spaces`
  with spaces; the tab width itself does not change the output. The status bar and the Format
  options dialog say when the comment overrides the settings. The saved settings are those of
  the Format options dialog, remembered between sessions (WIN-3). The CLI does the same: `xml-atlas-cli format FILE [--indent N] [--tabs]
  [--max-attributes N] [--ignore-document-settings]`.

## Acceptance tests

- `<name>Customer 5 &amp; Sons</name>` comes out unchanged.
- The 100 MiB fixture: the output differs from the input only in whitespace between tags.
- A CRLF file with a byte order mark keeps both.
- A file with the settings comment of FMT-15 (indent 4, 6 attributes) and a tag with 7
  attributes: Format puts each of the 7 on its own line, 8 spaces in; a tag with 6 stays as it
  is; the comment is unchanged; formatting again changes nothing.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker's Indent has four options: indent size (4), maximum number of attributes in a
  tag line (6), remove comments, and whether to show the options each time (XM-50). The Qt
  XML Atlas has the first two; the other two are in its dialog but not connected (QT-10).
  FMT-10 to FMT-12 take all of them.
- "Maximum number of attributes": a tag with more attributes than this gets one attribute per
  line (XM-53, QT-10).
- Both programs rewrite more than whitespace between tags. XML Marker trims text, splits mixed
  content over lines, rewrites every attribute as `name = "value"`, ignores
  `xml:space="preserve"` and alters comments (XM-52 to XM-55). The Qt XML Atlas does much the
  same and also turns `&amp;` into a bare `&`, which breaks the document (QT-10). FMT-1 to
  FMT-5 differ on purpose (`docs/decisions.md`, D12): Format must not change what the document
  means unless a setting asks for it (FMT-12, FMT-13).
- XML Marker collapses the tree after Indent (XM-56) and cannot undo it (XM-57). FMT-8 and
  FMT-9 ask for better.

## Decided

- Indent size, tabs or spaces, maximum attributes per tag line, remove comments and trim text
  are settings (FMT-10 to FMT-13). The defaults match XML Marker's, so the owner's files come
  out as they do today. Removing comments and trimming text are off by default.
- These three settings change more than layout whitespace. They are recorded as opt-in
  exceptions in `docs/decisions.md`, D12.
- A compact command (FMT-14).
- Remove comments keeps the settings comment, and neither option touches mixed content,
  CDATA or `xml:space="preserve"` (FMT-12, FMT-13). (Claude's reading of the owner's answers,
  2026-11-14; the owner can change it.)
- A document's own settings comment wins over the saved settings, for that document only
  (FMT-15). The owner's files use this comment.

## Questions

None open.
