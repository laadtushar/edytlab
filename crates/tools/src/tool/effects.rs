//! Per-track effect chain tools (#102).
//!
//! Every other effect in this crate is **baked**: `reverb` decodes the
//! track, processes it, writes a new content-addressed WAV and repoints
//! the clip. Undo works, because the DAG keeps the old node — but the
//! parameters are gone. You cannot open a reverb and change the room
//! size; you can only undo and run it again with different numbers.
//!
//! A chain keeps the parameters live on the track and applies them at
//! render, so they stay editable forever. That is a different editing
//! model rather than a bigger feature, which is why these five tools
//! exist alongside the destructive ones rather than replacing them.
//!
//! Which kinds actually render is decided by
//! `audio_engine::effect_chain`, not here. Validating the kind at
//! add-time would duplicate that registry and the two would drift; a
//! chain the renderer cannot honour fails the render with a message
//! naming the effect and saying whether it is unknown or merely not yet
//! streamable.

use serde::Deserialize;
use serde_json::{json, Value};
use session::EffectInstance;

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Look up a track's chain, or an actionable error naming the range.
fn check_effect_index(len: usize, index: usize, track: usize) -> Result<(), String> {
    if index >= len {
        return Err(format!(
            "effect index {index} out of range; track {track} has {len} effect{}",
            if len == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

// ─── add_effect ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AddArgs {
    track: usize,
    kind: String,
    #[serde(default)]
    params: Option<Value>,
    /// Where in the chain. Appended when absent.
    #[serde(default)]
    position: Option<usize>,
}

pub struct AddEffectTool;

impl Tool for AddEffectTool {
    fn name(&self) -> &'static str {
        "add_effect"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "add_effect",
            "Add a non-destructive effect to a track's chain. Unlike the destructive effect \
             tools, the parameters stay editable afterwards — set_effect_params can change them \
             without re-running anything, because the effect is applied at render rather than \
             baked into a new file. Effects apply in chain order, after track gain and volume \
             automation and before pan. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "kind": {
                        "type": "string",
                        "description": "Effect kind, e.g. gain, limiter, low_pass_filter, high_pass_filter, notch_filter."
                    },
                    "params": {
                        "type": "object",
                        "description": "Effect parameters, e.g. { \"cutoff_hz\": 800 }. Defaults are used for anything omitted."
                    },
                    "position": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Index in the chain. Appended to the end when omitted."
                    }
                },
                "required": ["track", "kind"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: AddArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let chain = &mut state.tracks[args.track].effects;
        let at = args.position.unwrap_or(chain.len()).min(chain.len());
        chain.insert(
            at,
            EffectInstance {
                kind: args.kind.clone(),
                params: args.params.unwrap_or_else(|| json!({})),
                bypassed: false,
            },
        );
        let total = chain.len();

        let new_id = match append_state(
            ctx,
            state,
            format!("add_effect track {} {} at {at}", args.track, args.kind),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "position": at,
            "effect_count": total,
            "summary": format!(
                "Added {} to track {} at position {at}; {total} effect{} in the chain",
                args.kind, args.track, if total == 1 { "" } else { "s" }
            ),
        })))
    }
}

// ─── remove_effect ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct IndexArgs {
    track: usize,
    effect_index: usize,
}

pub struct RemoveEffectTool;

impl Tool for RemoveEffectTool {
    fn name(&self) -> &'static str {
        "remove_effect"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "remove_effect",
            "Remove one effect from a track's chain by index. The rest keep their order. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "effect_index": { "type": "integer", "minimum": 0 }
                },
                "required": ["track", "effect_index"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: IndexArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let chain = &mut state.tracks[args.track].effects;
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let removed = chain.remove(args.effect_index).kind;
        let total = chain.len();

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "remove_effect track {} index {} ({removed})",
                args.track, args.effect_index
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "effect_count": total,
            "summary": format!(
                "Removed {removed} from track {}; {total} effect{} left",
                args.track, if total == 1 { "" } else { "s" }
            ),
        })))
    }
}

// ─── reorder_effects ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReorderArgs {
    track: usize,
    /// The chain's new order, as indices into the current chain.
    order: Vec<usize>,
}

pub struct ReorderEffectsTool;

impl Tool for ReorderEffectsTool {
    fn name(&self) -> &'static str {
        "reorder_effects"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "reorder_effects",
            "Reorder a track's effect chain. `order` is the new sequence given as indices into \
             the current chain — [2, 0, 1] moves the third effect to the front. Order matters: \
             a compressor before an EQ is a different sound from one after it. Appends a new \
             session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "order": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0 },
                        "description": "A permutation of the current indices. Must list every effect exactly once."
                    }
                },
                "required": ["track", "order"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: ReorderArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let chain = &state.tracks[args.track].effects;

        // A permutation, checked as one. Accepting a partial list would
        // silently drop effects, and accepting a repeat would silently
        // duplicate one — both are the class of bug this codebase keeps
        // finding, so the whole order is validated before anything moves.
        if args.order.len() != chain.len() {
            return Ok(ToolResult::Error(format!(
                "order must list all {} effect{} exactly once; got {} entries",
                chain.len(),
                if chain.len() == 1 { "" } else { "s" },
                args.order.len()
            )));
        }
        let mut seen = vec![false; chain.len()];
        for &i in &args.order {
            match seen.get_mut(i) {
                None => {
                    return Ok(ToolResult::Error(format!(
                        "order contains index {i}, which is out of range for {} effect{}",
                        chain.len(),
                        if chain.len() == 1 { "" } else { "s" }
                    )))
                }
                Some(true) => {
                    return Ok(ToolResult::Error(format!(
                        "order lists index {i} more than once; it must be a permutation"
                    )))
                }
                Some(slot) => *slot = true,
            }
        }

        let reordered: Vec<EffectInstance> = args.order.iter().map(|&i| chain[i].clone()).collect();
        state.tracks[args.track].effects = reordered;

        let new_id = match append_state(
            ctx,
            state,
            format!("reorder_effects track {} {:?}", args.track, args.order),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!("Reordered track {}'s effect chain", args.track),
        })))
    }
}

