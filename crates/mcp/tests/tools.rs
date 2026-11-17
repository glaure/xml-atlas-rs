//! The MCP tools on real files: a byte order mark and CRLF, Latin-1, a broken file, wide
//! parents and long lines. Saving must change only the bytes asked for (AGENTS.md, rule 7).

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use xml_atlas_mcp::{Files, MAX_TEXT, ToolError, call, tools};

/// A copy of a corpus file in a folder of its own, to change and save.
fn copy(name: &str, test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-{test}"));
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/corpus");
    std::fs::copy(corpus.join(name), &to).unwrap();
    to
}

fn write(name: &str, test: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-{test}"));
    std::fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    std::fs::write(&to, bytes).unwrap();
    to
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn run(docs: &mut Files, tool: &str, a: Value) -> Value {
    call(docs, tool, &args(a)).unwrap_or_else(|e| panic!("{tool}: {e}"))
}

#[test]
fn tools_are_described_whole() {
    let all = tools();
    let mut names: Vec<_> = all.iter().map(|t| t.name).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all.len(), "a name twice");
    for t in &all {
        assert_eq!(t.schema["type"], "object", "{}", t.name);
        for r in t.schema["required"].as_array().unwrap() {
            assert!(
                t.schema["properties"].get(r.as_str().unwrap()).is_some(),
                "{}: required {r} is not a property",
                t.name
            );
        }
    }
}

#[test]
fn read_and_edit_a_file_with_bom_and_crlf() {
    let path = copy("utf8_bom_crlf.xml", "bom");
    let before = std::fs::read(&path).unwrap();
    let mut docs = Files::default();
    let opened = run(&mut docs, "open", json!({"path": path}));
    assert_eq!(opened["root"], "orders");
    assert_eq!(opened["well_formed"], true);
    assert_eq!(opened["encoding"], "UTF-8");
    let doc = opened["document"].clone();

    let order = run(
        &mut docs,
        "element",
        json!({"document": doc, "path": "/orders/order"}),
    );
    assert_eq!(order["line"], 4);
    assert_eq!(
        order["attributes"][1],
        json!({"name": "city", "value": "Ä"})
    );
    assert_eq!(
        order["children"][0],
        json!({"name": "customer", "count": 1})
    );

    let rows = run(
        &mut docs,
        "children",
        json!({"document": doc, "path": "/orders"}),
    );
    assert_eq!(rows["total"], 1);
    assert_eq!(rows["rows"][0]["path"], "/orders/order");
    assert_eq!(rows["rows"][0]["cells"]["customer"], "Günther & Söhne");

    let found = run(
        &mut docs,
        "find",
        json!({"document": doc, "query": "Söhne"}),
    );
    assert_eq!(found["total"], 1);
    assert_eq!(found["matches"][0]["line"], 5);
    assert_eq!(found["matches"][0]["path"], "/orders/order/customer");

    run(
        &mut docs,
        "set_text",
        json!({"document": doc, "path": "/orders/order/customer", "text": "Ärger <&> Co"}),
    );
    run(&mut docs, "undo", json!({"document": doc}));
    run(&mut docs, "redo", json!({"document": doc}));
    run(&mut docs, "save", json!({"document": doc}));
    let after = std::fs::read(&path).unwrap();
    let old = "Günther &amp; Söhne".as_bytes();
    let new = "Ärger &lt;&amp;&gt; Co".as_bytes();
    let at = before.windows(old.len()).position(|w| w == old).unwrap();
    let expected = [&before[..at], new, &before[at + old.len()..]].concat();
    assert_eq!(after, expected, "only the customer's text changed");
    assert!(after.starts_with(b"\xef\xbb\xbf") && after.windows(2).any(|w| w == b"\r\n"));
}

#[test]
fn latin1_is_saved_as_latin1() {
    let path = copy("latin1.xml", "latin1");
    let mut docs = Files::default();
    let doc = run(&mut docs, "open", json!({"path": path}))["document"].clone();
    run(
        &mut docs,
        "set_attribute",
        json!({"document": doc, "path": "/r/n", "name": "a", "value": "Bäume"}),
    );
    run(&mut docs, "save", json!({"document": doc}));
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(6).any(|w| w == b"B\xe4ume\""), "{bytes:?}");
    assert!(bytes.windows(10).any(|w| w == b"Gr\xf6\xdfe \xfcber"));
}

