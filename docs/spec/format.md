# Format (re-indent)

Format makes the nesting visible without changing what the document means.

## Rules

- **FMT-1** `[done]` Format changes only whitespace between tags, and only inside an element
  whose children are all elements, comments or processing instructions. Each such child goes
  on its own line, indented two spaces per level.
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
- **FMT-10** `[planned]` Indent size and tabs or spaces are settings. The default is four
  spaces, as in XML Marker (XM-50). Today the indent is always two spaces.
- **FMT-11** `[planned]` A setting "maximum attributes in a tag line", default 6, as in XML
  Marker. A start tag with more attributes than that gets each attribute on its own line,
  indented one level deeper than the tag. Only the whitespace between attributes changes;
  names, values, quotes and the spacing around `=` stay as written (FMT-4).
- **FMT-12** `[planned]` A setting "remove comments", off by default. It removes comments,
  which is not a layout change.
- **FMT-13** `[planned]` A setting "trim text", off by default. It removes leading and trailing
  whitespace from text, which is not a layout change. Never inside `xml:space="preserve"`.
- **FMT-14** `[planned]` Compact: the opposite of Format. It removes the whitespace that Format
  would add or change, and keeps everything FMT-2 to FMT-5 protect.

## Acceptance tests

- `<name>Customer 5 &amp; Sons</name>` comes out unchanged.
- The 100 MiB fixture: the output differs from the input only in whitespace between tags.
- A CRLF file with a byte order mark keeps both.

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
  out as they do today. Removing comments and trimming text are off by default, so D12 holds
  unless the user asks.
- A compact command (FMT-14).

## Questions

1. The owner's files carry their indent settings in a comment near the top:

   ```xml
   <!--Indentation Settings
   XML Marker: Indent Size = 4
   XML Marker: Maximum attributes in tag line = 6
   XML Marker: Tab size = 4 as spaces
   -->
   ```

   Should Format read its settings from such a comment when there is one, so that every file
   is formatted the way it says? If so, is this layout of the comment fixed?
2. FMT-12 and FMT-13 change content. `docs/decisions.md`, D12, says Format changes only layout
   whitespace. Record them there as opt-in exceptions?
