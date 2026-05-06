//! Snapshot test for the system prompt.
//!
//! The committed `tests/snapshots/system_prompt.snap` captures the
//! exact byte content of `prompts/system.md`. Any prompt change must
//! also update the snapshot, which forces reviewers to look at it.
//!
//! We avoid `insta` here to keep the dev-dependency footprint small —
//! the comparison is just byte equality with a clear error message.

const SYSTEM_PROMPT_FILE: &str = include_str!("../prompts/system.md");
const SYSTEM_PROMPT_SNAPSHOT: &str = include_str!("snapshots/system_prompt.snap");

#[test]
fn system_prompt_matches_snapshot() {
    assert_eq!(
        SYSTEM_PROMPT_FILE, SYSTEM_PROMPT_SNAPSHOT,
        "system prompt drifted from snapshot. If this change is intentional, \
         update tests/snapshots/system_prompt.snap to match prompts/system.md."
    );
}

#[test]
fn system_prompt_constant_matches_file() {
    // The crate exposes the prompt as a constant via `include_str!`.
    // This guard catches a refactor that accidentally hard-codes a
    // copy in the source.
    assert_eq!(ai::prompt::SYSTEM_PROMPT, SYSTEM_PROMPT_FILE);
}

#[test]
fn voice_mode_prompt_loads() {
    assert!(!ai::prompt::VOICE_MODE_PROMPT.is_empty());
}
