//! Publication shared by the signal generators (#770).
//!
//! The file is complete before the session can refer to it, and float WAV
//! preserves the exact samples used in its content-addressed name
//! (`provenance::audio_hash`). Like `generate_tone` and `generate_noise`, it
//! goes in the project's `generated/` folder, which no sweep reclaims: a
//! replay on another OS can compute different samples (libm differences) and
//! so a different name, so the file must never be rebuilt from its op. A run
//! that is discarded after a lost race leaves its file there, as theirs does.

use std::path::{Path, PathBuf};

use serde_json::json;
use session::{Clip, SessionState, Track, TrackId};

use super::util::{append_state, set_session_length};
use crate::{ToolContext, ToolResult};

pub(super) const MAX_DURATION_SEC: f64 = 600.0;

// Ten minutes at 96 kHz: bound the allocation even if the session carries an
// unusually high sample rate. This is at most about 220 MiB of mono samples.
// So the 600 s limit shrinks above 96 kHz.
const MAX_FRAMES: usize = 57_600_000;

pub(super) fn frame_count(sample_rate: u32, duration_sec: f64) -> Result<usize, String> {
    if sample_rate == 0 {
        return Err("session sample rate must be positive".into());
    }
    if !duration_sec.is_finite() || duration_sec <= 0.0 || duration_sec > MAX_DURATION_SEC {
        return Err(format!(
            "duration must be positive and at most {MAX_DURATION_SEC} seconds"
        ));
    }
    let frames = (sample_rate as f64 * duration_sec).round();
    if frames < 1.0 {
        return Err("duration is too short: it rounds to zero frames at this sample rate".into());
    }
    if frames > MAX_FRAMES as f64 {
        return Err(format!(
            "generated audio is limited to {MAX_FRAMES} frames (600 s at 96 kHz, less above that rate)"
        ));
    }
    Ok(frames as usize)
}

fn publish(samples: &[f32], sample_rate: u32, directory: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(directory).map_err(|e| format!("mkdir failed: {e}"))?;
    let path = directory.join(format!(
        "{}.wav",
        crate::provenance::audio_hash(samples, sample_rate, 1)
    ));
    if path.is_file() {
        return Ok(path);
    }
    let temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|e| format!("could not prepare generated audio: {e}"))?;
    audio_engine::write_wav_f32(samples, sample_rate, 1, temporary.path())
        .map_err(|e| format!("write_wav failed: {e}"))?;
    // Content-addressed: a concurrent identical generation writes the same
    // bytes, so a plain atomic rename is safe (and portable, unlike the
    // exclusive form).
    temporary
        .persist(&path)
        .map(|_| path)
        .map_err(|e| format!("could not publish generated audio: {}", e.error))
}

pub(super) fn append_generated(
    ctx: &mut ToolContext,
    mut state: SessionState,
    samples: Vec<f32>,
    name: String,
    action: String,
) -> crate::Result<ToolResult> {
    if state.sample_rate == 0 || samples.is_empty() || samples.len() > MAX_FRAMES {
        return Ok(ToolResult::Error(
            "invalid generated audio length or sample rate".into(),
        ));
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Ok(ToolResult::Error(
            "generated audio contains non-finite samples".into(),
        ));
    }
    let path = match publish(
        &samples,
        state.sample_rate,
        &ctx.store.project_dir().join("generated"),
    ) {
        Ok(path) => path,
        Err(error) => return Ok(ToolResult::Error(error)),
    };
    let length = samples.len() as u64;
    let duration_sec = length as f64 / state.sample_rate as f64;
    let track_index = state.tracks.len();
    state.tracks.push(Track {
        id: TrackId::new(),
        name: name.clone(),
        clips: vec![Clip {
            source_path: path,
            start_in_track: 0,
            source_offset: 0,
            length,
            content_hash: Some(crate::provenance::legacy_sample_hash(&samples)),
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
            volume_envelope_frames: Default::default(),
        }],
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        soloed: false,
        effects: Vec::new(),
        sends: Vec::new(),
    });
    set_session_length(&mut state);
    let node = match append_state(ctx, state, action) {
        Ok(node) => node,
        Err(error) => return Ok(ToolResult::Error(error)),
    };
    Ok(ToolResult::Ok(json!({
        "node_id": node.to_hex(),
        "track_index": track_index,
        "duration_sec": duration_sec,
        "summary": format!("Generated {name} ({duration_sec:.3}s) as track {track_index}")
    })))
}
