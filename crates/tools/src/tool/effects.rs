//! Effect chain tools: a track's chain (#102), the master bus's (#460) or
//! a bus's (#533).
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
//! ## Buses
//!
//! `scope: "bus"` with `bus: "<name>"` edits a bus's chain
//! (`Bus::effects`). `create_bus` and `set_send` (#111) made a bus
//! something audio could reach and the renderer runs its chain on the sum
//! of what its sends feed it, then adds the result to the master; but a
//! bus's chain could not be written, so a bus was a name that did nothing.
//! The bus is **named** rather than numbered because that is how it is
//! made (`create_bus` takes a name) and how the model remembers it. A
//! name is not unique, though (`create_bus` accepts any), so the bus id
//! `create_bus` returned is accepted in its place: it is the only way to
//! reach one of two buses that share a name, and a name that matches two
//! is refused rather than guessed.
//!
//! A bus effect is heard only if some track sends to the bus. That is the
//! bus's job, not a limit of the tool, and the description says so.
//!
//! ## Which kinds
//!
//! Which kinds actually render is decided by
//! `audio_engine::effect_chain`, not here, and `add_effect` asks it (via
//! `check_effect_kind`) before it stores anything. It used to leave the
//! kind unchecked on the grounds that a second list would drift from the
//! registry; the cost was that an effect the renderer cannot stream
//! (reverb, echo, eq, compressor...) was accepted and then failed every
//! render until removed (#520). The render no longer fails over a stored
//! one, it leaves the entry out and warns, but an effect that is left out
//! does nothing, so it is still refused at the door. Asking the registry,
//! rather than keeping a list here, is what keeps the two from drifting. A
//! refused kind names the ones that work. The check is the same for every
//! chain, because it is made before the chain is looked at.
//!
//! ## Which parameters
//!
//! The renderer reads a parameter by name and uses the default when the
//! name is absent, so a name it never reads is not an error to it: a
//! `{"gain_db": 4.9}` on a gain, whose level is `db`, was stored, reported
//! as done and heard as 0 dB (#579). `add_effect` and `set_effect_params`
//! now ask the same registry for the kind's parameters
//! (`check_effect_params`) and refuse a name that is not one, naming it and
//! the ones that are. `set_effect_params` checks the keys of the call
//! against the kind of the effect it edits, in merge and in replace alike.
//! It does not look at what is stored: a project written before the check
//! can hold such a name, and it must still load, render and be edited. A
//! stored effect whose kind the renderer cannot run has no list to check
//! against, and its parameters are accepted as they always were.
//!
//! The refusal is the second try; the schema is what the model reads before
//! the first. The `params` description of `add_effect`, `set_effect_params`
//! and `audition_effect` therefore lists each streamable kind's names
//! (`effect_param_names`), written from the same table, so `db` is not left
//! to a guess and a kind or a parameter added to the table appears with no
//! other edit. `tests/effect_params_schema.rs` pins it.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Bus, EffectInstance, SessionState};
use uuid::Uuid;

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, check_effect_kind, check_effect_params, check_track_index, effect_param_names,
    load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

/// Which chain a call edits, as the model writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scope {
    Track,
    Master,
    Bus,
}

/// The chain a call edits, once its `scope`, `track` and `bus` have been
/// read together.
///
/// A bus is held as the text the call gave (a name or an id), not as an
/// index: finding it needs the session, which a call's arguments do not
/// have, and [`Target::chain`] does that when the head is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Track(usize),
    Master,
    Bus(String),
}

