//! Which bundled skills fire for which prompts.
//!
//! The skills under `apps/desktop/src-tauri/resources/skills` are loaded
//! here exactly as the app loads them, and each prompt in the tables is
//! checked for the full set of skills that fire, so a pattern that grows
//! too wide (or an overlap between two skills) fails here.
//!
//! The trigger matches the lower-cased current message plus earlier user
//! turns (`TriggerContext`), so a loose word keeps a skill loaded for the
//! rest of a conversation. That is why every bundled skill matches whole
//! words with a `\b...\b` regex: the old keyword lists were substring
//! matches, and "speaker" loaded `acx-audiobook` through "peak", "terms"
//! through "rms", "human" loaded `noise-reducer` through "hum", "bandpass"
//! `music-mix` through "band" and "abandoned" `export-guide` through
//! "done". Each word belongs to one skill.

use std::collections::BTreeSet;
use std::path::PathBuf;

use skills::{SkillLibrary, Trigger, TriggerContext};

fn skills_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src-tauri/resources/skills")
}

fn library() -> SkillLibrary {
    SkillLibrary::load_from(&skills_dir()).expect("the bundled skills load")
}

fn firing(lib: &SkillLibrary, message: &str, history: &[String]) -> BTreeSet<String> {
    let ctx = TriggerContext {
        user_message: message,
        history,
        mode: None,
    };
    lib.matches(&ctx).iter().map(|s| s.name.clone()).collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// Prompts and the exact set of skills that fire for each.
const TABLE: &[(&str, &[&str])] = &[
    // Substring hits the keyword lists used to have: none may fire.
    ("Split by speaker and label them", &[]),
    ("What are the terms of this licence?", &[]),
    ("Mute the human voice", &[]),
    ("Apply a bandpass filter around 1 kHz", &[]),
    ("The abandoned clips are at the end", &[]),
    // Generic words that no skill owns any more.
    ("How loud is it?", &[]),
    ("Save my work", &[]),
    ("Check the peak and the rms", &[]),
    ("Add a track", &[]),
    // Real intents, one skill each.
    ("master this to -14 LUFS", &["loudness-master"]),
    ("Normalize it for Spotify", &["loudness-master"]),
    ("remove the hum", &["noise-reducer"]),
    ("Get rid of the hiss and buzz", &["noise-reducer"]),
    ("prepare this for ACX", &["acx-audiobook"]),
    (
        "Is my audiobook narration within the noise floor limit?",
        &["acx-audiobook"],
    ),
    ("clean up my podcast", &["podcast-cleanup"]),
    ("export the final mix", &["export-guide"]),
    ("Export it as flac", &["export-guide"]),
    ("Mix this song, the drums are too loud", &["music-mix"]),
    ("Process the lead vocals", &["vocal-chain"]),
    (
        "Tighten the dialogue in this interview",
        &["dialog-enhancer"],
    ),
    ("Remove the long pauses", &["silence-cleaner"]),
    // Real overlaps: both skills apply, and their limiter advice agrees
    // (see `no_bundled_skill_calls_the_baked_limiter_tool`).
    (
        "Mastering this song for Spotify",
        &["loudness-master", "music-mix"],
    ),
    (
        "Clean up the background noise in my podcast",
        &["noise-reducer", "podcast-cleanup"],
    ),
];

#[test]
fn each_prompt_fires_exactly_the_skills_the_table_names() {
    let lib = library();
    let mut problems = Vec::new();
    for (prompt, expected) in TABLE {
        let got = firing(&lib, prompt, &[]);
        if got != set(expected) {
            problems.push(format!("{prompt:?}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn a_follow_up_keeps_the_skill_the_first_turn_loaded() {
    let lib = library();
    let history = vec!["prepare this for ACX".to_string()];
    assert_eq!(
        firing(&lib, "ok, go ahead", &history),
        set(&["acx-audiobook"])
    );
    assert!(firing(&lib, "ok, go ahead", &[]).is_empty());
}

#[test]
fn every_bundled_skill_is_a_word_boundary_regex_with_a_prompt_that_fires_it() {
    let lib = library();
    assert!(!lib.skills().is_empty(), "no bundled skills found");

    let covered: BTreeSet<String> = TABLE
        .iter()
        .flat_map(|(_, names)| names.iter().map(|n| n.to_string()))
        .collect();

    for skill in lib.skills() {
        match &skill.trigger {
            Trigger::Regex(re) => assert!(
                re.as_str().contains(r"\b"),
                "{}: pattern {:?} has no word boundary",
                skill.name,
                re.as_str()
            ),
            other => panic!(
                "{}: bundled skills use `trigger: regex` with word boundaries, got {other:?}",
                skill.name
            ),
        }
        assert!(
            covered.contains(&skill.name),
            "{}: no prompt in TABLE is expected to fire it",
            skill.name
        );
    }
}

/// `limiter` bakes into one track's file before the track's gain; the
/// effect (`add_effect` kind=limiter) runs last in the chain. A skill that
/// calls the tool for a delivery limiter contradicts the others, and two
/// of them firing together would apply two limiters.
#[test]
fn no_bundled_skill_calls_the_baked_limiter_tool() {
    let lib = library();
    let mut problems = Vec::new();
    for skill in lib.skills() {
        for (i, line) in skill.body.lines().enumerate() {
            let mut rest = line;
            while let Some(at) = rest.find("`limiter`") {
                let before = &rest[..at];
                let after = &rest[at + "`limiter`".len()..];
                // Allowed only where the skill says not to use the tool.
                let forbidden = before.trim_end().ends_with("not the")
                    || before.trim_end().ends_with("not use the")
                    || before.trim_end().ends_with("Do not use the");
                if !forbidden {
                    problems.push(format!("{}:{}: {line}", skill.name, i + 1));
                }
                rest = after;
            }
        }
    }
    assert!(
        problems.is_empty(),
        "use `add_effect` kind=limiter, not the `limiter` tool:\n{}",
        problems.join("\n")
    );
}
