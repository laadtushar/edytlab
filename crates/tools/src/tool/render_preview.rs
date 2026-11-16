//! `render_preview` — render a node's session state to a WAV in the
//! project's preview cache.
//!
//! Does NOT mutate the session graph; the rendered file is meant to be
//! played back or inspected. Every file it writes for the render is an
//! entry in the bounded, content-addressed preview cache
//! (`.audiograph/previews/`, see [`crate::preview_cache`]) and nowhere
//! else: the whole mix under the node's id, an excerpt under a key that
//! names its range. It used to render into an OS temp file and leave an
//! excerpt there for the caller to clean up, uncapped and outside the
//! project (#535), which is why it was held for approval under Plan first.
//! It is not now, so a render that cannot be stored in the cache fails
//! instead of landing somewhere else.
//!
//! ## The one other thing it may write
//!
//! Like every tool that renders a node, it first puts back that node's
//! audio if a history sweep removed it (`rederive::materialize`). That
//! writes `derived/<hash>.wav` files: the node's, and those of the history
//! it is rebuilt from. They are not new files. Each is the same bytes under
//! the content-addressed name the node already references, so the project
//! ends up holding nothing it did not already name. The preview cap does
//! not bound them; the reclaim sweep does, and it takes them out again when
//! the derived audio is over its own cap. `Tool::mutates` states this
//! exemption, and `tests/read_only_tools.rs` checks it is exactly this.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::render::{self, Audio, Rendered, Request, Span, Target};
use crate::schema::anthropic_tool;
use crate::tool::util::{attach_render_warnings, parse_args};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    node_id: String,
    #[serde(default)]
    range: Option<[u64; 2]>,
}

/// The answer for a cached preview. The cache holds the audio and not the
/// report, and a hit that said less than the render before it would look
/// like the problem had gone away: the warnings are a function of the
/// node's stored chains, so `render` read them from the state.
fn cached_result(rendered: &Rendered, audio: Audio) -> Value {
    let path = &rendered.path;
    let mut out = json!({
        "path": path.to_string_lossy(),
        "frames_written": audio.frames,
        "sample_rate": audio.sample_rate,
        "channels": audio.channels,
        "cached": true,
        "summary": format!(
            "Reused cached preview ({} frames, {} ch, {} Hz) at {}",
            audio.frames, audio.channels, audio.sample_rate,
            path.display(),
        ),
    });
    attach_render_warnings(&mut out, &render::sentences(&rendered.left_out));
    out
}

pub struct RenderPreviewTool;

impl Tool for RenderPreviewTool {
    fn name(&self) -> &'static str {
        "render_preview"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "render_preview",
            "Render a session node to a WAV file in the project's preview cache and return its path. Does not create a new session node. Optional `range` is a [start_sample, end_sample) pair into the rendered output. The same node (and range) is rendered once and reused; the cache is size-capped, so an old preview may be rendered again.",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "range": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0 },
                        "minItems": 2,
                        "maxItems": 2,
                    },
                },
                "required": ["node_id"],
                "additionalProperties": false,
            }),
        )
    }

    // Read-only: it appends no node and moves no head. What it writes is an
    // entry in the app's bounded, content-addressed preview cache, which
    // `Tool::mutates` does not count.
    fn mutates(&self) -> bool {
        false
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };

        let node_id = match session::NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node_id: {e}"))),
        };

        let span = match args.range {
            None => Span::Whole,
            Some([start, end]) => {
                if start >= end {
                    return Ok(ToolResult::Error(format!(
                        "invalid range: start ({start}) must be < end ({end})"
                    )));
                }
                Span::Frames(audio_engine::TimeRange {
                    start_frame: start,
                    end_frame: end,
                })
            }
        };

        // `render::node` puts the node's swept audio back, then serves the
        // cached entry or renders into the cache (#164, #535).
        let request = Request {
            node: node_id,
            span,
            target: Target::Cache {
                hold: false,
                verify: true,
            },
        };
        let rendered = match render::node(&*ctx.store, &*ctx.engine, request) {
            Ok(r) => r,
            Err(render::Error::Node(e)) => {
                return Ok(ToolResult::Error(format!("node lookup failed: {e}")))
            }
            Err(render::Error::Render(e)) => {
                return Ok(ToolResult::Error(format!("render failed: {e}")))
            }
            Err(e) => return Ok(ToolResult::Error(e.to_string())),
        };
        // A verified answer always has its audio: a hit that could not be
        // read was rendered over.
        debug_assert!(rendered.audio.is_some());
        let audio = rendered.audio.unwrap_or_default();

        if rendered.cached {
            return Ok(ToolResult::Ok(cached_result(&rendered, audio)));
        }
        let out_path = &rendered.path;
        let mut out = json!({
            "path": out_path.to_string_lossy(),
            "frames_written": audio.frames,
            "sample_rate": audio.sample_rate,
            "channels": audio.channels,
            "peak_dbfs": audio.peak_dbfs,
            "cached": false,
            "summary": format!(
                "Rendered preview ({} frames, {} ch, {} Hz) to {}",
                audio.frames, audio.channels, audio.sample_rate,
                out_path.display(),
            ),
        });
        attach_render_warnings(&mut out, &render::sentences(&rendered.left_out));
        Ok(ToolResult::Ok(out))
    }
}
