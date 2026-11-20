//! `xml-atlas-cli mcp` through the protocol, as a client such as Claude Code uses it: start,
//! initialize, list the tools, open a file with a byte order mark and CRLF, edit and save it.
//! The saved file changes only where asked (AGENTS.md, rule 7).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Client {
    fn start() -> Self {
        Self::start_with(&["mcp"], None)
    }

    /// Starts the server with `args`; `settings` names the settings file, next to which the
    /// window writes `agent.json`.
    fn start_with(args: &[&str], settings: Option<&std::path::Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xml-atlas-cli"));
        if let Some(s) = settings {
            command.env("XML_ATLAS_SETTINGS", s);
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            out,
            next: 0,
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and returns its result, skipping anything else the server sends.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(self.out.read_line(&mut line).unwrap() > 0, "server closed");
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == id {
                assert!(reply.get("error").is_none(), "{method}: {reply}");
                return reply["result"].clone();
            }
        }
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert_eq!(result["isError"], false, "{name}: {result}");
        result["structuredContent"].clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn an_agent_edits_and_saves_through_the_protocol() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-protocol");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Größe orders.xml");
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/engine/tests/corpus/utf8_bom_crlf.xml");
    std::fs::copy(&corpus, &path).unwrap();
    let before = std::fs::read(&path).unwrap();

    let mut c = Client::start();
    let info = c.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"},
        }),
    );
    assert_eq!(info["serverInfo"]["name"], "xml-atlas");
    assert!(info["capabilities"]["tools"].is_object());
    c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let list = c.request("tools/list", json!({}));
    let names: Vec<&str> = list["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for want in [
        "open", "element", "children", "find", "xpath", "set_text", "save",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }

    let opened = c.tool("open", json!({"path": path}));
    let doc = opened["document"].clone();
    assert_eq!(opened["root"], "orders");
    c.tool(
        "set_attribute",
        json!({"document": doc, "path": "/orders/order", "name": "city", "value": "Wien"}),
    );
    c.tool("save", json!({"document": doc}));

    // A refusal comes back as a tool error the agent can read, not a protocol error.
    let refused = c.request(
        "tools/call",
        json!({"name": "element", "arguments": {"document": doc, "path": "/nowhere"}}),
    );
    assert_eq!(refused["isError"], true);

    let after = std::fs::read(&path).unwrap();
    let expected = String::from_utf8(before)
        .unwrap()
        .replace("city=\"Ä\"", "city=\"Wien\"")
        .into_bytes();
    assert_eq!(after, expected);
}

/// `mcp --window` (D28): the calls go to the window, here a listener in this test that runs
/// them on files as the window runs them on its tabs. Without a window, the agent is told how
/// to let it in.
#[test]
fn an_agent_works_in_the_window() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-window");
    std::fs::create_dir_all(&dir).unwrap();
    let settings = dir.join("settings.toml");
    let path = dir.join("Größe.xml");
    std::fs::write(&path, "\u{feff}<r>\r\n  <a n=\"1\"/>\r\n</r>\r\n").unwrap();

    let mut c = Client::start_with(&["mcp", "--window"], Some(&settings));
    c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let no_window = c.request("tools/call", json!({"name": "documents", "arguments": {}}));
    assert_eq!(no_window["isError"], true);
    assert!(
        no_window.to_string().contains("Allow AI agents"),
        "{no_window}"
    );

    let listener = xml_atlas_mcp::bridge::Listener::start(dir.join("agent.json"), || {}).unwrap();
    let requests = listener.requests;
    let _guard = listener.guard;
    std::thread::spawn(move || {
        let mut docs = xml_atlas_mcp::Files::default();
        for r in requests.iter() {
            let result =
                xml_atlas_mcp::call(&mut docs, &r.tool, &r.arguments).map_err(|e| e.to_string());
            r.answer(result);
        }
    });
    let doc = c.tool("open", json!({"path": path}))["document"].clone();
    c.tool(
        "set_attribute",
        json!({"document": doc, "path": "/r/a", "name": "n", "value": "Bäume"}),
    );
    c.tool("save", json!({"document": doc}));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "\u{feff}<r>\r\n  <a n=\"Bäume\"/>\r\n</r>\r\n"
    );
}
