//! The window's side and the CLI's side of the local connection that lets an agent work in
//! the open window (`docs/decisions.md`, D28).
//!
//! The window listens on `127.0.0.1` and writes the port and a secret into `agent.json` next
//! to its settings file; only the user can read it. `xml-atlas-cli mcp --window` reads the
//! file, connects, and sends one line of JSON per tool call: `{"secret", "tool",
//! "arguments"}`. The window answers with one line: `{"result"}` or `{"error"}`. A call
//! without the right secret closes the connection.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Map, Value, json};

/// Names another settings file than the usual one; `agent.json` goes next to it (WIN-3).
pub const SETTINGS_VARIABLE: &str = "XML_ATLAS_SETTINGS";

/// The file the window writes while it lets agents in.
pub const ENDPOINT_FILE: &str = "agent.json";

/// How long the CLI waits for the window to answer one call: opening a large file on the
/// window's thread takes about a second per 100 MiB.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

/// A tool call from an agent, waiting for the window to run it.
pub struct Request {
    pub tool: String,
    pub arguments: Map<String, Value>,
    reply: Sender<Result<Value, String>>,
}

/// The window's end: it listens while it exists, and hands the calls over in
/// [`Listener::requests`].
pub struct Listener {
    pub requests: Receiver<Request>,
    connected: Arc<AtomicUsize>,
    /// Stops listening and removes the file when dropped.
    pub guard: Guard,
}

/// Ends a [`Listener`]: dropped, it stops listening and removes the file, so that no agent
/// finds the window any more. Connections already open end with their next call.
pub struct Guard {
    stop: Arc<AtomicBool>,
    port: u16,
    file: PathBuf,
}

/// The CLI's end: one connection to the window.
pub struct WindowClient {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    secret: String,
}

impl Request {
    /// Sends the window's answer back to the agent.
    pub fn answer(self, result: Result<Value, String>) {
        let _ = self.reply.send(result);
    }
}

impl Listener {
    /// Listens on a port the system picks and writes `file` ([`endpoint_file`]). `wake` is
    /// called when a request arrives, so the window draws a frame and runs it.
    pub fn start(file: PathBuf, wake: impl Fn() + Send + Sync + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let secret = new_secret()?;
        write_endpoint(&file, port, &secret)?;
        let (tx, requests) = channel();
        let connected = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let (accepted, stopped) = (connected.clone(), stop.clone());
        std::thread::Builder::new()
            .name("agent-listener".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let (tx, wake, count) = (tx.clone(), wake.clone(), accepted.clone());
                    let secret = secret.clone();
                    let _ = std::thread::Builder::new()
                        .name("agent-connection".into())
                        .spawn(move || {
                            count.fetch_add(1, Ordering::Relaxed);
                            wake();
                            if let Err(e) = serve(stream, &secret, &tx, &*wake) {
                                log::debug!("agent connection ended: {e}");
                            }
                            count.fetch_sub(1, Ordering::Relaxed);
                            wake();
                        });
                }
            })?;
        log::info!(
            "agents may connect on 127.0.0.1:{port} ({})",
            file.display()
        );
        Ok(Self {
            requests,
            connected,
            guard: Guard { stop, port, file },
        })
    }

    /// How many agents are connected now.
    pub fn connected(&self) -> usize {
        self.connected.load(Ordering::Relaxed)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // The listener waits in `accept`: a connection of our own wakes it to see `stop`.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        let _ = std::fs::remove_file(&self.file);
    }
}

impl WindowClient {
    /// Connects to the window that wrote `file`.
    pub fn connect(file: &Path) -> Result<Self, String> {
        let no_window = || {
            "no XML Atlas window lets agents in: open XML Atlas and turn on Allow AI \
             agents (MCP) in File > Settings"
                .to_string()
        };
        let text = std::fs::read_to_string(file).map_err(|_| no_window())?;
        let endpoint: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let port = endpoint["port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok());
        let secret = endpoint["secret"].as_str();
        let (Some(port), Some(secret)) = (port, secret) else {
            return Err(format!("{} is not as the window writes it", file.display()));
        };
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).map_err(|_| no_window())?;
        stream
            .set_read_timeout(Some(ANSWER_TIMEOUT))
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        Ok(Self {
            stream,
            reader,
            secret: secret.to_string(),
        })
    }

    /// Runs `tool` in the window: the window's result or refusal, or, as the outer `Err`, a
    /// connection that broke.
    pub fn call(
        &mut self,
        tool: &str,
        arguments: &Map<String, Value>,
    ) -> Result<Result<Value, String>, String> {
        let line = json!({"secret": self.secret, "tool": tool, "arguments": arguments});
        writeln!(self.stream, "{line}").map_err(|e| format!("the window went away: {e}"))?;
        let mut answer = String::new();
        let read = self
            .reader
            .read_line(&mut answer)
            .map_err(|e| format!("no answer from the window: {e}"))?;
        if read == 0 {
            return Err("the window closed the connection".into());
        }
        let answer: Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        match (answer.get("result"), answer.get("error")) {
            (Some(result), _) => Ok(Ok(result.clone())),
            (_, Some(error)) => Ok(Err(error.as_str().unwrap_or("refused").to_string())),
            _ => Err("the window's answer is empty".into()),
        }
    }
}

/// The settings file: [`SETTINGS_VARIABLE`] if set, else `xml-atlas/settings.toml` in the
/// system's configuration folder (WIN-3).
pub fn settings_file() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = env(SETTINGS_VARIABLE) {
        return Some(p);
    }
    let config = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    };
    config.map(|d| d.join("xml-atlas").join("settings.toml"))
}

/// Where the window writes [`ENDPOINT_FILE`]: next to the settings file.
pub fn endpoint_file() -> Option<PathBuf> {
    let settings = settings_file()?;
    Some(settings.parent()?.join(ENDPOINT_FILE))
}

/// Reads calls from one connection and answers them, until it closes or a call has the wrong
/// secret.
fn serve(
    stream: TcpStream,
    secret: &str,
    tx: &Sender<Request>,
    wake: &(dyn Fn() + Send + Sync),
) -> io::Result<()> {
    let mut out = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        let call: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", json!({"error": format!("not JSON: {e}")}))?;
                continue;
            }
        };
        if !same(
            call["secret"].as_str().unwrap_or("").as_bytes(),
            secret.as_bytes(),
        ) {
            writeln!(out, "{}", json!({"error": "wrong secret"}))?;
            return Ok(());
        }
        let tool = call["tool"].as_str().unwrap_or("").to_string();
        let arguments = call["arguments"].as_object().cloned().unwrap_or_default();
        let (reply, answer) = channel();
        if tx
            .send(Request {
                tool,
                arguments,
                reply,
            })
            .is_err()
        {
            return Ok(());
        }
        wake();
        let answer = match answer.recv() {
            Ok(Ok(result)) => json!({"result": result}),
            Ok(Err(error)) => json!({"error": error}),
            Err(_) => return Ok(()),
        };
        writeln!(out, "{answer}")?;
    }
    Ok(())
}

/// Compares two secrets in time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 32 random bytes from the system, as hex.
fn new_secret() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes the endpoint so that only the user can read it, replacing an older one.
fn write_endpoint(file: &Path, port: u16, secret: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = json!({"port": port, "secret": secret, "pid": std::process::id()}).to_string();
    let tmp = file.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut f = options.open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_compare_whole() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
        let a = new_secret().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_secret().unwrap());
    }
}
