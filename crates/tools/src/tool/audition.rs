//! `audition_effect` — hear an effect before it becomes a node (#166).
//!
//! To find out what `low_pass_filter cutoff: 4000` sounds like you had
//! to apply it, listen, and undo. Choosing a value therefore cost N
//! nodes for N guesses: you commit first and evaluate after, which is
//! backwards.
//!
//! This renders the session **as it would sound** with an effect added
//! to a track's chain, and appends nothing. Cancelling is not an
//! operation — there is nothing to cancel, because nothing was written
//! to the store. Accepting is `add_effect`, which is one node exactly
//! as applying it directly would be.
//!
//! ## Which chain
//!
//! The effect is added to the chain the call names, with the same
//! `scope`, `track` and `bus` arguments the five chain tools take (#533):
//! a track's, the master bus's, or a bus's. It was a track only, so the
//! model could not let anyone hear "a limiter on the master" before adding
//! it. The three are read by the same `Target` the chain tools use, so a
//! call that one accepts or refuses the other does too, and the amended
//! state is built by the same lookup, with the same errors for a track
//! that is not there or a bus that is not.
//!
//! ## Why chain effects and not destructive tools
//!
//! Chain effects (#102) are already applied at render time, so an
//! audition is a render of the current head with the chain temporarily
//! amended — which is why it can respect gain, pan, mute, solo, sends
//! and the master chain for free. It sounds like the result because it
//! *is* the result, minus the commit.
//!
//! A destructive tool would need `destructive_edit` split into
//! "compute the new samples" and "write and append", which is a
//! refactor of every one of them rather than a feature. Not attempted
//! here.
//!
//! ## Cost
//!
//! Auditions are cached by content, in their own directory, so nudging
//! a parameter and coming back to a value you already heard is
//! instant. They are excerpts and are deliberately kept away from the
//! preview cache: serving one in place of a whole mix would be a hit
//! that returns the wrong audio (#164).
//!
//! ## Why Plan first does not hold it
//!
//! It appends nothing and moves nothing. Its only file is an entry in that
//! cache (`.audiograph/auditions/`): content-addressed, size-capped and
//! rebuildable, which is the kind of write `Tool::mutates` does not count.
//! The cache removes its own staging file when a render fails (#535), so
//! there is no other file.
//!
//! ## Hearing it
//!
//! The result is tagged `type: "audition"`, which opts it into
//! `ai::ToolView` (not linkable from here — `ai` depends on this crate,
//! not the other way round) and puts a player under the tool badge that
//! produced it (#258). Before that tag existed the render happened and
//! the excerpt's absolute path was printed into the chat as JSON — the
//! work was done and there was no way to hear it from inside the app.
//!
//! ## Deferred: the parameter control
//!
//! #166 also asks that *"changing a parameter updates what you hear
//! without a manual re-render"* — a slider. That is **not built**, and
//! this note is the explicit deferral #258 asks for rather than a
//! silent omission; the last time this criterion went unrecorded, a
//! merge commit described the content cache as "what makes this usable
//! on a slider" while shipping no slider.
//!
//! What exists is the half that makes a slider cheap when someone
//! builds one: auditions are content-addressed, so re-auditioning a
//! value already heard is a cache hit rather than a render. What is
//! missing is a UI control that re-invokes the tool as it moves, which
//! is a frontend feature and not a property of this tool.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::preview_cache::{excerpt_key, PreviewCache};
use crate::schema::anthropic_tool;
use crate::tool::effects::{bus_property, scope_property, track_property, Scope, Target};
use crate::tool::util::{attach_render_warnings, check_effect_kind, check_effect_params};
use crate::{Tool, ToolContext, ToolResult};

/// Cache directory for auditions, under `.audiograph/`.
pub const AUDITION_DIR: &str = "auditions";

/// How much audio to audition when no range is given.
///
/// Long enough to judge a reverb tail, short enough that a parameter
/// nudge is not a wait.
const DEFAULT_WINDOW_SEC: f64 = 6.0;

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    scope: Option<Scope>,
    #[serde(default)]
    track: Option<usize>,
    #[serde(default)]
    bus: Option<String>,
    kind: String,
    #[serde(default)]
    params: Option<Value>,
    /// Region to hear, in seconds. Defaults to a few seconds from
    /// `from_sec`.
    #[serde(default)]
    start_sec: Option<f64>,
    #[serde(default)]
    end_sec: Option<f64>,
    /// Where in the chain to insert. Defaults to the end, which is
    /// where `add_effect` puts it.
    #[serde(default)]
    at: Option<usize>,
}

pub struct AuditionEffectTool;

