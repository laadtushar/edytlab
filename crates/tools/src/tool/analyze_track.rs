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

/// What `downbeats` is, said in the result as well as the description (#696):
/// every fourth beat from the first, not a detected bar line.
const DOWNBEATS_SOURCE: &str = "assumed_4_4";

/// What `sections` is (#696): one section for the whole file, not a
/// segmentation.
const SECTIONS_SOURCE: &str = "whole_track";

pub struct AnalyzeTrackTool;

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

impl Tool for AnalyzeTrackTool {
    fn name(&self) -> &'static str {
        "analyze_track"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "analyze_track",
            "Analyse a track in the session, or an audio file, and return BPM, key, beat grid, downbeats, sections, an RMS curve (one bin per ~100 ms), and EBU R128 integrated loudness in LUFS. `sections` is one whole-track section, not detected sections; `downbeats` are assumed every 4th beat from the first beat (4/4), not detected. `bpm_alternatives` gives the half and double of `bpm` with confidences (low `bpm_confidence`: check by ear); `bpm` is null with no steady beat. Give `track` (its index in the session) to analyse a track as it sits on the timeline, before its gain and effects; give `path` only for a file that is not in the session. Pure Rust, no models. Music features use a mono downmix; LUFS uses the original channels.",
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

    // Read-only: it decodes and analyses. `lane_audio_path` may write the
    // content-addressed flattened-track cache, which `Tool::mutates` allows.
    fn mutates(&self) -> bool {
        false
    }

    // Reads the head's audio and appends nothing (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
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

        let tempo_text = match report.bpm {
            Some(bpm) => format!("{bpm:.1} BPM (confidence {:.2})", report.bpm_confidence),
            None => "no steady beat".to_string(),
        };

