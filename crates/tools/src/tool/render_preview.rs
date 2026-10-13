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

use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::preview_cache::{excerpt_key, PreviewCache};
use crate::schema::anthropic_tool;
use crate::tool::util::attach_render_warnings;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    node_id: String,
    #[serde(default)]
    range: Option<[u64; 2]>,
}

/// What a cached preview reports about itself.
struct WavProbe {
    sample_rate: u32,
    channels: u16,
    frames: u64,
}

/// Read a WAV's `fmt ` and `data` chunk headers.
///
/// A cache hit still has to report frames, rate and channels, and those
/// are properties of the file rather than of the session — reading the
/// header keeps a hit honest without decoding a single sample, and a
/// file too damaged to parse reads as a miss rather than as a preview
/// with made-up numbers.
fn wav_header(path: &std::path::Path) -> Option<WavProbe> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff).ok()?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }

    let (mut rate, mut channels, mut bits, mut data_bytes) = (0u32, 0u16, 0u16, 0u64);
    let mut header = [0u8; 8];
    while file.read_exact(&mut header).is_ok() {
        let id = [header[0], header[1], header[2], header[3]];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64;
        match &id {
            b"fmt " => {
                let mut fmt = vec![0u8; size as usize];
                file.read_exact(&mut fmt).ok()?;
                if fmt.len() < 16 {
                    return None;
                }
                channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            }
            b"data" => {
                data_bytes = size;
                break;
            }
            _ => {
                // Chunks are word-aligned, so an odd size carries a pad
                // byte that is not counted in the size field.
                let skip = size + (size & 1);
                std::io::copy(&mut file.by_ref().take(skip), &mut std::io::sink()).ok()?;
            }
        }
    }

    let bytes_per_frame = (bits as u64 / 8) * channels as u64;
    if bytes_per_frame == 0 || rate == 0 {
        return None;
    }
    Some(WavProbe {
        sample_rate: rate,
        channels,
        frames: data_bytes / bytes_per_frame,
    })
}

/// Why a preview was not produced: the render, or storing it.
enum Failure {
    Render(audio_engine::Error),
    Cache(std::io::Error),
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure::Cache(e)
    }
}

/// What a cached preview reports, read from the file: its frames, rate and
/// channels are properties of the file rather than of the session. `None`
/// if the file cannot be read, which is a miss and not a preview with
/// made-up numbers.
///
/// The cache holds the audio and not the report, and a hit that said less
/// than the render before it would look like the problem had gone away.
/// The warnings are a function of the node's stored chains, so they are
/// read from the state, not stored beside the file.
fn cached_result(path: &Path, state: &session::SessionState) -> Option<Value> {
    let probe = wav_header(path)?;
    let mut out = json!({
        "path": path.to_string_lossy(),
        "frames_written": probe.frames,
        "sample_rate": probe.sample_rate,
        "channels": probe.channels,
        "cached": true,
        "summary": format!(
            "Reused cached preview ({} frames, {} ch, {} Hz) at {}",
            probe.frames, probe.channels, probe.sample_rate,
            path.display(),
        ),
    });
    attach_render_warnings(&mut out, &audio_engine::render_warnings(state));
    Some(out)
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
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let node_id = match session::NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node_id: {e}"))),
        };

        // Its audio may have been swept from history; put it back first.
        if let Err(e) = crate::rederive::materialize(ctx.store, node_id) {
            return Ok(ToolResult::Error(e));
        }

        let node = match ctx.store.get(node_id) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("node lookup failed: {e}"))),
        };

        let range = args.range.map(|[start, end]| audio_engine::TimeRange {
            start_frame: start,
            end_frame: end,
        });
        if let Some(r) = range {
            if r.start_frame >= r.end_frame {
                return Ok(ToolResult::Error(format!(
                    "invalid range: start ({}) must be < end ({})",
                    r.start_frame, r.end_frame
                )));
            }
        }

        // The whole mix is cached by node id (#164): the id is a hash of
        // the state, so undo and redo land on renders that already exist.
        // An excerpt is cached too, under a key that names its range, so it
        // is never served in place of the mix and is not left anywhere a
        // cap does not reach (#535).
        let key = match range {
            None => node_id,
            Some(r) => excerpt_key(node_id, r.start_frame, r.end_frame),
        };
        let cache = PreviewCache::new(ctx.store.project_dir());

        if let Some(cached) = cache.lookup(key) {
            if let Some(out) = cached_result(&cached, &node.state) {
                return Ok(ToolResult::Ok(out));
            }
            // A file that cannot be read is a miss: render over it below.
            let _ = std::fs::remove_file(&cached);
        }

        let engine = &mut *ctx.engine;
        let mut report = None;
        let stored = cache.get_or_render::<_, Failure>(key, |staged| {
            report = Some(
                engine
                    .render_to_wav(&node.state, staged, range)
                    .map_err(Failure::Render)?,
            );
            Ok(())
        });
        let (out_path, _) = match stored {
            Ok(v) => v,
            Err(Failure::Render(e)) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
            Err(Failure::Cache(e)) => {
                return Ok(ToolResult::Error(format!(
                    "could not store the preview in the project's preview cache ({}): {e}",
                    cache.dir().display()
                )))
            }
        };

        // Another render of the same key finished first: its file is the
        // one in the cache, and ours was never made.
        let Some(report) = report else {
            return Ok(match cached_result(&out_path, &node.state) {
                Some(out) => ToolResult::Ok(out),
                None => ToolResult::Error(format!(
                    "the cached preview at {} cannot be read",
                    out_path.display()
                )),
            });
        };

        let mut out = json!({
            "path": out_path.to_string_lossy(),
            "frames_written": report.frames_written,
            "sample_rate": report.sample_rate,
            "channels": report.channels,
            "peak_dbfs": report.peak_dbfs,
            "cached": false,
            "summary": format!(
                "Rendered preview ({} frames, {} ch, {} Hz) to {}",
                report.frames_written, report.channels, report.sample_rate,
                out_path.display(),
            ),
        });
        attach_render_warnings(&mut out, &report.warnings);
        Ok(ToolResult::Ok(out))
    }
}
