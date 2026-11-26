# Language

The language of the window's texts: English and German now, French and Spanish later
(`docs/decisions.md`, D45; issue #183). The order of work is built: German in the window's
own texts, in what the engine says and in the guide. A rule that is not built yet stays
`[planned]`.

## Rules

### What is translated

- **LANG-1** `[built]` The window's own texts are shown in the chosen language: menus and
  submenus, context menus, the toolbar's tooltips, hints on hover and the reasons greyed
  entries give (WIN-14), dialogs, Settings, the bars (Find, Go to line, XPath), headers of
  panes and columns that are not names from the document, notices, the status bar, and
  error messages. Also the texts in an SVG file the diagram exports.
- **LANG-2** `[built]` What the engine says is translated too: the problems' messages,
  parse errors included, why a reference finds nothing, XPath's messages and results'
  words, and the names of undo steps as *Undo* and *Redo* show them, also for a change an
  agent made.
- **LANG-3** `[planned]` Never translated:
  - anything from a document: names, values, text, file names and paths;
  - what the CLI prints, what the MCP tools answer, the logs, and the keys and values in
    `settings.toml`, in rules files and in a document's Format settings comment (FMT-15);
  - names: XML Atlas, XML, XPath, UTF-8 and other encodings, JSON, MCP, XPath expressions
    and their functions, and the names of fonts and licenses;
  - the example expressions and rules a field offers to take or insert;
  - the comments the reference preview adds to its text, such as `<!-- XML Atlas: finds
    nothing -->` (REF-5): they are part of that text, which can be copied and which the
    CLI's `preview` prints too. The same reason in the problems list is translated (LANG-2).
- **LANG-4** `[built]` Texts that come from outside the program are shown as they come,
  after a translated lead that says what failed: messages of the operating system, of the
  regular expression library and of the JSON library, and of the XML parser library where XML Atlas does not
  know the kind of error. The parser errors it knows are translated (LANG-2). The file
  dialogs are the system's and follow its language, not this setting.
- **LANG-5** `[planned]` The language never changes a document: the bytes that Format, an
  edit or Save write are the same in every language (AGENTS.md, rule 7).

### Choosing the language

- **LANG-6** `[built]` Settings offers *System*, *English* and *Deutsch*, in a section of
  its own, *Localization* (German *Lokalisierung*), listed after *Appearance*, with no
  menu entry. The row's label reads *Language / Sprache* in every language, so that it is found by someone who cannot read the window.
  Each language is named in itself, whatever the window shows. The default is *English*. The choice is
  remembered with the other settings (WIN-3) as `system`, `english` or `german`; a value
  that cannot be read counts as the default. *Reset to defaults* of its section puts it
  back to *English* (WIN-11).
- **LANG-7** `[done]` *System* shows German when the system's language is German, of any
  country (`de-DE`, `de-AT`, `de-CH`), and English for every other language and when the
  system's language cannot be read. It is read at the start; a change of the system's
  language counts from the next start.
- **LANG-8** `[built]` A change applies at once, without a restart, to every tab (WIN-11).
  Documents, their selection, undo history and unsaved changes, open bars and windows with
  their positions, and the panes' sizes stay as they are. A message already in the status
  bar may stay in the old language until the next one replaces it. Names of undo steps and
  the problems list follow at once.

### Keys

- **LANG-9** `[done]` In German, keys are shown by their German names wherever keys are
  shown (WIN-13): *Strg* for Ctrl, *Umschalt* for Shift, *Entf* for Delete; *Alt* and the
  letters and function keys stay. The other keys we show are *Eingabe* (Enter), *Esc*,
  *Tab*, *Rücktaste* (Backspace), *Bild auf* and *Bild ab* (Page Up and Down; the arrows
  they are often written with are not in the program's fonts, LANG-15), *Pos1* (Home),
  *Ende* (End), and *Pfeil hoch*, *Pfeil runter*, *Pfeil links* and *Pfeil rechts* for the
  arrows. On macOS, Cmd and the system's symbols stay as in English. Which keys act does
  not change with the language.

### Texts and their files

- **LANG-10** `[done]` Each language has one file, `apps/xml-atlas/lang/<code>.toml`:
  `en.toml` and `de.toml`. A text has a name in lower case with underscores, which begins
  with the part of the window it belongs to (`menu_file_save`, `find_not_found`). A
  placeholder is written `{name}`, in lower case; a brace that is meant as such is written
  twice. A placeholder takes the text it is given: a number is made into text before, as
  LANG-17 says, the same way for every use.
- **LANG-11** `[done]` A text whose wording depends on a number gives its forms, `one`
  and `other`, and takes the number as `{count}`. English and German use `one` for exactly
  1 and `other` for every other number, 0 included. French and Spanish get their own rule
  when they are added, and may give a form for whole millions.
- **LANG-12** `[done]` `en.toml` names every text and its placeholders. Where another
  file lacks a text, or a form the text needs, the English text is shown, whole: never a
  text's name, an empty place or a mix of two languages' forms. A text that is empty counts
  as lacking. The build succeeds and names what is lacking in a warning.
- **LANG-13** `[done]` The build fails, and says which file, which text and why, when a
  file cannot be read, has a text or a form that `en.toml` has not, or gives a text other
  placeholders than the English one. A placeholder may stand in another place in the
  sentence, and more than once. Every form of a plural may leave out `{count}`, as in *one
  file*.
- **LANG-14** `[built]` The texts are built into the program: no file is read or parsed at
  run time, and the installed program needs no language files. A text without placeholders
  costs no allocation when it is shown. Opening a file and the cost of a frame do not
  change with the language (AGENTS.md, rules 1 and 2).

### Layout

- **LANG-15** `[built]` In every language, at the window's usual sizes, nothing is cut off
  or overlaps that does not in English: menus, dialogs, Settings, the bars and the status
  bar. A longer text makes its place wider or wraps. The program's fonts have every
  character its texts use.

### The guide

- **LANG-16** `[built]` The built-in guide (WIN-4) is shown in the window's language:
  `help.md` in English, `help.de.md` in German, with the same sections in the same order.
  It names menus, entries and keys as the window shows them in that language. A language
  without a guide shows the English one.

### Wording

- **LANG-17** `[built]` Numbers the window writes in its own texts follow the language.
  In German they have a decimal comma (`12,3 MiB`), and a point where thousands are grouped
  in English with a comma (`1.000`, today in the diagram). No grouping is added where there
  is none. Line and column numbers, what Go to line takes, an XPath's number result and
  every value from a document are the same in every language.
- **LANG-18** `[planned]` German uses *Sie* where it addresses the user (*Wählen Sie zuerst
  ein Element aus*), and no address where that reads as well (*Zuerst ein Element wählen*).
  Never *du*. All texts and the guide use these words:

  | English | German |
  |---|---|
  | element, attribute | Element, Attribut |
  | tree, grid | Baum, Tabelle |
  | text view | Textansicht |
  | diagram, toolbar | Diagramm, Symbolleiste |
  | tab, pane | Tab, Bereich |
  | Format, Compact | Formatieren, Kompakt |
  | Find, Replace | Suchen, Ersetzen |
  | Drill Up, Drill Down | Ebene höher, Ebene tiefer |
  | bookmark | Lesezeichen |
  | reference, rules file | Verweis, Regeldatei |
  | well-formed | wohlgeformt |
  | undo, redo | Rückgängig, Wiederholen |

## Order of work

From D45; each step is a pull request or more.

1. These rules.
2. The language files and their build step, the setting, and menus, context menus, toolbar
   and Settings in German (LANG-6 to LANG-14, and LANG-1 for those parts). Built. In those
   parts, the engine's texts (the names of undo steps, why a rules file cannot be used)
   are English until step 4, and what the status bar says until step 3. Tests draw the
   menus and every section of Settings in German and find nothing cut off (LANG-15); the
   rest of the window was not looked at in that step. macOS shows its keys the same in German.
   The About card was translated ahead of step 3, with its new look (owner, 2026-11-26).
3. The window's other texts (the rest of LANG-1, LANG-4), in parts:
   1. The bars (Find, Go to line, XPath with its results) and the status bar, with the update
      and install notice. Built; their numbers follow LANG-17. Where the Find bar has no room
      for what a search says, as in German beside the tree, it goes on a line under the bar.
      Messages made of the engine's words stay English until step 4: why an expression, a line
      number or an edit is refused, undo step names, Format's summary of its settings, and why
      a reference finds nothing. Why GitHub's answer to the update check cannot be read is
      still English too.
   2. Dialogs: unsaved changes, saving in the file's encoding, *Format options*, *Font*, the
      rules editor with its own question, the guide's title, and the file types the program
      names in the system's file dialogs; also why a font cannot be used, which the first
      part left in English. Built. A test draws each dialog in German and finds nothing cut
      off. The engine's words in them stay English until step 4 (what the rules editor
      finds and counts, why a file cannot be read or saved), and the guide until step 5.
   3. Panes and what is left: the empty window, an untitled tab, the notices above the
      views, the grid with its attributes and the cell pane, the problems list, the reference
      preview and the rules check in its header, the diagram and its SVG export, and why the
      update check failed. Built. Tests draw these in German, but not the empty window;
      they check that the panes, the notices and the diagram's header cut nothing off. With
      this, what is left in English is the engine's (step 4) and the guide (step 5).
4. The engine's messages made into values the window can translate, a part at a time
   (LANG-2). The engine's own texts stay as they are. In parts:
   1. The names of undo steps and the settings *Format* names. Built. A test holds the
      window's English to the engine's.
   2. Refused edits and saves, Find and Go to line. Built: why a structured edit, an edit,
      *Format* or Go to line is refused. These were values already, so the engine did not
      change. What is wrong with text that is not well-formed is led in German, with its
      byte; the parser's message in it is the third part's. Saving what the file's encoding
      cannot hold and Find's own messages were German already. Where Go to line has one
      line, the window says *1 line*; the engine says *1 lines*. A test holds the window's
      English to the engine's otherwise.
   3. XML problems, the XML parser library's known errors among them (LANG-4), and the
      warnings about encodings. Built, in the problems list, the status bar, the text view's
      hint and the refusals of the second part. The library's known errors are its syntax
      errors, its errors for a document that is not well-formed, and bytes that are not
      UTF-8. A test holds the window's English to the engine's and reads broken files in
      German. The one kind of warning left in English is a reference's (fifth part).
   4. XPath. Built: why an expression cannot be read or run, as it is typed and when run.
      What a result is, and how many nodes, was German already; paths to nodes are XPath and
      stay (LANG-3). A test gets the errors from the engine and holds the window's English to
      its. Where the XPath bar has no room for why a query failed, it goes under the bar
      (LANG-15), as in the Find bar.
   5. References and rules files, in two parts:
      1. Why a reference stands for nothing, in the problems list and when going to its
         target. Built. The preview's comments stay English (LANG-3). A test gets every
         reason from the engine and holds the window's English to its. With this, no
         warning of the problems list is left in English.
      2. Rules files: why one cannot be used, read or saved, and what the check finds and
         counts. Built, in Settings, the rules editor, the reference preview and the check
         in its header. The names of a rules file's keys stay (LANG-3). A test gets the
         check's findings from the engine and holds the window's English to its. The JSON
         library's message has a German lead and no English one.
5. The German guide (LANG-16). Built. Tests hold it to the English guide's sections, list
   items, paragraphs and `code`, look for the German window's names of menus, entries and
   buttons in it, and draw it in a small window. No one has proofread its German yet.

## Acceptance tests

- A new installation on a German system starts in English. Choose *Deutsch* in Settings: the
  menus, the toolbar's tooltips and Settings are German at once, and the shortcuts read
  *Strg+S* and *Entf*. Quit and start again: still German.
- Choose *System* on a system set to Austrian German: German. On one set to French:
  English.
- Open a file with a byte order mark, CRLF line endings and non-ASCII text, change an
  element's name, switch the language: the tab is still modified, *Undo* names the step in
  the new language and takes it back. Format and save in German, then in English from the
  same file: the two results are the same bytes.
- In German, break a closing tag: the problem is listed in German with the element's name as
  in the file. `xml-atlas-cli stats` and an agent's `problems` say it in English.
- Remove one text from `de.toml` and build: a warning names it, and the window shows it in
  English among German ones. Add a text that `en.toml` has not, or rename a placeholder:
  the build fails and names the file and the text.
- With the window 1280 wide, open every menu and every section of Settings in German:
  nothing is cut off.

## Seen in the reference programs

Nothing. Whether XML Marker or the Qt XML Atlas offer other languages has not been looked
at.

## Decided

From D45 (owner, 2026-11-26): English and German, French and Spanish later (LANG-1);
*System*, *English* and *Deutsch*, English by default (LANG-6); the guide translated
(LANG-16); German key names, Cmd on macOS (LANG-9); CLI, MCP and logs in English (LANG-3);
the engine's messages translated in the window (LANG-2); a file per language, checked at
build, English where a text is lacking (LANG-10 to LANG-14); the order of work.

Added in writing these rules, for the owner to object to: the list of what is not
translated (LANG-3); *System* read at the start only (LANG-7); the German names of keys
beyond *Strg*, *Umschalt* and *Entf* (LANG-9); an empty text counts as lacking (LANG-12);
the guide's fallback (LANG-16).

Answered when these rules were written (owner, 2026-11-26), all as proposed: *Sie*, or no
address (LANG-18); German numbers with a decimal comma, and a point only where thousands are
grouped today (LANG-17); the setting as the first row of Settings > Appearance, labelled
*Language / Sprache* (LANG-6); the preview's comments stay English (LANG-3); the German
words for the main terms (LANG-18). The list of what was added in writing, above, was put
to the owner with these questions and not objected to.

Changed after seeing step 2 in the window (owner, 2026-11-26): the setting has a section of
its own, *Localization*, instead of the first row of Appearance (LANG-6).

Decided before step 4's third part (owner, 2026-11-26): the errors of the XML parser
library that XML Atlas knows are translated, instead of shown as they come (LANG-2,
LANG-4).

## Decisions needed

None.

## Questions

- Does egui show texts of its own anywhere in the window, in English only? None was seen
  in the German menus and Settings (their drawn texts, step 2); the rest of the window is
  to look at in step 3.
- Do the installers and the desktop entry get German texts too (`install.md`)? D45 speaks of
  the window only; nothing here changes them.
- Who reads the German through before a release?
