//! HTTP JSON-RPC client for remote MCP servers.
//!
//! Implements the Streamable HTTP transport: every JSON-RPC request is
//! a POST to a single endpoint, and the server answers with either
//! `application/json` or an `text/event-stream` carrying the response
//! as SSE frames. Both shapes are accepted, because which one you get
//! is the server's choice and can vary per request.
//!
//! The older two-endpoint HTTP+SSE transport (GET a stream, read an
//! `endpoint` event, POST elsewhere) is deliberately not implemented —
//! it was superseded, and supporting it would mean maintaining a second
//! connection dance for servers that are increasingly rare.
//!
//! **Why the IO runs on its own thread.** The registry is called from
//! `async` Tauri commands. `reqwest::blocking` builds its own runtime
//! and is documented not to be used from inside one, so all requests
//! are handed to a dedicated `std::thread` that owns the client, with
//! the caller waiting on a channel under the same deadline the stdio
//! transport uses. This mirrors `StdioClient` rather than introducing a
//! second concurrency pattern.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::{McpError, Result, SecretRef};
use crate::transport::{parse_tool_descriptors, ToolDescriptor, REQUEST_TIMEOUT};

/// Connect timeout, kept well inside [`REQUEST_TIMEOUT`] so a dead host
/// reports as unreachable rather than as a generic timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// One unit of work for the IO thread: a JSON-RPC frame to POST, and
/// the channel its response should come back on.
struct Job {
    body: Value,
    reply: Sender<Result<Value>>,
}

/// A remote MCP server reached over Streamable HTTP.
///
/// Dropping this closes the job channel, which ends the IO thread.
#[derive(Debug)]
pub struct HttpClient {
    jobs: Sender<Job>,
    next_id: u64,
}

impl HttpClient {
    /// Build a client and start its IO thread.
    ///
    /// Header values may use the `<keychain:slot>` placeholder, resolved
    /// through `resolve_secret` exactly as stdio `env` values are — an
    /// unresolved placeholder is an error before any request is sent, so
    /// a missing secret cannot surface later as a puzzling 401.
    pub fn connect(
        url: &str,
        headers: &HashMap<String, String>,
        mut resolve_secret: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(McpError::InvalidConfig(format!(
                "server url must start with http:// or https://, got `{url}`"
            )));
        }

        let mut resolved = reqwest::header::HeaderMap::new();
        for (name, raw) in headers {
            let value = match SecretRef::parse(raw) {
                Some(SecretRef(slot)) => resolve_secret(slot).ok_or_else(|| {
                    McpError::InvalidConfig(format!(
                        "header `{name}` references missing keychain slot `{slot}`"
                    ))
                })?,
                None => raw.clone(),
            };
            let header_name: reqwest::header::HeaderName = name
                .parse()
                .map_err(|_| McpError::InvalidConfig(format!("invalid header name `{name}`")))?;
            let mut header_value = reqwest::header::HeaderValue::from_str(&value)
                .map_err(|_| McpError::InvalidConfig(format!("invalid value for `{name}`")))?;
            // Auth material should not be echoed by anything that
            // debug-prints the header map.
            header_value.set_sensitive(true);
            resolved.insert(header_name, header_value);
        }

        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let url = url.to_string();

        // The client is built *on* the IO thread: `reqwest::blocking`
        // constructs a runtime, which must not happen on a thread that
        // may already be inside one.
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        std::thread::spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .default_headers(resolved)
                .build()
            {
                Ok(c) => {
                    let _ = ready_tx.send(Ok(()));
                    c
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(McpError::InvalidConfig(format!(
                        "could not build http client: {e}"
                    ))));
                    return;
                }
            };
            // Set by the server on `initialize`; every later request must
            // echo it or the server treats the call as a new session.
            let mut session_id: Option<String> = None;
            while let Ok(job) = jobs_rx.recv() {
                let outcome = post_once(&client, &url, &job.body, &mut session_id);
                // A dropped receiver just means the caller timed out and
                // moved on; the thread keeps serving later requests.
                let _ = job.reply.send(outcome);
            }
        });

        ready_rx
            .recv_timeout(CONNECT_TIMEOUT)
            .map_err(|_| McpError::InvalidConfig("http client thread failed to start".into()))??;

        Ok(Self {
            jobs: jobs_tx,
            next_id: 1,
        })
    }

    /// MCP handshake. Returns the server's advertised name, if any.
    pub fn initialize(&mut self) -> Result<Option<String>> {
        let resp = self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "edytlab", "version": env!("CARGO_PKG_VERSION") }
            }),
        )?;
        Ok(resp
            .get("serverInfo")
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_string))
    }

    pub fn list_tools(&mut self) -> Result<Vec<ToolDescriptor>> {
        let resp = self.request("tools/list", json!({}))?;
        parse_tool_descriptors(&resp)
    }

    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<Value> {
        self.request("tools/call", json!({ "name": name, "arguments": args }))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let (reply_tx, reply_rx) = mpsc::channel();
        self.jobs
            .send(Job {
                body,
                reply: reply_tx,
            })
            .map_err(|_| McpError::InvalidConfig("http client thread has stopped".into()))?;

        wait_for(reply_rx, method)
    }
}

