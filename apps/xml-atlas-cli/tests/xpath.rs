//! `xml-atlas-cli xpath` prints what an XPath 1.0 expression selects (D35, XP-5).

use std::process::Command;

#[test]
fn xpath_prints_nodes_values_and_errors() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-cli-xpath-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.xml");
    std::fs::write(
        &file,
        "\u{feff}<r>\r\n  <a n=\"1\">Größe</a>\r\n  <a n=\"2\"/>\r\n  <a n=\"3\"/>\r\n</r>\r\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_xml-atlas-cli"))
            .arg("xpath")
            .arg(&file)
            .args(args)
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    };
    // Nodes: line, path and value, at most --limit of them, then the count.
    let (ok, out, _) = run(&["//a", "--limit", "2"]);
    assert!(ok);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "       2: /r/a[1]  Größe");
    assert_eq!(lines[1], "       3: /r/a[2]");
    assert!(lines[2].starts_with("3 nodes in "), "{out}");
    assert_eq!(lines.len(), 3);
    let (_, out, _) = run(&["//a[2]/@n"]);
    assert_eq!(out.lines().next(), Some("       3: /r/a[2]/@n  2"));
    // A value, and what kind it is.
    let (ok, out, _) = run(&["sum(//@n) div count(//a)"]);
    assert!(ok);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "2");
    assert!(lines[1].starts_with("a number, in "), "{out}");
    // A wrong expression fails and says where.
    let (ok, _, err) = run(&["//a[@n = '1'"]);
    assert!(!ok);
    assert!(err.contains("expected `]`, found the end at 13"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}
