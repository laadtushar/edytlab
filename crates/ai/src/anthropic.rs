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
    /// `output_config: {"effort": ...}`. `None` leaves it out of the
    /// body entirely, so a request with no effort set is byte-for-byte
    /// what it was before the setting existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
}

/// How hard the model should work on a response: the Messages API's
/// `output_config.effort`.
///
/// The wire strings are exact. The API answers anything else with a 400
/// (`output_config.effort: Input should be 'low', 'medium', 'high',
/// 'xhigh' or 'max'`), and a top-level `effort` with "Extra inputs are
/// not permitted", so the field is nested and the variants serialise to
/// precisely these names.
///
/// Higher effort is slower and spends more output tokens, some of them
/// on thinking the model does before it answers. Leaving it unset uses
/// the model's own default, which is what everyone gets today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    /// Every level, lowest first.
    pub const ALL: [Effort; 5] = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];

    /// The string the API (and the keychain slot) uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }

    /// Parse the wire string. Anything that is not exactly one of the
    /// five names is `None`: a stored value from some future build, or a
    /// hand-edited one, must read as "unset" rather than reach the API
    /// and fail every request with a 400.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s.trim())
    }

    /// The smallest `max_tokens` a request at this effort should carry.
    ///
    /// `max_tokens` caps everything the model writes, thinking
    /// included, and above `low` the model thinks before it answers.
    /// Against the live API a plan request capped at 1024 tokens came
    /// back at `medium` truncated half the time and at `high` and above
    /// as a thinking block with no text at all (`stop_reason:
    /// max_tokens`); at 4096 `max` was still seen spending 4766. So a
    /// cap that is plenty at the default is a failed request here, and
    /// the setting would trade a better answer for none.
    pub fn min_max_tokens(self) -> u32 {
        match self {
            Effort::Low => 0,
            Effort::Medium => 4096,
            Effort::High => 8192,
            Effort::XHigh | Effort::Max => 16384,
        }
    }
}

/// `base`, raised to what `effort` needs. Unset leaves it alone.
pub fn max_tokens_for(base: u32, effort: Option<Effort>) -> u32 {
    effort.map_or(base, |e| base.max(e.min_max_tokens()))
}

/// The `output_config` request field. Today it carries only `effort`.
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub struct OutputConfig {
    pub effort: Effort,
}

impl OutputConfig {
    pub fn effort(effort: Effort) -> Self {
        Self { effort }
    }
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
    /// The model's thinking, as streamed back before its answer.
    ///
    /// A higher reasoning effort makes the model think first, and the
    /// API sends that as a `thinking` block ahead of the text and tool
    /// calls. Kept so the next request replays it exactly as received:
    /// the `signature` is what lets the server accept it back, and
    /// dropping it costs the model its own reasoning across a tool
    /// round-trip. Never shown to the user and never sent to a provider
    /// that did not produce it.
    Thinking {
        thinking: String,
        signature: String,
    },
    /// A thinking block the server withheld. Opaque: it is replayed
    /// verbatim and never read.
    RedactedThinking {
        data: String,
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
    // populated by serde from the SSE envelope; reserved for logging/telemetry
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
        // populated by serde; text content arrives via TextDelta events instead
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        // populated by serde; args stream in via InputJsonDelta events instead
        input: Value,
    },
    /// Starts empty; the text arrives as `thinking_delta`s and the
    /// signature as one `signature_delta`. Every field defaults, so a
    /// block shaped a little differently from today's still parses
    /// instead of ending the turn on a protocol error.
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    RedactedThinking {
        #[serde(default)]
        data: String,
    },
    /// Any block type this client does not model. Skipped, along with
    /// its deltas.
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
    ThinkingDelta {
        #[serde(default)]
        thinking: String,
    },
    SignatureDelta {
        #[serde(default)]
        signature: String,
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
    // populated by serde from the API error envelope; message is what callers display
    pub kind: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The API takes exactly these five strings and answers anything
    /// else with a 400, so the spelling is the contract.
    #[test]
    fn effort_serialises_as_the_exact_wire_strings() {
        let wire = [
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::XHigh, "xhigh"),
            (Effort::Max, "max"),
        ];
        assert_eq!(wire.len(), Effort::ALL.len(), "a level is untested");
        for (effort, text) in wire {
            assert_eq!(serde_json::to_value(effort).unwrap(), json!(text));
            assert_eq!(effort.as_str(), text);
            assert_eq!(
                serde_json::from_value::<Effort>(json!(text)).unwrap(),
                effort
            );
        }
        for bad in ["extra high", "XHigh", "ultra", ""] {
            assert!(serde_json::from_value::<Effort>(json!(bad)).is_err());
        }
    }

    fn request(output_config: Option<OutputConfig>) -> Value {
        let messages = [Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        }];
        serde_json::to_value(MessagesRequest {
            model: "claude-sonnet-4-6",
            max_tokens: 16,
            system: Vec::new(),
            messages: &messages,
            tools: None,
            tool_choice: None,
            stream: true,
            output_config,
        })
        .unwrap()
    }

    /// Nested under `output_config`; a top-level `effort` is rejected by
    /// the API ("Extra inputs are not permitted").
    #[test]
    fn a_set_effort_is_nested_under_output_config() {
        let body = request(Some(OutputConfig::effort(Effort::XHigh)));
        assert_eq!(body["output_config"], json!({ "effort": "xhigh" }));
        assert!(body.get("effort").is_none());
    }

    /// Unset must leave no trace: the body is what it was before the
    /// setting existed, key for key.
    #[test]
    fn an_unset_effort_leaves_the_body_as_it_was() {
        assert_eq!(
            request(None),
            json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 16,
                "system": [],
                "messages": [{ "role": "user", "content": [{ "type": "text", "text": "hi" }] }],
                "stream": true
            })
        );
    }

    #[test]
    fn a_set_effort_raises_the_token_cap_and_never_lowers_it() {
        assert_eq!(max_tokens_for(1024, None), 1024);
        assert_eq!(max_tokens_for(1024, Some(Effort::Low)), 1024);
        assert_eq!(max_tokens_for(1024, Some(Effort::Medium)), 4096);
        assert_eq!(max_tokens_for(1024, Some(Effort::High)), 8192);
        assert_eq!(max_tokens_for(1024, Some(Effort::XHigh)), 16384);
        assert_eq!(max_tokens_for(1024, Some(Effort::Max)), 16384);
        // A cap that is already larger stays.
        assert_eq!(max_tokens_for(100_000, Some(Effort::Max)), 100_000);
        // And the floors rise with the level.
        let floors: Vec<u32> = Effort::ALL.iter().map(|e| e.min_max_tokens()).collect();
        assert!(floors.windows(2).all(|w| w[0] <= w[1]), "{floors:?}");
    }

    /// Replayed as received: the server checks the signature.
    #[test]
    fn thinking_blocks_serialise_in_the_shape_the_api_sends_them() {
        let blocks = vec![
            ContentBlock::Thinking {
                thinking: "hmm".into(),
                signature: "SIG".into(),
            },
            ContentBlock::RedactedThinking {
                data: "DATA".into(),
            },
        ];
        assert_eq!(
            serde_json::to_value(&blocks).unwrap(),
            json!([
                { "type": "thinking", "thinking": "hmm", "signature": "SIG" },
                { "type": "redacted_thinking", "data": "DATA" }
            ])
        );
    }
}
