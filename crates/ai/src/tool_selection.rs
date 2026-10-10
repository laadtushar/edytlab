//! Which tools a small-context model is sent, and how they are written
//! down (#395).
//!
//! The system prompt is about a kilobyte. The tool schemas are most of a
//! first request, and every one of them was sent on every request: about
//! 57 KB, which a local model with an 8,192-token context refuses. A
//! provider that says it needs less ([`crate::ToolSet::Slim`]) gets the
//! set built here instead.
//!
//! # What goes in
//!
//! From the tools the turn permits (the agent profile's whitelist and the
//! Capabilities menu have already narrowed them, so nothing here can bring
//! back a disabled tool), in this order of priority:
//!
//! 1. tools the user's message names, by name or by a word of the name
//!    ("reverb", "pitch it up", "time stretch"), the first mentioned
//!    first, and the tools an approved plan names;
//! 2. [`SLIM_CORE_TOOLS`], the edits people ask for most;
//! 3. tools the system prompt's own text points at in backticks
//!    (agent profile, matched skills);
//! 4. tools the model has already called in this conversation, so a
//!    follow-up does not lose the tool it was just using.
//!
//! They are added in that order while the whole set stays within
//! [`SLIM_TOOLS_BUDGET_BYTES`], and the result is sorted by name. If every
//! permitted tool fits (a small agent profile), all are sent: a whitelist
//! the user chose is not cut down to the core.
//!
//! # How they are written
//!
//! Each description is cut to its first sentence and the parts of a schema
//! the model can do without (`default`, `examples`, `title`, an
//! `additionalProperties: false`) are dropped. Names, types, enums,
//! `required` and the bounds are kept as they were. That is safe because
//! the dispatcher validates a call against the tool's own full schema, not
//! against what the model was shown: shortening what the model reads
//! cannot loosen what is accepted.
//!
//! # What stays the same
//!
//! The set is computed once per turn, so every round trip of a turn sends
//! the same tools in the same order and a local server can reuse the
//! prompt it has already processed. A tool that is permitted but was not
//! sent still runs if the model names it: the dispatcher's allow-list is
//! the permission, this is only what the model is shown.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::anthropic::{ContentBlock, Message};

/// The edits people ask for most, always sent when permitted. Alphabetical.
///
/// Chosen for what a message does not name: "cut", "pan", "undo" and
/// "export" are no part of a tool's name (`cut_range`, `set_pan`,
/// `revert_to`, `render_final`), and "louder" names none of the gain
/// tools. A tool whose name the user would say (`limiter`, `reverb`,
/// `silence_region`) is left out, because the message brings it in. The
/// whole core, compacted, must stay within 6,000 bytes so that the named
/// tools have room in [`SLIM_TOOLS_BUDGET_BYTES`]; a test holds it there.
///
/// A new tool does not need to be here: a message that names it brings it
/// in. Add one when a local model should see it without being asked.
pub const SLIM_CORE_TOOLS: &[&str] = &[
    "analyze_track",
    "compressor",
    "cut_range",
    "eq",
    "fade",
    "gain",
    "high_pass_filter",
    "mute_track",
    "noise_reduction",
    "normalize",
    "normalize_loudness",
    "render_final",
    "revert_to",
    "set_pan",
    "set_track_gain",
];

/// The most the compacted tool descriptors may add up to, in bytes of
/// their JSON in Anthropic's shape.
///
/// With the system prompt and the conversation that is a first request of
/// about ten kilobytes, which `tests/request_size.rs` holds under half of
/// an 8,192-token context. Chat-completions wraps each tool in about forty
/// more bytes; that test measures the real request, so the wrapper is
/// accounted for there and not here.
pub const SLIM_TOOLS_BUDGET_BYTES: usize = 8_000;

/// Words in tool names that say nothing about which tool is meant. A user
/// who writes "remove the region on this track" has named no tool, and
/// without this list `remove_*`, `*_region` and `*_track` would all match.
const GENERIC_TOKENS: &[&str] = &[
    "track",
    "effect",
    "effects",
    "region",
    "node",
    "nodes",
    "range",
    "session",
    "selection",
    "apply",
    "change",
    "create",
    "remove",
    "removal",
    "final",
    "preview",
    "multiple",
    "under",
    "name",
    "high",
    "pass",
    "time",
    "report",
    "finder",
    "export",
    "generate",
    "insert",
];

