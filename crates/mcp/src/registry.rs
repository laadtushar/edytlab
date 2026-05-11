//! Runtime registry of MCP servers.
//!
//! Owns the in-memory state of which servers are running, the tools
//! each one advertises, and the last-error string for ones that
//! failed to start. The Tauri command layer drives the lifecycle via
//! `start`, `stop`, and `restart`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::config::{McpServerConfig, McpTransport};
use crate::transport::{StdioClient, ToolDescriptor};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpServerStatus {
    Stopped,
    Running,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerSummary {
    pub id: String,
    pub transport: &'static str,
    pub status: McpServerStatus,
    pub tools_count: usize,
    /// Last error message, if `status == Error`. Surfaced verbatim in
    /// the Settings UI so the user can fix what's wrong.
    pub last_error: Option<String>,
}

/// One namespaced tool exposed by a remote MCP server. Mirrors the
/// design-doc `RemoteToolDescriptor`: the `wire_name` is what the
/// model sees (mangled to fit Anthropic's 64-char regex); the
/// `display_name` is the unmangled `<server>::<tool>` shown in the
/// capabilities menu.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteToolDescriptor {
    pub wire_name: String,
    pub display_name: String,
    pub server: String,
    pub tool: String,
    pub description: String,
}

struct RunningServer {
    /// Held so the child stays alive; dropping this struct kills the
    /// child via `StdioClient::drop`. Actual JSON-RPC dispatch hangs
    /// off this in the follow-up that wires remote tool invocation.
    #[allow(dead_code)]
    client: StdioClient,
    tools: Vec<ToolDescriptor>,
}

#[derive(Default)]
struct Inner {
    running: HashMap<String, RunningServer>,
    errors: HashMap<String, String>,
}

#[derive(Default)]
pub struct McpRegistry {
    inner: Mutex<Inner>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Status summary for `id` — useful for `list_mcp_servers`.
    pub fn summary(&self, id: &str, cfg: &McpServerConfig) -> McpServerSummary {
        let inner = self.inner.lock().expect("registry mutex poisoned");
        let status = if inner.running.contains_key(id) {
            McpServerStatus::Running
        } else if inner.errors.contains_key(id) {
            McpServerStatus::Error
        } else {
            McpServerStatus::Stopped
        };
        let tools_count = inner.running.get(id).map(|r| r.tools.len()).unwrap_or(0);
        let last_error = inner.errors.get(id).cloned();
        McpServerSummary {
            id: id.to_string(),
            transport: match cfg.transport() {
                McpTransport::Stdio => "stdio",
                McpTransport::Sse => "sse",
            },
            status,
            tools_count,
            last_error,
        }
    }

    /// Start a server. SSE transport is not yet implemented — calls
    /// return Err so the UI can render a clear "coming soon" badge
    /// rather than a silent failure.
    pub fn start(
        &self,
        id: &str,
        cfg: &McpServerConfig,
        resolve_secret: impl FnMut(&str) -> Option<String>,
    ) -> Result<(), String> {
        let (mut client, _transport) = match cfg {
            McpServerConfig::Stdio {
                command,
                args,
                env,
                enabled,
            } => {
                if !enabled {
                    return Err("server is disabled".into());
                }
                let client = StdioClient::spawn(command, args, env, resolve_secret)
                    .map_err(|e| e.to_string())?;
                (client, McpTransport::Stdio)
            }
            McpServerConfig::Sse { .. } => {
                return Err("SSE transport is not yet supported in this build".into());
            }
        };
        client.initialize().map_err(|e| e.to_string())?;
        let tools = client.list_tools().map_err(|e| e.to_string())?;

        let mut inner = self.inner.lock().expect("registry mutex poisoned");
        inner.errors.remove(id);
        inner
            .running
            .insert(id.to_string(), RunningServer { client, tools });
        Ok(())
    }

    /// Stop a server, killing its child process.
    pub fn stop(&self, id: &str) {
        let mut inner = self.inner.lock().expect("registry mutex poisoned");
        // Dropping the `RunningServer` triggers `StdioClient::drop`,
        // which kills the child.
        inner.running.remove(id);
    }

