//! Anthropic Messages API: request/response types, SSE event parsing,
//! and a thin streaming HTTP client.
//!
//! We avoid the official SDK because we need fine control over the
//! streaming wire format and the tool-result conversation shape, and the
//! SDK's API is still moving. Hitting the HTTP endpoint directly with
//! `reqwest` keeps the surface small.
//!
//! Wire format references:
//! * Messages: <https://docs.anthropic.com/en/api/messages>
//! * Streaming: <https://docs.anthropic.com/en/api/messages-streaming>
//!
//! The streaming protocol emits SSE events with these `event:` names:
//! `message_start`, `content_block_start`, `content_block_delta`,
//! `content_block_stop`, `message_delta`, `message_stop`, `ping`,
//! `error`. The `data:` payload is JSON whose `type` field mirrors the
//! event name. We parse only the fields the agent loop needs; unknown
//! fields are ignored via `serde(other)` so server additions don't break
//! us.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Outgoing request body for `POST /v1/messages`.
///
/// `system` is a vector of typed text blocks (rather than a plain
/// string) so we can attach `cache_control: ephemeral` to the prompt.
/// The Anthropic API accepts both shapes; we always use the typed form.
#[derive(Debug, Serialize)]
pub struct MessagesRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub system: Vec<SystemBlock<'a>>,
    pub messages: &'a [Message],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    pub stream: bool,
}

/// `cache_control: { "type": "ephemeral" }` block — Anthropic's prompt
/// caching primitive. Attaching this to the system prompt and the tool
/// schema array marks them as a stable cacheable prefix. The cache TTL
/// (~5 min ephemeral, longer for `1h`) is server-managed; we use the
/// default ephemeral lifetime everywhere in Phase 1.
#[derive(Debug, Serialize)]
pub struct SystemBlock<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: &'static str,
}

impl CacheControl {
    pub const EPHEMERAL: Self = Self { kind: "ephemeral" };
}

/// `tool_choice` parameter. Phase 1 always uses `auto`.
#[derive(Debug, Serialize, Clone, Copy)]
pub struct ToolChoice {
    #[serde(rename = "type")]
    pub kind: &'static str,
}

impl ToolChoice {
    pub const AUTO: Self = Self { kind: "auto" };
}

/// One conversation message. Mirrors Anthropic's `messages` array entry.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Content blocks inside a message. Phase 1 uses `text`, `tool_use` and
/// `tool_result`; the other variants Anthropic supports (e.g. `image`,
/// `document`) are out of scope.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// `is_error` differentiates a successful tool result from a failure
    /// the model should retry. The Anthropic schema accepts a string OR
    /// an array of content blocks for `content`; we always send a string
    /// (JSON-encoded if structured) for simplicity.
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

// ---------------------------------------------------------------------
// Streaming events
// ---------------------------------------------------------------------

/// Top-level streaming event. Each SSE `data:` line carries one of
/// these. We use `#[serde(tag = "type")]` so the discriminator doubles
/// as the event name.
///
/// Unknown variants surface as [`StreamEvent::Other`] so a future server
/// addition (say, a new `message_pause` event) won't blow up the
/// parser.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: MessageMeta,
    },
    ContentBlockStart {
        index: u32,
        content_block: ContentBlockStart,
    },
    ContentBlockDelta {
        index: u32,
        delta: ContentBlockDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        delta: MessageDeltaInner,
    },
    MessageStop,
    Ping,
    Error {
        error: ApiError,
    },
    /// Catch-all for unknown event types — we don't fail the loop on
    /// server additions.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct MessageMeta {
    pub id: String,
    #[allow(dead_code)]
    pub model: String,
}

/// `content_block_start.content_block` payload. For text the server
/// sends an empty string; for tool_use the id, name, and an empty
/// `input` object are sent up-front and the args stream in via
/// `input_json_delta` events.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockStart {
    Text {
        #[allow(dead_code)]
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[allow(dead_code)]
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct MessageDeltaInner {
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiError {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub kind: String,
    pub message: String,
}
