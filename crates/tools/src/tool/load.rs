//! `load` tool — decode a file and add a new track to the session.
//!
//! Phase 1 simplification was: `load` REPLACES the head's session state
//! with a fresh single-track session. M21 lifts that — a `load` against
//! an existing session APPENDS a new track. There is still no head?
//! Then we create a new session as before. The session-rate stays at the
//! first-loaded source's rate (subsequent loads at a different rate are
//! resampled to the project rate at render time, per M21 acceptance #3).
//!
//! Render-graph invariant: the M22 streaming renderer opens every clip
//! source via `WavStreamReader` (hound, WAV-only). Symphonia decodes far
//! more formats (mp3, flac, ogg, m4a, ...), so a load of a non-WAV
//! source would resolve here but blow up at render with
//! "Ill-formed WAVE file: no RIFF tag found". To keep the renderer
//! contract honest, anything that fails to open as a WAV is transcoded
//! to a CAS-addressed sibling WAV under `<source_dir>/derived/<hash>.wav`
//! and the clip is pointed at that path instead.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use session::{
    BusGraph, Clip, KeyMap, SessionNode, SessionState, TempoMap, Track, TrackId, Transcript,
};

use audio_decoder::{DecodedAudio, WavStreamReader};

use crate::schema::{anthropic_tool, object_schema};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
}

pub struct LoadTool;

impl Tool for LoadTool {
    fn name(&self) -> &'static str {
        "load"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "load",
            "Decode an audio file and add it to the session as a new track. With no current head this creates a fresh single-track session; otherwise the file is appended as a new track on the current head, leaving existing tracks intact. Returns the new session node id, the new track's index, and the source's sample rate, length, and channel count.",
            object_schema(&[("path", "string", true)]),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let path = PathBuf::from(&args.path);
        if !path.exists() {
            return Ok(ToolResult::Error(format!(
                "file not found: {}",
                path.display()
            )));
        }

        let decoded = match audio_decoder::decode_file(&path) {
            Ok(d) => d,
            Err(e) => return Ok(ToolResult::Error(format!("decode failed: {e}"))),
        };

        let chans = decoded.channels as usize;
        if chans == 0 {
            return Ok(ToolResult::Error("decoded source has 0 channels".into()));
        }
        let length_samples = (decoded.samples.len() / chans) as u64;

        // The renderer opens clip sources via the WAV-only streaming
        // reader. If the original file isn't a WAV the streaming reader
        // can open, transcode `decoded` to a CAS-addressed sibling WAV
        // and point the clip there. WAV sources that the streaming
        // reader accepts are used as-is so the byte-identity guarantee
        // for unity-passthrough renders still holds.
        let source_path = match ensure_streamable_wav(&path, &decoded) {
            Ok(p) => p,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };

        let track = Track {
            id: TrackId::new(),
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("track")
                .to_string(),
            clips: vec![Clip {
                source_path,
                start_in_track: 0,
                source_offset: 0,
                length: length_samples,
                content_hash: None,
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
            }],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: Vec::new(),
            sends: Vec::new(),
        };

        // If there's already a head, append; otherwise create a fresh session.
        let (state, track_index) = match ctx.store.head() {
            Some(head) => match ctx.store.get(head) {
                Ok(node) => {
                    let mut s = node.state;
                    let new_index = s.tracks.len();
                    s.tracks.push(track);
                    // The session's `length_samples` tracks the longest track
                    // so renders know the project's overall bound. Project
                    // sample rate is whatever the first load established —
                    // mismatched sources are resampled at render time.
                    if length_samples > s.length_samples {
                        s.length_samples = length_samples;
                    }
                    (s, new_index)
                }
                Err(e) => return Ok(ToolResult::Error(format!("failed to read head node: {e}"))),
            },
            None => {
                let s = SessionState {
                    tracks: vec![track],
                    bus_routing: BusGraph::default(),
                    master_chain: Vec::new(),
                    tempo_map: TempoMap::default(),
                    key_map: None::<KeyMap>,
                    transcript: None::<Transcript>,
                    sample_rate: decoded.sample_rate,
                    length_samples,
                    annotations: Vec::new(),
                };
                (s, 0)
            }
        };

        let node = SessionNode {
            // `parent` and `id` are overwritten by `Store::append`.
            id: session::NodeId([0u8; 32]),
            parent: None,
            created_at: Utc::now(),
            label: Some(format!("load {}", path.display())),
            reasoning: None,
            state,
            op: None,
        };

