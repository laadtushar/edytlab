//! System prompt loading and model defaults.
//!
//! The prompt files live next to this crate's `Cargo.toml`. They are
//! embedded at compile time via [`include_str!`] so the binary doesn't
//! depend on a runtime resource layout.
//!
//! Phase 1 ships a single canonical voice/podcast prompt. The voice-mode
//! variant exists as a stub for forward-compat with the Phase 2 prompt
//! split.

/// Default Anthropic model name used when [`crate::AnthropicConfig`] does
/// not override it. Pinned to a stable Sonnet release id.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// Default Anthropic API base URL. Tests substitute a `wiremock` server.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Anthropic API version header value.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Maximum tool calls allowed per [`crate::Agent::turn`]. Hard cap; the
/// agent returns an error to the caller if exceeded.
pub const MAX_TOOL_CALLS_PER_TURN: usize = 10;

/// `max_tokens` used in outgoing Anthropic requests. Generous for chat
/// responses but cheap enough for Phase 1.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// The Phase 1 system prompt. Embedded at compile time; the snapshot
/// test in `tests/prompt_snapshot.rs` asserts this matches the file
/// committed under `prompts/system.md`.
pub const SYSTEM_PROMPT: &str = include_str!("../prompts/system.md");

/// Voice/podcast mode prompt stub. Phase 1 default; identical in spirit
/// to [`SYSTEM_PROMPT`].
pub const VOICE_MODE_PROMPT: &str = include_str!("../prompts/voice_mode.md");
