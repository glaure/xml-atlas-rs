//! Small real-world files: encodings, byte order marks, line endings, namespaces, DOCTYPE,
//! mixed content, malformed input, and generated extremes (one huge line, deep nesting).
//!
//! The files live in `tests/corpus/`. `.gitattributes` keeps git from changing their line
//! endings. A test for a rule that is not built yet is `#[ignore]`d with the rule's id from
//! `docs/spec/`; run them with `cargo test -- --ignored`.

use std::path::PathBuf;

use xml_atlas_engine::{Document, Edit, FormatOptions, SaveError, Unencodable};

use xml_atlas_engine::xml::{NodeId, NodeKind};

/// Two spaces per level, no attribute wrapping, nothing removed or trimmed: Format then
/// changes only whitespace between tags, which is what the corpus tests check.
const TWO_SPACES: FormatOptions = FormatOptions {
    indent_size: 2,
    tabs: false,
    max_attributes: 0,
    remove_comments: false,
    trim_text: false,
};

/// Well-formed UTF-8 files. Every one of them must open without error and format cleanly.
const WELL_FORMED_UTF8: &[&str] = &[
    "utf8_bom_crlf.xml",
    "utf8_no_declaration.xml",
    "tabs_and_mixed_content.xml",
    "namespaces.xml",
    "doctype.xml",
    "one_line.xml",
];

const BOM: &[u8] = b"\xEF\xBB\xBF";

// ---------------------------------------------------------------------------------------------
// Every well-formed file

#[test]
fn well_formed_files_open_without_error() {
    for name in WELL_FORMED_UTF8 {
        let doc = open(name);
        assert!(doc.index.error.is_none(), "{name}: {:?}", doc.index.error);
        assert!(
            doc.index.root_element().is_some(),
            "{name}: no root element"
        );
    }
}

/// FMT-1 to FMT-6 on every file: same elements, same bytes apart from whitespace, byte order
/// mark and line endings kept, and formatting twice changes nothing.
#[test]
fn format_changes_only_whitespace_on_every_file() {
    for name in WELL_FORMED_UTF8 {
        let doc = open(name);
        let out = doc
            .format(&TWO_SPACES)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = Document::from_bytes(out.clone());

        assert!(again.index.error.is_none(), "{name}: output does not parse");
        assert_eq!(
            element_names(&doc),
            element_names(&again),
            "{name}: elements"
        );
        assert_eq!(
            without_whitespace(&doc.src),
            without_whitespace(&out),
            "{name}: more than whitespace changed"
        );
        assert_eq!(
            doc.src.starts_with(BOM),
            out.starts_with(BOM),
            "{name}: BOM"
        );
        if doc.src.windows(2).any(|w| w == b"\r\n") {
            assert!(only_crlf(&out), "{name}: LF line ending in a CRLF file");
        }
        assert_eq!(
            again.format(&TWO_SPACES).unwrap(),
            out,
            "{name}: not stable"
        );
    }
}

