//! Integration tests for the Streamable HTTP transport.
//!
//! These drive a real TCP listener rather than a mocking library: the
//! behaviour under test *is* HTTP framing and content-type handling, so
//! a fake that speaks the same abstraction would test nothing. The
//! server is deliberately minimal — it reads a request, matches on the
//! JSON-RPC `method`, and writes a canned response.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use mcp::HttpClient;

/// How a scripted server should answer one request.
#[derive(Clone)]
struct Reply {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

impl Reply {
    fn json(body: &str) -> Self {
        Self {
            status: "200 OK",
            content_type: "application/json",
            body: body.to_string(),
        }
    }
    fn sse(body: &str) -> Self {
        Self {
            status: "200 OK",
            content_type: "text/event-stream",
            body: body.to_string(),
        }
    }
}

/// Spawn a server that answers each request by looking up the JSON-RPC
/// `method` in `routes`. Returns its base URL and a receiver of the
/// raw request text, so tests can assert on what was actually sent.
fn scripted_server(routes: Vec<(&'static str, Reply)>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let request = read_request(&mut stream);
            let reply = routes
                .iter()
                .find(|(method, _)| request.contains(&format!("\"method\":\"{method}\"")))
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| {
                    Reply::json(
                        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no route"}}"#,
                    )
                });
            let _ = tx.send(request);
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nMcp-Session-Id: sess-1\r\nConnection: close\r\n\r\n{}",
                reply.status,
                reply.content_type,
                reply.body.len(),
                reply.body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (format!("http://127.0.0.1:{port}/mcp"), rx)
}

/// Read headers, then exactly `Content-Length` bytes of body.
fn read_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // Headers terminate at the blank line.
    while !buf.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => return String::from_utf8_lossy(&buf).to_string(),
            Ok(_) => buf.push(byte[0]),
        }
    }
    let headers = String::from_utf8_lossy(&buf).to_string();
    let len: usize = headers
        .lines()
        .find_map(|l| {
            l.strip_prefix("Content-Length: ")
                .or_else(|| l.strip_prefix("content-length: "))
        })
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 && stream.read_exact(&mut body).is_err() {
        return headers;
    }
    format!("{headers}{}", String::from_utf8_lossy(&body))
}

const INIT_OK: &str =
    r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"demo"},"capabilities":{}}}"#;

#[test]
fn initialize_and_list_tools_over_json() {
    let (url, _rx) = scripted_server(vec![
        ("initialize", Reply::json(INIT_OK)),
        (
            "tools/list",
            Reply::json(
                r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[
                    {"name":"search","description":"find things",
                     "inputSchema":{"type":"object","properties":{"q":{"type":"string"}}}}
                ]}}"#,
            ),
        ),
    ]);

    let mut client = HttpClient::connect(&url, &HashMap::new(), |_| None).expect("connect");
    let name = client.initialize().expect("initialize");
    assert_eq!(name.as_deref(), Some("demo"));

    let tools = client.list_tools().expect("tools/list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "search");
    // The schema must survive intact — the dispatcher advertises it to
    // the model, so dropping it would leave the tool uncallable.
    assert_eq!(tools[0].schema["properties"]["q"]["type"], "string");
}

/// The same exchange, but the server answers with an event stream. A
/// Streamable HTTP server may choose either shape per request.
#[test]
fn a_server_answering_with_an_event_stream_works_identically() {
    let (url, _rx) = scripted_server(vec![
        (
            "initialize",
            Reply::sse(&format!("event: message\ndata: {INIT_OK}\n\n")),
        ),
        (
            "tools/call",
            Reply::sse(
                "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n\n",
            ),
        ),
    ]);

    let mut client = HttpClient::connect(&url, &HashMap::new(), |_| None).expect("connect");
    client.initialize().expect("initialize over sse");
    let out = client
        .call_tool("anything", serde_json::json!({"x": 1}))
        .expect("tools/call over sse");
    assert_eq!(out["content"][0]["text"], "done");
}

/// A JSON-RPC `error` member must surface as an `Err`, not be handed
/// back as if it were a result — the agent would otherwise report
/// success to the model.
#[test]
fn a_json_rpc_error_becomes_an_err() {
    let (url, _rx) = scripted_server(vec![
        ("initialize", Reply::json(INIT_OK)),
        (
            "tools/call",
            Reply::json(
                r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"unknown tool bogus"}}"#,
            ),
        ),
    ]);

    let mut client = HttpClient::connect(&url, &HashMap::new(), |_| None).expect("connect");
    client.initialize().expect("initialize");
    let err = client
        .call_tool("bogus", serde_json::json!({}))
        .expect_err("must not report a protocol error as success");
    assert!(err.to_string().contains("unknown tool bogus"), "got: {err}");
}

/// An HTTP failure should carry the body, which is where servers put
/// the actual explanation.
#[test]
fn an_http_error_includes_the_response_body() {
    let (url, _rx) = scripted_server(vec![(
        "initialize",
        Reply {
            status: "401 Unauthorized",
            content_type: "application/json",
            body: r#"{"error":"token expired"}"#.to_string(),
        },
    )]);

    let mut client = HttpClient::connect(&url, &HashMap::new(), |_| None).expect("connect");
    let err = client.initialize().expect_err("401 must fail");
    let msg = err.to_string();
    assert!(msg.contains("401"), "got: {msg}");
    assert!(msg.contains("token expired"), "body should be shown: {msg}");
}

/// Configured headers must reach the server, with `<keychain:slot>`
/// resolved — this is how a remote server gets its bearer token.
#[test]
fn resolved_headers_are_sent_on_every_request() {
    let (url, rx) = scripted_server(vec![("initialize", Reply::json(INIT_OK))]);

    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        "<keychain:remote_token>".to_string(),
    );
    let mut client = HttpClient::connect(&url, &headers, |slot| {
        assert_eq!(slot, "remote_token");
        Some("Bearer super-secret".to_string())
    })
    .expect("connect");
    client.initialize().expect("initialize");

    let seen = rx.recv_timeout(Duration::from_secs(5)).expect("a request");
    assert!(
        seen.contains("Bearer super-secret"),
        "resolved header missing from request: {seen}"
    );
    // Both content types are advertised so the server may pick either.
    assert!(seen.contains("text/event-stream"), "accept header: {seen}");
}

/// A server that accepts the connection and never answers must not hang
/// the caller forever — the whole reason this transport runs its IO on
/// a thread. Bounded well under the 10s request deadline.
#[test]
fn a_silent_server_times_out_rather_than_hanging() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        // Accept and hold the connection open, answering nothing.
        let held: Vec<TcpStream> = listener.incoming().filter_map(|s| s.ok()).collect();
        thread::sleep(Duration::from_secs(60));
        drop(held);
    });

    let url = format!("http://127.0.0.1:{port}/mcp");
    let mut client = HttpClient::connect(&url, &HashMap::new(), |_| None).expect("connect");

    let started = std::time::Instant::now();
    let err = client.initialize().expect_err("must not hang");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "returned only after {:?}",
        started.elapsed()
    );
    let _ = err;
}
