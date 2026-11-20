//! `xml-atlas-cli structure` prints one line per path of element names (D36).

use std::process::Command;

#[test]
fn structure_prints_each_path_with_its_count() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-cli-structure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("doc.xml");
    std::fs::write(
        &file,
        "\u{feff}<r>\r\n  <a n=\"1\">Größe</a>\r\n  <a k=\"2\"><b/></a>\r\n  <c/>\r\n</r>\r\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xml-atlas-cli"))
        .arg("structure")
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[..4],
        [
            "r \u{d7}1",
            "  a \u{d7}2 @n @k (text)",
            "    b \u{d7}1",
            "  c \u{d7}1"
        ]
    );
    assert!(lines[4].starts_with("4 paths in "), "{out}");
    assert_eq!(lines.len(), 5);
    std::fs::remove_dir_all(&dir).unwrap();
}
