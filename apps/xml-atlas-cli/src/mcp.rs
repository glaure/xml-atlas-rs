//! `xml-atlas-cli mcp`: an MCP server over standard input and output (`docs/decisions.md`,
//! D27, step 1). The tools are `xml-atlas-mcp`'s; this module only speaks the protocol,
//! with `rmcp`.

use std::sync::{Arc, Mutex};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use xml_atlas_mcp::Files;

/// What the server tells the agent about itself when it starts.
const INSTRUCTIONS: &str = "XML Atlas opens large XML files quickly and edits them without \
changing anything else in the file. Open a file with `open`, then read it with `element`, \
`children`, `text` and `find`, using element paths such as `/orders/order[2]/customer`. \
Changes stay in memory, each one undo step, until `save`. Lists come in pages: use `offset` \
and `limit`.";

struct Server {
    docs: Mutex<Files>,
    tools: Vec<Tool>,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("xml-atlas", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
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
        let result = {
            let mut docs = self
                .docs
                .lock()
                .map_err(|_| ErrorData::internal_error("a tool call failed before", None))?;
            xml_atlas_mcp::call(&mut *docs, &request.name, &args)
        };
        log::info!(
            "{}: {}",
            request.name,
            if result.is_ok() { "ok" } else { "refused" }
        );
        Ok(match result {
            Ok(value) => CallToolResult::structured(value).into(),
            // An error the agent can act on is a result, not a protocol error.
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into(),
        })
    }
}

/// Serves until the client closes standard input.
pub fn run() -> anyhow::Result<()> {
    let tools = xml_atlas_mcp::tools()
        .into_iter()
        .map(|t| {
            let schema = match t.schema {
                serde_json::Value::Object(o) => o,
                _ => unreachable!("every tool's schema is an object"),
            };
            Tool::new(t.name, t.description, Arc::new(schema))
                .annotate(ToolAnnotations::new().read_only(t.read_only))
        })
        .collect();
    let server = Server {
        docs: Mutex::new(Files::default()),
        tools,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let service = server.serve(rmcp::transport::stdio()).await?;
            service.waiting().await?;
            anyhow::Ok(())
        })
}
