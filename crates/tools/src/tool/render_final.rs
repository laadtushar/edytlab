//! `render_final` — render a node's session state to a user-chosen
//! path.
//!
//! WAV, FLAC and MP3. The schema advertises exactly what the encoders
//! can produce: it listed `mp3` for "forward compatibility" once and
//! that meant the tool promised an export it could not perform, which
//! is worse than not offering it.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    node_id: String,
    format: String,
    out_path: String,
    /// MP3 only. Ignored by the lossless formats.
    #[serde(default)]
    bitrate_kbps: Option<u32>,
    /// Tags to write on the exported file (#170). Absent writes none.
    #[serde(default)]
    metadata: Option<MetadataArgs>,
    /// Write the session's markers as chapters. Off by default: a
    /// marker is a working annotation, and not every one of them is a
    /// chapter someone wants shipped.
    #[serde(default)]
    markers_as_chapters: bool,
}

/// The tags a person would put on a file they are sending somewhere.
#[derive(Debug, Deserialize)]
struct MetadataArgs {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    year: Option<String>,
    #[serde(default)]
    comment: Option<String>,
}

/// Widest CBR bitrate an argument may request, in kbps.
///
/// The encoder snaps to the nearest valid Layer III value anyway; this
/// exists so an obvious mistake (`bitrate_kbps: 192000`, meaning bps)
/// is refused with an explanation rather than silently snapped to 320.
const MP3_MIN_KBPS: u32 = 32;
const MP3_MAX_KBPS: u32 = 320;

pub struct RenderFinalTool;

