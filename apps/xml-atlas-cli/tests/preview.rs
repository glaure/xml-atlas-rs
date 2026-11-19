//! `xml-atlas-cli preview` expands repeated elements as the preview pane does (REF-11).

use std::process::Command;

#[test]
fn preview_expands_repeats_unless_told_not_to() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-cli-repeat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("xml-atlas-references.json"),
        r#"{"repeat": {"count": "Count", "index": ["[Index]"]}}"#,
    )
    .unwrap();
    let file = dir.join("doc.xml");
    std::fs::write(
        &file,
        "\u{feff}<R>\r\n  <Größe Count=\"2\" N=\"[Index]\"/>\r\n</R>\r\n",
    )
    .unwrap();
    let run = |extra: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_xml-atlas-cli"))
            .arg("preview")
            .arg(&file)
            .arg("/R")
            .args(extra)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    assert_eq!(
        run(&[]).trim_end(),
        "<R>\r\n  <Größe0 N=\"0\"/>\r\n  <Größe1 N=\"1\"/>\r\n</R>"
    );
    assert_eq!(
        run(&["--no-repeat"]).trim_end(),
        "<R>\r\n  <Größe Count=\"2\" N=\"[Index]\"/>\r\n</R>"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
