//! Shared helpers for individual tools.
//!
//! Tools that mutate state need to (1) load the current head's state,
//! (2) clone-and-modify it, and (3) append a new node. Argument
//! validation also has a few common patterns (track index in range,
//! sample range well-formed). Centralised here so each tool stays
//! focused on its semantics.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;
use session::{NodeId, SessionNode, SessionState, Track};

use crate::{ToolContext, ToolResult};

/// Load the current head's [`SessionState`]. Returns `Err(message)`
/// shaped for [`crate::ToolResult::Error`] when there is no head or the
/// store can't materialise the node.
pub(crate) fn load_head_state(ctx: &ToolContext) -> Result<SessionState, String> {
    let head = ctx
        .store
        .head()
        .ok_or_else(|| "no session loaded; call `load` first".to_string())?;
    let node = ctx
        .store
        .get(head)
        .map_err(|e| format!("failed to read head node: {e}"))?;
    Ok(node.state)
}

/// Wrap `state` in a [`SessionNode`] and append it to the store. The
/// store overwrites `parent` and `id`, so we leave them at sentinel
/// values.
pub(crate) fn append_state(
    ctx: &mut ToolContext,
    state: SessionState,
    label: impl Into<String>,
) -> Result<NodeId, String> {
    let node = SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: Utc::now(),
        label: Some(label.into()),
        reasoning: None,
        state,
    };
    ctx.store
        .append(node)
        .map_err(|e| format!("session append failed: {e}"))
}

/// Look up `track_index` against `tracks`, producing an actionable
/// error message when out of range (matches the format pinned by the
/// M08 acceptance criteria).
pub(crate) fn check_track_index(tracks: &[Track], track_index: usize) -> Result<(), String> {
    if track_index >= tracks.len() {
        return Err(format!(
            "track index {track_index} out of range; session has {} track{}",
            tracks.len(),
            if tracks.len() == 1 { "" } else { "s" },
        ));
    }
    Ok(())
}

/// Run a destructive sample-buffer edit against the first clip of
/// `state.tracks[track_idx]`, write the result to a CAS-addressed WAV
/// under the source's sibling `derived/` directory, swap the clip to
/// point at the new file, and append a new session node.
///
/// The `edit_fn` receives the clip's interleaved sample window and the
/// source sample rate. It mutates the buffer in place (length changes
/// allowed — `insert_silence` extends, the others preserve length).
///
/// Returns a [`ToolResult::Ok`] with `{ node_id, summary }` on success
/// or a [`ToolResult::Error`] with a human-readable message on any
/// validation / IO failure. The dispatcher contract is "all tool-level
/// failures are surfaced as `ToolResult::Error`", same as `gain` and
/// `cut_range`.
pub(crate) fn destructive_edit<F>(
    ctx: &mut ToolContext,
    track_idx: usize,
    edit_fn: F,
    label: impl Into<String>,
) -> ToolResult
where
    F: FnOnce(&mut Vec<f32>, u32),
{
    // The overwhelming majority of edits leave the channel layout
    // alone, so they keep the two-argument closure and simply hand the
    // incoming count straight back.
    destructive_edit_rechannel(
        ctx,
        track_idx,
        |samples, sample_rate, channels| {
            edit_fn(samples, sample_rate);
            channels
        },
        label,
    )
}