impl Target {
    /// Read the three arguments as one. `track` alone is a track, which is
    /// every call made before `scope` existed; `bus` alone is a bus, since
    /// it can mean nothing else; `scope: "master"` is the master bus and
    /// has neither to give.
    ///
    /// Each mistake gets its own message, because each has its own fix:
    /// the call that left everything out needs to learn the master and the
    /// buses exist, and the one that gave both a track and a bus needs to
    /// pick.
    pub(crate) fn resolve(
        scope: Option<Scope>,
        track: Option<usize>,
        bus: Option<&str>,
    ) -> Result<Self, String> {
        match (scope, track, bus) {
            (None | Some(Scope::Track), Some(track), None) => Ok(Self::Track(track)),
            (None | Some(Scope::Bus), None, Some(bus)) => Ok(Self::Bus(bus.to_string())),
            (Some(Scope::Master), None, None) => Ok(Self::Master),
            (Some(Scope::Master), Some(track), _) => Err(format!(
                "scope \"master\" edits the master bus, which has no track index; drop `track` \
                 (got {track}), or drop scope to edit track {track}'s chain"
            )),
            (Some(Scope::Master), None, Some(bus)) => Err(format!(
                "scope \"master\" edits the master bus, which is not a named bus; drop `bus` \
                 (got {bus:?}), or use scope \"bus\" to edit that bus's chain"
            )),
            (Some(Scope::Bus), Some(track), _) => Err(format!(
                "scope \"bus\" edits a bus's chain, which has no track index; drop `track` \
                 (got {track})"
            )),
            (Some(Scope::Track), _, Some(bus)) => Err(format!(
                "scope \"track\" edits a track's chain and takes no `bus` (got {bus:?}); drop \
                 `bus`, or use scope \"bus\" to edit that bus's chain"
            )),
            (None, Some(track), Some(bus)) => Err(format!(
                "got both `track` ({track}) and `bus` ({bus:?}); pass either `track` for a \
                 track's chain or `bus` for a bus's chain, not both"
            )),
            (Some(Scope::Track), None, None) => Err(
                "scope \"track\" needs `track`, the index of the track whose chain to edit"
                    .to_string(),
            ),
            (Some(Scope::Bus), None, None) => Err(
                "scope \"bus\" needs `bus`, the name of the bus whose chain to edit (create_bus \
                 makes one)"
                    .to_string(),
            ),
            (None, None, None) => Err(
                "say which chain to edit: `track` (a track index) for that track's chain, `bus` \
                 (a bus name) for a bus's, or scope \"master\" for the master bus, which \
                 processes the whole mix"
                    .to_string(),
            ),
        }
    }

    /// For a sentence: "track 2", "the master bus" or `bus "Drums"`.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Track(track) => format!("track {track}"),
            Self::Master => "the master bus".to_string(),
            Self::Bus(bus) => format!("bus {bus:?}"),
        }
    }

    /// For a node label: "track 2", "master" or `bus "Drums"`.
    fn label(&self) -> String {
        match self {
            Self::Track(track) => format!("track {track}"),
            Self::Master => "master".to_string(),
            Self::Bus(bus) => format!("bus {bus:?}"),
        }
    }

    /// The chain this target names in `state`, or the error for a track
    /// or a bus that is not there.
    pub(crate) fn chain<'s>(
        &self,
        state: &'s mut SessionState,
    ) -> Result<&'s mut Vec<EffectInstance>, String> {
        match self {
            Self::Track(track) => {
                check_track_index(&state.tracks, *track)?;
                Ok(&mut state.tracks[*track].effects)
            }
            Self::Master => Ok(&mut state.master_chain),
            Self::Bus(bus) => {
                let at = find_bus(&state.bus_routing.buses, bus)?;
                Ok(&mut state.bus_routing.buses[at].effects)
            }
        }
    }
}

/// `"Drums" (3f2a…)`, for a list of buses to choose from.
fn bus_listing(buses: &[Bus]) -> String {
    buses
        .iter()
        .map(|b| format!("{:?} ({})", b.name, b.id))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The bus a `bus` argument names: the one with that name, or failing that
/// the one with that id.
///
/// A name that two buses share is refused, not resolved to the first, and
/// the refusal lists the ids that tell them apart; an id never matches two.
/// Matching is exact, as it is for effect kinds: a near miss is an unknown
/// bus, and the list in the error shows the spelling that works.
fn find_bus(buses: &[Bus], key: &str) -> Result<usize, String> {
    let named: Vec<usize> = buses
        .iter()
        .enumerate()
        .filter(|(_, b)| b.name == key)
        .map(|(i, _)| i)
        .collect();
    match named.as_slice() {
        [one] => return Ok(*one),
        [] => {}
        several => {
            let several: Vec<Bus> = several.iter().map(|&i| buses[i].clone()).collect();
            return Err(format!(
                "{} buses are named {key:?}; pass the id of the one you mean as `bus`: {}",
                several.len(),
                bus_listing(&several)
            ));
        }
    }
    if let Some(at) = Uuid::parse_str(key)
        .ok()
        .and_then(|id| buses.iter().position(|b| b.id == id))
    {
        return Ok(at);
    }
    Err(if buses.is_empty() {
        "this session has no buses; create one with create_bus first".to_string()
    } else {
        format!(
            "no bus named {key:?}; this session has: {}",
            bus_listing(buses)
        )
    })
}

/// The `scope` property, shared so the tools word it alike.
pub(crate) fn scope_property() -> Value {
    json!({
        "type": "string",
        "enum": ["track", "master", "bus"],
        "description": "Which chain to edit. \"track\" (the default when `track` is given) is \
            one track's chain. \"master\" is the master bus, where every track is summed: an \
            effect there processes the whole mix, and keeps processing it when a track is \
            edited later. \"bus\" (the default when `bus` is given) is a bus made with \
            create_bus. Omit `track` when scope is \"master\" or \"bus\"."
    })
}

/// The `track` property, optional now that the master bus and the buses
/// need none.
pub(crate) fn track_property() -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "description": "Zero-based track index. Required unless scope is \"master\" or \"bus\"."
    })
}

