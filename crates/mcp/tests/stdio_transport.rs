//! End-to-end tests for the stdio JSON-RPC transport.
//!
//! Each test drives a real child process — `sh` scripts standing in for
//! MCP servers — so the framing, the deadline, and the stderr capture
//! are exercised the way a genuine server would exercise them. No mocks:
//! the failure modes we care about (a server that never answers, one
//! that dies during the handshake) only reproduce with a real pipe.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mcp::StdioClient;

/// Spawn `sh -c <script>` as if it were an MCP server.
fn spawn_script(script: &str) -> mcp::Result<StdioClient> {
    StdioClient::spawn(
        "sh",
        &["-c".to_string(), script.to_string()],
        &HashMap::new(),
        |_| None,
    )
}

/// A server that answers `initialize` correctly, then `tools/list`.
/// `sh` reads one request line at a time and replies with a canned
/// frame, which is all the client needs to complete a handshake.
const WELL_BEHAVED: &str = r#"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"test-server"}}}\n' ;;
    *'"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo it","inputSchema":{"type":"object"}}]}}\n' ;;
  esac
done
"#;

#[test]
fn initialize_and_list_tools_round_trip() {
    let mut client = spawn_script(WELL_BEHAVED).expect("spawn");

    let name = client.initialize().expect("initialize");
    assert_eq!(name.as_deref(), Some("test-server"));

    let tools = client.list_tools().expect("tools/list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert_eq!(tools[0].description, "Echo it");
}

/// The regression this suite exists for: a server that accepts the
/// request and never answers must fail on the deadline instead of
/// blocking forever. Before the reader-thread change, `read_line`
/// blocked with no timeout — one wedged server held the registry mutex
/// and, transitively, the dispatcher and the whole agent loop.
///
/// Run with a short handshake budget so the test does not wait out the
/// real 60 s; what it proves is that the budget is honoured at all.
#[test]
fn unresponsive_server_times_out_instead_of_hanging() {
    // Reads stdin so the pipe stays open, but never writes a reply.
    let mut client = spawn_script("while IFS= read -r line; do :; done")
        .expect("spawn")
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(1));

    let started = Instant::now();
    let err = client
        .initialize()
        .expect_err("must not succeed against a silent server");
    let elapsed = started.elapsed();

    let msg = err.to_string();
    assert!(
        msg.contains("did not respond within 2s"),
        "expected a timeout on the handshake budget, got: {msg}"
    );
    // Generous slack for a loaded CI box, while still failing loudly if
    // the deadline is ignored entirely.
    assert!(
        elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(20),
        "timeout took {elapsed:?} — deadline is not being honoured"
    );
}

/// #340. The handshake has its own budget, longer than a request's,
/// because the first answer carries the server's whole cold start. Held
/// to the request budget, a server that simply took a while to start —
/// `npx` fetching its package, or Git-Bash's `sh` on a loaded Windows
/// runner — failed its first handshake and nothing else.
///
/// Here the server needs 2 s to start, the handshake may take 6 s and a
/// request 1 s: the handshake must succeed, and a request afterwards must
/// still be held to the shorter budget.
#[test]
fn a_slow_start_gets_the_handshake_budget_not_the_request_budget() {
    let mut client = spawn_script(
        r#"
sleep 2
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"slow-start"}}}\n' ;;
  esac
done
"#,
    )
    .expect("spawn")
    .with_timeouts(Duration::from_secs(6), Duration::from_secs(1));

    let name = client
        .initialize()
        .expect("a 2 s start is inside the 6 s handshake budget");
    assert_eq!(name.as_deref(), Some("slow-start"));

    // This server never answers `tools/list`.
    let started = Instant::now();
    let err = client
        .list_tools()
        .expect_err("tools/list is never answered");
    let elapsed = started.elapsed();
    assert!(
        err.to_string().contains("did not respond within 1s"),
        "expected a timeout on the request budget, got: {err}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "tools/list took {elapsed:?}: it must use the request budget, not the handshake's"
    );
}

/// A server that exits immediately closes stdout. The client must
/// report that rather than wait out the full deadline.
#[test]
fn server_that_exits_reports_closed_stdout() {
    let mut client = spawn_script("exit 0").expect("spawn");

    let started = Instant::now();
    let err = client.initialize().expect_err("must fail");
    let elapsed = started.elapsed();

    // Whether the failure surfaces as EOF on stdout or `EPIPE` on the
    // write depends on which side of the race the child loses, and both
    // mean the same thing. Assert on the condition, not the mechanism.
    assert!(
        err.to_string().contains("exited"),
        "expected a server-exited error, got: {err}"
    );
    assert!(
        elapsed.as_secs() < 10,
        "EOF should surface immediately, took {elapsed:?}"
    );
}

/// stderr used to be discarded, so a server that failed to start gave
/// the user nothing to act on. Its last lines are now folded into the
/// error message.
#[test]
fn stderr_is_surfaced_in_the_error_message() {
    let mut client =
        spawn_script("echo 'FATAL: GITHUB_TOKEN is not set' >&2; exit 1").expect("spawn");

    let err = client.initialize().expect_err("must fail");
    let msg = err.to_string();

    assert!(
        msg.contains("GITHUB_TOKEN is not set"),
        "stderr should be surfaced to the user, got: {msg}"
    );
}

/// Frames addressed to a different id (server-initiated notifications,
/// say) are skipped without consuming the caller's response.
#[test]
fn unrelated_frames_are_skipped() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","method":"notifications/message","params":{}}\n'
      printf '{"jsonrpc":"2.0","id":99,"result":{"ignored":true}}\n'
      printf '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"patient"}}}\n' ;;
  esac
done
"#;
    let mut client = spawn_script(script).expect("spawn");
    let name = client.initialize().expect("initialize");
    assert_eq!(name.as_deref(), Some("patient"));
}

/// A JSON-RPC error response surfaces as an `Err`, with the server's
/// own message preserved.
#[test]
fn jsonrpc_error_response_is_an_error() {
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"unsupported protocol version"}}\n' ;;
  esac
done
"#;
    let mut client = spawn_script(script).expect("spawn");
    let err = client.initialize().expect_err("must fail");
    assert!(
        err.to_string().contains("unsupported protocol version"),
        "server error message should be preserved, got: {err}"
    );
}

/// A missing `<keychain:slot>` secret fails before the child is
/// spawned, so the user gets the precise slot name rather than a
/// server that starts and then mysteriously misbehaves.
#[test]
fn unresolved_keychain_placeholder_fails_before_spawn() {
    let mut env = HashMap::new();
    env.insert("TOKEN".to_string(), "<keychain:absent_slot>".to_string());

    let result = StdioClient::spawn("sh", &["-c".to_string(), "true".to_string()], &env, |_| {
        None
    });
    let msg = match result {
        Ok(_) => panic!("must not spawn with an unresolved secret"),
        Err(e) => e.to_string(),
    };
    assert!(msg.contains("absent_slot"), "got: {msg}");
    assert!(msg.contains("TOKEN"), "got: {msg}");
}