/// [`destructive_edit`] for edits that change the channel layout.
///
/// The closure receives the source's channel count and returns the
/// count its buffer now has. That return value is what gets written
/// into the WAV header and used to recompute the clip length — the
/// plain `destructive_edit` always wrote the *source's* count, so a
/// tool that halved or doubled the buffer produced a file whose header
/// disagreed with its contents. Playback then reinterprets the frames:
/// half the samples under a stereo header plays twice as fast and an
/// octave high, and twice the samples under a mono header plays half
/// as fast and an octave low.
pub(crate) fn destructive_edit_rechannel<F>(
    ctx: &mut ToolContext,
    track_idx: usize,
    edit_fn: F,
    label: impl Into<String>,
) -> ToolResult
where
    F: FnOnce(&mut Vec<f32>, u32, u16) -> u16,
{
    let label = label.into();

    let mut state = match load_head_state(ctx) {
        Ok(s) => s,
        Err(msg) => return ToolResult::Error(msg),
    };

    if let Err(msg) = check_track_index(&state.tracks, track_idx) {
        return ToolResult::Error(msg);
    }

    let Some(clip) = state.tracks[track_idx].clips.first().cloned() else {
        return ToolResult::Error(format!("track {track_idx} has no clips; nothing to edit"));
    };

    // Decode the source WAV into interleaved f32. The audio-decoder
    // returns the entire file regardless of clip window, so we slice
    // down to `[source_offset, source_offset + length)` in frames.
    let decoded = match audio_decoder::decode_file(&clip.source_path) {
        Ok(d) => d,
        Err(e) => {
            return ToolResult::Error(format!(
                "failed to decode {}: {e}",
                clip.source_path.display()
            ))
        }
    };
    let sample_rate = decoded.sample_rate;
    let channels = decoded.channels;
    if channels == 0 {
        return ToolResult::Error("source has zero channels".into());
    }
    let stride = channels as usize;
    let total_frames = (decoded.samples.len() / stride) as u64;
    let src_start = clip.source_offset.min(total_frames);
    let src_end = clip
        .source_offset
        .saturating_add(clip.length)
        .min(total_frames);
    let start_idx = (src_start as usize) * stride;
    let end_idx = (src_end as usize) * stride;
    let mut window: Vec<f32> = decoded.samples[start_idx..end_idx].to_vec();

    // Apply the user-provided edit. It reports the channel count its
    // buffer now has, which may differ from the source's.
    let channels_out = edit_fn(&mut window, sample_rate, channels).max(1);
    let stride_out = channels_out as usize;

    // CAS-address the result under `<source_dir>/derived/<hash>.wav`.
    let parent: &Path = clip.source_path.parent().unwrap_or_else(|| Path::new("."));
    let derived_dir: PathBuf = parent.join("derived");
    if let Err(e) = std::fs::create_dir_all(&derived_dir) {
        return ToolResult::Error(format!(
            "failed to create derived dir {}: {e}",
            derived_dir.display()
        ));
    }

    // Hash the post-edit interleaved samples. We serialize each f32 as
    // little-endian bytes so the hash is deterministic across platforms
    // and across rustc versions (no transmute / no endianness assumption).
    let mut hasher = blake3::Hasher::new();
    for s in &window {
        hasher.update(&s.to_le_bytes());
    }
    let hash = hasher.finalize();
    let hash_hex = hash.to_hex().to_string();
    let cas_path = derived_dir.join(format!("{hash_hex}.wav"));

    if !cas_path.exists() {
        if let Err(e) = audio_engine::write_wav(&window, sample_rate, channels_out, &cas_path) {
            return ToolResult::Error(format!(
                "failed to write CAS wav {}: {e}",
                cas_path.display()
            ));
        }
    }

    // Update the clip in place: point at the new source, zero offset,
    // length recomputed from the post-edit buffer.
    let new_length_frames = (window.len() / stride_out) as u64;
    let clip_mut = &mut state.tracks[track_idx].clips[0];
    clip_mut.source_path = cas_path;
    clip_mut.source_offset = 0;
    clip_mut.length = new_length_frames;
    clip_mut.content_hash = Some(*hash.as_bytes());

    // Recompute `length_samples` as the max of every track's max-clip
    // length. This matches the convention used elsewhere in the
    // dispatcher (cut_range tracks length deltas; gain leaves it alone;
    // here the clip itself changes length, so a fresh max is safest).
    state.length_samples = state
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.start_in_track + c.length))
        .max()
        .unwrap_or(0);

    let new_id = match append_state(ctx, state, label.clone()) {
        Ok(id) => id,
        Err(msg) => return ToolResult::Error(msg),
    };

    ToolResult::Ok(json!({
        "node_id": new_id.to_hex(),
        "summary": label,
    }))
}

/// Interleave stride of the first clip on `track_idx`, read from the
/// decoded source.
///
/// `destructive_edit` hands its closure only `(samples, sample_rate)`,
/// so a tool that needs to convert seconds into an index has to learn
/// the channel count separately. Getting this wrong is not a subtle
/// error: indexing an interleaved stereo buffer as if it were mono
/// covers half the requested span and, when the length lands odd,
/// swaps left and right for everything after it.
pub(crate) fn track_channels(ctx: &mut ToolContext, track_idx: usize) -> Result<usize, String> {
    let state = load_head_state(ctx)?;
    check_track_index(&state.tracks, track_idx)?;
    let clip = state.tracks[track_idx]
        .clips
        .first()
        .ok_or_else(|| format!("track {track_idx} has no clips"))?;
    let decoded = audio_decoder::decode_file(&clip.source_path)
        .map_err(|e| format!("failed to decode {}: {e}", clip.source_path.display()))?;
    Ok((decoded.channels as usize).max(1))
}