        Ok(ToolResult::Ok(json!({
            // null when the audio has no steady pulse (a pad, a drone).
            "bpm": report.bpm,
            "bpm_confidence": report.bpm_confidence,
            "bpm_alternatives": report
                .bpm_alternatives
                .iter()
                .map(|a| json!({
                    "bpm": a.bpm,
                    "relation": a.relation.as_str(),
                    "confidence": a.confidence,
                }))
                .collect::<Vec<_>>(),
            "key": report.key,
            "key_score": round2(report.key_score),
            "key_runner_up": report.key_runner_up,
            "key_runner_up_score": round2(report.key_runner_up_score),
            "beats": report.beats,
            "downbeats": report.downbeats,
            "downbeats_source": DOWNBEATS_SOURCE,
            "sections_source": SECTIONS_SOURCE,
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
                "{}: {}, {} ({:.2}; next {} {:.2}), {} beats, integrated loudness {:.1} LUFS",
                audio_path.display(),
                tempo_text,
                report.key,
                report.key_score,
                report.key_runner_up,
                report.key_runner_up_score,
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

    /// `downbeats` and `sections` are placeholders (#696): the description
    /// and the result both say so, so neither can be read as analysis.
    #[test]
    fn the_description_says_sections_and_downbeats_are_placeholders() {
        let s = AnalyzeTrackTool.schema();
        let d = s["description"].as_str().expect("description");
        assert!(
            d.contains("`sections` is one whole-track section, not detected sections"),
            "{d}"
        );
        assert!(
            d.contains(
                "`downbeats` are assumed every 4th beat from the first beat (4/4), not detected"
            ),
            "{d}"
        );
        // The tempo may be an octave out; the description says where the
        // alternatives and the confidence are, and that there may be no beat.
        assert!(
            d.contains("`bpm_alternatives` gives the half and double of `bpm`")
                && d.contains("`bpm_confidence`")
                && d.contains("`bpm` is null with no steady beat"),
            "{d}"
        );
    }

    #[test]
    fn the_result_labels_downbeats_and_sections_as_placeholders() {
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
        match AnalyzeTrackTool
            .invoke(json!({ "path": wav.to_string_lossy() }), &mut ctx)
            .unwrap()
        {
            ToolResult::Ok(v) => {
                assert_eq!(v["downbeats_source"], "assumed_4_4");
                assert_eq!(v["sections_source"], "whole_track");
                // The labels describe what is there: one section, named "track".
                let sections = v["sections"].as_array().expect("sections");
                assert_eq!(sections.len(), 1, "{v}");
                assert_eq!(sections[0]["name"], "track");
            }
            ToolResult::Error(e) => panic!("tool errored: {e}"),
        }
    }

    /// Write `samples` as a mono 16-bit WAV.
    fn write_wav(path: &Path, sample_rate: u32, samples: &[f32]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for s in samples {
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn analyse_path(path: &Path) -> Value {
        let dir = path.parent().unwrap();
        let (mut store, mut engine) = ctx_parts(dir);
        let mut clipboard: Option<crate::Clipboard> = None;
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        match AnalyzeTrackTool
            .invoke(json!({ "path": path.to_string_lossy() }), &mut ctx)
            .unwrap()
        {
            ToolResult::Ok(v) => v,
            ToolResult::Error(e) => panic!("tool errored: {e}"),
        }
    }

    /// A kick on every beat and a hat on every off-beat, as #696 found them
    /// read as double tempo: 90 BPM came out as 179.
    #[test]
    fn kick_and_off_beat_hats_read_at_the_kick_tempo_with_the_double_offered() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("groove.wav");
        let sr = 44_100u32;
        let beat = 60.0f32 / 90.0;
        let mut out = vec![0.0f32; (16.0 * sr as f32) as usize];
        let mut rng = 7u64;
        let mut k = 0;
        while (0.25 + k as f32 * beat) < 15.5 {
            let t = 0.25 + k as f32 * beat;
            let mut phase = 0.0f32;
            for i in 0..(sr as f32 * 0.25) as usize {
                let ts = i as f32 / sr as f32;
                phase += std::f32::consts::TAU * (50.0 + 90.0 * (-ts * 30.0).exp()) / sr as f32;
                let at = (t * sr as f32) as usize + i;
                if at < out.len() {
                    out[at] += 0.8 * (-ts * 14.0).exp() * phase.sin();
                }
            }
            let mut prev = 0.0f32;
            for i in 0..(sr as f32 * 0.04) as usize {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let n = ((rng >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0;
                let at = ((t + beat / 2.0) * sr as f32) as usize + i;
                if at < out.len() {
                    out[at] += 0.25 * (-(i as f32 / sr as f32) * 90.0).exp() * (n - prev) * 0.5;
                }
                prev = n;
            }
            k += 1;
        }
        write_wav(&wav, sr, &out);
        let v = analyse_path(&wav);
        let bpm = v["bpm"].as_f64().expect("bpm");
        assert!(
            (bpm - 90.0).abs() <= 1.5,
            "90 BPM groove read as {bpm}: {v}"
        );
        assert!(v["bpm_confidence"].as_f64().unwrap() >= 0.5, "{v}");
        let alts = v["bpm_alternatives"].as_array().expect("alternatives");
        let double = alts
            .iter()
            .find(|a| a["relation"] == "double")
            .expect("double offered");
        assert!(
            (double["bpm"].as_f64().unwrap() - 180.0).abs() <= 3.0,
            "{v}"
        );
        assert!(v["key_score"].is_number() && v["key_runner_up"].is_string());
        assert!(v["key_runner_up_score"].is_number());
        let summary = v["summary"].as_str().unwrap();
        assert!(
            summary.contains("90.") && summary.contains("confidence"),
            "{summary}"
        );
    }

    /// A pad has no pulse: `bpm` is null and the rest is still returned.
    #[test]
    fn a_pad_has_a_key_and_a_null_bpm() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("pad.wav");
        let sr = 44_100u32;
        let out: Vec<f32> = (0..sr as usize * 6)
            .map(|i| {
                let t = i as f32 / sr as f32;
                // B-flat minor: B-flat3, D-flat4, F4.
                [233.08f32, 277.18, 349.23]
                    .iter()
                    .map(|f| (std::f32::consts::TAU * f * t).sin() / 3.0)
                    .sum::<f32>()
            })
            .collect();
        write_wav(&wav, sr, &out);
        let v = analyse_path(&wav);
        assert!(v["bpm"].is_null(), "{v}");
        assert_eq!(v["bpm_alternatives"].as_array().unwrap().len(), 0, "{v}");
        assert_eq!(v["key"], "A# minor", "{v}");
        assert!(
            v["summary"].as_str().unwrap().contains("no steady beat"),
            "{v}"
        );
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
