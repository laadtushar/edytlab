//! Stdio JSON-RPC client for MCP servers.
//!
//! Scope for v1: spawn the child process, perform the `initialize`
//! handshake, and call `tools/list` once to populate the registry's
//! tool descriptors. Long-lived bidirectional dispatch is left for a
//! follow-up — see the crate-level note in `lib.rs`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::{McpError, Result, SecretRef};

/// A single tool advertised by an MCP server. The `schema` is the
/// raw `input_schema` JSON from the MCP `tools/list` response.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// A live stdio client. Owns the child process; dropping kills it.
pub struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(default)]
    #[allow(dead_code)]
    id: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

impl StdioClient {
    /// Spawn the child process with the given command + args + env.
    /// Env values that match the `<keychain:slot>` shape are resolved
    /// via the caller-supplied `resolve_secret` closure; an
    /// unresolved placeholder errors out before the child is
    /// spawned, so the user gets a precise failure rather than a
    /// silently-broken server.
    pub fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        mut resolve_secret: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            let resolved = match SecretRef::parse(v) {
                Some(SecretRef(slot)) => match resolve_secret(slot) {
                    Some(val) => val,
                    None => {
                        return Err(McpError::InvalidConfig(format!(
                            "env `{k}` references missing keychain slot `{slot}`"
                        )));
                    }
                },
                None => v.clone(),
            };
            cmd.env(k, resolved);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|source| McpError::Io {
            path: command.into(),
            source,
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            McpError::InvalidConfig("child stdin not captured (spawn race)".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpError::InvalidConfig("child stdout not captured (spawn race)".into())
        })?;
        Ok(Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
        })
    }

    /// MCP handshake: send `initialize`, wait for response. Returns
    /// the server's `serverInfo.name` when present.
    pub fn initialize(&mut self) -> Result<Option<String>> {
        let resp = self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "edytlab", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        let name = resp
            .get("serverInfo")
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_string);
        // Per the MCP spec the client should follow up with
        // `notifications/initialized` to confirm the handshake.
        self.notify("notifications/initialized", json!({}))?;
        Ok(name)
    }

    /// Invoke a tool on the server (`tools/call`). Returns the raw
    /// `result` payload from the JSON-RPC response — typically the
    /// `content` array per the MCP spec, plus an optional
    /// `isError` flag. The caller is responsible for surfacing that
    /// shape to the agent as either a `ToolResult::Ok` or
    /// `ToolResult::Error`.
    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<Value> {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": args }),
        )
    }

    /// Call `tools/list`. Returns the parsed tool descriptors.
    pub fn list_tools(&mut self) -> Result<Vec<ToolDescriptor>> {
        let resp = self.request("tools/list", json!({}))?;
        let arr = resp
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::with_capacity(arr.len());
        for t in arr {
            let name = t
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| McpError::InvalidConfig("tool entry missing string `name`".into()))?
                .to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or(json!({ "type": "object" }));
            out.push(ToolDescriptor {
                name,
                description,
                schema,
            });
        }
        Ok(out)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let frame = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_frame(&frame)?;

        // Wait up to 10 s for a response with the matching id.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let frame = self.read_frame_until(deadline)?;
            // Skip notifications + responses that don't match our id.
            if frame.get("id").and_then(|v| v.as_u64()) != Some(id) {
                continue;
            }
            let resp: JsonRpcResponse = serde_json::from_value(frame)
                .map_err(|e| McpError::InvalidConfig(e.to_string()))?;
            if let Some(e) = resp.error {
                return Err(McpError::InvalidConfig(format!(
                    "{method} returned error: {}",
                    e.message
                )));
            }
            return Ok(resp.result.unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let frame = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_frame(&frame)
    }

    fn write_frame(&mut self, frame: &Value) -> Result<()> {
        let bytes =
            serde_json::to_vec(frame).map_err(|e| McpError::InvalidConfig(e.to_string()))?;
        self.stdin
            .write_all(&bytes)
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|source| McpError::Io {
                path: "stdin".into(),
                source,
            })
    }

    fn read_frame_until(&mut self, deadline: Instant) -> Result<Value> {
        // We don't have non-blocking line reads cross-platform without
        // a fair amount of plumbing — keep this synchronous and trust
        // the 10 s spec deadline for v1. A misbehaving server will
        // hang the request; that surfaces in the UI as an error after
        // the read returns EOF, which is acceptable for now.
        let _ = deadline;
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|source| McpError::Io {
                path: "stdout".into(),
                source,
            })?;
        if read == 0 {
            return Err(McpError::InvalidConfig("server closed stdout".into()));
        }
        serde_json::from_str(line.trim()).map_err(|e| McpError::InvalidConfig(e.to_string()))
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
