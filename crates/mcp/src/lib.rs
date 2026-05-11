//! MCP (Model Context Protocol) server management.
//!
//! Phase 5 of the extensibility roadmap. This crate owns the on-disk
//! registration file (`~/.edytlab/mcp.json`), keychain-backed secret
//! resolution, and the per-server runtime state (running / stopped /
//! errored + last-known tool list).
//!
//! **Scope note for v1.** The dispatcher's `invoke` is synchronous;
//! routing the agent's tool calls through async stdio / SSE clients
//! requires non-trivial surgery on `ai::run_turn`. This crate ships
//! the *management* layer end-to-end (config, secret resolution,
//! process lifecycle, tools/list discovery) so users can register,
//! start, and inspect MCP servers from the Settings panel. Wiring
//! agent-side remote tool invocation is queued as a follow-up; the
//! `tools::ToolDispatcher` integration point is left as
//! `register_remote_tools`-shaped TODO in `McpRegistry`.
//!
//! On the wire we speak JSON-RPC 2.0 over stdio per the MCP spec:
//! requests are line-delimited JSON objects; `initialize` is the
//! handshake; `tools/list` returns the server's tools.

mod config;
mod registry;
mod transport;

pub use config::{
    load_config, save_config, McpConfig, McpError, McpServerConfig, McpTransport, Result, SecretRef,
};
pub use registry::{
    namespaced_wire_name, McpRegistry, McpServerStatus, McpServerSummary, RemoteToolDescriptor,
};
pub use transport::{StdioClient, ToolDescriptor};
