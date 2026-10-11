//! Effect chain tools: a track's chain (#102) or the master bus's (#460).
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
//! ## The master bus
//!
//! The same five tools edit the master chain, `SessionState::master_chain`,
//! when called with `scope: "master"` in place of a `track`. That chain
//! has rendered since #110 and the diff layer has had `EffectScope::Master`
//! for as long, but nothing could write to it. "A limiter on the master"
//! was met by rendering every track into a new "Master" track and muting
//! the sources: a baked mix, which goes stale the moment a source track is
//! edited.
//!
//! The target is a second argument rather than a second set of tools or a
//! union-typed `track`. Five more tools would have been five more
//! descriptions in every request and a second copy of every check, and a
//! `track` that is sometimes an integer and sometimes `"master"` is a
//! schema shape no other tool here uses and not every provider accepts.
//! `track` stays what it always was, so a call written before the scope
//! existed means what it meant.
//!
//! Which kinds actually render is decided by
//! `audio_engine::effect_chain`, not here, and `add_effect` asks it (via
//! `check_effect_kind`) before it stores anything. It used to leave the
//! kind unchecked on the grounds that a second list would drift from the
//! registry; the cost was that an effect the renderer cannot stream
//! (reverb, echo, eq, compressor...) was accepted and then failed every
//! render until removed (#520). Asking the registry, rather than keeping
//! a list here, is what keeps the two from drifting. A refused kind names
//! the ones that work.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{EffectInstance, SessionState};

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_effect_kind, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// Which chain a call edits, as the model writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scope {
    Track,
    Master,
}

/// The chain a call edits, once its `scope` and `track` have been read
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Track(usize),
    Master,
}

impl Target {
    /// Read the two arguments as one. `track` alone is a track, which is
    /// every call made before `scope` existed; `scope: "master"` is the
    /// master bus and has no index to give.
    ///
    /// Each mistake gets its own message, because each has its own fix:
    /// the call that left both out needs to learn the master exists, and
    /// the one that gave both needs to learn the master has no index.
    fn resolve(scope: Option<Scope>, track: Option<usize>) -> Result<Self, String> {
        match (scope, track) {
            (None | Some(Scope::Track), Some(track)) => Ok(Self::Track(track)),
            (Some(Scope::Master), None) => Ok(Self::Master),
            (Some(Scope::Master), Some(track)) => Err(format!(
                "scope \"master\" edits the master bus, which has no track index; drop `track` \
                 (got {track}), or drop scope to edit track {track}'s chain"
            )),
            (Some(Scope::Track), None) => Err(
                "scope \"track\" needs `track`, the index of the track whose chain to edit"
                    .to_string(),
            ),
            (None, None) => Err(
                "say which chain to edit: `track` (a track index) for that track's chain, or \
                 scope \"master\" for the master bus, which processes the whole mix"
                    .to_string(),
            ),
        }
    }

    /// For a sentence: "track 2" or "the master bus".
    fn describe(self) -> String {
        match self {
            Self::Track(track) => format!("track {track}"),
            Self::Master => "the master bus".to_string(),
        }
    }

    /// For a node label: "track 2" or "master".
    fn label(self) -> String {
        match self {
            Self::Track(track) => format!("track {track}"),
            Self::Master => "master".to_string(),
        }
    }

    /// The chain this target names in `state`, or the error for a track
    /// that is not there.
    fn chain(self, state: &mut SessionState) -> Result<&mut Vec<EffectInstance>, String> {
        match self {
            Self::Track(track) => {
                check_track_index(&state.tracks, track)?;
                Ok(&mut state.tracks[track].effects)
            }
            Self::Master => Ok(&mut state.master_chain),
        }
    }
}

/// The `scope` property, shared so the five tools word it alike.
fn scope_property() -> Value {
    json!({
        "type": "string",
        "enum": ["track", "master"],
        "description": "Which chain to edit. \"track\" (the default when `track` is given) is \
            one track's chain. \"master\" is the master bus, where every track is summed: an \
            effect there processes the whole mix, and keeps processing it when a track is \
            edited later. Omit `track` when scope is \"master\"."
    })
}

/// The `track` property, optional now that the master bus needs none.
fn track_property() -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "description": "Zero-based track index. Required unless scope is \"master\"."
    })
}

