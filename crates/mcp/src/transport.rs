//! Stdio JSON-RPC client for MCP servers.
//!
//! Spawns the child process, performs the `initialize` handshake,
//! discovers tools via `tools/list`, and serves `tools/call` for the
//! lifetime of the connection.
//!
//! Reads are deadline-bounded. `BufRead::read_line` blocks with no
//! timeout, and callers hold the registry mutex across a request, so
//! an inline read would let one unresponsive server wedge the
//! dispatcher and the agent loop with it. stdout and stderr are each
//! drained by a thread instead; requests wait on a channel.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::{McpError, Result, SecretRef};

/// How long a single JSON-RPC request waits for its matching response
/// before giving up. The MCP spec has no mandated value; 10 s is long
/// enough for a cold `npx` server to answer `initialize` and short
/// enough that a wedged server doesn't look like a hang to the user.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Number of trailing stderr lines retained per server for diagnostics.
const STDERR_TAIL_LINES: usize = 20;

/// How long an error path waits for the stderr drain thread to catch
/// up before giving up on including a diagnosis.
const STDERR_GRACE: Duration = Duration::from_millis(250);

/// Poll interval while waiting out [`STDERR_GRACE`].
const STDERR_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A single tool advertised by an MCP server. The `schema` is the
/// raw `input_schema` JSON from the MCP `tools/list` response.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// A live stdio client. Owns the child process; dropping kills it.
///
/// stdout and stderr are each drained by a dedicated thread.
/// `BufRead::read_line` has no timeout of its own, so reading inline
/// would let a server that accepts a request and never answers block
/// the caller forever — and because callers hold the registry (and
/// transitively the dispatcher) mutex across a request, that single
/// hung server would wedge the whole app. Moving reads onto a thread
/// lets [`StdioClient::read_frame_until`] honour a real deadline.
pub struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    /// Lines from the child's stdout, newest last. Disconnects when the
    /// child closes stdout or the reader thread hits an IO error.
    stdout_rx: Receiver<String>,
    /// Ring buffer of the child's most recent stderr lines. MCP servers
    /// report missing env vars, auth failures, and crash traces there,
    /// so it is folded into error messages rather than discarded.
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(default)]
    // populated by serde for protocol completeness; not inspected after parsing
    // required by JSON-RPC spec; value is only needed for request correlation
    #[allow(dead_code)]
    id: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    // required by JSON-RPC spec; only `message` is surfaced to callers
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
            .stderr(Stdio::piped());
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
        let stderr = child.stderr.take().ok_or_else(|| {
            McpError::InvalidConfig("child stderr not captured (spawn race)".into())
        })?;

        // Drain stdout on a thread so reads can be deadline-bounded.
        // Both threads exit on EOF, which `Drop` guarantees by killing
        // the child.
        let (tx, stdout_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        // Receiver dropped: the client is gone, stop reading.
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "mcp: stdout read failed");
                        break;
                    }
                }
            }
        });

        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        {
            let tail = Arc::clone(&stderr_tail);
            let server = command.to_string();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    tracing::warn!(server = %server, line = %line, "mcp: server stderr");
                    // A poisoned tail is not worth panicking a drain
                    // thread over — diagnostics are best-effort.
                    if let Ok(mut t) = tail.lock() {
                        if t.len() == STDERR_TAIL_LINES {
                            t.pop_front();
                        }
                        t.push_back(line);
                    }
                }
            });
        }

        Ok(Self {
            child,
            stdin,
            stdout_rx,
            stderr_tail,
            next_id: 1,
        })
    }

    /// Trailing stderr rendered for inclusion in an error message, or
    /// `""` when the server has said nothing. Turns an opaque
    /// "server closed stdout" into something the user can act on.
    ///
    /// `grace` bounds how long to wait for the drain thread to catch
    /// up. A server that fails to start typically writes its diagnosis
    /// and exits immediately, and stdout EOF routinely beats the stderr
    /// drain — without a grace period the most useful message would be
    /// dropped exactly when it matters most. Callers pass
    /// [`Duration::ZERO`] when the server is known to still be alive.
    fn stderr_context(&self, grace: Duration) -> String {
        let deadline = Instant::now() + grace;
        loop {
            if let Ok(tail) = self.stderr_tail.lock() {
                if !tail.is_empty() {
                    let joined = tail
                        .iter()
                        .map(|l| l.trim_end())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    return format!("; last stderr: {joined}");
                }
            }
            if Instant::now() >= deadline {
                return String::new();
            }
            std::thread::sleep(STDERR_POLL_INTERVAL);
        }
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
        self.request("tools/call", json!({ "name": name, "arguments": args }))
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

        // One deadline for the whole exchange, not per read: a server
        // that streams unrelated notifications must not extend the
        // budget indefinitely.
        let deadline = Instant::now() + REQUEST_TIMEOUT;
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

    /// Read one JSON frame, giving up at `deadline`.
    ///
    /// The reader thread does the blocking work; here we only wait on
    /// the channel, so an unresponsive server costs at most the
    /// remaining budget rather than hanging forever.
    fn read_frame_until(&mut self, deadline: Instant) -> Result<Value> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let line = match self.stdout_rx.recv_timeout(remaining) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => {
                // The server is still alive and has had the whole
                // budget to write anything it wanted to; no grace
                // period needed.
                return Err(McpError::InvalidConfig(format!(
                    "server did not respond within {}s{}",
                    REQUEST_TIMEOUT.as_secs(),
                    self.stderr_context(Duration::ZERO)
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(McpError::InvalidConfig(format!(
                    "server closed stdout{}",
                    self.stderr_context(STDERR_GRACE)
                )));
            }
        };
        serde_json::from_str(line.trim()).map_err(|e| McpError::InvalidConfig(e.to_string()))
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
