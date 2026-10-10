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

/// `max_tokens` for one model step of a turn, whichever provider it goes to.
///
/// It caps everything the model writes in the step, *thinking included*.
/// Anthropic's 5.x models (Sonnet 5.5, Opus 5.5, Haiku 5.5, Fable 5.1) think
/// by default, whether or not the request asks, and OpenAI counts reasoning
/// tokens against `max_completion_tokens` too
/// (<https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create>).
/// A cap sized for a model that answers straight away (this was 4096) is
/// spent on reasoning before the first word of the reply, or before the tool
/// call the step was for. 8192 is what [`crate::anthropic::Effort::High`]
/// raises the cap to for an explicit `high`, and Sonnet 5.5 defaults to
/// `high`. `max_tokens` is a ceiling and not a charge: a step that needs
/// less pays for less.
///
/// The one number goes to every provider, and not every model can write that
/// much: OpenAI answers a request above a model's output limit with a 400
/// instead of lowering it, and its older models stop at 4,096. So it is a
/// cap on what a request may ask for, and each request lowers it to the limit
/// of its own model where [`crate::models::max_output_tokens`] knows one
/// (every chat-completions body, in `OpenAIProvider` and `one_shot_body`).
/// Raising this constant further means checking that table first.
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

/// `max_tokens` for the plan request. The plan is the main model's own
/// work, so it needs the room a step has: on a model that thinks first, a
/// cap of 1024 came back as a thinking block with no text, which reads as
/// "the model returned no plan". Clamped per model like a step's. See
/// [`DEFAULT_MAX_TOKENS`].
pub const PLAN_MAX_TOKENS: u32 = 8192;

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
