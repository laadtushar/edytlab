//! The tools that write a chain effect's parameters name them in their
//! schema (#579).
//!
//! `add_effect`, `set_effect_params` and `audition_effect` refuse a
//! parameter name the renderer does not read (`effect_param_names.rs`), but a
//! refusal is a second try. The schema is what the model reads before the
//! first one, and it said only `{ "cutoff_hz": 800 }` and a limiter's
//! `{ "ceiling_db": -1, "release_ms": 80 }`, and for `set_effect_params`
//! nothing at all. A gain's level is `db`; with nothing to say so, `gain_db`
//! (the name `set_track_gain`'s result uses) is the natural guess, and it
//! was the one the agent made when it re-filmed the beatmatch demo.
//!
//! These tests read the kinds and their names from the table the renderer
//! builds a chain from (`audio_engine::effect_chain::params_of`), not from a
//! list written out here. A kind or a parameter added to the table has to
//! appear in the descriptions without anyone remembering to put it there,
//! and that is what this checks: the sentence is generated from the table, so
//! a name cannot be in one and not the other.

use audio_engine::effect_chain::{params_of, streamable_kinds};
use tools::ToolDispatcher;

/// The tools that take an effect's parameters, and that must therefore say
/// what they are called.
const TOOLS: [&str; 3] = ["add_effect", "set_effect_params", "audition_effect"];

/// The `params` property's description in `tool`'s schema.
fn params_description(tool: &str) -> String {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let schema = dispatcher
        .get(tool)
        .unwrap_or_else(|| panic!("{tool} is registered"))
        .schema();
    schema["input_schema"]["properties"]["params"]["description"]
        .as_str()
        .unwrap_or_else(|| panic!("{tool}: `params` has no description in {schema}"))
        .to_string()
}

/// The whole words of `text`, so that `db` is found in "gain: db" and not
/// inside "ceiling_db".
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .collect()
}

/// **The ticket.** A gain's level is `db`, and the schema of every tool that
/// writes it says so, so the model does not have to guess.
#[test]
fn the_params_description_says_a_gain_takes_db() {
    for tool in TOOLS {
        let description = params_description(tool);
        assert!(
            description.contains("gain: db"),
            "{tool}: should say a gain takes `db`: {description}"
        );
    }
}

/// Every streamable kind appears, with each parameter name its table entry
/// carries, in the order the table lists them. The names are read from
/// `params_of`, so a new parameter in the table fails here until the
/// description shows it.
#[test]
fn every_streamable_kind_and_every_parameter_it_reads_is_named() {
    let kinds: Vec<&str> = streamable_kinds().collect();
    assert!(!kinds.is_empty(), "the registry streams some kinds");

    for tool in TOOLS {
        let description = params_description(tool);
        for kind in &kinds {
            let specs = params_of(kind).expect("a streamable kind has a parameter list");
            let names: Vec<&str> = specs.iter().map(|p| p.name).collect();
            let entry = if names.is_empty() {
                format!("{kind}: none")
            } else {
                format!("{kind}: {}", names.join(", "))
            };
            assert!(
                description.contains(&entry),
                "{tool}: should list `{entry}`: {description}"
            );
            for name in &names {
                assert!(
                    words(&description).contains(name),
                    "{tool}: should name `{name}` as a word of its own: {description}"
                );
            }
        }
    }
}

/// The description lists what the renderer streams and nothing else: a kind
/// that cannot go in a chain yet is refused by `add_effect`, and naming its
/// parameters would invite the call.
#[test]
fn the_params_description_names_no_kind_the_renderer_cannot_stream() {
    for tool in TOOLS {
        let description = params_description(tool);
        let present = words(&description);
        for not_yet in [
            "reverb",
            "echo",
            "compressor",
            "eq",
            "tremolo",
            "phaser",
            "noise_gate",
            "distortion",
        ] {
            assert!(
                !present.contains(&not_yet),
                "{tool}: names `{not_yet}`, which cannot go in a chain: {description}"
            );
        }
    }
}
