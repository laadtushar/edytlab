//! Tauri events emitted by the AI agent.
//!
//! Each event has a stable name ("`agent://...`") and a typed payload
//! struct serialised with `serde`. Frontend listeners in
//! `apps/desktop/src/lib/tauri-bridge.ts` mirror these shapes; the field
//! names in JSON output are the source of truth for that alignment.
//!
//! All payload structs are kept simple (named fields, owned `String`s)
//! so mismatches between the Rust source and the hand-authored TS
//! bindings are easy to spot in code review.

use serde::Serialize;

/// `agent://text-delta` — a chunk of streamed assistant text.
pub const TEXT_DELTA: &str = "agent://text-delta";
/// `agent://tool-call` — the model started invoking a tool.
pub const TOOL_CALL: &str = "agent://tool-call";
/// `agent://node-created` — a tool call produced a new session node.
pub const NODE_CREATED: &str = "agent://node-created";
/// `agent://done` — the turn finished (success).
pub const DONE: &str = "agent://done";
/// `agent://plan` — mashup mode plan awaiting frontend approval.
pub const PLAN: &str = "agent://plan";

#[derive(Debug, Clone, Serialize)]
pub struct TextDeltaPayload {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallPayload {
    pub name: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeCreatedPayload {
    /// Hex-encoded session node id.
    pub node_id: String,
}

/// `agent://done` carries no payload; we still serialise an empty
/// object so the frontend's `listen<{}>(...)` call sees a valid
/// payload object rather than `undefined`.
#[derive(Debug, Clone, Serialize)]
pub struct DonePayload {}

/// `agent://plan` — emitted in mashup mode before tool execution.
/// The frontend renders an approval card and calls `approve_plan` to
/// unblock the agent loop.
#[derive(Debug, Clone, Serialize)]
pub struct PlanPayload {
    pub steps: Vec<serde_json::Value>,
}