/// Look up an effect in a chain, or an actionable error naming the range.
fn check_effect_index(len: usize, index: usize, target: Target) -> Result<(), String> {
    if index >= len {
        let whose = match target {
            Target::Track(track) => format!("track {track} has"),
            Target::Master => "the master bus has".to_string(),
        };
        return Err(format!(
            "effect index {index} out of range; {whose} {len} effect{}",
            if len == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

// ─── add_effect ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AddArgs {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    track: Option<usize>,
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
            "Add a non-destructive effect to a track's chain, or to the master bus with scope \
             \"master\". Unlike the destructive effect tools, the parameters stay editable \
             afterwards — set_effect_params can change them without re-running anything, because \
             the effect is applied at render rather than baked into a new file. A track's effects \
             apply in chain order, after track gain and volume automation and before pan. The \
             master bus is where every track is summed, after sends: use scope \"master\" for a \
             limiter, a filter or a gain on the whole mix, for example \"a limiter on the master \
             at -1 dB\". It stays live when a track is edited later, which a mix rendered into a \
             new track with mix_to_new_track does not, so prefer it to mixing down. Appends a new \
             session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "kind": {
                        "type": "string",
                        "description": "Effect kind, e.g. gain, limiter, low_pass_filter, high_pass_filter, notch_filter."
                    },
                    "params": {
                        "type": "object",
                        "description": "Effect parameters, e.g. { \"cutoff_hz\": 800 }, or { \"ceiling_db\": -1, \"release_ms\": 80 } for a limiter. Defaults are used for anything omitted."
                    },
                    "position": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Index in the chain. Appended to the end when omitted."
                    }
                },
                "required": ["kind"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: AddArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let target = match Target::resolve(args.scope, args.track) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_effect_kind(&args.kind) {
            return Ok(ToolResult::Error(e));
        }
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let chain = match target.chain(&mut state) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
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
            format!("add_effect {} {} at {at}", target.label(), args.kind),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "position": at,
            "effect_count": total,
            "summary": format!(
                "Added {} to {} at position {at}; {total} effect{} in the chain",
                args.kind, target.describe(), if total == 1 { "" } else { "s" }
            ),
        })))
    }
}

// ─── remove_effect ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct IndexArgs {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    track: Option<usize>,
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
            "Remove one effect from a track's chain, or from the master bus's with scope \
             \"master\", by index. The rest keep their order. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "effect_index": { "type": "integer", "minimum": 0 }
                },
                "required": ["effect_index"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: IndexArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let target = match Target::resolve(args.scope, args.track) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let chain = match target.chain(&mut state) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, target) {
            return Ok(ToolResult::Error(e));
        }
        let removed = chain.remove(args.effect_index).kind;
        let total = chain.len();

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "remove_effect {} index {} ({removed})",
                target.label(),
                args.effect_index
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "effect_count": total,
            "summary": format!(
                "Removed {removed} from {}; {total} effect{} left",
                target.describe(), if total == 1 { "" } else { "s" }
            ),
        })))
    }
}

// ─── reorder_effects ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReorderArgs {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    track: Option<usize>,
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
            "Reorder a track's effect chain, or the master bus's with scope \"master\". `order` \
             is the new sequence given as indices into the current chain — [2, 0, 1] moves the \
             third effect to the front. Order matters: a compressor before an EQ is a different \
             sound from one after it. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "order": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0 },
                        "description": "A permutation of the current indices. Must list every effect exactly once."
                    }
                },
                "required": ["order"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: ReorderArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let target = match Target::resolve(args.scope, args.track) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let chain = match target.chain(&mut state) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

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
        *chain = reordered;

        let new_id = match append_state(
            ctx,
            state,
            format!("reorder_effects {} {:?}", target.label(), args.order),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!("Reordered {}'s effect chain", target.describe()),
        })))
    }
}

// ─── set_effect_params ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ParamsArgs {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    track: Option<usize>,
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
            "Change an effect's parameters in place, on a track or on the master bus (scope \
             \"master\"). This is what a non-destructive chain buys: the audio is not \
             re-processed and nothing is re-rendered until you ask, so tweaking a cutoff costs \
             nothing and is undoable like any other edit. Keys are merged into the existing \
             parameters by default; pass replace=true to swap the object wholesale. Appends a \
             new session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "effect_index": { "type": "integer", "minimum": 0 },
                    "params": { "type": "object" },
                    "replace": {
                        "type": "boolean",
                        "description": "Replace all parameters instead of merging. Default false."
                    }
                },
                "required": ["effect_index", "params"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: ParamsArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let target = match Target::resolve(args.scope, args.track) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(e)),
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
        let chain = match target.chain(&mut state) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, target) {
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
                "set_effect_params {} index {} ({kind})",
                target.label(),
                args.effect_index
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
                "Updated {kind} on {} (effect {})",
                target.describe(), args.effect_index
            ),
        })))
    }
}