/// Reject a `[start_sec, end_sec)` window that is reversed or not a
/// finite number, before it reaches slice arithmetic.
///
/// Tools that take bare `start_sec` / `end_sec` were clamping each end
/// to the buffer length *independently*, which leaves `start > end`
/// intact for a reversed window — and `samples[start..end]` then
/// panics, taking the whole app down. Asking for `start_sec: 10,
/// end_sec: 5` is an easy slip for a model to make, so this is reported
/// the way every other bad argument is, rather than silently treated as
/// an empty selection that hides the mistake.
pub(crate) fn check_seconds_order(start_sec: f64, end_sec: f64) -> Result<(), String> {
    if !start_sec.is_finite() || !end_sec.is_finite() {
        return Err(format!(
            "invalid range: start_sec ({start_sec}) and end_sec ({end_sec}) must be finite numbers"
        ));
    }
    if start_sec < 0.0 || end_sec < 0.0 {
        return Err(format!(
            "invalid range: start_sec ({start_sec}) and end_sec ({end_sec}) must not be negative"
        ));
    }
    if start_sec >= end_sec {
        return Err(format!(
            "invalid range: start_sec ({start_sec}) must be < end_sec ({end_sec})"
        ));
    }
    Ok(())
}

/// [`check_seconds_order`] for tools whose bounds are optional. A
/// missing bound means "from the start" / "to the end", so only a pair
/// that is present on both sides can be out of order.
pub(crate) fn check_optional_seconds_order(
    start_sec: Option<f64>,
    end_sec: Option<f64>,
) -> Result<(), String> {
    match (start_sec, end_sec) {
        (Some(s), Some(e)) => check_seconds_order(s, e),
        (Some(s), None) | (None, Some(s)) if !s.is_finite() || s < 0.0 => Err(format!(
            "invalid range: {s} must be a finite, non-negative number of seconds"
        )),
        _ => Ok(()),
    }
}

/// Validate `[start, end)` against a track's total length. Returns the
/// pair as `(usize, usize)` to make downstream slice math less noisy.
pub(crate) fn check_sample_range(
    start: u64,
    end: u64,
    track_length: u64,
) -> Result<(u64, u64), String> {
    if start >= end {
        return Err(format!(
            "invalid range: start_sample ({start}) must be < end_sample ({end})"
        ));
    }
    if end > track_length {
        return Err(format!(
            "end_sample ({end}) exceeds track length ({track_length})"
        ));
    }
    Ok((start, end))
}

// ---------------------------------------------------------------------------
// Biquad filter
// ---------------------------------------------------------------------------

/// Direct Form II biquad filter state (per channel).
pub(crate) struct BiquadState {
    pub z1: f32,
    pub z2: f32,
}

impl BiquadState {
    pub(crate) fn new() -> Self {
        Self { z1: 0.0, z2: 0.0 }
    }
}

/// Biquad coefficients [b0, b1, b2, a1, a2] (a0 normalised to 1).
pub(crate) struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

/// Normalised angular frequency for a biquad, held safely below Nyquist.
///
/// The coefficient formulas below assume `0 < w0 < π`. At or above
/// Nyquist `sin(w0)` goes to zero and then negative, which flips the
/// sign of `alpha` and pushes the filter's poles outside the unit
/// circle — it stops attenuating and starts diverging exponentially,
/// so the render saturates into a full-scale square wave. Asking a
/// 44.1 kHz track for a 30 kHz low-pass is an easy thing for a model to
/// do, and the intent ("pass everything") is clear, so the frequency is
/// clamped rather than rejected.
fn safe_w0(freq_hz: f32, sample_rate: u32) -> f32 {
    let sr = sample_rate.max(1) as f32;
    // 0.45·sr keeps a little headroom below Nyquist, where the bilinear
    // transform's frequency warping is still well behaved.
    let ceiling = sr * 0.45;
    let clamped = if freq_hz.is_finite() {
        freq_hz.clamp(1.0, ceiling.max(1.0))
    } else {
        ceiling.max(1.0)
    };
    2.0 * std::f32::consts::PI * clamped / sr
}

