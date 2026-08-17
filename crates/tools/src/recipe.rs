//! Recipes: the edit chain without the audio (#162).
//!
//! A session is two separable things — the audio, and the sequence of
//! decisions made about it. Since #151 every node records the tool and
//! parameters that produced it, and since #163 the ops that read
//! outside the session close over what they read. So the second half
//! can be exported on its own: a few KB of JSON, replayable against
//! someone else's recording.
//!
//! Not a preset of settings. An actual replayable edit history,
//! inspectable before it runs.
//!
//! ## Why this is safe here specifically
//!
//! Three properties have to hold together, and all three now do:
//!
//! 1. **The operations are recorded** — `NodeOp { tool, params, … }`.
//! 2. **The DSP is deterministic** — no RNG in the edit path;
//!    `generate_noise` uses a fixed-seed LCG.
//! 3. **A replay is self-verifying** — a derived file is named
//!    `blake3(its own samples)`, so a rebuild is checked against the
//!    name it must produce. Drift is detected rather than silently
//!    substituted.
//!
//! ## What a recipe refuses to do
//!
//! Run silently when it cannot keep its promise. Before any step
//! executes, the whole recipe is checked: a step that reads a model, or
//! one whose provenance was never recorded, is reported **by name** and
//! nothing runs. Half-applying an edit chain and then stopping would
//! leave a session in a state nobody chose.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped when the on-disk shape changes in a way older readers cannot
/// understand. A recipe from the future is refused rather than guessed
/// at.
pub const RECIPE_FORMAT_VERSION: u32 = 1;

/// One recorded operation, exactly as the node carries it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecipeStep {
    pub tool: String,
    pub params: Value,
    /// What the op closed over (#163): a source's content hash, a
    /// clipboard blob, a model identity. Carried so a replay can check
    /// its inputs rather than trust them.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub inputs: Value,
    /// False for steps that read something no recipe can carry — model
    /// weights, most obviously.
    pub replayable: bool,
    /// The human sentence the node was labelled with, so the file reads
    /// as a description of the work and not only as data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// An edit chain, root first, with no audio in it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recipe {
    pub format_version: u32,
    /// Version of the code that recorded the chain. Diagnostic: a
    /// replay that drifts is easier to explain with it than without.
    pub engine_version: String,
    /// Optional human name, since a recipe is meant to be handed to
    /// someone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub steps: Vec<RecipeStep>,
}

impl Recipe {
    /// Steps that cannot be replayed, in order, with the reason.
    ///
    /// Empty means the whole chain can run. This is what a caller shows
    /// the user *before* executing, which is the difference between a
    /// recipe and a black box.
    pub fn blockers(&self) -> Vec<String> {
        self.steps
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.replayable)
            .map(|(i, s)| {
                let why = if s.inputs.get("model").is_some() {
                    let model = s.inputs["model"]["id"].as_str().unwrap_or("an ML model");
                    format!("reads {model}, whose weights are not part of a recipe")
                } else {
                    "reads something outside the session that was not recorded".to_string()
                };
                format!("step {} (`{}`) {why}", i + 1, s.tool)
            })
            .collect()
    }
}

/// Build a recipe from the chain ending at `head`.
///
/// Walks parents to a root and reverses, so the steps read in the order
/// they were performed. A node with no recorded op — written before
/// provenance existed, or produced outside the dispatcher — becomes a
/// step marked unreplayable rather than being skipped: dropping it
/// would produce a recipe that runs and quietly does something else.
pub fn export(store: &session::Store, head: session::NodeId) -> Result<Recipe, String> {
    let mut steps = Vec::new();
    let mut cursor = Some(head);

    while let Some(id) = cursor {
        let node = store
            .get(id)
            .map_err(|e| format!("failed to read node {}: {e}", id.to_hex()))?;
        match &node.op {
            Some(op) => steps.push(RecipeStep {
                tool: op.tool.clone(),
                params: op.params.clone(),
                inputs: op.inputs.clone(),
                replayable: op.reproducible,
                label: node.label.clone(),
            }),
            None => steps.push(RecipeStep {
                tool: "<unrecorded>".to_string(),
                params: Value::Null,
                inputs: Value::Null,
                replayable: false,
                label: node.label.clone(),
            }),
        }
        cursor = node.parent;
    }

    steps.reverse();
    Ok(Recipe {
        format_version: RECIPE_FORMAT_VERSION,
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        name: None,
        steps,
    })
}

/// What happened when a recipe ran.
#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub steps_applied: usize,
    /// Session head after the last step.
    pub head: Option<String>,
    /// Per-step notes, in order — what ran and what it produced.
    pub notes: Vec<String>,
}