        let id = match ctx.store.append(node) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(format!("session append failed: {e}"))),
        };

        Ok(ToolResult::Ok(json!({
            "node_id": id.to_hex(),
            "track_index": track_index,
            "sample_rate": decoded.sample_rate,
            "length_samples": length_samples,
            "channels": decoded.channels,
            "summary": format!(
                "Loaded {} ({} ch, {} Hz, {} samples) as track {} on session head {}",
                path.display(),
                decoded.channels,
                decoded.sample_rate,
                length_samples,
                track_index,
                id.to_hex(),
            ),
        })))
    }
}

/// Return a path to a WAV the M22 streaming renderer can read.
///
/// If `src` already opens cleanly via `WavStreamReader`, returns it
/// unchanged so the unity-passthrough byte-identity path still applies.
/// Otherwise, writes `decoded` to a CAS-addressed sibling WAV at
/// `<src.parent>/derived/<hash>.wav` and returns that path. The hash is
/// over sample-rate, channel count and little-endian sample bytes, so
/// the same audio always lands on the same name — whether it is the same
/// file re-loaded or a copy of it from somewhere else.
fn ensure_streamable_wav(src: &Path, decoded: &DecodedAudio) -> Result<PathBuf, String> {
    if WavStreamReader::open(src).is_ok() {
        return Ok(src.to_path_buf());
    }

    let parent: &Path = src.parent().unwrap_or_else(|| Path::new("."));
    let derived_dir = parent.join("derived");
    std::fs::create_dir_all(&derived_dir).map_err(|e| {
        format!(
            "failed to create derived dir {}: {e}",
            derived_dir.display()
        )
    })?;

    // Pre-convert samples to LE bytes in one buffer so blake3 sees a
    // single contiguous slice. Per-f32 `hasher.update` calls are
    // measurably slow for multi-minute files; the byte form here keeps
    // the same cross-platform determinism convention as
    // `destructive_edit` (each sample as LE bytes) without paying per-
    // sample call overhead.
    let mut bytes = Vec::with_capacity(decoded.samples.len() * 4);
    for s in &decoded.samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    // The source path is deliberately NOT hashed (#160). It used to be,
    // which meant the same audio imported from two locations — a copy, a
    // move, a re-download — produced two different names and was stored
    // twice, at roughly 55 MB per re-import for a five-minute stereo
    // file. That defeats content addressing for exactly the case it
    // exists to serve: two files whose samples, rate and channel count
    // all match *are* the same audio for every purpose this store has.
    //
    // Dropping it does not risk collisions. The hash still covers every
    // property of the decoded audio, so two genuinely different files
    // cannot land on one name without colliding blake3 itself.
    let mut hasher = blake3::Hasher::new();
    hasher.update(&decoded.sample_rate.to_le_bytes());
    hasher.update(&(decoded.channels as u32).to_le_bytes());
    hasher.update(&bytes);
    let cas_path = derived_dir.join(format!("{}.wav", hasher.finalize().to_hex()));

    if !cas_path.exists() {
        audio_engine::write_wav(
            &decoded.samples,
            decoded.sample_rate,
            decoded.channels,
            &cas_path,
        )
        .map_err(|e| {
            format!(
                "failed to transcode {} -> {}: {e}",
                src.display(),
                cas_path.display()
            )
        })?;
    }

    Ok(cas_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_wav(path: &Path, samples: &[f32], sr: u32, ch: u16) {
        let spec = hound::WavSpec {
            channels: ch,
            sample_rate: sr,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            w.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn wav_source_is_passed_through() {
        let dir = TempDir::new().unwrap();
        let wav = dir.path().join("a.wav");
        write_wav(&wav, &[0.0; 1024], 44_100, 1);
        let decoded = audio_decoder::decode_file(&wav).unwrap();
        let out = ensure_streamable_wav(&wav, &decoded).unwrap();
        assert_eq!(out, wav, "real WAV must not be re-encoded");
    }

    #[test]
    fn non_wav_source_is_transcoded_to_cas_wav() {
        let dir = TempDir::new().unwrap();
        // A bogus path that pretends to be mp3. `decode_file` is not
        // called here — we hand-build a `DecodedAudio` and rely on the
        // `WavStreamReader::open` probe to fail because the file is
        // either absent or not a WAV.
        let fake = dir.path().join("song.mp3");
        std::fs::write(&fake, b"not a wav").unwrap();
        let decoded = DecodedAudio {
            samples: vec![0.0; 2048],
            sample_rate: 48_000,
            channels: 2,
        };
        let out = ensure_streamable_wav(&fake, &decoded).unwrap();
        assert_ne!(out, fake, "non-WAV source must be transcoded");
        // The renderer must be able to open the result.
        WavStreamReader::open(&out).expect("transcoded file is a valid WAV");
        assert!(out.starts_with(dir.path().join("derived")));
    }
}
