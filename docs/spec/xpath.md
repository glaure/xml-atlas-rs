# XPath

Selecting nodes with an XPath 1.0 expression: every `order` whose `status` is `open`, how many
`item`s there are, the `price` of the order with id 42. XML Atlas evaluates the expression on
its index of the file, so nothing is built when a file opens and a query on a large file takes
a fraction of a second (`docs/decisions.md`, D35).

## Rules

- **XP-1** `[built]` *Edit > XPath…* (Ctrl+Shift+X) opens a query bar under the find bar.
  The expression is checked as it is typed: a valid one says *valid*; a wrong one frames
  the field in the error colour, underlines the character where it goes wrong, says what
  is expected below the bar ("expected `]`, found the end at 18"), and is not run. A prefix
  the root element does not declare is wrong too.
  Enter or *Run* runs the expression; *▾* lists the last 20 expressions run in this session;
  Escape or *✕* closes the bar and its results. A query runs off the window's thread: the bar
  shows a spinner and the time, and *Stop* ends it. A new query stops the one before.
- **XP-2** `[done]` The expression is XPath 1.0 (W3C, 1999): location paths with all 13 axes
  and their abbreviations (`//`, `.`, `..`, `@`), node tests, predicates, filter
  expressions, `|`, the operators, and the 27 functions of the core library. The context is
  the root node. Where it differs, on purpose:
  - Text that is only whitespace is not a node, as XML Atlas does not keep it. Between
    elements it is formatting; `count(/r/node())` counts elements, comments and text with
    something in it.
  - A name without a prefix matches elements without a namespace and also those in the
    default namespace the root element declares, so `//order` works in a file whose root
    says `xmlns="urn:example"` (as XPath 2.0's default element namespace does). Attributes
    without a prefix have no namespace, as in XPath 1.0.
  - A prefix means what the root element declares it as; `xml` is always declared. A prefix
    the root element does not declare is an error that names it.
  - `id()` finds elements by `xml:id`. Without a DTD nothing else is known to be an ID.
  - Variables (`$x`) are not supported; there is nothing to bind them to.
- **XP-3** `[built]` Nodes are listed in a panel at the bottom, as the problems are: one row
  per node with its line, its path (`/orders/order[3]/@id`) and its value on one line. The
  first is selected when the query ends; a click selects another, and F3 and Shift+F3 step
  through them while the bar is open, going on from the other end. Selecting a node selects
  its element in tree and grid and scrolls the text to it; an attribute, text, a comment or
  a processing instruction is marked in the text as a grid cell is (TEXT-11). The bar says
  how many there are and which is selected ("3 of 120 nodes"). Only the rows on screen are
  laid out, so a million results scroll as ten do.
- **XP-4** `[built]` A number, string or boolean is shown in the bar, with its kind:
  `count(//item)` → `634183`, a number. An expression that cannot be read says what is
  expected and where ("expected `]`, found the end at 18"); one that cannot be evaluated
  says why ("a path needs a node-set, not a number").
- **XP-5** `[done]` After the document changes, the results are out of date: the bar says so,
  the panel shows no rows, and Enter runs the query again. Each tab keeps its own query and
  results.
- **XP-6** `[done]` `xml-atlas-cli xpath FILE EXPR` prints each node's line, path and value,
  at most `--limit` of them (20), then how many there are and the time; or the value and its
  kind. A wrong expression fails with the message.
- **XP-7** `[done]` Agents have the MCP tool `xpath`, on files (`xml-atlas-cli mcp`) and on
  the open tabs (`--window`, D28): a node-set comes as its total and a page of nodes (100 by
  default), each with kind, path, line, column, name and value (cut at 200 characters); a
  number, string or boolean as its type and value. A query that runs longer than 5 seconds
  is stopped and the agent is told to narrow it down, so a query in the window never holds
  it up for long.

## How to check it

- `//order[@id='20000']`, `count(//item)`, `//order[customer/name[contains(., 'Sons')]]`,
  `//*` and `sum(//price)` on `fixtures/orders.xml` (100 MiB) and `orders-300.xml`: each
  ends in under half a second at 300 MiB (Windows 11, release build, 2026-11-20: 284, 83,
  409, 98 and 170 ms). The counts agree with counting the tags in the file.
- Open a file with a byte order mark, CRLF line endings, non-ASCII names and CDATA:
  `//名前/text()` marks the text and the CDATA after it as one node.
- Run `//*` on 300 MiB and type in the text at once: the window keeps responding; the results
  say they are out of date.
- `//a[` says "expected an expression, found the end at 5".

## Seen in the reference programs

Not looked at yet: whether XML Marker or the Qt XML Atlas query with XPath.

## Decided

- Our own XPath 1.0 on the index, not a crate with a tree of its own; a query bar and a
  results panel; names without a prefix also match the root's default namespace; first the
  engine, the CLI and the window, then the MCP tool (XP-7). (Owner, 2026-11-19, D35.)

## Questions

- Should the last expressions be remembered between sessions, as the recent files are?