/// FMT-14 on every file: Compact keeps everything but layout whitespace, and formatting the
/// compacted file gives what formatting the original gives.
#[test]
fn compact_round_trips_on_every_file() {
    for name in WELL_FORMED_UTF8 {
        let doc = open(name);
        let small = doc
            .compact(&TWO_SPACES)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = Document::from_bytes(small.clone());
        assert!(again.index.error.is_none(), "{name}: output does not parse");
        assert_eq!(
            element_names(&doc),
            element_names(&again),
            "{name}: elements"
        );
        assert_eq!(
            without_whitespace(&doc.src),
            without_whitespace(&small),
            "{name}: more than whitespace changed"
        );
        assert_eq!(
            again.format(&TWO_SPACES).unwrap(),
            doc.format(&TWO_SPACES).unwrap(),
            "{name}: format(compact) differs from format"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// One file each

#[test]
fn utf8_with_bom_and_crlf() {
    let doc = open("utf8_bom_crlf.xml");
    assert_eq!(name(&doc, root(&doc)), "orders");
    assert_eq!(doc.line(2), b"<orders>");
    assert_eq!(
        doc.line(1),
        "<!-- Grüße aus Graz – 日本語のコメント -->".as_bytes()
    );
    let order = find(&doc, "order");
    assert!(attrs(&doc, order).contains(&("city".into(), "Ä".into())));
    assert_eq!(text(&doc, find(&doc, "customer")), "Günther & Söhne");
    assert_eq!(text(&doc, find(&doc, "note")), "naïve café");
}

#[test]
fn utf8_without_declaration_and_character_references() {
    let doc = open("utf8_no_declaration.xml");
    let titles: Vec<String> = all(&doc, "title").map(|t| text(&doc, t)).collect();
    assert_eq!(titles, ["日本語の本", "Ελληνικά", "📚 📖 ü"]);
}

#[test]
fn tabs_mixed_content_cdata_and_quoting() {
    let doc = open("tabs_and_mixed_content.xml");

    let p = find(&doc, "p");
    let kinds: Vec<NodeKind> = doc.index.children(p).map(|c| kind(&doc, c)).collect();
    assert_eq!(kinds, [NodeKind::Text, NodeKind::Element, NodeKind::Text]);

    assert_eq!(text(&doc, find(&doc, "code")), "  keep   this  ");
    assert_eq!(
        text(&doc, find(&doc, "script")),
        "if (a < b && c > d) { x = \"]]>\"; }"
    );
    assert!(
        doc.index
            .children(root(&doc))
            .any(|c| kind(&doc, c) == NodeKind::ProcessingInstruction)
    );
    assert_eq!(
        attrs(&doc, find(&doc, "attrs")),
        [
            ("single".into(), "it\"s".into()),
            ("gt".into(), "a > b".into()),
            ("quot".into(), "\"q\"".into()),
            ("empty".into(), String::new()),
        ]
    );
}

#[test]
fn namespace_prefixes_are_part_of_the_name() {
    let doc = open("namespaces.xml");
    assert_eq!(name(&doc, root(&doc)), "cat:catalog");
    let item = find(&doc, "cat:item");
    assert!(attrs(&doc, item).contains(&("xlink:href".into(), "#a".into())));
    assert_eq!(text(&doc, item), "Ä");
    assert_eq!(text(&doc, find(&doc, "item")), "plain");
}

#[test]
fn doctype_with_internal_subset() {
    let doc = open("doctype.xml");
    assert!(
        doc.index
            .children(0)
            .any(|c| kind(&doc, c) == NodeKind::DocType)
    );
    assert_eq!(name(&doc, root(&doc)), "note");
    assert_eq!(text(&doc, find(&doc, "to")), "Tove");
}

#[test]
fn whole_document_on_one_line() {
    let doc = open("one_line.xml");
    assert_eq!(doc.line_count(), 1);
    assert_eq!(element_names(&doc), ["r", "a", "b", "c", "c", "d"]);
    assert_eq!(text(&doc, find(&doc, "d")), "ü");
}

/// OPEN-9: everything before the first error is there, and the error has a position: the
/// start tag of the element that is not closed.
#[test]
fn malformed_file_keeps_what_came_before_the_error() {
    let doc = open("malformed_unclosed.xml");
    let error = doc.index.error.as_ref().expect("an error");
    assert_eq!(doc.line_of(error.offset), 4, "{error}");
    assert!(error.reason.to_string().contains("<order>"), "{error}");
    assert_eq!(
        &element_names(&doc)[..4],
        ["shop", "owner", "orders", "order"]
    );
    assert_eq!(text(&doc, find(&doc, "owner")), "Müller");
}

/// OPEN-10: `</orders>` closes the `order` left open in it, and what follows is read as if
/// nothing had happened.
#[test]
fn malformed_file_recovers_after_the_error() {
    let doc = open("malformed_unclosed.xml");
    assert_eq!(
        element_names(&doc),
        ["shop", "owner", "orders", "order", "after"]
    );
    let after = find(&doc, "after");
    assert_eq!(doc.index.node(after).parent, root(&doc));
    assert_eq!(text(&doc, after), "not reached");
    let order = find(&doc, "order");
    let orders_end = String::from_utf8_lossy(&doc.src).find("</orders>").unwrap() as u64;
    assert_eq!(doc.index.node(order).end, orders_end);
    assert!(doc.index.more_errors.is_empty());
}

/// OPEN-13: the problems of each corpus file, by line.
#[test]
fn problems_of_the_corpus() {
    use xml_atlas_engine::problems::Severity::Error;
    let cases: &[(&str, &[(xml_atlas_engine::problems::Severity, usize)])] = &[
        ("utf8_bom_crlf.xml", &[]),
        ("namespaces.xml", &[]),
        ("malformed_unclosed.xml", &[(Error, 4)]),
        // Read in their own encoding (OPEN-8): nothing to report.
        ("latin1.xml", &[]),
        ("windows1252.xml", &[]),
        ("utf16le_bom.xml", &[]),
        ("utf16be_bom.xml", &[]),
    ];
    for (file, expected) in cases {
        let doc = open(file);
        let got: Vec<_> = doc
            .problems()
            .iter()
            .map(|p| (p.severity, doc.line_of(p.offset)))
            .collect();
        assert_eq!(&got, expected, "{file}");
    }
}

// ---------------------------------------------------------------------------------------------
// Other encodings (OPEN-8): converted to UTF-8 when opened.

#[test]
fn latin1_from_declaration() {
    assert_encoded_file("latin1.xml", "Größe über");
}

#[test]
fn windows1252_from_declaration() {
    let doc = open("windows1252.xml");
    assert_eq!(text(&doc, find(&doc, "n")), "5 € – „gut“");
}

#[test]
fn utf16_little_endian_from_bom() {
    assert_encoded_file("utf16le_bom.xml", "Größe 日本");
}

#[test]
fn utf16_big_endian_from_bom() {
    assert_encoded_file("utf16be_bom.xml", "Größe 日本");
}

/// OPEN-8: a file in another encoding is saved byte for byte as it was, and an edit is
/// saved in that encoding too.
#[test]
fn encoded_files_save_as_they_were() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-encodings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for file in [
        "latin1.xml",
        "windows1252.xml",
        "utf16le_bom.xml",
        "utf16be_bom.xml",
        "utf8_bom_crlf.xml",
    ] {
        let original = std::fs::read(corpus(file)).unwrap();
        let mut doc = open(file);
        let out = dir.join(file);
        doc.save(&out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), original, "{file} unchanged");
        // `ä` typed at the start of the second line is one character, in the file's own
        // encoding: one byte in Latin-1 and Windows-1252, two in UTF-16 and UTF-8.
        let at = doc.line_starts[1] as usize;
        doc.apply(
            Edit::new(xml_atlas_engine::StepName::Other("Type".into()))
                .replace(at..at, "ä".as_bytes().to_vec()),
        )
        .unwrap();
        doc.save(&out).unwrap();
        let again = Document::open(&out).unwrap();
        assert_eq!(again.encoding(), doc.encoding(), "{file}");
        assert_eq!(again.src, doc.src, "{file}");
        let grew = if file.starts_with("latin1") || file.starts_with("windows") {
            1
        } else {
            2
        };
        assert_eq!(
            std::fs::read(&out).unwrap().len(),
            original.len() + grew,
            "{file}"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// OPEN-8: a character Latin-1 cannot hold is refused where it is, nothing is written, and
/// Save as UTF-8 writes the file in UTF-8 with its declaration saying so.
#[test]
fn a_character_latin1_cannot_hold_is_refused() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-refuse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("latin1.xml");
    std::fs::copy(corpus("latin1.xml"), &out).unwrap();
    let original = std::fs::read(&out).unwrap();
    let mut doc = Document::open(&out).unwrap();
    let at = find_bytes(&doc.src, "Größe".as_bytes());
    doc.apply(
        Edit::new(xml_atlas_engine::StepName::Other("Type".into()))
            .replace(at..at, "日本 ".as_bytes().to_vec()),
    )
    .unwrap();
    match doc.save(&out) {
        Err(SaveError::Encoding(Unencodable::Char { offset, ch, .. })) => {
            assert_eq!((offset, ch), (at, '日'));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read(&out).unwrap(), original, "nothing written");
    assert!(doc.convert_to_utf8());
    doc.save(&out).unwrap();
    let saved = std::fs::read(&out).unwrap();
    let text = std::str::from_utf8(&saved).expect("UTF-8 now");
    assert!(
        text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "{text}"
    );
    assert!(text.contains("日本 Größe"));
    assert_eq!(Document::open(&out).unwrap().encoding(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Generated extremes

#[test]
fn one_huge_line() {
    let rows = 200_000;
    let mut src = String::from("<rows>");
    for i in 0..rows {
        src.push_str(&format!("<row id=\"{i}\">wert {i} ä</row>"));
    }
    src.push_str("</rows>");
    let doc = Document::from_bytes(src.into_bytes());
    assert!(doc.index.error.is_none());
    assert_eq!(doc.line_count(), 1);
    assert_eq!(doc.index.element_count, rows + 1);

    let out = doc.format(&TWO_SPACES).unwrap();
    assert_eq!(Document::from_bytes(out).line_count(), rows as usize + 3);
}

#[test]
fn deep_nesting() {
    let depth = 20_000;
    let src = format!("{}<leaf/>{}", "<a>".repeat(depth), "</a>".repeat(depth));
    let doc = Document::from_bytes(src.into_bytes());
    assert!(doc.index.error.is_none());
    assert_eq!(doc.index.max_depth as usize, depth + 1);
}

// ---------------------------------------------------------------------------------------------
// Helpers

fn corpus(name: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "tests", "corpus", name]
        .iter()
        .collect()
}

fn open(name: &str) -> Document {
    let path = corpus(name);
    Document::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn find_bytes(src: &[u8], what: &[u8]) -> usize {
    src.windows(what.len())
        .position(|w| w == what)
        .expect("found")
}

fn assert_encoded_file(file: &str, expected_text: &str) {
    let doc = open(file);
    assert!(doc.index.error.is_none(), "{file}: {:?}", doc.index.error);
    assert_eq!(name(&doc, root(&doc)), "r");
    let n = find(&doc, "n");
    assert_eq!(text(&doc, n), expected_text);
    assert!(attrs(&doc, n).contains(&("a".into(), "Äpfel".into())));
}

fn root(doc: &Document) -> NodeId {
    doc.index.root_element().expect("a root element")
}

fn kind(doc: &Document, id: NodeId) -> NodeKind {
    doc.index.node(id).kind
}

fn name(doc: &Document, id: NodeId) -> String {
    String::from_utf8_lossy(doc.index.name(&doc.src, id)).into_owned()
}

fn attrs(doc: &Document, id: NodeId) -> Vec<(String, String)> {
    doc.index.attributes(&doc.src, id)
}

/// Elements in document order.
fn elements(doc: &Document) -> impl Iterator<Item = NodeId> + '_ {
    (0..doc.index.len() as NodeId).filter(|&id| kind(doc, id) == NodeKind::Element)
}

fn element_names(doc: &Document) -> Vec<String> {
    elements(doc).map(|id| name(doc, id)).collect()
}

fn all<'d>(doc: &'d Document, element: &'d str) -> impl Iterator<Item = NodeId> + 'd {
    elements(doc).filter(move |&id| name(doc, id) == element)
}

fn find(doc: &Document, element: &str) -> NodeId {
    all(doc, element)
        .next()
        .unwrap_or_else(|| panic!("no element {element}"))
}

/// The text and CDATA of an element, folded or as children, decoded and joined.
fn text(doc: &Document, id: NodeId) -> String {
    doc.index.element_text(&doc.src, id).into_owned()
}

fn without_whitespace(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect()
}

fn only_crlf(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| b != b'\n' || (i > 0 && bytes[i - 1] == b'\r'))
}
