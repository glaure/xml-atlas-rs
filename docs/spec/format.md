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

## Questions

1. The Qt XML Atlas has indent options: indent size, spaces or tabs, "maximum number of
   attributes in tag" and "remove comments". Which of these do you use?
2. What does "maximum number of attributes in tag" do: wrap attributes onto new lines?
3. Should Format ever change text content, for example trim it? Today it never does.
4. Do you need the opposite: remove all layout whitespace to make the file small?