// ---------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------

/// A tool descriptor, written shorter: its description cut to the first
/// sentence and its `input_schema` compacted. Anything that is not a
/// descriptor-shaped object is returned as it was.
pub(crate) fn compact_tool(tool: &Value) -> Value {
    let Some(obj) = tool.as_object() else {
        return tool.clone();
    };
    let mut out = Map::new();
    for (key, value) in obj {
        let compacted = match (key.as_str(), value) {
            ("description", Value::String(d)) => Value::String(first_sentence(d)),
            ("input_schema", schema) => compact_schema(schema),
            _ => value.clone(),
        };
        out.insert(key.clone(), compacted);
    }
    Value::Object(out)
}

/// Keywords whose value maps a name to a schema. The names in these maps
/// are data (a property may be called `default`), so they are never
/// filtered, only their values are compacted.
const SCHEMA_MAPS: &[&str] = &["properties", "definitions", "$defs", "patternProperties"];

/// Keywords whose value is a list of schemas.
const SCHEMA_LISTS: &[&str] = &["oneOf", "anyOf", "allOf"];

/// Keywords the model can do without: they describe the schema, or hint a
/// value the description already gives.
const DROPPED_KEYWORDS: &[&str] = &["default", "examples", "title", "$comment", "$schema"];

fn compact_schema(node: &Value) -> Value {
    let Some(obj) = node.as_object() else {
        return node.clone();
    };
    let mut out = Map::new();
    for (key, value) in obj {
        let key_str = key.as_str();
        if DROPPED_KEYWORDS.contains(&key_str) {
            continue;
        }
        // Only the boolean `false`: it is what `object_schema` writes on
        // every tool and says nothing a model acts on. A schema there
        // would be a real constraint.
        if key_str == "additionalProperties" && value == &Value::Bool(false) {
            continue;
        }
        let compacted = match (key_str, value) {
            ("description", Value::String(d)) => Value::String(first_sentence(d)),
            ("items", Value::Array(list)) => {
                Value::Array(list.iter().map(compact_schema).collect())
            }
            ("items" | "additionalProperties" | "not" | "if" | "then" | "else", schema) => {
                compact_schema(schema)
            }
            (k, Value::Array(list)) if SCHEMA_LISTS.contains(&k) => {
                Value::Array(list.iter().map(compact_schema).collect())
            }
            (k, Value::Object(map)) if SCHEMA_MAPS.contains(&k) => Value::Object(
                map.iter()
                    .map(|(name, schema)| (name.clone(), compact_schema(schema)))
                    .collect(),
            ),
            _ => value.clone(),
        };
        out.insert(key.clone(), compacted);
    }
    Value::Object(out)
}

/// The first sentence of `text`, with runs of whitespace made single
/// spaces.
///
/// A sentence ends at a full stop followed by a space and then a capital
/// letter or a backtick. That is deliberately narrow: "e.g. -14 for
/// streaming", "i.e. the head" and "1.0 (default 1.5)" do not end one, and
/// a text with no such stop is returned whole.
pub(crate) fn first_sentence(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let bytes = text.as_bytes();
    for (i, _) in text.match_indices(". ") {
        let next = bytes.get(i + 2).copied().unwrap_or(0);
        if !(next.is_ascii_uppercase() || next == b'`') {
            continue;
        }
        let word_start = text[..i].rfind(' ').map_or(0, |p| p + 1);
        let word = text[word_start..i].to_ascii_lowercase();
        if word == "e.g" || word == "i.e" {
            continue;
        }
        return text[..=i].to_string();
    }
    text
}

// ---------------------------------------------------------------------
// Mentions
// ---------------------------------------------------------------------

/// The lowercase words of `text`, split on anything that cannot be part of
/// a tool name.
fn words_of(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// How early in the message `name` is mentioned: the position of the first
/// of `words` that names the tool, or `None` if none does.
///
/// A word names a tool when it is the tool's full name, or when it is, or
/// begins, or is the start of, one of the name's distinctive parts:
/// "reversed" names `reverse` and "labels" names `label`, while "reverb"
/// does not name `reverse`. Both sides must be four letters or more for
/// that, so `eq` and `gain` are matched only by their full names and "do"
/// matches nothing.
fn mention_rank(name: &str, words: &[String]) -> Option<usize> {
    let tokens: Vec<&str> = name
        .split('_')
        .filter(|t| t.len() >= 4 && !GENERIC_TOKENS.contains(t))
        .collect();
    words.iter().position(|w| {
        w == name
            || (w.len() >= 4
                && tokens
                    .iter()
                    .any(|t| w.starts_with(t) || t.starts_with(w.as_str())))
    })
}

/// Names written in backticks in `text`, which is how the system prompt
/// and an agent profile point at a tool.
fn backticked_names(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2)
}

