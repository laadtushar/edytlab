//! Bus creation and send routing.
//!
//! `BusGraph` has been in the session schema since Phase 1 and nothing
//! could put audio through it — `Bus` had a name and an effect list but
//! no input. #111 gave `Track` a `sends` list and taught the renderer to
//! honour it; these tools are what let the agent reach any of that.
//!
//! Shipping the engine without them would repeat #108, where the
//! spectrum chart worked and no call site could produce one.
//!
//! A send is **parallel**: the track still goes to master at full level
//! and the bus receives an additional scaled copy. That is what reverb
//! and delay want. It is not a sub-mix group — for that the track's
//! output would have to be diverted rather than copied, which needs a
//! different field and is a separate change.

use serde::Deserialize;
use serde_json::{json, Value};
use session::{Bus, Send};
use uuid::Uuid;

use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// create_bus
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CreateArgs {
    name: String,
}

pub struct CreateBusTool;

impl Tool for CreateBusTool {
    fn name(&self) -> &'static str {
        "create_bus"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "create_bus",
            "Create an effects bus. Tracks send a scaled copy of themselves to a bus with \
             `set_send`, the bus processes that sum, and the result is added to the master \
             mix. Use this for a shared reverb or delay: one instance fed by several tracks, \
             rather than the same effect applied destructively to each. Returns the bus id. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "e.g. \"Reverb\"" },
                },
                "required": ["name"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: CreateArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let id = Uuid::new_v4();
        state.bus_routing.buses.push(Bus {
            id,
            name: args.name.clone(),
            effects: Vec::new(),
        });

        let new_id = match append_state(ctx, state, format!("create_bus {}", args.name)) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "bus_id": id.to_string(),
            "summary": format!(
                "Created bus {:?} ({}). Route tracks to it with set_send.",
                args.name, id
            ),
        })))
    }
}

// ---------------------------------------------------------------------------
// set_send
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SendArgs {
    track: usize,
    bus_id: String,
    level_db: f32,
}

pub struct SetSendTool;

impl Tool for SetSendTool {
    fn name(&self) -> &'static str {
        "set_send"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "set_send",
            "Route a copy of a track to a bus at the given level. The track still reaches the \
             master mix at full level — this adds a parallel copy, which is how a send differs \
             from moving the track onto the bus. Setting the same track and bus again replaces \
             the level; use a very low level or remove_send to undo. The tap is post-fader, so \
             changing the track's gain changes what it sends. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "bus_id": { "type": "string", "description": "id returned by create_bus" },
                    "level_db": {
                        "type": "number",
                        "description": "Level of the copy in dB; 0 sends at full level, -12 is subtle"
                    },
                },
                "required": ["track", "bus_id", "level_db"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: SendArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let bus_id = match Uuid::parse_str(&args.bus_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid bus_id: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        // Reject a dangling bus here rather than at render time. The
        // render does check — a send to a missing bus fails the whole
        // render — but failing at the point the mistake is made names
        // the tool call that caused it.
        if !state.bus_routing.buses.iter().any(|b| b.id == bus_id) {
            let known: Vec<String> = state
                .bus_routing
                .buses
                .iter()
                .map(|b| format!("{} ({})", b.name, b.id))
                .collect();
            return Ok(ToolResult::Error(if known.is_empty() {
                "this session has no buses; create one with create_bus first".to_string()
            } else {
                format!(
                    "no bus with id {bus_id}; this session has: {}",
                    known.join(", ")
                )
            }));
        }

        let track = &mut state.tracks[args.track];
        match track.sends.iter_mut().find(|s| s.bus_id == bus_id) {
            Some(existing) => existing.level_db = args.level_db,
            None => track.sends.push(Send {
                bus_id,
                level_db: args.level_db,
            }),
        }

        let label = format!(
            "set_send track {} -> bus {} at {:.1} dB",
            args.track, bus_id, args.level_db
        );
        let new_id = match append_state(ctx, state, label.clone()) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track": args.track,
            "bus_id": bus_id.to_string(),
            "level_db": args.level_db,
            "summary": format!(
                "Track {} now sends a copy to bus {} at {:.1} dB",
                args.track, bus_id, args.level_db
            ),
        })))
    }
}

// ---------------------------------------------------------------------------
// remove_send
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RemoveArgs {
    track: usize,
    bus_id: String,
}

pub struct RemoveSendTool;

impl Tool for RemoveSendTool {
    fn name(&self) -> &'static str {
        "remove_send"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "remove_send",
            "Stop routing a track to a bus. The track keeps going to the master mix. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "bus_id": { "type": "string" },
                },
                "required": ["track", "bus_id"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: RemoveArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let bus_id = match Uuid::parse_str(&args.bus_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid bus_id: {e}"))),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        let track = &mut state.tracks[args.track];
        let before = track.sends.len();
        track.sends.retain(|s| s.bus_id != bus_id);
        if track.sends.len() == before {
            return Ok(ToolResult::Error(format!(
                "track {} does not send to bus {bus_id}",
                args.track
            )));
        }

        let label = format!("remove_send track {} -> bus {}", args.track, bus_id);
        let new_id = match append_state(ctx, state, label) {
            Ok(id) => id,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "track": args.track,
            "bus_id": bus_id.to_string(),
            "summary": format!("Track {} no longer sends to bus {bus_id}", args.track),
        })))
    }
}
