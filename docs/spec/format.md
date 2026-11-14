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
  the DOCTYPE are copied exactly. Entity references stay as written.
- **FMT-5** `[done]` The byte order mark and the line-ending style (LF or CRLF) are kept.
- **FMT-6** `[done]` Formatting a formatted document changes nothing.
- **FMT-7** `[done]` A document that is not well-formed is not formatted; the status bar says
  why.
- **FMT-8** `[gap]` After Format, the selection and the expanded tree rows are as before.
  Today the selection returns to the root element.
- **FMT-9** `[planned]` Format can be undone.

## Acceptance tests

- `<name>Customer 5 &amp; Sons</name>` comes out unchanged.
- The 100 MiB fixture: the output differs from the input only in whitespace between tags.
- A CRLF file with a byte order mark keeps both.

## Seen in the reference programs

Details and limits in `reference.md`.

- XML Marker's Indent has four options: indent size (4), maximum number of attributes in a
  tag line (6), remove comments, and whether to show the options each time (XM-50). The Qt
  XML Atlas has the first two; the other two are in its dialog but not connected (QT-10).
- "Maximum number of attributes": a tag with more attributes than this gets one attribute per
  line (XM-53, QT-10).
- Both programs rewrite more than whitespace between tags. XML Marker trims text, splits mixed
  content over lines, rewrites every attribute as `name = "value"`, ignores
  `xml:space="preserve"` and alters comments (XM-52 to XM-55). The Qt XML Atlas does much the
  same and also turns `&amp;` into a bare `&`, which breaks the document (QT-10). FMT-1 to
  FMT-5 differ on purpose (`docs/decisions.md`, D12): Format must not change what the document
  means.
- XML Marker collapses the tree after Indent (XM-56). FMT-8 asks for better.
- XML Atlas indents two spaces; both reference programs default to four.

## Decisions needed

1. Indent size: stay at two spaces, change the default to four, or make it a setting?
2. Tabs instead of spaces as an option? XML Marker has a separate tab setting (XM-47).
3. Wrapping attributes onto their own lines above a threshold. It changes only whitespace
   inside a start tag, so it does not change meaning, but FMT-4 today copies start tags
   exactly. Add it as an option that is off by default?
4. "Remove comments" as an option? It is not a layout change.

## Questions

1. Which of XML Marker's indent options do you actually use?
2. Should Format ever change text content, for example trim it? Today it never does.
3. Do you need the opposite: remove all layout whitespace to make the file small?