/// Read a recipe from JSON, refusing shapes this build cannot honour.
pub fn parse(text: &str) -> Result<Recipe, String> {
    let recipe: Recipe =
        serde_json::from_str(text).map_err(|e| format!("not a readable recipe: {e}"))?;
    if recipe.format_version > RECIPE_FORMAT_VERSION {
        return Err(format!(
            "recipe format version {} is newer than this build understands ({RECIPE_FORMAT_VERSION})",
            recipe.format_version
        ));
    }
    if recipe.steps.is_empty() {
        return Err("recipe contains no steps".to_string());
    }
    Ok(recipe)
}

/// Substitute the source file a `load` step points at.
///
/// Applying a recipe to *different* audio is the main reason to have
/// one, and it means the recorded source hash will not match — which is
/// correct and expected, not a failure. Swapping the path here makes
/// that an explicit choice rather than something that happens because
/// nobody checked.
pub fn retarget(recipe: &mut Recipe, new_source: &str) -> usize {
    let mut swapped = 0;
    for step in &mut recipe.steps {
        if step.tool == "load" {
            step.params["path"] = Value::String(new_source.to_string());
            // The recorded hash describes audio this step no longer
            // reads. Keeping it would make a later verification claim
            // the source "changed", which is true but useless.
            if step.inputs.get("source").is_some() {
                step.inputs["source"]["path"] = Value::String(new_source.to_string());
                step.inputs["source"]["audio_hash"] = Value::Null;
            }
            swapped += 1;
        }
    }
    swapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(tool: &str, replayable: bool, inputs: Value) -> RecipeStep {
        RecipeStep {
            tool: tool.to_string(),
            params: json!({}),
            inputs,
            replayable,
            label: None,
        }
    }

    #[test]
    fn a_clean_chain_has_no_blockers() {
        let r = Recipe {
            format_version: 1,
            engine_version: "0".into(),
            name: None,
            steps: vec![
                step("load", true, Value::Null),
                step("gain", true, Value::Null),
            ],
        };
        assert!(r.blockers().is_empty());
    }

    /// The refusal has to name the step, and say why — "this recipe
    /// cannot run" with no further detail is not actionable.
    #[test]
    fn a_model_step_blocks_and_names_itself() {
        let r = Recipe {
            format_version: 1,
            engine_version: "0".into(),
            name: None,
            steps: vec![
                step("load", true, Value::Null),
                step(
                    "transcribe",
                    false,
                    json!({ "model": { "id": "whisper", "version": "base.onnx" } }),
                ),
            ],
        };
        let blockers = r.blockers();
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].contains("step 2"), "{blockers:?}");
        assert!(blockers[0].contains("transcribe"), "{blockers:?}");
        assert!(blockers[0].contains("whisper"), "{blockers:?}");
    }

    #[test]
    fn a_future_format_is_refused_rather_than_guessed_at() {
        let text = json!({
            "format_version": RECIPE_FORMAT_VERSION + 1,
            "engine_version": "9.9.9",
            "steps": [{ "tool": "gain", "params": {}, "replayable": true }],
        })
        .to_string();
        let err = parse(&text).unwrap_err();
        assert!(err.contains("newer than this build"), "{err}");
    }

    #[test]
    fn an_empty_recipe_is_refused() {
        let text = json!({
            "format_version": 1,
            "engine_version": "0",
            "steps": [],
        })
        .to_string();
        assert!(parse(&text).unwrap_err().contains("no steps"));
    }

    /// Retargeting swaps the path *and* drops the hash it invalidates.
    #[test]
    fn retargeting_a_load_clears_the_hash_it_invalidates() {
        let mut r = Recipe {
            format_version: 1,
            engine_version: "0".into(),
            name: None,
            steps: vec![RecipeStep {
                tool: "load".into(),
                params: json!({ "path": "/old/in.wav" }),
                inputs: json!({ "source": { "path": "/old/in.wav", "audio_hash": "abc" } }),
                replayable: true,
                label: None,
            }],
        };
        assert_eq!(retarget(&mut r, "/new/other.wav"), 1);
        assert_eq!(r.steps[0].params["path"], json!("/new/other.wav"));
        assert!(r.steps[0].inputs["source"]["audio_hash"].is_null());
    }

    #[test]
    fn a_recipe_round_trips_through_json() {
        let r = Recipe {
            format_version: 1,
            engine_version: "1.2.3".into(),
            name: Some("podcast chain".into()),
            steps: vec![step("gain", true, Value::Null)],
        };
        let text = serde_json::to_string_pretty(&r).unwrap();
        // Human-readable is an acceptance criterion, not a nicety: the
        // tool name has to be legible in the file.
        assert!(text.contains("\"gain\""), "{text}");
        assert_eq!(parse(&text).unwrap(), r);
    }
}
