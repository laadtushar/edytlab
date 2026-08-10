//! MCP (Model Context Protocol) server management.
//!
//! Phase 5 of the extensibility roadmap. This crate owns the on-disk
//! registration file (`~/.edytlab/mcp.json`), keychain-backed secret
//! resolution, and the per-server runtime state (running / stopped /
//! errored + last-known tool list).
//!
//! Agent-side invocation is wired: the desktop app's `mcp_tool` module
//! wraps each entry from [`McpRegistry::remote_tools`] in a
//! `tools::Tool` and registers it with the shared `ToolDispatcher`, so
//! a model-issued call reaches [`McpRegistry::call_remote_tool`]
//! through the same path as a built-in tool. The transport is
//! deliberately synchronous for that reason — the agent loop never has
//! to bridge async-from-sync.
//!
//! Reads are deadline-bounded (see `transport`): a server that accepts
//! a request and never answers would otherwise block a caller holding
//! the registry mutex, and transitively the dispatcher and the whole
//! agent loop.
//!
//! Two transports, both speaking JSON-RPC 2.0:
//!
//! * **stdio** (`transport`) — a local child process; requests are
//!   line-delimited JSON on its stdin/stdout.
//! * **remote HTTP** (`http`) — the Streamable HTTP transport: each
//!   request is a POST, and the server answers with either JSON or an
//!   event stream. Config still keys this as `sse` for compatibility
//!   with files written before it was implemented.
//!
//! `initialize` is the handshake, `tools/list` discovers tools, and
//! `tools/call` invokes one.

mod config;
mod http;
mod registry;
mod transport;

pub use config::{
    load_config, save_config, McpConfig, McpError, McpServerConfig, McpTransport, Result, SecretRef,
};
pub use http::HttpClient;
pub use registry::{
    namespaced_wire_name, McpRegistry, McpServerStatus, McpServerSummary, RemoteToolDescriptor,
};
pub use transport::{StdioClient, ToolDescriptor};
