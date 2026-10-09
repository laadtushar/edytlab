//! `analyze_track` tool — decode an audio file, hand it to
//! [`audio_analysis::analyze`], and surface the resulting feature set
//! (BPM, key, beats, downbeats, sections, RMS curve, integrated LUFS).
//!
//! The analysis is pure (no model weights to source, no caches), so
//! the tool is a thin wrapper: decode, call `analyze`, return.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, lane_audio_path, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

/// A track in the session, or a file that is not in it (#410).
///
/// It took only a path, and a track's path is nowhere the model can see:
/// asked how loud track 0 was, or what tempo it was in, the model asked
/// the user for a file path instead of answering.
#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    track: Option<usize>,
    #[serde(default)]
    path: Option<String>,
}

pub struct AnalyzeTrackTool;

impl Tool for AnalyzeTrackTool {
    fn name(&self) -> &'static str {
        "analyze_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "analyze_track",
            "Analyse a track in the session, or an audio file, and return BPM, key, beat grid, downbeats, sections, an RMS curve (one bin per ~100 ms), and EBU R128 integrated loudness in LUFS. Give `track` (its index in the session) to analyse a track as it sits on the timeline, before its gain and effects; give `path` only for a file that is not in the session. Pure-Rust analysis: no model weights or env vars required. The audio is downmixed to mono internally for the music-feature passes; LUFS is measured on the original interleaved signal.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Index of a track in the session." },
                    "path": { "type": "string", "description": "An audio file that is not in the session." }
                },
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let audio_path = match (args.track, args.path) {
            (Some(track), None) => {
                let state = match load_head_state(ctx) {
                    Ok(s) => s,
                    Err(e) => return Ok(ToolResult::Error(e)),
                };
                if let Err(e) = check_track_index(&state.tracks, track) {
                    return Ok(ToolResult::Error(e));
                }
                // The track as it sits on the timeline: its one source
                // when that is the whole track, a flattened file otherwise.
                match lane_audio_path(ctx.store.project_dir(), &state.tracks[track].clips) {
                    Some(p) => p,
                    None => return Ok(ToolResult::Error(format!("track {track} has no audio"))),
                }
            }
            (None, Some(path)) => PathBuf::from(path),
            (Some(_), Some(_)) => {
                return Ok(ToolResult::Error(
                    "give `track` or `path`, not both".to_string(),
                ))
            }
            (None, None) => {
                return Ok(ToolResult::Error(
                    "give `track` (a track's index in the session) or `path` (a file)".to_string(),
                ))
            }
        };
        if !audio_path.exists() {
            return Ok(ToolResult::Error(format!(
                "file not found: {}",
                audio_path.display()
            )));
        }

        let decoded = match audio_decoder::decode_file(&audio_path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };

        let report = match audio_analysis::analyze(&decoded) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(format!("analysis failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "bpm": report.bpm,
            "key": report.key,
            "beats": report.beats,
            "downbeats": report.downbeats,
            "sections": report
                .sections
                .iter()
                .map(|s| json!({
                    "name": s.name,
                    "start_s": s.start_s,
                    "end_s": s.end_s,
                }))
                .collect::<Vec<_>>(),
            "rms_curve": report.rms_curve,
            "lufs_integrated": report.lufs_integrated,
            "summary": format!(
                "{}: {:.1} BPM, {}, {} beats, integrated loudness {:.1} LUFS",
                audio_path.display(),
                report.bpm,
                report.key,
                report.beats.len(),
                report.lufs_integrated,
            ),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::dispatcher::ToolDispatcher;

    /// Write a 4-second 120 BPM click WAV to `path`.
    fn write_click_wav(path: &Path, sample_rate: u32, duration_s: f32, bpm: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        let n = (duration_s * sample_rate as f32) as usize;
        let period = 60.0 / bpm;
        let click_len = (sample_rate as f32 * 0.01) as usize;
        let mut samples = vec![0.0f32; n];
        let mut t = 0.0f32;
        while (t * sample_rate as f32) as usize + click_len < n {
            let start = (t * sample_rate as f32) as usize;
            for i in 0..click_len {
                let env = (1.0 - i as f32 / click_len as f32).max(0.0);
                samples[start + i] = (i as f32 * 0.5).sin() * env;
            }
            t += period;
        }
        for s in samples {
            let v = (s * i16::MAX as f32) as i16;
            writer.write_sample(v).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn schema_advertises_track_and_path() {
        let tool = AnalyzeTrackTool;
        let s = tool.schema();
        assert_eq!(s["name"], "analyze_track");
        assert!(s["input_schema"]["properties"]["path"].is_object());
        assert!(s["input_schema"]["properties"]["track"].is_object());
    }

    fn ctx_parts(dir: &Path) -> (session::Store, audio_engine::Engine) {
        (
            session::Store::open(dir).unwrap(),
            audio_engine::Engine::new(),
        )
    }

    /// The question a person asks: "what tempo is track 0?", with no
    /// path anywhere in sight (#410).
    #[test]
    fn a_loaded_track_is_analysed_by_its_index() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("click.wav");
        write_click_wav(&wav, 44_100, 8.0, 120.0);
        let (mut store, mut engine) = ctx_parts(dir.path());
        let mut clipboard: Option<crate::Clipboard> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let d = ToolDispatcher::default_dispatcher();
        d.invoke("load", json!({ "path": wav }), &mut ctx).unwrap();

        match d
            .invoke("analyze_track", json!({ "track": 0 }), &mut ctx)
            .unwrap()
        {
            ToolResult::Ok(v) => {
                let bpm = v["bpm"].as_f64().expect("bpm");
                assert!((bpm - 120.0).abs() <= 1.0, "BPM ~120, got {bpm}");
            }
            ToolResult::Error(e) => panic!("tool errored: {e}"),
        }
        match d
            .invoke("analyze_track", json!({ "track": 3 }), &mut ctx)
            .unwrap()
        {
            ToolResult::Error(e) => assert!(e.contains("out of range"), "{e}"),
            ToolResult::Ok(v) => panic!("analysed a track that does not exist: {v}"),
        }
    }

    #[test]
    fn neither_or_both_is_refused_with_what_to_give() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, mut engine) = ctx_parts(dir.path());
        let mut clipboard: Option<crate::Clipboard> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let tool = AnalyzeTrackTool;
        match tool.invoke(json!({}), &mut ctx).unwrap() {
            ToolResult::Error(e) => assert!(e.contains("`track`") && e.contains("`path`"), "{e}"),
            ToolResult::Ok(v) => panic!("{v}"),
        }
        match tool
            .invoke(json!({ "track": 0, "path": "x.wav" }), &mut ctx)
            .unwrap()
        {
            ToolResult::Error(e) => assert!(e.contains("not both"), "{e}"),
            ToolResult::Ok(v) => panic!("{v}"),
        }
    }

    #[test]
    fn dispatcher_includes_analyze_track() {
        let d = ToolDispatcher::default_dispatcher();
        assert!(d.get("analyze_track").is_some());
    }

    #[test]
    fn analyze_click_wav_returns_bpm_near_120() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("click.wav");
        write_click_wav(&wav, 44_100, 8.0, 120.0);

        let mut store = session::Store::open(dir.path()).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard: Option<crate::Clipboard> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };

        let tool = AnalyzeTrackTool;
        let result = tool
            .invoke(json!({ "path": wav.to_string_lossy() }), &mut ctx)
            .unwrap();
        match result {
            ToolResult::Ok(v) => {
                let bpm = v["bpm"].as_f64().expect("bpm number");
                assert!(
                    (bpm - 120.0).abs() <= 1.0,
                    "expected BPM ~120, got {bpm} (full result: {v})"
                );
                assert!(v["key"].is_string());
                assert!(v["beats"].is_array());
                assert!(v["lufs_integrated"].is_number());
            }
            ToolResult::Error(e) => panic!("tool errored: {e}"),
        }
    }

    #[test]
    fn missing_file_returns_tool_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = session::Store::open(dir.path()).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard: Option<crate::Clipboard> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let tool = AnalyzeTrackTool;
        let result = tool
            .invoke(json!({ "path": "/nope/does-not-exist.wav" }), &mut ctx)
            .unwrap();
        assert!(matches!(result, ToolResult::Error(_)));
    }
}
