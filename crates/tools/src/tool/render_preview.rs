//! `render_preview` — render a node's session state to a temp WAV.
//!
//! Does NOT mutate the session graph; the rendered file is meant to be
//! played back or inspected. The temp file is materialised inside the
//! OS tempdir and its path returned to the caller; the caller (or the
//! agent) is responsible for cleanup. We intentionally do not return a
//! `tempfile::TempPath` because the `ToolResult` payload crosses a
//! JSON boundary.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
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

pub struct RenderPreviewTool;

impl Tool for RenderPreviewTool {
    fn name(&self) -> &'static str {
        "render_preview"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "render_preview",
            "Render a session node to a temporary WAV file and return its path. Does not create a new session node. Optional `range` is a [start_sample, end_sample) pair into the rendered output.",
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

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let node_id = match session::NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node_id: {e}"))),
        };

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

        // A whole-session render is cached by node id (#164): the id is
        // a hash of the state, so undo and redo land on renders that
        // already exist. A ranged render is not — the range is not part
        // of the key, and caching one under the node's name would serve
        // an excerpt as if it were the mix.
        if range.is_none() {
            let cache = crate::PreviewCache::new(ctx.store.project_dir());
            let cached = cache.path_for(node_id);
            if cached.is_file() {
                // Re-read the header rather than storing a sidecar: the
                // numbers are a property of the file, and a file that
                // cannot be read is a miss, not a report of nothing.
                if let Some(probe) = wav_header(&cached) {
                    // Touch through the cache so LRU counts this play.
                    let _ = cache.get_or_render::<_, std::io::Error>(node_id, |_| {
                        unreachable!("the file was just checked to exist")
                    });
                    return Ok(ToolResult::Ok(json!({
                        "path": cached.to_string_lossy(),
                        "frames_written": probe.frames,
                        "sample_rate": probe.sample_rate,
                        "channels": probe.channels,
                        "cached": true,
                        "summary": format!(
                            "Reused cached preview ({} frames, {} ch, {} Hz) at {}",
                            probe.frames, probe.channels, probe.sample_rate,
                            cached.display(),
                        ),
                    })));
                }
            }
        }

        // Miss, or a ranged render. Render into the cache when it is a
        // whole-session preview, and into a temp file when it is not.
        let staging = match tempfile::Builder::new()
            .prefix("edytlab-preview-")
            .suffix(".wav")
            .tempfile()
        {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(format!("tempfile creation failed: {e}"))),
        };
        let staged_path: PathBuf = staging.path().to_path_buf();
        // Keep the file on disk after the handle drops; the caller now
        // owns cleanup.
        let _ = staging.into_temp_path().keep();

        let report = match ctx.engine.render_to_wav(&node.state, &staged_path, range) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
        };

        let out_path = if range.is_none() {
            let cache = crate::PreviewCache::new(ctx.store.project_dir());
            match cache.get_or_render::<_, std::io::Error>(node_id, |dest| {
                std::fs::rename(&staged_path, dest).or_else(|_| {
                    // A rename across filesystems fails; the tempdir is
                    // often on a different one from the project.
                    std::fs::copy(&staged_path, dest).map(|_| ())
                })
            }) {
                Ok((path, _)) => path,
                // A cache that cannot be written is not a reason to
                // throw away a render that succeeded.
                Err(_) => staged_path.clone(),
            }
        } else {
            staged_path.clone()
        };

        Ok(ToolResult::Ok(json!({
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
        })))
    }
}
