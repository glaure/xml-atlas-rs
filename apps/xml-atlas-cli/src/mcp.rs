//! `xml-atlas-cli mcp`: an MCP server over standard input and output (`docs/decisions.md`,
//! D27). The tools are `xml-atlas-mcp`'s; this module only speaks the protocol, with `rmcp`.
//! They work on files kept in memory (step 1), or, with `--window`, on the documents open in
//! the window (step 2, D28).

use std::sync::{Arc, Mutex};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Map, Value};
use xml_atlas_mcp::Files;
use xml_atlas_mcp::bridge::{self, WindowClient};

/// What the server tells the agent about itself when it starts.
const INSTRUCTIONS: &str = "XML Atlas opens large XML files quickly and edits them without \
changing anything else in the file. Open a file with `open`, then read it with `element`, \
`children`, `text` and `find`, using element paths such as `/orders/order[2]/customer`. \
Changes stay in memory, each one undo step, until `save`. Lists come in pages: use `offset` \
and `limit`.";

/// Added when the tools work in the window.
const WINDOW_INSTRUCTIONS: &str = " These tools work in the XML Atlas window the user has \
open: `documents` lists its tabs, the user sees each change selected and can undo it, and a \
tab with unsaved changes cannot be closed.";

/// Where the tools' documents are.
enum Backend {
    /// Files kept in memory for this session (step 1).
    Files(Mutex<Files>),
    /// The open window, connected when the first call comes (step 2).
    Window(Mutex<Option<WindowClient>>),
}

struct Server {
    backend: Backend,
    tools: Vec<Tool>,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let instructions = match self.backend {
            Backend::Files(_) => INSTRUCTIONS.to_string(),
            Backend::Window(_) => format!("{INSTRUCTIONS}{WINDOW_INSTRUCTIONS}"),
        };
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("xml-atlas", env!("CARGO_PKG_VERSION")))
            .with_instructions(instructions)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.clone()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = request.arguments.unwrap_or_default();
        let result = self.run(&request.name, &args)?;
        log::info!(
            "{}: {}",
            request.name,
            if result.is_ok() { "ok" } else { "refused" }
        );
        Ok(match result {
            Ok(value) => CallToolResult::structured(value).into(),
            // An error the agent can act on is a result, not a protocol error.
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]).into(),
        })
    }
}

impl Server {
    /// Runs a tool where the documents are; `Err` inside is a refusal for the agent.
    fn run(
        &self,
        tool: &str,
        args: &Map<String, Value>,
    ) -> Result<Result<Value, String>, ErrorData> {
        match &self.backend {
            Backend::Files(docs) => {
                let mut docs = docs.lock().map_err(poisoned)?;
                Ok(xml_atlas_mcp::call(&mut *docs, tool, args).map_err(|e| e.to_string()))
            }
            Backend::Window(client) => {
                let mut client = client.lock().map_err(poisoned)?;
                Ok(window_call(&mut client, tool, args))
            }
        }
    }
}

/// A lock that a panicking tool call left behind.
fn poisoned<T>(_: T) -> ErrorData {
    ErrorData::internal_error("a tool call failed before", None)
}

/// Passes a call to the window, connecting first when there is no connection; a connection
/// that broke is made again once, as the window may have been started again.
fn window_call(
    client: &mut Option<WindowClient>,
    tool: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let file = bridge::endpoint_file().ok_or("there is no folder for XML Atlas's settings")?;
    for attempt in 0..2 {
        if client.is_none() {
            *client = Some(WindowClient::connect(&file)?);
        }
        let Some(c) = client.as_mut() else {
            continue;
        };
        match c.call(tool, args) {
            Ok(answer) => return answer,
            Err(broken) => {
                *client = None;
                if attempt == 1 {
                    return Err(broken);
                }
            }
        }
    }
    Err("the window could not be reached".into())
}

/// Serves until the client closes standard input; with `window`, in the open window.
pub fn run(window: bool) -> anyhow::Result<()> {
    let tools = xml_atlas_mcp::tools()
        .into_iter()
        .map(|t| {
            let schema = match t.schema {
                Value::Object(o) => o,
                _ => unreachable!("every tool's schema is an object"),
            };
            Tool::new(t.name, t.description, Arc::new(schema))
                .annotate(ToolAnnotations::new().read_only(t.read_only))
        })
        .collect();
    let backend = if window {
        Backend::Window(Mutex::new(None))
    } else {
        Backend::Files(Mutex::new(Files::default()))
    };
    let server = Server { backend, tools };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let service = server.serve(rmcp::transport::stdio()).await?;
            service.waiting().await?;
            anyhow::Ok(())
        })
}