    /// Restart = stop + start.
    pub fn restart(
        &self,
        id: &str,
        cfg: &McpServerConfig,
        resolve_secret: impl FnMut(&str) -> Option<String>,
    ) -> Result<(), String> {
        self.stop(id);
        match self.start(id, cfg, resolve_secret) {
            Ok(()) => Ok(()),
            Err(e) => {
                let mut inner = self.inner.lock().expect("registry mutex poisoned");
                inner.errors.insert(id.to_string(), e.clone());
                Err(e)
            }
        }
    }

    /// Snapshot the union of every running server's tools, namespaced
    /// for safe exposure to the model. Empty when no servers are
    /// running.
    pub fn remote_tools(&self) -> Vec<RemoteToolDescriptor> {
        let inner = self.inner.lock().expect("registry mutex poisoned");
        let mut out = Vec::new();
        for (id, server) in &inner.running {
            for t in &server.tools {
                let wire_name = namespaced_wire_name(id, &t.name);
                out.push(RemoteToolDescriptor {
                    wire_name,
                    display_name: format!("{id}::{}", t.name),
                    server: id.clone(),
                    tool: t.name.clone(),
                    description: t.description.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        out
    }
}

/// Mangle `<server>::<tool>` into an Anthropic-compatible wire name
/// (`^[a-zA-Z0-9_-]{1,64}$`). When the natural `<server>__<tool>`
/// form fits, return it verbatim. Otherwise truncate and append an
/// 8-hex blake3 suffix so two long names that share a prefix don't
/// collide.
pub fn namespaced_wire_name(server: &str, tool: &str) -> String {
    fn sanitise(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }
    let sserver = sanitise(server);
    let stool = sanitise(tool);
    let natural = format!("{sserver}__{stool}");
    if natural.len() <= 64 {
        return natural;
    }
    // 64 = prefix + "_" + 8 hex chars  →  prefix = 55. Split between
    // server and tool so neither side disappears entirely; leave at
    // least 8 chars to the tool name.
    let suffix = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(server.as_bytes());
        hasher.update(b"::");
        hasher.update(tool.as_bytes());
        let hex = hasher.finalize().to_hex();
        hex.as_str()[..8].to_string()
    };
    let budget = 64 - 1 - suffix.len(); // 55
    let tool_keep = stool.len().min(budget.saturating_sub(2)); // leave >= 2 chars for server
    let server_keep = budget.saturating_sub(tool_keep + 2);
    let trimmed_server: String = sserver.chars().take(server_keep).collect();
    let trimmed_tool: String = stool.chars().take(tool_keep).collect();
    format!("{trimmed_server}__{trimmed_tool}_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaced_short_name_round_trip() {
        assert_eq!(
            namespaced_wire_name("github", "create_issue"),
            "github__create_issue"
        );
    }

    #[test]
    fn namespaced_sanitises_unsafe_chars() {
        let out = namespaced_wire_name("@scope/pkg", "do.it");
        assert!(out
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        // The natural form `_scope_pkg__do_it` is well within 64.
        assert!(out.contains("__"));
    }

    #[test]
    fn namespaced_truncates_with_hash_when_too_long() {
        let server = "a".repeat(60);
        let tool = "b".repeat(60);
        let out = namespaced_wire_name(&server, &tool);
        assert!(out.len() <= 64, "got {} chars: {}", out.len(), out);
        assert!(out.contains("__"));
        assert!(out.split('_').last().unwrap().len() == 8);
    }

    #[test]
    fn namespaced_collisions_are_unlikely() {
        // Two long server names with the same prefix must differ in
        // the wire name thanks to the blake3 suffix.
        let big = "a".repeat(60);
        let a = namespaced_wire_name(&format!("{big}1"), "tool");
        let b = namespaced_wire_name(&format!("{big}2"), "tool");
        assert_ne!(a, b);
    }
}