impl BiquadCoeffs {
    /// Second-order Butterworth high-pass filter.
    pub(crate) fn high_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        let w0 = safe_w0(cutoff_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * 0.707_f32);
        let cos_w0 = w0.cos();
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Second-order Butterworth low-pass filter.
    pub(crate) fn low_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        let w0 = safe_w0(cutoff_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * 0.707_f32);
        let cos_w0 = w0.cos();
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Notch (band-reject) filter.
    pub(crate) fn notch(center_hz: f32, q: f32, sample_rate: u32) -> Self {
        let w0 = safe_w0(center_hz, sample_rate);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = 1.0;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

/// Process interleaved `samples` in-place with a biquad filter.
/// Only processes the frame range [start_frame, end_frame).
pub(crate) fn biquad_process(
    samples: &mut [f32],
    channels: usize,
    coeffs: &BiquadCoeffs,
    start_frame: usize,
    end_frame: usize,
) {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let end = end_frame.min(total_frames);
    let start = start_frame.min(end);
    let mut states: Vec<BiquadState> = (0..channels).map(|_| BiquadState::new()).collect();
    for frame in start..end {
        let base = frame * channels;
        for (ch, st) in states.iter_mut().enumerate() {
            let idx = base + ch;
            let x = samples[idx];
            let y = coeffs.b0 * x + st.z1;
            st.z1 = coeffs.b1 * x - coeffs.a1 * y + st.z2;
            st.z2 = coeffs.b2 * x - coeffs.a2 * y;
            samples[idx] = y;
        }
    }
}

#[cfg(test)]
mod biquad_tests {
    use super::{biquad_process, BiquadCoeffs};

    #[test]
    fn high_pass_attenuates_dc() {
        let mut samples = vec![1.0f32; 4410]; // 0.1s at 44100
        let coeffs = BiquadCoeffs::high_pass(1000.0, 44100);
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        let tail_mean: f32 = samples[4000..].iter().sum::<f32>() / 410.0;
        assert!(
            tail_mean.abs() < 0.01,
            "DC should be attenuated by HPF, got {tail_mean}"
        );
    }

    /// A cutoff above Nyquist must not turn the filter into an oscillator.
    ///
    /// Without the clamp the poles land outside the unit circle and the
    /// output grows exponentially: by the end of a tenth of a second the
    /// samples are astronomically large, and the render clips into a
    /// full-scale square wave.
    #[test]
    fn low_pass_above_nyquist_stays_bounded() {
        let mut samples = vec![0.5f32; 4410];
        let coeffs = BiquadCoeffs::low_pass(30_000.0, 44_100);
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak.is_finite() && peak <= 1.0,
            "cutoff above Nyquist must stay bounded, got peak {peak}"
        );
        // Clamping to just under Nyquist means "pass everything", so the
        // signal should still be recognisably itself rather than silence.
        let tail_mean: f32 = samples[4000..].iter().sum::<f32>() / 410.0;
        assert!(
            (tail_mean - 0.5).abs() < 0.05,
            "a low-pass above Nyquist should pass the signal, got {tail_mean}"
        );
    }

    #[test]
    fn high_pass_above_nyquist_stays_bounded() {
        let mut samples = vec![0.5f32; 4410];
        let coeffs = BiquadCoeffs::high_pass(30_000.0, 44_100);
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak.is_finite() && peak <= 1.0,
            "cutoff above Nyquist must stay bounded, got peak {peak}"
        );
    }

    #[test]
    fn notch_above_nyquist_stays_bounded() {
        let mut samples = vec![0.5f32; 4410];
        let coeffs = BiquadCoeffs::notch(30_000.0, 1.0, 44_100);
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak.is_finite() && peak <= 1.0,
            "centre above Nyquist must stay bounded, got peak {peak}"
        );
    }

    /// A non-finite frequency must not produce NaN coefficients, which
    /// would poison the whole buffer through the filter's delay line.
    #[test]
    fn non_finite_cutoff_yields_finite_coefficients() {
        for freq in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            let mut samples = vec![0.5f32; 512];
            let coeffs = BiquadCoeffs::low_pass(freq, 44_100);
            biquad_process(&mut samples, 1, &coeffs, 0, 512);
            assert!(
                samples.iter().all(|s| s.is_finite()),
                "cutoff {freq} produced non-finite output"
            );
        }
    }

    #[test]
    fn low_pass_passes_dc() {
        let mut samples = vec![1.0f32; 4410];
        let coeffs = BiquadCoeffs::low_pass(5000.0, 44100);
        biquad_process(&mut samples, 1, &coeffs, 0, 4410);
        // DC (0 Hz) should pass through low-pass — tail should be near 1.0
        let tail_mean: f32 = samples[4000..].iter().sum::<f32>() / 410.0;
        assert!(
            tail_mean > 0.9,
            "DC should pass through LPF, got {tail_mean}"
        );
    }
}