/// The `bus` property.
pub(crate) fn bus_property() -> Value {
    json!({
        "type": "string",
        "description": "Name of the bus to edit, as given to create_bus. Required when scope is \
            \"bus\". The id create_bus returned works too, and is the only way to pick between \
            two buses that share a name."
    })
}

/// Look up an effect in a chain, or an actionable error naming the range.
fn check_effect_index(len: usize, index: usize, target: &Target) -> Result<(), String> {
    if index >= len {
        let whose = match target {
            Target::Track(track) => format!("track {track} has"),
            Target::Master => "the master bus has".to_string(),
            Target::Bus(bus) => format!("bus {bus:?} has"),
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
    #[serde(default)]
    bus: Option<String>,
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
            "Add a non-destructive effect to a track's chain, to the master bus with scope \
             \"master\", or to a bus with scope \"bus\" and its name. Unlike the destructive \
             effect tools, the parameters stay editable afterwards — set_effect_params can change \
             them without re-running anything, because the effect is applied at render rather \
             than baked into a new file. A track's effects apply in chain order, after track gain \
             and volume automation and before pan. The master bus is where every track is summed, \
             after sends: use scope \"master\" for a limiter, a filter or a gain on the whole \
             mix, for example \"a limiter on the master at -1 dB\". It stays live when a track \
             is edited later, which a mix rendered into a new track with mix_to_new_track does \
             not, so prefer it to mixing down. A bus (from create_bus) processes what tracks send \
             it with set_send, so an effect on a bus nothing sends to is not heard. Appends a new \
             session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "bus": bus_property(),
                    "kind": {
                        "type": "string",
                        "description": "Effect kind, e.g. gain, limiter, low_pass_filter, high_pass_filter, notch_filter."
                    },
                    "params": {
                        "type": "object",
                        "description": format!(
                            "Parameter names by kind: {}. Other names are refused; omitted ones use defaults.",
                            effect_param_names()
                        )
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
        let target = match Target::resolve(args.scope, args.track, args.bus.as_deref()) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_effect_kind(&args.kind) {
            return Ok(ToolResult::Error(e));
        }
        // The names, too (#579): a key the renderer does not read is
        // stored and reported as done, and the effect then does nothing.
        if let Some(params) = &args.params {
            if let Err(e) = check_effect_params(&args.kind, params) {
                return Ok(ToolResult::Error(e));
            }
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
    #[serde(default)]
    bus: Option<String>,
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
            "Remove one effect, by index, from a track's chain, the master bus's (scope \
             \"master\") or a bus's (scope \"bus\"). The rest keep their order. Appends a new \
             session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "bus": bus_property(),
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
        let target = match Target::resolve(args.scope, args.track, args.bus.as_deref()) {
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
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, &target) {
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
    #[serde(default)]
    bus: Option<String>,
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
            "Reorder a track's effect chain, the master bus's (scope \"master\") or a bus's \
             (scope \"bus\"). `order` is the new sequence given as indices into the current \
             chain — [2, 0, 1] moves the third effect to the front. Order matters: a compressor \
             before an EQ is a different sound from one after it. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "bus": bus_property(),
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
        let target = match Target::resolve(args.scope, args.track, args.bus.as_deref()) {
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
    #[serde(default)]
    bus: Option<String>,
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
            "Change an effect's parameters in place, on a track, on the master bus (scope \
             \"master\") or on a bus (scope \"bus\"). This is what a non-destructive chain \
             buys: the audio is not re-processed and nothing is re-rendered until you ask, so \
             tweaking a cutoff costs nothing and is undoable like any other edit. Keys are merged \
             into the existing parameters by default; pass replace=true to swap the object \
             wholesale. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "bus": bus_property(),
                    "effect_index": { "type": "integer", "minimum": 0 },
                    "params": {
                        "type": "object",
                        "description": format!(
                            "Parameters to change, named by the effect's kind: {}. Other names are refused.",
                            effect_param_names()
                        )
                    },
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
        let target = match Target::resolve(args.scope, args.track, args.bus.as_deref()) {
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
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, &target) {
            return Ok(ToolResult::Error(e));
        }
        let effect = &mut chain[args.effect_index];
        // The keys of this call, against the effect's own kind. What is
        // already stored is not looked at (#579): an older project can hold
        // a name that is not a parameter, and merging a good key into it,
        // or replacing the lot, must still work.
        if let Err(e) = check_effect_params(&effect.kind, &args.params) {
            return Ok(ToolResult::Error(e));
        }
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
    #[serde(default)]
    bus: Option<String>,
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
            "Bypass or re-enable one effect on a track, on the master bus (scope \"master\") or \
             on a bus (scope \"bus\") without removing it, so its settings survive an A/B. A \
             bypassed effect renders identically to one that is absent. Appends a new session \
             node.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "bus": bus_property(),
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
        let target = match Target::resolve(args.scope, args.track, args.bus.as_deref()) {
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
        if let Err(e) = check_effect_index(chain.len(), args.effect_index, &target) {
            return Ok(ToolResult::Error(e));
        }
        let kind = chain[args.effect_index].kind.clone();
        // Switching an effect on is a way into the chain, like adding one.
        // A project saved before `add_effect` checked its kind can hold a
        // bypassed effect the renderer cannot stream; it is skipped without
        // comment because it is bypassed, and switched back on it would be
        // skipped with a warning and do nothing.
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
    use super::{check_effect_index, find_bus, Scope, Target};
    use serde_json::json;
    use session::{Bus, EffectInstance};
    use uuid::Uuid;

    fn bus(name: &str) -> Bus {
        Bus {
            id: Uuid::new_v4(),
            name: name.to_string(),
            effects: Vec::<EffectInstance>::new(),
        }
    }

    #[test]
    fn an_in_range_index_is_accepted() {
        assert!(check_effect_index(3, 0, &Target::Track(0)).is_ok());
        assert!(check_effect_index(3, 2, &Target::Track(0)).is_ok());
        assert!(check_effect_index(1, 0, &Target::Master).is_ok());
        assert!(check_effect_index(1, 0, &Target::Bus("Drums".into())).is_ok());
    }

    #[test]
    fn an_out_of_range_index_names_the_range() {
        let e = check_effect_index(2, 5, &Target::Track(1)).expect_err("out of range");
        assert!(e.contains("index 5"), "{e}");
        assert!(e.contains("track 1 has 2 effects"), "{e}");
        let e = check_effect_index(2, 5, &Target::Master).expect_err("out of range");
        assert!(e.contains("the master bus has 2 effects"), "{e}");
        let e = check_effect_index(2, 5, &Target::Bus("Drums".into())).expect_err("out of range");
        assert!(e.contains("bus \"Drums\" has 2 effects"), "{e}");
    }

    /// An empty chain is the case a caller hits first, so its message
    /// should read properly rather than saying "0 effects" with a plural
    /// bug.
    #[test]
    fn an_empty_chain_pluralises_correctly() {
        let e = check_effect_index(0, 0, &Target::Track(0)).expect_err("empty");
        assert!(e.contains("0 effects"), "{e}");
        let e = check_effect_index(1, 4, &Target::Track(0)).expect_err("one");
        assert!(e.contains("1 effect;") || e.contains("1 effect"), "{e}");
        let e = check_effect_index(0, 0, &Target::Master).expect_err("empty master");
        assert!(e.contains("the master bus has 0 effects"), "{e}");
        let e = check_effect_index(0, 0, &Target::Bus("Drums".into())).expect_err("empty bus");
        assert!(e.contains("bus \"Drums\" has 0 effects"), "{e}");
    }

    /// `track` alone is a track, and that is what every call written
    /// before the scope existed means.
    #[test]
    fn a_track_alone_is_that_tracks_chain() {
        assert_eq!(Target::resolve(None, Some(2), None), Ok(Target::Track(2)));
        assert_eq!(
            Target::resolve(Some(Scope::Track), Some(0), None),
            Ok(Target::Track(0))
        );
    }

    #[test]
    fn scope_master_is_the_master_bus() {
        assert_eq!(
            Target::resolve(Some(Scope::Master), None, None),
            Ok(Target::Master)
        );
    }

    /// `bus` alone is a bus, as `track` alone is a track.
    #[test]
    fn a_bus_alone_or_with_its_scope_is_that_buses_chain() {
        assert_eq!(
            Target::resolve(None, None, Some("Drums")),
            Ok(Target::Bus("Drums".into()))
        );
        assert_eq!(
            Target::resolve(Some(Scope::Bus), None, Some("Drums")),
            Ok(Target::Bus("Drums".into()))
        );
    }

    /// The bad combinations each get a message that says what to do
    /// instead, and none of them is resolved by guessing.
    #[test]
    fn the_bad_combinations_say_what_to_pass() {
        let neither = Target::resolve(None, None, None).expect_err("no target");
        assert!(
            neither.contains("track") && neither.contains("master") && neither.contains("bus"),
            "{neither}"
        );

        let both = Target::resolve(Some(Scope::Master), Some(1), None).expect_err("both");
        assert!(both.contains("no track index"), "{both}");

        let track_without_index =
            Target::resolve(Some(Scope::Track), None, None).expect_err("no index");
        assert!(
            track_without_index.contains("needs `track`"),
            "{track_without_index}"
        );

        let bus_without_name = Target::resolve(Some(Scope::Bus), None, None).expect_err("no bus");
        assert!(
            bus_without_name.contains("needs `bus`"),
            "{bus_without_name}"
        );
    }

    /// Of the twelve ways to fill in (scope, track, bus), exactly the
    /// ones that name one chain resolve, and every other is an error.
    #[test]
    fn only_a_combination_that_names_one_chain_resolves() {
        let scopes = [
            None,
            Some(Scope::Track),
            Some(Scope::Master),
            Some(Scope::Bus),
        ];
        let mut resolved = 0;
        for scope in scopes {
            for track in [None, Some(3)] {
                for bus in [None, Some("Drums")] {
                    if Target::resolve(scope, track, bus).is_ok() {
                        resolved += 1;
                    }
                }
            }
        }
        // track alone, scope track + track, bus alone, scope bus + bus,
        // scope master alone.
        assert_eq!(resolved, 5);
    }

    #[test]
    fn labels_and_sentences_name_the_chain() {
        assert_eq!(Target::Track(3).label(), "track 3");
        assert_eq!(Target::Master.label(), "master");
        assert_eq!(Target::Bus("Drums".into()).label(), "bus \"Drums\"");
        assert_eq!(Target::Track(3).describe(), "track 3");
        assert_eq!(Target::Master.describe(), "the master bus");
        assert_eq!(Target::Bus("Drums".into()).describe(), "bus \"Drums\"");
    }

    #[test]
    fn a_bus_is_found_by_name_or_by_id() {
        let buses = [bus("Drums"), bus("Room")];
        assert_eq!(find_bus(&buses, "Room"), Ok(1));
        assert_eq!(find_bus(&buses, &buses[0].id.to_string()), Ok(0));
        // A uuid in capitals is the same uuid.
        assert_eq!(
            find_bus(&buses, &buses[1].id.to_string().to_uppercase()),
            Ok(1)
        );
    }

    #[test]
    fn a_name_two_buses_share_is_refused_with_their_ids() {
        let buses = [bus("Verb"), bus("Drums"), bus("Verb")];
        let e = find_bus(&buses, "Verb").expect_err("ambiguous");
        assert!(e.contains("2 buses"), "{e}");
        assert!(
            e.contains(&buses[0].id.to_string()) && e.contains(&buses[2].id.to_string()),
            "{e}"
        );
        assert!(!e.contains(&buses[1].id.to_string()), "{e}");
        // Either id then finds its own bus.
        assert_eq!(find_bus(&buses, &buses[2].id.to_string()), Ok(2));
    }

    #[test]
    fn an_unknown_bus_lists_the_buses_there_are() {
        let buses = [bus("Drums"), bus("Room")];
        let e = find_bus(&buses, "Reverb").expect_err("unknown");
        assert!(e.contains("\"Reverb\""), "{e}");
        assert!(
            e.contains("\"Drums\"")
                && e.contains("\"Room\"")
                && e.contains(&buses[0].id.to_string()),
            "{e}"
        );
        let e = find_bus(&[], "Reverb").expect_err("no buses");
        assert!(e.contains("no buses") && e.contains("create_bus"), "{e}");
        // The scope enum is a schema matter; `Scope` is only read here.
        assert_eq!(
            serde_json::from_value::<Scope>(json!("bus")).unwrap(),
            Scope::Bus
        );
    }
}
