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
/// `agent://tool-call-end` — a previously-started tool call finished, or
/// will never run. Forwards the `ok` flag so the UI can mark badges
/// error / success without waiting for the next `node-created`, and
/// `not_run` for a call that was announced but never dispatched (a
/// declined or unanswered held step, or one over the tool budget), which
/// is neither.
pub const TOOL_CALL_END: &str = "agent://tool-call-end";
/// `agent://node-created` — a tool call produced a new session node.
pub const NODE_CREATED: &str = "agent://node-created";
/// `agent://done` — the turn finished (success).
pub const DONE: &str = "agent://done";
/// `agent://plan` — a plan, or the held first edit, awaiting frontend
/// approval. The held edit (#415) is the first step that would change the
/// session when Plan first is on and the model wrote no plan; its steps
/// are that step's concrete tool calls.
pub const PLAN: &str = "agent://plan";
/// The user declined a plan or a held edit. The turn ended having run
/// nothing that changes the session. For a held edit, the calls had
/// already been announced, so their `agent://tool-call-end`
/// (`ok: false, not_run: true`) comes first and there is no
/// `agent://done`.
pub const PLAN_REJECTED: &str = "agent://plan-rejected";
/// A plan was asked for and none arrived (#267). Distinct from receiving
/// no `agent://plan` at all, which means no plan was requested for this
/// turn. `first_edit_held` says whether the turn will now hold its first
/// edit for approval (#415) or proceed without a gate.
pub const PLAN_UNAVAILABLE: &str = "agent://plan-unavailable";

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
pub struct ToolCallEndPayload {
    pub id: String,
    pub ok: bool,
    /// True when the call was announced but never dispatched: the user
    /// declined or reworded the held step, did not answer, or the step
    /// would have gone past the tool budget. `ok` is then false and there
    /// is no `view`. Always serialised, so listeners never have to tell
    /// "absent" from "false".
    pub not_run: bool,
    /// The drawable part of the tool's result, when it has one — see
    /// [`ai::ToolView`]. Serialises as a `type`-tagged object, or is
    /// omitted entirely so listeners see `undefined` rather than `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<ai::ToolView>,
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

/// `agent://plan` — emitted before tool execution when a plan, or the
/// first edit, needs approval. The frontend renders an approval card and
/// calls `approve_plan` or `reject_plan` to unblock the agent loop.
#[derive(Debug, Clone, Serialize)]
pub struct PlanPayload {
    pub steps: Vec<serde_json::Value>,
}

/// Why no plan arrived. Carries the failure class so the composer can say
/// what happened rather than only that it happened, and whether the first
/// edit will be held for approval instead.
#[derive(Debug, Clone, Serialize)]
pub struct PlanUnavailablePayload {
    pub reason: String,
    /// True when Plan first is on and the turn will hold its first edit
    /// for approval; false when it proceeds without a gate.
    pub first_edit_held: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend reads `first_edit_held` to choose between "the first
    /// edit will be shown for your approval" and "continuing without",
    /// so the field name on the wire is part of the contract (#415).
    #[test]
    fn plan_unavailable_carries_whether_the_first_edit_is_held() {
        let held = serde_json::to_value(PlanUnavailablePayload {
            reason: "the model did not return a plan".to_string(),
            first_edit_held: true,
        })
        .expect("serialises");
        assert_eq!(held["first_edit_held"], serde_json::json!(true));
        assert_eq!(held["reason"], "the model did not return a plan");

        let free = serde_json::to_value(PlanUnavailablePayload {
            reason: "x".to_string(),
            first_edit_held: false,
        })
        .expect("serialises");
        assert_eq!(free["first_edit_held"], serde_json::json!(false));
    }

    /// The badge reads "not run" only when `not_run` is true, and an
    /// absent field would be indistinguishable from an older backend, so
    /// the flag is always on the wire, and a call that never ran carries
    /// no `view`.
    #[test]
    fn tool_call_end_says_whether_the_call_ran() {
        let declined = serde_json::to_value(ToolCallEndPayload {
            id: "t1".to_string(),
            ok: false,
            not_run: true,
            view: None,
        })
        .expect("serialises");
        assert_eq!(declined["id"], "t1");
        assert_eq!(declined["ok"], serde_json::json!(false));
        assert_eq!(declined["not_run"], serde_json::json!(true));
        assert!(declined.get("view").is_none(), "{declined}");

        let failed = serde_json::to_value(ToolCallEndPayload {
            id: "t2".to_string(),
            ok: false,
            not_run: false,
            view: None,
        })
        .expect("serialises");
        assert_eq!(failed["ok"], serde_json::json!(false));
        assert_eq!(failed["not_run"], serde_json::json!(false));
    }
}