// ─── set_effect_params ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ParamsArgs {
    track: usize,
    effect_index: usize,
    params: Value,
    /// Replace the whole parameter object rather than merging into it.
    #[serde(default)]
    replace: bool,
}

pub struct SetEffectParamsTool;

impl Tool for SetEffectParamsTool {
    fn name(&self) -> &'static str {
        "set_effect_params"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_effect_params",
            "Change an effect's parameters in place. This is what a non-destructive chain buys: \
             the audio is not re-processed and nothing is re-rendered until you ask, so tweaking \
             a cutoff costs nothing and is undoable like any other edit. Keys are merged into the \
             existing parameters by default; pass replace=true to swap the object wholesale. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "effect_index": { "type": "integer", "minimum": 0 },
                    "params": { "type": "object" },
                    "replace": {
                        "type": "boolean",
                        "description": "Replace all parameters instead of merging. Default false."
                    }
                },
                "required": ["track", "effect_index", "params"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: ParamsArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if !args.params.is_object() {
            return Ok(ToolResult::Error(
                "params must be an object of parameter names to values".into(),
            ));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let chain = &mut state.tracks[args.track].effects;
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let effect = &mut chain[args.effect_index];
        if args.replace {
            effect.params = args.params.clone();
        } else {
            // Merge, so "make the cutoff 500" does not silently clear
            // every other parameter back to its default.
            let base = effect.params.as_object_mut();
            match (base, args.params.as_object()) {
                (Some(base), Some(patch)) => {
                    for (k, v) in patch {
                        base.insert(k.clone(), v.clone());
                    }
                }
                // The stored params were not an object (an older node, or
                // hand-edited). Replacing is the only coherent merge.
                _ => effect.params = args.params.clone(),
            }
        }
        let kind = effect.kind.clone();
        let params = effect.params.clone();

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "set_effect_params track {} index {} ({kind})",
                args.track, args.effect_index
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "kind": kind,
            "params": params,
            "summary": format!(
                "Updated {kind} on track {} (effect {})",
                args.track, args.effect_index
            ),
        })))
    }
}

// ─── set_effect_bypassed ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BypassArgs {
    track: usize,
    effect_index: usize,
    bypassed: bool,
}

pub struct SetEffectBypassedTool;

impl Tool for SetEffectBypassedTool {
    fn name(&self) -> &'static str {
        "set_effect_bypassed"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_effect_bypassed",
            "Bypass or re-enable one effect without removing it, so its settings survive an A/B. \
             A bypassed effect renders identically to one that is absent. Appends a new session \
             node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "minimum": 0 },
                    "effect_index": { "type": "integer", "minimum": 0 },
                    "bypassed": { "type": "boolean" }
                },
                "required": ["track", "effect_index", "bypassed"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: BypassArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let chain = &mut state.tracks[args.track].effects;
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, args.track) {
            return Ok(ToolResult::Error(e));
        }
        chain[args.effect_index].bypassed = args.bypassed;
        let kind = chain[args.effect_index].kind.clone();

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "set_effect_bypassed track {} index {} -> {}",
                args.track, args.effect_index, args.bypassed
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "bypassed": args.bypassed,
            "summary": format!(
                "{} {kind} on track {}",
                if args.bypassed { "Bypassed" } else { "Re-enabled" },
                args.track
            ),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::check_effect_index;

    #[test]
    fn an_in_range_index_is_accepted() {
        assert!(check_effect_index(3, 0, 0).is_ok());
        assert!(check_effect_index(3, 2, 0).is_ok());
    }

    #[test]
    fn an_out_of_range_index_names_the_range() {
        let e = check_effect_index(2, 5, 1).expect_err("out of range");
        assert!(e.contains("index 5"), "{e}");
        assert!(e.contains("2 effects"), "{e}");
    }

    /// An empty chain is the case a caller hits first, so its message
    /// should read properly rather than saying "0 effects" with a plural
    /// bug.
    #[test]
    fn an_empty_chain_pluralises_correctly() {
        let e = check_effect_index(0, 0, 0).expect_err("empty");
        assert!(e.contains("0 effects"), "{e}");
        let e = check_effect_index(1, 4, 0).expect_err("one");
        assert!(e.contains("1 effect;") || e.contains("1 effect"), "{e}");
    }
}
