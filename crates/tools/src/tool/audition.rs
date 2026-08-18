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

use serde::Deserialize;
use serde_json::{json, Value};

use crate::preview_cache::PreviewCache;
use crate::schema::anthropic_tool;
use crate::tool::util::check_track_index;
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
    track: usize,
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
            "Hear what an effect would sound like on a track without applying it. Renders a few \
             seconds of the session with the effect added to that track's chain and returns a WAV \
             to play. Appends no session node, so there is nothing to undo — call `add_effect` \
             with the same arguments to keep it. The audition includes gain, pan, mute, solo, \
             sends and the master chain, so it sounds like the result will.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Zero-based track index" },
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
                "required": ["track", "kind"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

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
        if let Err(msg) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(msg));
        }

        // The amended chain. This state is never appended — it exists
        // to be rendered and then forgotten.
        let effect = session::EffectInstance {
            kind: args.kind.clone(),
            params: args.params.clone().unwrap_or_else(|| json!({})),
            bypassed: false,
        };
        let chain = &mut state.tracks[args.track].effects;
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

        Ok(ToolResult::Ok(json!({
            "path": path.to_string_lossy(),
            "cached": hit.is_cached(),
            "start_sec": start,
            "end_sec": end,
            "track": args.track,
            "kind": args.kind,
            "summary": format!(
                "Auditioning {} on track {} over {:.2}s–{:.2}s. Nothing was added to the \
                 session — call add_effect with the same arguments to keep it.",
                args.kind, args.track, start, end,
            ),
        })))
    }
}

/// A content key for an audition: the state it would render, plus the
/// region.
///
/// `NodeId` is a 32-byte content hash and this is one — of a state that
/// deliberately never becomes a node. Hashing the range in as well is
/// what keeps two excerpts of the same settings apart.
fn audition_key(state: &session::SessionState, start: u64, end: u64) -> session::NodeId {
    let base = session::NodeId::from_state(state)
        .map(|id| id.0)
        .unwrap_or([0u8; 32]);
    let mut hasher = blake3::Hasher::new();
    hasher.update(&base);
    hasher.update(&start.to_le_bytes());
    hasher.update(&end.to_le_bytes());
    session::NodeId(*hasher.finalize().as_bytes())
}
