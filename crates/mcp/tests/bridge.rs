//! The window's connection (D28) without a window: a listener, a loop that runs the calls as
//! the window does between frames, and the CLI's client.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use serde_json::{Value, json};
use xml_atlas_mcp::bridge::{Listener, WindowClient};
use xml_atlas_mcp::{Files, call};

fn args(v: Value) -> serde_json::Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn calls_reach_the_window_and_strangers_do_not() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-bridge");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("agent.json");
    let xml = dir.join("Größe.xml");
    std::fs::write(&xml, "\u{feff}<r>\r\n  <a n=\"1\"/>\r\n</r>\r\n").unwrap();

    let listener = Listener::start(file.clone(), || {}).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "only the user may read the secret");
    }
    // The window: runs each call on its documents, between frames.
    let requests = listener.requests;
    let window = std::thread::spawn(move || {
        let mut docs = Files::default();
        for r in requests.iter() {
            let result = call(&mut docs, &r.tool, &r.arguments).map_err(|e| e.to_string());
            r.answer(result);
        }
    });
    let _guard = listener.guard;
    let mut client = WindowClient::connect(&file).unwrap();
    let opened = client
        .call("open", &args(json!({"path": xml})))
        .unwrap()
        .unwrap();
    let doc = opened["document"].clone();
    client
        .call(
            "set_attribute",
            &args(json!({"document": doc, "path": "/r/a", "name": "n", "value": "zwei"})),
        )
        .unwrap()
        .unwrap();
    let refused = client
        .call(
            "element",
            &args(json!({"document": doc, "path": "/nowhere"})),
        )
        .unwrap()
        .unwrap_err();
    assert!(refused.contains("nowhere"), "{refused}");
    client
        .call("save", &args(json!({"document": doc})))
        .unwrap()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&xml).unwrap(),
        "\u{feff}<r>\r\n  <a n=\"zwei\"/>\r\n</r>\r\n"
    );

    // A caller without the secret gets nothing done and is sent away.
    let endpoint: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let port = endpoint["port"].as_u64().unwrap() as u16;
    let mut stranger = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let wrong = json!({"secret": "0".repeat(64), "tool": "documents", "arguments": {}});
    writeln!(stranger, "{wrong}").unwrap();
    let mut reader = BufReader::new(stranger);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("wrong secret"), "{line}");
    line.clear();
    assert_eq!(
        reader.read_line(&mut line).unwrap(),
        0,
        "the connection is closed"
    );

    drop(client);
    drop(_guard);
    assert!(!file.exists());
    drop(window);
}

#[test]
fn stopping_removes_the_file() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-bridge-stop");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("agent.json");
    let listener = Listener::start(file.clone(), || {}).unwrap();
    assert!(file.exists());
    drop(listener);
    assert!(!file.exists());
    let e = WindowClient::connect(&file).err().unwrap();
    assert!(e.contains("Allow AI agents"), "{e}");
}