impl Tool for AuditionEffectTool {
    fn name(&self) -> &'static str {
        "audition_effect"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "audition_effect",
            "Hear what an effect would sound like on a track, on the master bus (scope \
             \"master\") or on a bus (scope \"bus\") without applying it. Renders a few \
             seconds of the session with the effect added to that chain and returns a WAV to \
             play. Appends no session node, so there is nothing to undo — call `add_effect` with \
             the same arguments to keep it. The audition includes gain, pan, mute, solo, sends \
             and the master chain, so it sounds like the result will.",
            json!({
                "type": "object",
                "properties": {
                    "scope": scope_property(),
                    "track": track_property(),
                    "bus": bus_property(),
                    "kind": {
                        "type": "string",
                        "description": "Effect kind, e.g. gain, limiter, low_pass_filter, high_pass_filter, notch_filter.",
                    },
                    "params": {
                        "type": "object",
                        "description": "Effect parameters, the same shape add_effect takes.",
                    },
                    "start_sec": { "type": "number", "description": "Start of the region to hear" },
                    "end_sec": { "type": "number", "description": "End of the region to hear" },
                    "at": {
                        "type": "integer",
                        "description": "Position in the chain; defaults to the end, as add_effect does.",
                    },
                },
                "required": ["kind"],
                "additionalProperties": false,
            }),
        )
    }

    // Read-only: it appends no node and moves no head. Its one file is an
    // entry in the bounded audition cache, which `Tool::mutates` does not
    // count.
    fn mutates(&self) -> bool {
        false
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let target = match Target::resolve(args.scope, args.track, args.bus.as_deref()) {
            Ok(t) => t,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        // Before the session is read. The renderer leaves out a kind
        // it cannot run (#520), so an audition of one would render the
        // session as it already is and hand back a clip that sounds like
        // nothing happened: a no-op that looks like a result. Refused here
        // with the kinds that work instead, as `add_effect` refuses it.
        if let Err(msg) = check_effect_kind(&args.kind) {
            return Ok(ToolResult::Error(msg));
        }
        // The names too (#579). The renderer uses the default for a name it
        // does not read, so an audition of `{"gain_db": 4.9}` would sound
        // exactly like the session as it is, and the tool promises that
        // `add_effect` with the same arguments keeps what was heard, which
        // refuses them.
        if let Some(params) = &args.params {
            if let Err(msg) = check_effect_params(&args.kind, params) {
                return Ok(ToolResult::Error(msg));
            }
        }

        let Some(head) = ctx.store.head() else {
            return Ok(ToolResult::Error(
                "no session loaded; call `load` first".to_string(),
            ));
        };
        let node = match ctx.store.get(head) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("failed to read head node: {e}"))),
        };

        let mut state = node.state.clone();

        // The amended chain. This state is never appended — it exists
        // to be rendered and then forgotten.
        let effect = session::EffectInstance {
            kind: args.kind.clone(),
            params: args.params.clone().unwrap_or_else(|| json!({})),
            bypassed: false,
        };
        let chain = match target.chain(&mut state) {
            Ok(c) => c,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        let at = args.at.unwrap_or(chain.len()).min(chain.len());
        chain.insert(at, effect);

        // The region. Seconds in, frames out — the renderer works in
        // frames and the caller thinks in seconds.
        let sr = state.sample_rate.max(1) as f64;
        let total_sec = state.length_samples as f64 / sr;
        let start = args.start_sec.unwrap_or(0.0).max(0.0);
        let end = args
            .end_sec
            .unwrap_or_else(|| (start + DEFAULT_WINDOW_SEC).min(total_sec.max(start + 0.1)));
        if end <= start {
            return Ok(ToolResult::Error(format!(
                "end_sec ({end}) must be greater than start_sec ({start})"
            )));
        }
        let range = audio_engine::TimeRange {
            start_frame: (start * sr) as u64,
            end_frame: (end * sr) as u64,
        };

        // Keyed by what it contains: the amended state and the region.
        // Two identical auditions therefore share a file, which is what
        // makes nudging a parameter back to a value you already heard
        // instant.
        let key = audition_key(&state, range.start_frame, range.end_frame);
        let cache = PreviewCache::in_dir(ctx.store.project_dir(), AUDITION_DIR);

        let engine = &mut *ctx.engine;
        let rendered = cache.get_or_render::<_, std::io::Error>(key, |path| {
            engine
                .render_to_wav(&state, path, Some(range))
                .map(|_report| ())
                .map_err(|e| std::io::Error::other(e.to_string()))
        });
        let (path, hit) = match rendered {
            Ok(v) => v,
            Err(e) => return Ok(ToolResult::Error(format!("audition render failed: {e}"))),
        };

        let mut out = json!({
            // Opts this result into `ai::ToolView`, which is what
            // carries the excerpt to a player in the transcript (#258).
            // Without the tag the render still happens and the path is
            // merely recited into the chat, which is where this tool
            // spent its first release: a full render the user could not
            // hear without leaving the app.
            "type": "audition",
            "path": path.to_string_lossy(),
            "cached": hit.is_cached(),
            "start_sec": start,
            "end_sec": end,
            "kind": args.kind,
            // Which chain, in words ("track 1", "the master bus", `bus
            // "Drums"`), for the player's caption: a master or bus
            // audition has no track to name.
            "target": target.describe(),
            "summary": format!(
                "Auditioning {} on {} over {:.2}s–{:.2}s. Nothing was added to the \
                 session — call add_effect with the same arguments to keep it.",
                args.kind, target.describe(), start, end,
            ),
        });
        // A track audition has always carried its index; it still does.
        if let Target::Track(track) = &target {
            out["track"] = json!(track);
        }
        // The audition renders the whole session around the effect, so an
        // entry stored elsewhere in it that the renderer cannot run is left
        // out of this clip too. Read from the amended state rather than the
        // report, so a cached clip says the same as the render that made it.
        attach_render_warnings(&mut out, &audio_engine::render_warnings(&state));
        Ok(ToolResult::Ok(out))
    }
}

/// A content key for an audition: the state it would render, plus the
/// region.
///
/// `NodeId` is a 32-byte content hash and this is one — of a state that
/// deliberately never becomes a node. Hashing the range in as well is
/// what keeps two excerpts of the same settings apart.
fn audition_key(state: &session::SessionState, start: u64, end: u64) -> session::NodeId {
    let base = session::NodeId::from_state(state).unwrap_or(session::NodeId([0u8; 32]));
    excerpt_key(base, start, end)
}
