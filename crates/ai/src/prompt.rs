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

/// Most tool calls the model may make in one [`crate::Agent::turn`].
///
/// Protection against a model that never stops calling tools, not an
/// error: a normal request (one mastering pass is 7 to 10 calls) stays
/// well under it. The model is told the number in its system prompt
/// ([`tool_budget_line`]). When a step would go past it, that step is not
/// run, each of its calls is answered "not run", and the model gets one
/// last request with tools off to say what was done and what is left. The
/// turn then ends normally (#439). The edits already made stay.
pub const MAX_TOOL_CALLS_PER_TURN: usize = 20;

/// The line of the system prompt that tells the model its tool budget.
///
/// Built from [`MAX_TOOL_CALLS_PER_TURN`] rather than written into
/// `prompts/system.md`, so the number the prompt states and the number the
/// loop enforces cannot drift apart. Without it the model plans as if
/// there were no limit and meets the cap on its last call (#439).
pub(crate) fn tool_budget_line() -> String {
    format!(
        "You can make at most {MAX_TOOL_CALLS_PER_TURN} tool calls per request, and calls past \
         that are not run. Use the fewest that do the job; for a bigger job, do the first \
         part, then say what is left so the user can ask you to continue."
    )
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_line_states_the_cap_the_loop_enforces() {
        let line = tool_budget_line();
        assert!(
            line.contains(&format!("at most {MAX_TOOL_CALLS_PER_TURN} tool calls")),
            "the prompt does not state the cap: {line:?}"
        );
        assert!(
            !line.contains('\n'),
            "it is one line of the prompt: {line:?}"
        );
    }
}
