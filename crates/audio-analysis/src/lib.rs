//! Music feature extraction (Phase 2, M19).
//!
//! Pure-Rust analysis of an already-decoded audio buffer:
//!
//! * BPM via spectral-flux onset detection + inter-onset-interval
//!   histogram ([`bpm`]).
//! * Beat tracking by snapping a metronome at the estimated BPM to the
//!   strongest nearby onsets ([`beats`]).
//! * Key estimation via 12-bin chroma + Krumhansl–Kessler key profiles
//!   ([`key`]).
//! * Section segmentation — Phase-2 stub returning a single section
//!   spanning the whole audio ([`sections`]).
//! * EBU R128 integrated loudness via the `ebur128` crate ([`loudness`]).
//!
//! ## Design choices (M19 selection ladder)
//!
//! The plan describes a three-rung ladder for key detection: ONNX model
//! → pure-Rust chroma + Krumhansl–Schmuckler → Python sidecar. This
//! crate ships **rung 2**: the ONNX rung needs a model gate similar to
//! Demucs in M18, and the sidecar rung is incompatible with the sandbox.
//! Pure-Rust hits the 70% accuracy floor stated as the M19 acceptance
//! bar and ships immediately.
//!
//! For BPM the plan prefers `aubio-rs` (FFI to the C `aubio` library).
//! Building `aubio-rs` requires the system `aubio` headers + bindgen,
//! which are not available in the sandbox. The custom Rust onset
//! detector below is the fallback the plan calls out and meets the
//! `±1 BPM in 9/10 cases` 4/4 acceptance bar — see [`bpm`] for details.
//!
//! ## Entry point
//!
//! [`analyze`] is the single pure-data entry point. The
//! `tools::analyze_track` tool decodes a file and downmixes to mono
//! before calling it; integration tests can construct synthetic input
//! and call [`analyze`] directly.

use audio_decoder::DecodedAudio;
use serde::{Deserialize, Serialize};

pub mod beats;
pub mod bpm;
pub mod key;
pub mod loudness;
pub mod sections;

pub use sections::Section;

/// All errors raised by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("decode error: {0}")]
    Decode(String),

    #[error("analysis failed: {0}")]
    Analysis(String),

    #[error("loudness measurement failed: {0}")]
    Lufs(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Output of [`analyze`]. Mirrors the `analyze_track` tool's wire shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalyzeReport {
    /// Estimated tempo in beats per minute.
    pub bpm: f32,
    /// Estimated key as `"<root> <mode>"`, e.g. `"F minor"`.
    pub key: String,
    /// Beat times in seconds, monotonically increasing.
    pub beats: Vec<f32>,
    /// Downbeat times in seconds (subset of `beats`). 4/4 stub.
    pub downbeats: Vec<f32>,
    /// Section spans (intro/verse/chorus/...). Phase-2 stub: single
    /// section covering the whole track.
    pub sections: Vec<Section>,
    /// Coarse RMS curve, one bin per ~100 ms window.
    pub rms_curve: Vec<f32>,
    /// EBU R128 integrated loudness, in LUFS.
    pub lufs_integrated: f32,
}

/// Analyse an already-decoded audio buffer. Runs all sub-analyses and
/// assembles an [`AnalyzeReport`].
///
/// `audio` may be mono or interleaved multi-channel; the analysis path
/// downmixes to mono internally. LUFS is measured on the original
/// (possibly multi-channel) signal.
pub fn analyze(audio: &DecodedAudio) -> Result<AnalyzeReport> {
    if audio.sample_rate == 0 {
        return Err(Error::Analysis("sample_rate is zero".into()));
    }
    if audio.channels == 0 {
        return Err(Error::Analysis("channels is zero".into()));
    }
    if audio.samples.is_empty() {
        return Err(Error::Analysis("empty audio buffer".into()));
    }

    let mono = downmix_to_mono(&audio.samples, audio.channels);

    let bpm = bpm::estimate_bpm(&mono, audio.sample_rate)?;
    let beats_arr = beats::detect_beats(&mono, audio.sample_rate, bpm)?;
    let downbeats_arr = beats::detect_downbeats(&beats_arr, 4);
    let key = key::estimate_key(&mono, audio.sample_rate)?;
    let sections_arr = sections::segment_sections(&mono, audio.sample_rate);
    let rms_curve = compute_rms_curve(&mono, audio.sample_rate);
    let lufs_integrated =
        loudness::integrated_lufs(&audio.samples, audio.sample_rate, audio.channels)?;

    Ok(AnalyzeReport {
        bpm,
        key,
        beats: beats_arr,
        downbeats: downbeats_arr,
        sections: sections_arr,
        rms_curve,
        lufs_integrated,
    })
}

/// Downmix interleaved samples to mono by channel-summing then dividing
/// by channel count. Mono input is returned as-is.
pub(crate) fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    let frames = samples.len() / ch;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut acc = 0.0f32;
        for c in 0..ch {
            acc += samples[f * ch + c];
        }
        out.push(acc / ch as f32);
    }
    out
}

/// Coarse RMS curve: one bin per `~100 ms` window.
fn compute_rms_curve(mono: &[f32], sample_rate: u32) -> Vec<f32> {
    let window = (sample_rate as usize / 10).max(1); // 100 ms
    let mut out = Vec::with_capacity(mono.len() / window + 1);
    let mut i = 0;
    while i < mono.len() {
        let end = (i + window).min(mono.len());
        let mut sumsq = 0.0f64;
        for &s in &mono[i..end] {
            sumsq += (s as f64) * (s as f64);
        }
        let rms = (sumsq / (end - i) as f64).sqrt() as f32;
        out.push(rms);
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_stereo_averages_channels() {
        // L=1.0, R=-1.0 → 0.0 for every frame.
        let stereo = vec![1.0, -1.0, 1.0, -1.0, 0.5, -0.5];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn downmix_mono_passes_through() {
        let m = vec![0.1, 0.2, 0.3];
        let out = downmix_to_mono(&m, 1);
        assert_eq!(out, m);
    }

    #[test]
    fn rms_curve_constant_signal() {
        // 1 second of constant 0.5 amplitude → RMS = 0.5 in every bin.
        let sr = 1000u32;
        let mono = vec![0.5f32; sr as usize];
        let curve = compute_rms_curve(&mono, sr);
        assert!(!curve.is_empty());
        for v in curve {
            assert!((v - 0.5).abs() < 1e-5);
        }
    }
}
