//! `xml-atlas-cli preview` expands repeated elements as the preview pane does (REF-11).

use std::process::Command;

#[test]
fn preview_expands_repeats_unless_told_not_to() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-cli-repeat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rules = dir.join("rules.json");
    std::fs::write(
        &rules,
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
            .arg("--rules")
            .arg(&rules)
            .env("XML_ATLAS_SETTINGS", dir.join("settings.toml"))
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

/// Without `--rules` the configuration folder decides, not a file beside the document
/// (REF-13, D41).
#[test]
fn rules_come_from_the_configuration_folder() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-cli-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (config, docs) = (dir.join("config"), dir.join("docs"));
    std::fs::create_dir_all(&docs).unwrap();
    let rules = r#"{"includes": [{"element": "Include", "attribute": "src"}]}"#;
    std::fs::write(docs.join("xml-atlas-references.json"), rules).unwrap();
    let file = docs.join("Größe.xml");
    std::fs::write(&file, "<R><P/><Include src=\"../P\"/></R>").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_xml-atlas-cli"))
            .arg("references")
            .arg(&file)
            .env("XML_ATLAS_SETTINGS", config.join("settings.toml"))
            .output()
            .unwrap()
    };
    let out = run();
    assert!(
        !out.status.success(),
        "a file beside the document is not used"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("--rules"));

    let folder = xml_atlas_engine::rules_folder::RulesFolder::new(&config);
    let name = folder
        .import(&docs.join("xml-atlas-references.json"))
        .unwrap();
    assert!(!run().status.success(), "imported, it is off");
    folder.set_on(&name, true).unwrap();
    folder.set_use_for(&name, "Größe.xml").unwrap();
    let out = run();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 found"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// `references --check` says what is wrong with the rules, and fails only on an error
/// (REF-14).
#[test]
fn check_names_the_mistakes_in_the_rules() {
    let dir = std::env::temp_dir().join(format!("xml-atlas-cli-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (file, rules) = (dir.join("doc.xml"), dir.join("rules.json"));
    std::fs::write(&file, "<R><Größe/></R>").unwrap();
    let run = |text: &str| {
        std::fs::write(&rules, text).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_xml-atlas-cli"))
            .args(["references", "--check"])
            .arg(&file)
            .arg("--rules")
            .arg(&rules)
            .env("XML_ATLAS_SETTINGS", dir.join("settings.toml"))
            .output()
            .unwrap();
        (out.status.success(), String::from_utf8(out.stdout).unwrap())
    };
    let (ok, out) = run("\u{feff}{\r\n  \"include\": []\r\n}");
    assert!(ok, "warnings do not fail: {out}");
    assert!(
        out.contains(":2:3: warning:") && out.contains("\"includes\"?"),
        "{out}"
    );
    let (ok, out) = run("{\n  \"includes\": [],\n}");
    assert!(!ok && out.contains(":3:1: error: trailing comma"), "{out}");
    std::fs::remove_dir_all(dir).unwrap();
}