/// The tool names of the `tool_use` blocks in `conversation`, in the
/// order they were first called.
fn tools_called(conversation: &[Message]) -> Vec<&str> {
    let mut seen = HashSet::new();
    conversation
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .filter(|n| seen.insert(*n))
        .collect()
}

// ---------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------

fn name_of(tool: &Value) -> Option<&str> {
    tool.get("name").and_then(Value::as_str)
}

fn json_len(tool: &Value) -> usize {
    serde_json::to_string(tool).map_or(usize::MAX, |s| s.len())
}

/// The tools to send a [`crate::ToolSet::Slim`] provider this turn.
///
/// `available` is the tool list as it stands after the whitelist, an array
/// of Anthropic-shaped descriptors; nothing outside it is ever returned.
/// `user_text` is this turn's message, `prompt_parts` the parts of the
/// system prompt that can point at tools, and `conversation` the history
/// before this turn's reply. See the module docs for the order.
///
/// The result is sorted by name, so it does not depend on the dispatcher's
/// order, and the same inputs give the same bytes.
pub(crate) fn slim_tool_schemas(
    available: Value,
    user_text: &str,
    prompt_parts: &[&str],
    conversation: &[Message],
) -> Value {
    let Some(list) = available.as_array() else {
        return available;
    };
    let mut compacted: Vec<(String, Value, usize)> = list
        .iter()
        .filter_map(|tool| {
            let name = name_of(tool)?.to_string();
            let tool = compact_tool(tool);
            let size = json_len(&tool);
            Some((name, tool, size))
        })
        .collect();
    compacted.sort_by(|a, b| a.0.cmp(&b.0));
    compacted.dedup_by(|a, b| a.0 == b.0);

    let total: usize = compacted.iter().map(|(_, _, size)| size).sum();
    if total <= SLIM_TOOLS_BUDGET_BYTES {
        return Value::Array(compacted.into_iter().map(|(_, tool, _)| tool).collect());
    }

    // The tools the message names, the first mentioned first: when more
    // are named than fit, it is the ones asked for first that are kept.
    let words = words_of(user_text);
    let mut mentioned: Vec<(usize, &str)> = compacted
        .iter()
        .filter_map(|(name, _, _)| Some((mention_rank(name, &words)?, name.as_str())))
        .collect();
    mentioned.sort();
    let named_in_message = mentioned.into_iter().map(|(_, name)| name);
    let core = SLIM_CORE_TOOLS.iter().copied();
    let named_in_prompt = prompt_parts.iter().flat_map(|p| backticked_names(p));
    let called = tools_called(conversation).into_iter();
    let priority: Vec<String> = named_in_message
        .chain(core)
        .chain(named_in_prompt)
        .chain(called)
        .map(str::to_string)
        .collect();

    let mut chosen: HashSet<String> = HashSet::new();
    let mut used = 0usize;
    for name in priority {
        if chosen.contains(&name) {
            continue;
        }
        let Some((_, _, size)) = compacted.iter().find(|(n, _, _)| *n == name) else {
            continue; // Not permitted this turn, or not a tool at all.
        };
        // A tool that would not fit is skipped, not the end of the list:
        // a smaller one further down may.
        if used + size <= SLIM_TOOLS_BUDGET_BYTES {
            used += size;
            chosen.insert(name);
        }
    }

    Value::Array(
        compacted
            .into_iter()
            .filter(|(name, _, _)| chosen.contains(name))
            .map(|(_, tool, _)| tool)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anthropic::Role;
    use serde_json::json;
    use tools::ToolDispatcher;

    // -----------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------

    fn all_tools() -> Value {
        ToolDispatcher::default_dispatcher().tool_schemas()
    }

    fn names(tools: &Value) -> Vec<String> {
        tools
            .as_array()
            .expect("an array")
            .iter()
            .map(|t| t["name"].as_str().expect("a name").to_string())
            .collect()
    }

    fn core() -> Vec<String> {
        SLIM_CORE_TOOLS.iter().map(|s| s.to_string()).collect()
    }

    /// What a Slim provider is sent for `text`, with the whole registry
    /// permitted, no prompt text pointing at tools and no history.
    fn sent_for(text: &str) -> Vec<String> {
        names(&slim_tool_schemas(all_tools(), text, &[], &[]))
    }

    /// A message by the model that called `tool`.
    fn assistant_called(tool: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call_1".into(),
                name: tool.into(),
                input: json!({}),
            }],
        }
    }

    /// A tool whose JSON is about `size` bytes.
    fn padded_tool(name: &str, size: usize) -> Value {
        let base = json!({
            "name": name,
            "description": "",
            "input_schema": { "type": "object", "properties": {} },
        });
        let pad = size.saturating_sub(json_len(&base));
        json!({
            "name": name,
            "description": "x".repeat(pad),
            "input_schema": { "type": "object", "properties": {} },
        })
    }

    // -----------------------------------------------------------------
    // The core
    // -----------------------------------------------------------------

    #[test]
    fn every_core_tool_is_registered() {
        let registered = names(&all_tools());
        for name in SLIM_CORE_TOOLS {
            assert!(
                registered.iter().any(|r| r == name),
                "{name} is in SLIM_CORE_TOOLS but no such tool is registered"
            );
        }
    }

    #[test]
    fn the_core_is_alphabetical_and_has_no_repeats() {
        let mut sorted = SLIM_CORE_TOOLS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, SLIM_CORE_TOOLS);
    }

    #[test]
    fn the_core_alone_compacts_to_at_most_6000_bytes() {
        let total: usize = all_tools()
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| SLIM_CORE_TOOLS.contains(&t["name"].as_str().unwrap()))
            .map(|t| json_len(&compact_tool(t)))
            .sum();
        assert!(
            total <= 6_000,
            "the core compacts to {total} B, leaving {} B of the {SLIM_TOOLS_BUDGET_BYTES} B budget \
             for tools a message names. Trim the core rather than raising the number.",
            SLIM_TOOLS_BUDGET_BYTES.saturating_sub(total)
        );
    }

    // -----------------------------------------------------------------
    // Selection
    // -----------------------------------------------------------------

    #[test]
    fn a_plain_request_gets_exactly_the_core() {
        for text in [
            "Make track 0 louder by 6 dB.",
            "cut the first 10 seconds",
            "fade out the end",
            "mute the second track",
            "pan it left",
            "undo that",
            "export it as an mp3",
            "what is the tempo?",
            "do it",
        ] {
            assert_eq!(sent_for(text), core(), "for {text:?}");
        }
    }

    #[test]
    fn a_tool_the_message_names_is_sent() {
        for (text, wanted) in [
            ("add some reverb", "reverb"),
            ("pitch it up", "pitch_shift"),
            ("time_stretch track 1", "time_stretch"),
            ("play it reversed", "reverse"),
            ("put an echo on it", "echo"),
            ("remove the silences", "silence_region"),
            ("add labels", "label"),
        ] {
            let sent = sent_for(text);
            assert!(
                sent.iter().any(|n| n == wanted),
                "{text:?} should send {wanted}: {sent:?}"
            );
            for core_tool in SLIM_CORE_TOOLS {
                assert!(
                    sent.iter().any(|n| n == core_tool),
                    "{text:?} lost the core tool {core_tool}"
                );
            }
        }
    }

    #[test]
    fn a_word_that_only_looks_like_a_tool_name_sends_nothing() {
        // "reverb" begins like "reverse"; "tone" is only a word of the
        // name `generate_tone` when it stands alone; two-letter words
        // match nothing.
        let sent = sent_for("add reverb");
        assert!(sent.iter().any(|n| n == "reverb"));
        assert!(!sent.iter().any(|n| n == "reverse"), "{sent:?}");
        assert_eq!(sent_for("be it so, no go"), core());
    }

    #[test]
    fn generic_words_add_nothing() {
        for text in [
            "remove the region on this track",
            "apply a change to this part",
            "create a new session node",
            "what is the high range, the final time",
        ] {
            assert_eq!(sent_for(text), core(), "for {text:?}");
        }
    }

    #[test]
    fn the_tool_mentioned_first_wins_when_not_all_fit() {
        let all = names(&all_tools());
        let non_core: Vec<&String> = all
            .iter()
            .filter(|n| !SLIM_CORE_TOOLS.contains(&n.as_str()))
            .collect();
        // Every non-core tool's own name, `reverb` first: far more than
        // the budget has room for.
        let mut text = String::from("reverb");
        for n in &non_core {
            text.push(' ');
            text.push_str(n);
        }
        let sent = slim_tool_schemas(all_tools(), &text, &[], &[]);
        let total: usize = sent.as_array().unwrap().iter().map(json_len).sum();
        assert!(
            total <= SLIM_TOOLS_BUDGET_BYTES,
            "{total} B is over the {SLIM_TOOLS_BUDGET_BYTES} B budget"
        );
        let sent = names(&sent);
        assert!(sent.iter().any(|n| n == "reverb"), "{sent:?}");
        assert!(
            sent.len() < all.len(),
            "naming everything must not send everything"
        );
    }

    #[test]
    fn the_budget_caps_the_set_and_prefers_named_tools_to_the_core() {
        // Three 3,000-byte tools: named first, then core, then the rest.
        let available = Value::Array(vec![
            padded_tool("gain", 3_000),
            padded_tool("fade", 3_000),
            padded_tool("zzz_named", 3_000),
            padded_tool("zzz_other", 3_000),
        ]);
        let sent = names(&slim_tool_schemas(available, "use zzz_named", &[], &[]));
        assert_eq!(
            sent,
            vec!["fade", "zzz_named"],
            "named first, then the core"
        );
    }

    #[test]
    fn a_tool_too_big_to_fit_is_skipped_and_a_smaller_one_after_it_is_kept() {
        let available = Value::Array(vec![
            padded_tool("gain", 5_000),
            padded_tool("fade", 4_000),
            padded_tool("eq", 1_000),
        ]);
        // Priority is core order: eq, fade, gain. eq (1,000) and fade
        // (4,000) fit; gain (5,000) would make 10,000 and is skipped.
        let sent = names(&slim_tool_schemas(available, "", &[], &[]));
        assert_eq!(sent, vec!["eq", "fade"]);
    }

    #[test]
    fn a_small_whitelist_is_sent_whole() {
        // Five tools nowhere near the core: a profile that allows only
        // these gets these, not the core, and the model is not told of
        // tools it cannot use.
        let all = all_tools();
        let five = ["reverb", "echo", "tremolo", "phaser", "distortion"];
        let available = Value::Array(
            all.as_array()
                .unwrap()
                .iter()
                .filter(|t| five.contains(&t["name"].as_str().unwrap()))
                .cloned()
                .collect(),
        );
        let sent = names(&slim_tool_schemas(available, "make it louder", &[], &[]));
        assert_eq!(
            sent,
            vec!["distortion", "echo", "phaser", "reverb", "tremolo"]
        );
    }

    #[test]
    fn a_disabled_core_tool_is_never_sent() {
        let all = all_tools();
        let available = Value::Array(
            all.as_array()
                .unwrap()
                .iter()
                .filter(|t| t["name"] != "gain" && t["name"] != "reverb")
                .cloned()
                .collect(),
        );
        // `gain` is core and `reverb` is named, but neither is permitted.
        let sent = names(&slim_tool_schemas(
            available,
            "add reverb and gain",
            &["`gain` and `reverb` are good"],
            &[assistant_called("gain")],
        ));
        assert!(
            !sent.iter().any(|n| n == "gain" || n == "reverb"),
            "{sent:?}"
        );
        assert!(sent.iter().any(|n| n == "fade"));
    }

    #[test]
    fn a_backticked_prompt_name_is_sent_and_a_plain_word_is_not() {
        let backticked = names(&slim_tool_schemas(
            all_tools(),
            "hello",
            &["Prefer `time_stretch` over resampling."],
            &[],
        ));
        assert!(
            backticked.iter().any(|n| n == "time_stretch"),
            "{backticked:?}"
        );

        let plain = names(&slim_tool_schemas(
            all_tools(),
            "hello",
            &["Prefer reverse over resampling."],
            &[],
        ));
        assert!(!plain.iter().any(|n| n == "reverse"), "{plain:?}");
    }

    #[test]
    fn a_tool_called_earlier_stays() {
        let history = [assistant_called("echo")];
        let sent = names(&slim_tool_schemas(all_tools(), "now louder", &[], &history));
        assert!(sent.iter().any(|n| n == "echo"), "{sent:?}");

        // A call to something that is not a tool at all changes nothing.
        let history = [assistant_called("no_such_tool")];
        let sent = names(&slim_tool_schemas(all_tools(), "now louder", &[], &history));
        assert_eq!(sent, core());
    }

    #[test]
    fn the_result_is_sorted_and_does_not_depend_on_the_input_order() {
        let forward = slim_tool_schemas(all_tools(), "add reverb and echo", &[], &[]);
        let mut reversed_input = all_tools().as_array().unwrap().clone();
        reversed_input.reverse();
        let backward = slim_tool_schemas(
            Value::Array(reversed_input),
            "add reverb and echo",
            &[],
            &[],
        );
        assert_eq!(forward, backward);
        let sent = names(&forward);
        let mut sorted = sent.clone();
        sorted.sort();
        assert_eq!(sent, sorted);
    }

    #[test]
    fn a_non_array_is_returned_as_it_was() {
        assert_eq!(
            slim_tool_schemas(json!({"not": "a list"}), "x", &[], &[]),
            json!({"not": "a list"})
        );
    }

    // -----------------------------------------------------------------
    // Compaction
    // -----------------------------------------------------------------

    #[test]
    fn first_sentence_cuts_at_a_stop_before_a_capital() {
        assert_eq!(
            first_sentence("Apply a gain. Composes with the existing one."),
            "Apply a gain."
        );
        assert_eq!(
            first_sentence("Fade in (default 1.0). Appends a new session node."),
            "Fade in (default 1.0)."
        );
        assert_eq!(
            first_sentence("NOT IMPLEMENTED IN THIS BUILD. Would run the model."),
            "NOT IMPLEMENTED IN THIS BUILD."
        );
        assert_eq!(
            first_sentence("Normalise a track. `limiter` follows it."),
            "Normalise a track."
        );
    }

    #[test]
    fn first_sentence_does_not_cut_inside_an_example_or_a_number() {
        for whole in [
            "e.g. -14 for streaming, -23 for broadcast.",
            "Gain of 1.5 dB",
            "no stop here",
            // A sentence that starts with a parameter name is part of the
            // explanation of the parameters before it.
            "threshold_db is the floor. min_silence_ms is the gap.",
            "",
        ] {
            assert_eq!(first_sentence(whole), whole, "for {whole:?}");
        }
        assert_eq!(
            first_sentence("Target, e.g. Spotify. Omit it."),
            "Target, e.g. Spotify.",
            "an `e.g.` does not end a sentence; the next stop does"
        );
        assert_eq!(
            first_sentence("That is, i.e. Spotify. Omit it."),
            "That is, i.e. Spotify."
        );
    }

    #[test]
    fn first_sentence_collapses_whitespace() {
        assert_eq!(
            first_sentence("Apply  a\n   gain.\n\nComposes."),
            "Apply a gain."
        );
    }

    #[test]
    fn compaction_keeps_what_the_model_needs_to_call_the_tool() {
        let tool = json!({
            "name": "fade",
            "description": "Fade a clip in or out. Appends a node.",
            "input_schema": {
                "type": "object",
                "title": "FadeArgs",
                "$schema": "http://json-schema.org/draft-07/schema#",
                "additionalProperties": false,
                "required": ["track", "kind"],
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "kind": { "type": "string", "enum": ["in", "out"], "default": "in" },
                    "seconds": {
                        "type": "number",
                        "description": "Length of the fade. Must be positive.",
                        "examples": [1.0, 2.0],
                        "default": 1.0
                    }
                }
            }
        });
        let compact = compact_tool(&tool);
        assert_eq!(
            compact,
            json!({
                "name": "fade",
                "description": "Fade a clip in or out.",
                "input_schema": {
                    "type": "object",
                    "required": ["track", "kind"],
                    "properties": {
                        "track": { "type": "integer", "minimum": 0 },
                        "kind": { "type": "string", "enum": ["in", "out"] },
                        "seconds": { "type": "number", "description": "Length of the fade." }
                    }
                }
            })
        );
    }

    #[test]
    fn a_property_named_like_a_keyword_survives() {
        let tool = json!({
            "name": "t",
            "description": "T.",
            "input_schema": {
                "type": "object",
                "required": ["default", "title"],
                "properties": {
                    "default": { "type": "number", "default": 3 },
                    "title": { "type": "string", "title": "Title" },
                    "description": { "type": "string", "description": "Words. More words." },
                    "examples": { "type": "array", "items": { "type": "string", "default": "x" } }
                }
            }
        });
        let compact = compact_tool(&tool);
        let props = &compact["input_schema"]["properties"];
        for key in ["default", "title", "description", "examples"] {
            assert!(props.get(key).is_some(), "the property {key:?} was dropped");
        }
        assert_eq!(props["default"], json!({ "type": "number" }));
        assert_eq!(props["description"]["description"], "Words.");
        assert_eq!(props["examples"]["items"], json!({ "type": "string" }));
        assert_eq!(
            compact["input_schema"]["required"],
            json!(["default", "title"])
        );
    }

    #[test]
    fn only_a_false_additional_properties_is_dropped() {
        let schema = json!({
            "type": "object",
            "properties": {
                "closed": { "type": "object", "additionalProperties": false },
                "open": { "type": "object", "additionalProperties": true },
                "typed": {
                    "type": "object",
                    "additionalProperties": { "type": "string", "description": "A. B." }
                }
            }
        });
        let compact = compact_schema(&schema);
        let props = &compact["properties"];
        assert_eq!(props["closed"], json!({ "type": "object" }));
        assert_eq!(props["open"]["additionalProperties"], json!(true));
        assert_eq!(
            props["typed"]["additionalProperties"],
            json!({ "type": "string", "description": "A." })
        );
    }

    #[test]
    fn compaction_reaches_definitions_and_alternatives() {
        let schema = json!({
            "definitions": { "Curve": { "type": "object", "title": "Curve", "description": "A. B." } },
            "properties": {
                "c": { "$ref": "#/definitions/Curve" },
                "either": { "anyOf": [{ "type": "null", "title": "N" }, { "$ref": "#/definitions/Curve" }] }
            }
        });
        let compact = compact_schema(&schema);
        assert_eq!(
            compact["definitions"]["Curve"],
            json!({ "type": "object", "description": "A." })
        );
        assert_eq!(
            compact["properties"]["c"],
            json!({ "$ref": "#/definitions/Curve" })
        );
        assert_eq!(
            compact["properties"]["either"]["anyOf"][0],
            json!({ "type": "null" })
        );
    }

    /// The parts of a schema a call is made of: each property's path, its
    /// type and enum, and what is required. Walks `properties` and `items`
    /// only, so it does not share the compaction's own traversal.
    fn call_shape(schema: &Value, path: &str, out: &mut Vec<String>) {
        out.push(format!(
            "{path}: type={} enum={} required={} minimum={} maximum={} ref={}",
            schema["type"],
            schema["enum"],
            schema["required"],
            schema["minimum"],
            schema["maximum"],
            schema["$ref"]
        ));
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (name, sub) in props {
                call_shape(sub, &format!("{path}.{name}"), out);
            }
        }
        if let Some(items) = schema.get("items") {
            call_shape(items, &format!("{path}[]"), out);
        }
    }

    #[test]
    fn every_registered_tool_keeps_the_shape_of_a_call() {
        for tool in all_tools().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let compact = compact_tool(tool);
            assert_eq!(compact["name"], tool["name"], "{name}: the name changed");
            let mut before = Vec::new();
            let mut after = Vec::new();
            call_shape(&tool["input_schema"], name, &mut before);
            call_shape(&compact["input_schema"], name, &mut after);
            assert_eq!(before, after, "{name}: the shape of a call changed");
            assert!(
                json_len(&compact) <= json_len(tool),
                "{name}: compaction made it longer"
            );
        }
    }

    #[test]
    fn every_compacted_schema_is_still_a_valid_schema() {
        for tool in all_tools().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let compact = compact_tool(tool);
            // `{}` may well fail a tool's `required`; what must not happen
            // is the schema itself being rejected.
            if let Err(reason) = tools::schema::validate(&compact["input_schema"], &json!({})) {
                assert!(
                    !reason.starts_with("invalid input_schema"),
                    "{name}: compaction broke the schema: {reason}"
                );
            }
        }
    }

    #[test]
    fn compaction_makes_the_whole_list_much_smaller() {
        let full: usize = all_tools().as_array().unwrap().iter().map(json_len).sum();
        let compact: usize = all_tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| json_len(&compact_tool(t)))
            .sum();
        assert!(
            compact * 5 <= full * 4,
            "compaction saved too little: {full} B to {compact} B"
        );
    }
}