#[test]
fn a_broken_file_is_read_but_not_edited() {
    let path = copy("malformed_unclosed.xml", "broken");
    let mut docs = Files::default();
    let opened = run(&mut docs, "open", json!({"path": path}));
    assert_eq!(opened["well_formed"], false);
    let doc = opened["document"].clone();
    let problems = run(&mut docs, "problems", json!({"document": doc}));
    assert!(problems["total"].as_u64().unwrap() >= 1);
    assert_eq!(problems["problems"][0]["severity"], "error");
    let refused = call(
        &mut docs,
        "rename",
        &args(json!({"document": doc, "path": "/shop/owner", "name": "x"})),
    );
    assert!(matches!(refused, Err(ToolError::Refused(_))), "{refused:?}");
}

#[test]
fn a_replace_that_would_break_the_document_is_undone() {
    let path = copy("utf8_bom_crlf.xml", "replace");
    let mut docs = Files::default();
    let doc = run(&mut docs, "open", json!({"path": path}))["document"].clone();
    let before = run(&mut docs, "text", json!({"document": doc}))["text"].clone();
    let refused = call(
        &mut docs,
        "replace",
        &args(json!({"document": doc, "query": "</note>", "with": ""})),
    );
    assert!(matches!(refused, Err(ToolError::Refused(_))), "{refused:?}");
    assert_eq!(
        run(&mut docs, "text", json!({"document": doc}))["text"],
        before
    );
    let replaced = run(
        &mut docs,
        "replace",
        json!({"document": doc, "query": "café", "with": "Café"}),
    );
    assert_eq!(replaced["replaced"], 1);
}

#[test]
fn lists_come_in_pages() {
    let items: String = (1..=250).map(|i| format!("<item n=\"{i}\"/>\n")).collect();
    let path = write(
        "wide.xml",
        "pages",
        format!("<list>\n{items}</list>\n").as_bytes(),
    );
    let mut docs = Files::default();
    let doc = run(&mut docs, "open", json!({"path": path}))["document"].clone();
    let page = run(
        &mut docs,
        "children",
        json!({"document": doc, "path": "/list", "offset": 200, "limit": 100}),
    );
    assert_eq!(page["total"], 250);
    assert_eq!(page["rows"].as_array().unwrap().len(), 50);
    assert_eq!(page["rows"][0]["path"], "/list/item[201]");
    assert_eq!(page["rows"][0]["cells"]["@n"], "201");
    // The path a row gives leads back to it.
    let item = run(
        &mut docs,
        "element",
        json!({"document": doc, "path": "/list/item[201]"}),
    );
    assert_eq!(item["attributes"][0]["value"], "201");
    let found = run(
        &mut docs,
        "find",
        json!({"document": doc, "query": "item", "limit": 5}),
    );
    assert_eq!(found["total"], 250);
    assert_eq!(found["matches"].as_array().unwrap().len(), 5);
}

#[test]
fn long_text_is_cut() {
    let long = "x".repeat(MAX_TEXT * 2);
    let path = write(
        "long.xml",
        "long",
        format!("<r a=\"{long}\">{long}</r>").as_bytes(),
    );
    let mut docs = Files::default();
    let doc = run(&mut docs, "open", json!({"path": path}))["document"].clone();
    let r = run(&mut docs, "element", json!({"document": doc, "path": "/r"}));
    let text = r["text"].as_str().unwrap();
    assert!(
        text.len() < MAX_TEXT + 100 && text.ends_with("bytes in all]"),
        "{}",
        text.len()
    );
    let source = run(&mut docs, "text", json!({"document": doc}));
    assert_eq!(source["cut_at_64_kib"], true);
    assert!(source["text"].as_str().unwrap().len() <= MAX_TEXT);
}

#[test]
fn unknown_documents_and_tools_are_named() {
    let mut docs = Files::default();
    let e = call(
        &mut docs,
        "element",
        &args(json!({"document": 7, "path": "/r"})),
    )
    .unwrap_err();
    assert_eq!(e.to_string(), "no open document 7; `documents` lists them");
    let e = call(&mut docs, "nonsense", &Map::new()).unwrap_err();
    assert_eq!(e.to_string(), "unknown tool nonsense");
}