// ─── set_effect_bypassed ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BypassArgs {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    track: Option<usize>,
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
            "Bypass or re-enable one effect on a track or on the master bus (scope \"master\") \
             without removing it, so its settings survive an A/B. A bypassed effect renders \
             identically to one that is absent. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "effect_index": { "type": "integer", "minimum": 0 },
                    "bypassed": { "type": "boolean" }
                },
                "required": ["effect_index", "bypassed"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: BypassArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let target = match Target::resolve(args.scope, args.track) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let chain = match target.chain(&mut state) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, target) {
            return Ok(ToolResult::Error(e));
        }
        let kind = chain[args.effect_index].kind.clone();
        // Switching an effect on is a way into the chain, like adding one.
        // A project saved before `add_effect` checked its kind can hold a
        // bypassed effect the renderer cannot stream; it renders because
        // bypassed entries are skipped, and would stop rendering the
        // moment it was switched back on.
        if !args.bypassed {
            if let Err(e) = check_effect_kind(&kind) {
                return Ok(ToolResult::Error(format!(
                    "effect {} ({kind}) stays bypassed: {e}",
                    args.effect_index
                )));
            }
        }
        chain[args.effect_index].bypassed = args.bypassed;

        let new_id = match append_state(
            ctx,
            state,
            format!(
                "set_effect_bypassed {} index {} -> {}",
                target.label(),
                args.effect_index,
                args.bypassed
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "bypassed": args.bypassed,
            "summary": format!(
                "{} {kind} on {}",
                if args.bypassed { "Bypassed" } else { "Re-enabled" },
                target.describe()
            ),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::{check_effect_index, Scope, Target};

    #[test]
    fn an_in_range_index_is_accepted() {
        assert!(check_effect_index(3, 0, Target::Track(0)).is_ok());
        assert!(check_effect_index(3, 2, Target::Track(0)).is_ok());
        assert!(check_effect_index(1, 0, Target::Master).is_ok());
    }

    #[test]
    fn an_out_of_range_index_names_the_range() {
        let e = check_effect_index(2, 5, Target::Track(1)).expect_err("out of range");
        assert!(e.contains("index 5"), "{e}");
        assert!(e.contains("track 1 has 2 effects"), "{e}");
        let e = check_effect_index(2, 5, Target::Master).expect_err("out of range");
        assert!(e.contains("the master bus has 2 effects"), "{e}");
    }

    /// An empty chain is the case a caller hits first, so its message
    /// should read properly rather than saying "0 effects" with a plural
    /// bug.
    #[test]
    fn an_empty_chain_pluralises_correctly() {
        let e = check_effect_index(0, 0, Target::Track(0)).expect_err("empty");
        assert!(e.contains("0 effects"), "{e}");
        let e = check_effect_index(1, 4, Target::Track(0)).expect_err("one");
        assert!(e.contains("1 effect;") || e.contains("1 effect"), "{e}");
        let e = check_effect_index(0, 0, Target::Master).expect_err("empty master");
        assert!(e.contains("the master bus has 0 effects"), "{e}");
    }

    /// `track` alone is a track, and that is what every call written
    /// before the scope existed means.
    #[test]
    fn a_track_alone_is_that_tracks_chain() {
        assert_eq!(Target::resolve(None, Some(2)), Ok(Target::Track(2)));
        assert_eq!(
            Target::resolve(Some(Scope::Track), Some(0)),
            Ok(Target::Track(0))
        );
    }

    #[test]
    fn scope_master_is_the_master_bus() {
        assert_eq!(
            Target::resolve(Some(Scope::Master), None),
            Ok(Target::Master)
        );
    }

    /// The three bad combinations each get a message that says what to do
    /// instead, and none of them is resolved by guessing.
    #[test]
    fn the_bad_combinations_say_what_to_pass() {
        let neither = Target::resolve(None, None).expect_err("no target");
        assert!(
            neither.contains("track") && neither.contains("master"),
            "{neither}"
        );

        let both = Target::resolve(Some(Scope::Master), Some(1)).expect_err("both");
        assert!(both.contains("no track index"), "{both}");

        let track_without_index = Target::resolve(Some(Scope::Track), None).expect_err("no index");
        assert!(
            track_without_index.contains("needs `track`"),
            "{track_without_index}"
        );
    }

    #[test]
    fn labels_and_sentences_name_the_chain() {
        assert_eq!(Target::Track(3).label(), "track 3");
        assert_eq!(Target::Master.label(), "master");
        assert_eq!(Target::Track(3).describe(), "track 3");
        assert_eq!(Target::Master.describe(), "the master bus");
    }
}