fn wait_for(rx: Receiver<Result<Value>>, method: &str) -> Result<Value> {
    match rx.recv_timeout(REQUEST_TIMEOUT) {
        Ok(outcome) => outcome,
        Err(RecvTimeoutError::Timeout) => Err(McpError::InvalidConfig(format!(
            "{method} timed out after {}s",
            REQUEST_TIMEOUT.as_secs()
        ))),
        Err(RecvTimeoutError::Disconnected) => Err(McpError::InvalidConfig(format!(
            "{method} failed: http client thread stopped"
        ))),
    }
}

/// POST one JSON-RPC frame and return its `result`.
fn post_once(
    client: &reqwest::blocking::Client,
    url: &str,
    body: &Value,
    session_id: &mut Option<String>,
) -> Result<Value> {
    let mut req = client
        .post(url)
        // Declaring both tells a Streamable HTTP server it may answer
        // with a plain JSON body or upgrade to an event stream.
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if let Some(sid) = session_id.as_deref() {
        req = req.header("Mcp-Session-Id", sid);
    }

    let response = req
        .json(body)
        .send()
        .map_err(|e| McpError::InvalidConfig(format!("request failed: {e}")))?;

    if let Some(sid) = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
    {
        *session_id = Some(sid.to_string());
    }

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    let text = response
        .text()
        .map_err(|e| McpError::InvalidConfig(format!("could not read response body: {e}")))?;

    if !status.is_success() {
        // The body usually explains far more than the status code, so
        // carry a bounded slice of it into the error the user sees.
        let detail: String = text.chars().take(300).collect();
        return Err(McpError::InvalidConfig(format!(
            "HTTP {status}{}",
            if detail.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", detail.trim())
            }
        )));
    }

    let frame: Value = if content_type.contains("text/event-stream") {
        first_sse_data(&text).ok_or_else(|| {
            McpError::InvalidConfig("event stream contained no JSON-RPC message".into())
        })?
    } else {
        serde_json::from_str(&text)
            .map_err(|e| McpError::InvalidConfig(format!("malformed JSON response: {e}")))?
    };

    if let Some(err) = frame.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(McpError::InvalidConfig(format!(
            "server returned error: {message}"
        )));
    }

    Ok(frame.get("result").cloned().unwrap_or(Value::Null))
}

/// Pull the first JSON-RPC message out of an SSE body.
///
/// Only `data:` lines carry payload; `event:`, `id:`, comments (`:`)
/// and blank separators are skipped. A single logical message may span
/// several `data:` lines, which the spec says to join with newlines.
fn first_sse_data(body: &str) -> Option<Value> {
    let mut buffer = String::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            continue;
        }
        // A blank line terminates the event: try what we have.
        if line.trim().is_empty() && !buffer.is_empty() {
            if let Ok(v) = serde_json::from_str::<Value>(&buffer) {
                return Some(v);
            }
            buffer.clear();
        }
    }
    serde_json::from_str::<Value>(&buffer).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_body_with_one_event_parses() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let v = first_sse_data(body).expect("should parse");
        assert_eq!(v["result"]["ok"], json!(true));
    }

    /// A message split across `data:` lines is joined with newlines, per
    /// the SSE spec — getting this wrong yields invalid JSON.
    #[test]
    fn sse_body_joins_multiline_data() {
        let body = "data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":7,\"result\":42}\n\n";
        let v = first_sse_data(body).expect("should parse");
        assert_eq!(v["result"], json!(42));
        assert_eq!(v["id"], json!(7));
    }

    /// Comments and retry directives are noise and must not break the
    /// parse — servers emit them as keepalives.
    #[test]
    fn sse_body_ignores_comments_and_directives() {
        let body = ": keepalive\nretry: 1000\nid: 9\ndata: {\"result\":\"fine\"}\n\n";
        let v = first_sse_data(body).expect("should parse");
        assert_eq!(v["result"], json!("fine"));
    }

    #[test]
    fn sse_body_without_data_is_none() {
        assert!(first_sse_data(": just a comment\n\n").is_none());
    }

    #[test]
    fn connect_rejects_a_non_http_url() {
        let err = HttpClient::connect("ftp://example.com", &HashMap::new(), |_| None)
            .expect_err("must reject non-http scheme");
        assert!(err.to_string().contains("http://"), "got: {err}");
    }

    /// An unresolved secret is caught before any request leaves the
    /// process, so the failure names the missing slot instead of
    /// surfacing later as an opaque 401.
    #[test]
    fn connect_rejects_an_unresolved_keychain_placeholder() {
        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            "<keychain:missing>".to_string(),
        );
        let err = HttpClient::connect("https://example.com/mcp", &headers, |_| None)
            .expect_err("must reject a missing secret");
        assert!(err.to_string().contains("missing"), "got: {err}");
    }
}