impl Tool for RenderFinalTool {
    fn name(&self) -> &'static str {
        "render_final"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "render_final",
            "Render a session node to a final audio file at the user's chosen path. \
             format=\"wav\" is uncompressed, \"flac\" is lossless at roughly half the \
             size and identical audio, \"mp3\" is lossy but plays anywhere — prefer \
             flac when the user wants to send a file somewhere and quality matters, \
             mp3 when size or compatibility matters. bitrate_kbps applies to mp3 only \
             and defaults to 192.",
            json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string" },
                    "format": { "type": "string", "enum": ["wav", "flac", "mp3"] },
                    "out_path": { "type": "string" },
                    "metadata": {
                        "type": "object",
                        "description": "Tags for the exported file. FLAC gets Vorbis comments, MP3 gets ID3v2. WAV has no standard tag container worth using and ignores this.",
                        "properties": {
                            "title": { "type": "string" },
                            "artist": { "type": "string" },
                            "album": { "type": "string" },
                            "year": { "type": "string", "description": "Four-digit year" },
                            "comment": { "type": "string" },
                        },
                        "additionalProperties": false,
                    },
                    "markers_as_chapters": {
                        "type": "boolean",
                        "description": "Write the session's markers as chapters. Off by default — a marker is a working annotation, and not every one is a chapter worth shipping.",
                    },
                    "bitrate_kbps": {
                        "type": "integer",
                        "minimum": MP3_MIN_KBPS,
                        "maximum": MP3_MAX_KBPS,
                        "description": "MP3 CBR target; snapped to the nearest valid Layer III rate. Ignored for wav and flac."
                    },
                },
                "required": ["node_id", "format", "out_path"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Anything not in the schema enum is refused by validation
        // before it reaches here. This arm is the belt to that braces.
        let transcode = match args.format.as_str() {
            "wav" => None,
            "flac" => Some(Encoded::Flac),
            "mp3" => Some(Encoded::Mp3),
            other => {
                return Ok(ToolResult::Error(format!(
                    "unknown format {other:?}; supported: \"wav\", \"flac\", \"mp3\""
                )));
            }
        };

        if let Some(kbps) = args.bitrate_kbps {
            if args.format != "mp3" {
                return Ok(ToolResult::Error(format!(
                    "bitrate_kbps applies to mp3 only; {} is not a lossy format",
                    args.format
                )));
            }
            if !(MP3_MIN_KBPS..=MP3_MAX_KBPS).contains(&kbps) {
                return Ok(ToolResult::Error(format!(
                    "bitrate_kbps must be between {MP3_MIN_KBPS} and {MP3_MAX_KBPS}; got {kbps}"
                )));
            }
        }

        let node_id = match session::NodeId::from_hex(&args.node_id) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("invalid node_id: {e}"))),
        };

        let node = match ctx.store.get(node_id) {
            Ok(n) => n,
            Err(e) => return Ok(ToolResult::Error(format!("node lookup failed: {e}"))),
        };

        let out_path = PathBuf::from(&args.out_path);

        // Both compressed formats render via WAV and transcode rather
        // than through a second render path. `render_streaming` writes
        // incrementally through `hound`, and giving it a parallel branch
        // per codec would mean a copy of the mixdown loop each — the
        // duplication that produced #80 and #81. The intermediate is
        // lossless 16-bit PCM either way, so what the encoder receives
        // is exactly what the WAV export would have contained.
        let render_target = if transcode.is_some() {
            match tempfile::Builder::new().suffix(".wav").tempfile() {
                Ok(t) => Some(t),
                Err(e) => return Ok(ToolResult::Error(format!("temp file failed: {e}"))),
            }
        } else {
            None
        };
        let render_path = render_target
            .as_ref()
            .map(|t| t.path().to_path_buf())
            .unwrap_or_else(|| out_path.clone());

        // Built before the render so a bad chapter list is refused
        // before the expensive part rather than after it.
        let mut tags = audio_engine::Tags {
            title: args.metadata.as_ref().and_then(|m| m.title.clone()),
            artist: args.metadata.as_ref().and_then(|m| m.artist.clone()),
            album: args.metadata.as_ref().and_then(|m| m.album.clone()),
            year: args.metadata.as_ref().and_then(|m| m.year.clone()),
            comment: args.metadata.as_ref().and_then(|m| m.comment.clone()),
            chapters: Vec::new(),
        };
        if args.markers_as_chapters {
            let mut chapters: Vec<audio_engine::Chapter> = node
                .state
                .annotations
                .iter()
                .filter_map(|a| match &a.kind {
                    // Point markers only. A region has a duration, and a
                    // chapter is a start — turning one into the other
                    // would silently discard the half the user drew.
                    session::AnnotationKind::Marker { time_sec } => Some(audio_engine::Chapter {
                        start_sec: *time_sec,
                        title: a.name.clone(),
                    }),
                    _ => None,
                })
                .collect();
            // In time order, because a chapter list out of order is a
            // chapter list no player will read.
            chapters.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
            tags.chapters = chapters;
        }
        if !tags.is_empty() && transcode.is_none() {
            return Ok(ToolResult::Error(
                "WAV has no standard tag container worth using; export as flac or mp3 to carry \
                 metadata, or drop the metadata argument"
                    .to_string(),
            ));
        }
        let mut tag_warning: Option<String> = None;

        let report = match ctx.engine.render_to_wav(&node.state, &render_path, None) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
        };

        if let Some(kind) = transcode {
            let decoded = match audio_decoder::decode_file(&render_path) {
                Ok(d) => d,
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "could not re-read the render for {} encoding: {e}",
                        args.format
                    )))
                }
            };
            let encoded = match kind {
                Encoded::Flac => audio_engine::write_flac(
                    &decoded.samples,
                    decoded.sample_rate,
                    decoded.channels,
                    &out_path,
                ),
                Encoded::Mp3 => audio_engine::write_mp3(
                    &decoded.samples,
                    decoded.sample_rate,
                    decoded.channels,
                    args.bitrate_kbps,
                    &out_path,
                ),
            };
            if let Err(e) = encoded {
                return Ok(ToolResult::Error(format!(
                    "{} encoding failed: {e}",
                    args.format
                )));
            }

            // Tags go on after encoding: FLAC's blocks are rewritten in
            // place and an ID3 tag is a prefix, so neither touches a
            // sample. A tagging failure does not fail the export — the
            // audio is already written and correct, and losing the file
            // over a missing title would be the worse outcome.
            if !tags.is_empty() {
                let tagged = match kind {
                    Encoded::Flac => audio_engine::tag_flac(&out_path, &tags),
                    Encoded::Mp3 => audio_engine::tag_mp3(&out_path, &tags),
                };
                if let Err(e) = tagged {
                    tag_warning = Some(format!("the audio was written but tagging failed: {e}"));
                }
            }
        }

        let mut out = json!({
            "path": out_path.to_string_lossy(),
            "frames_written": report.frames_written,
            "sample_rate": report.sample_rate,
            "channels": report.channels,
            "peak_dbfs": report.peak_dbfs,
            "format": args.format,
            "summary": format!(
                "Rendered final {} ({} frames, {} ch, {} Hz) to {}",
                args.format, report.frames_written, report.channels, report.sample_rate,
                out_path.display(),
            ),
        });
        if args.format == "mp3" {
            out["bitrate_kbps"] =
                json!(args.bitrate_kbps.unwrap_or(audio_engine::MP3_DEFAULT_KBPS));
        }
        if !tags.is_empty() {
            out["tagged"] = json!(tag_warning.is_none());
            out["chapters"] = json!(tags.chapters.len());
        }
        if let Some(warning) = tag_warning {
            out["warning"] = json!(warning);
            out["summary"] = json!(format!(
                "{} — {warning}",
                out["summary"].as_str().unwrap_or_default()
            ));
        }
        Ok(ToolResult::Ok(out))
    }
}

/// Which encoder runs over the intermediate WAV.
#[derive(Debug, Clone, Copy)]
enum Encoded {
    Flac,
    Mp3,
}
