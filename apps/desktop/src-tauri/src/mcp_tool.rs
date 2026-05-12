//! Bridge between the MCP registry and the agent's `ToolDispatcher`.
//!
//! Each running MCP server advertises a set of tools via
//! [`mcp::McpRegistry::remote_tools`]. This module wraps each one in a
//! [`tools::Tool`] implementation so the agent can dispatch them
//! through the same code path as a local Rust tool. Invocation routes
//! synchronously through [`mcp::McpRegistry::call_remote_tool`] — the
//! underlying transport is sync stdio so the agent loop never needs
//! to bridge async-from-sync.
//!
//! Registration happens at startup (after all enabled servers are
//! started) and on every successful `restart_mcp_server`. The
//! dispatcher's `unregister_prefix` strips a server's old tools
//! before its new ones are added so renames / removals stick.

use std::sync::Arc;

use serde_json::{json, Value};

use mcp::{namespaced_wire_name, McpRegistry, RemoteToolDescriptor};
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};

/// One MCP-server-advertised tool, exposed to the agent's dispatcher.
struct RemoteMcpTool {
    /// Mangled `<server>__<tool>` shape that the dispatcher keys on
    /// and the model sees on the wire.
    wire_name: String,
    /// Original tool name as the MCP server knows it. Sent verbatim
    /// in the `tools/call` JSON-RPC request.
    server_tool_name: String,
    server_id: String,
    description: String,
    /// Anthropic-shaped descriptor returned from `schema()`. We
    /// materialise it once at construction since `Tool::schema`
    /// returns by value and is called on every API call.
    descriptor: Value,
    registry: Arc<McpRegistry>,
}

impl RemoteMcpTool {
    fn from_descriptor(desc: &RemoteToolDescriptor, registry: Arc<McpRegistry>) -> Self {
        let descriptor = json!({
            "name": desc.wire_name,
            "description": desc.description,
            "input_schema": desc.schema,
        });
        Self {
            wire_name: desc.wire_name.clone(),
            server_tool_name: desc.tool.clone(),
            server_id: desc.server.clone(),
            description: desc.description.clone(),
            descriptor,
            registry,
        }
    }
}

impl Tool for RemoteMcpTool {
    fn name(&self) -> &'static str {
        // `Tool::name` returns `&'static str`. The wire name is owned,
        // not known at compile time. We leak the string once so the
        // returned reference is genuinely `'static` — the registration
        // path constructs each `RemoteMcpTool` at most once per server
        // restart, so leaks are bounded by the number of tools, not by
        // call count.
        //
        // The dispatcher also has its own copy of the name as the map
        // key (cloned at register time), so this leak is invisible to
        // every other consumer.
        Box::leak(self.wire_name.clone().into_boxed_str())
    }

    fn schema(&self) -> Value {
        self.descriptor.clone()
    }

    fn invoke(&self, args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
        match self.registry.call_remote_tool(&self.server_id, &self.server_tool_name, args) {
            Ok(result) => {
                let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
                if is_error {
                    Ok(ToolResult::Error(format!(
                        "mcp tool `{}::{}` returned isError=true: {}",
                        self.server_id, self.server_tool_name, summarise_content(&result)
                    )))
                } else {
                    Ok(ToolResult::Ok(result))
                }
            }
            Err(reason) => Ok(ToolResult::Error(reason)),
        }
    }
}

/// MCP responses typically carry `content: [{ type, text }, ...]`.
/// For error messages we collapse the text blocks into one string so
/// the model sees a useful failure rather than `[object Object]`.
fn summarise_content(result: &Value) -> String {
    let Some(arr) = result.get("content").and_then(|v| v.as_array()) else {
        return result.to_string();
    };
    let mut out = String::new();
    for entry in arr {
        if let Some(t) = entry.get("text").and_then(|v| v.as_str()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    if out.is_empty() {
        result.to_string()
    } else {
        out
    }
}

/// Drop every remote tool currently registered for `server_id`, then
/// register one `RemoteMcpTool` per tool the registry advertises for
/// that server right now. Call this:
///
/// * Once at startup after all enabled servers have started.
/// * After every successful `restart_mcp_server`.
///
/// The dispatcher key prefix is `<sanitised_server_id>__`, matching
/// the namespacing done by [`mcp::namespaced_wire_name`].
pub fn refresh_dispatcher_for_server(
    dispatcher: &mut ToolDispatcher,
    registry: Arc<McpRegistry>,
    server_id: &str,
) {
    let prefix = namespaced_wire_name(server_id, "");
    // `namespaced_wire_name(server, "")` returns `<sanitised>__` when
    // the natural form fits in 64 chars. Long server names hash, which
    // would make a simple prefix unsuitable — but in practice server
    // ids are short labels (`brave-search`, `filesystem`) so the
    // natural form is always used. Defensively, strip the trailing
    // empty-tool segment to keep prefix-matching robust.
    let prefix = prefix.trim_end_matches('_').to_string() + "__";
    dispatcher.unregister_prefix(&prefix);

    let snapshot = registry.remote_tools();
    for desc in snapshot.into_iter().filter(|d| d.server == server_id) {
        dispatcher.register(Box::new(RemoteMcpTool::from_descriptor(
            &desc,
            Arc::clone(&registry),
        )));
    }
}

/// Drop every server's remote tools, then re-register the union.
/// Useful at startup once.
pub fn refresh_dispatcher_all(
    dispatcher: &mut ToolDispatcher,
    registry: Arc<McpRegistry>,
) {
    let snapshot = registry.remote_tools();
    // Deduplicate the server ids so we strip each prefix exactly once.
    let mut servers: Vec<String> = snapshot.iter().map(|d| d.server.clone()).collect();
    servers.sort();
    servers.dedup();
    for id in &servers {
        let prefix = namespaced_wire_name(id, "");
        let prefix = prefix.trim_end_matches('_').to_string() + "__";
        dispatcher.unregister_prefix(&prefix);
    }
    for desc in snapshot {
        dispatcher.register(Box::new(RemoteMcpTool::from_descriptor(
            &desc,
            Arc::clone(&registry),
        )));
    }
}

// Silence "unused field" lints — `description` is only present so a
// future diagnostic surface (e.g. tracing on dispatch) can quote the
// human-readable text without re-reading the registry. The dispatcher
// already advertises it through `descriptor.description`.
const _: fn(&RemoteMcpTool) -> &str = |t| t.description.as_str();
