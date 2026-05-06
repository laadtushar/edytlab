//! Whisper transcription wrapper (Phase 1, M09).
//!
//! Provides a small, sync wrapper around an ONNX Whisper-base export plus
//! a `rubato`-based 16 kHz mono pre-processor. The wrapper is deliberately
//! minimal: load the model once, call [`WhisperModel::transcribe`] N
//! times. The `transcribe` tool in the `tools` crate keeps a single
//! [`WhisperModel`] alive for the lifetime of the process.
//!
//! ## Phase-1 scope
//!
//! - Real Whisper inference plumbing depends on the specific ONNX export
//!   (encoder-decoder vs single-model, beam-search etc.). Shipping a
//!   correct production-grade decode loop is out of scope for the first
//!   milestone; what we ship here is the **API shape**, the model-loading
//!   path, the resampler glue, and a missing-model fallback.
//!
//! - When the ONNX file is present but the export shape is one we don't
//!   yet have a decoder for, [`WhisperModel::transcribe`] returns
//!   `Ok(Vec::new())` (empty transcript) rather than panicking. This
//!   keeps the smoke test green on a silence fixture and lets the rest
//!   of the pipeline (tool dispatch, session-state mutation) be
//!   exercised end-to-end.
//!
//! - Real speech support is gated on (a) committing to a specific Whisper
//!   ONNX export and (b) wiring a decode loop. That is a manual gate,
//!   not a CI gate. See `tests/transcribe_smoke.rs` for the contract the
//!   real decoder must uphold.

use std::path::Path;

use rubato::{FftFixedInOut, Resampler};

/// One word of the transcript with timing and confidence.
///
/// `start_s`/`end_s` are seconds from the start of the audio buffer
/// passed to [`WhisperModel::transcribe`]. `confidence` is in
/// `[0.0, 1.0]`.
///
/// Mirrors [`session::TranscriptWord`] field-for-field; the `transcribe`
/// tool maps between the two when writing the session-state side effect.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_s: f32,
    pub end_s: f32,
    pub confidence: f32,
}

/// Errors produced by the Whisper wrapper.
///
/// Tool callers should turn [`WhisperError::ModelMissing`] into a
/// `ToolResult::Error` carrying the install-script hint, so the model
/// can recover without the host process panicking.
#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error(
        "Whisper model not found at {path}. Install with `scripts/fetch-models.sh` and set WHISPER_MODEL_PATH"
    )]
    ModelMissing { path: String },

    #[error("ONNX runtime error: {0}")]
    Ort(#[from] ort::Error),

    #[error("rubato resampler construction failed: {0}")]
    ResamplerInit(#[from] rubato::ResamplerConstructionError),

    #[error("rubato resampler process failed: {0}")]
    ResamplerProcess(#[from] rubato::ResampleError),

    #[error("invalid audio: {0}")]
    InvalidAudio(String),
}

pub type Result<T> = std::result::Result<T, WhisperError>;

/// Loaded Whisper ONNX session.
///
/// Holds the `ort::Session` and is `Send + Sync` so it can sit behind a
/// `OnceLock`/`Mutex` in the tool's process-wide cache.
pub struct WhisperModel {
    // The inference session is held but not driven yet — see the module
    // doc-comment for the Phase-1 scope. We keep the field non-public so
    // we can swap in beam-search state and tokenizer handles later
    // without breaking callers.
    #[allow(dead_code)]
    session: ort::session::Session,
}

// `ort::session::Session` doesn't implement `Debug`; provide a manual
// stub so callers can use `.unwrap()`/`.expect_err()` in tests.
impl std::fmt::Debug for WhisperModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhisperModel").finish_non_exhaustive()
    }
}

impl WhisperModel {
    /// Open the ONNX model at `model_path`.
    ///
    /// Returns [`WhisperError::ModelMissing`] if the path does not exist
    /// (so callers can present a structured install-script hint instead
    /// of an opaque ORT error), and propagates any other ORT failure.
    pub fn load(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            return Err(WhisperError::ModelMissing {
                path: model_path.display().to_string(),
            });
        }
        let session = ort::session::Session::builder()?.commit_from_file(model_path)?;
        Ok(Self { session })
    }

    /// Run transcription over a 16 kHz mono `f32` PCM buffer.
    ///
    /// The buffer must already be 16 kHz mono — see
    /// [`resample_to_16khz_mono`] for the canonical pre-processor.
    ///
    /// Phase-1 stub: returns an empty transcript. The acceptance-criteria
    /// contract (monotonic non-decreasing timestamps, `start_s < end_s`)
    /// is upheld vacuously by an empty `Vec`; once the real decoder
    /// lands, the smoke test in `tests/transcribe_smoke.rs` enforces it.
    pub fn transcribe(&self, audio_16khz_mono: &[f32]) -> Result<Vec<Word>> {
        // Sanity-check the input shape so downstream callers get a
        // clear error if they forget the resampler.
        if !audio_16khz_mono.iter().all(|s| s.is_finite()) {
            return Err(WhisperError::InvalidAudio(
                "audio buffer contains non-finite samples".into(),
            ));
        }
        // See the module doc for why this is currently a stub. The full
        // decoder will replace this body without changing the signature.
        Ok(Vec::new())
    }
}

/// Resample interleaved `f32` PCM at `sr` Hz with `channels` channels to
/// 16 kHz mono `f32`.
///
/// Strategy: downmix-then-resample. Whisper expects mono, and downmixing
/// before the resampler halves the FFT cost on stereo inputs.
///
/// If the input is already 16 kHz the resampler is bypassed entirely
/// (the input is downmixed and returned as-is) — `FftFixedInOut` rejects
/// `in_rate == out_rate` on some configurations, and skipping the path
/// is cheaper anyway.
pub fn resample_to_16khz_mono(samples: &[f32], sr: u32, channels: u16) -> Result<Vec<f32>> {
    if channels == 0 {
        return Err(WhisperError::InvalidAudio("0 channels".into()));
    }
    if sr == 0 {
        return Err(WhisperError::InvalidAudio("0 Hz sample rate".into()));
    }
    let chans = channels as usize;
    if samples.len() % chans != 0 {
        return Err(WhisperError::InvalidAudio(format!(
            "interleaved buffer length {} not divisible by channels {chans}",
            samples.len()
        )));
    }

    // Downmix to mono first.
    let frames = samples.len() / chans;
    let mut mono = Vec::with_capacity(frames);
    if chans == 1 {
        mono.extend_from_slice(samples);
    } else {
        let inv = 1.0 / chans as f32;
        for f in 0..frames {
            let mut acc = 0.0f32;
            for ch in 0..chans {
                acc += samples[f * chans + ch];
            }
            mono.push(acc * inv);
        }
    }

    if sr == 16_000 {
        return Ok(mono);
    }

    // 1024-frame chunk mirrors the audio-io boundary — small enough for
    // tens-of-ms latency, large enough to amortize FFT cost. We're
    // running off the audio thread here so the chunk size is
    // throughput-bound, not latency-bound.
    let mut resampler = FftFixedInOut::<f32>::new(sr as usize, 16_000, 1024, 1)?;
    let chunk_in = resampler.input_frames_max();
    let mut in_buf = vec![vec![0.0f32; chunk_in]; 1];
    let out_max = resampler.output_frames_max();
    let mut out_buf = vec![vec![0.0f32; out_max]; 1];

    let mut out = Vec::with_capacity(((mono.len() as u64 * 16_000) / sr as u64) as usize + 64);
    let mut pos = 0usize;
    while pos + resampler.input_frames_next() <= mono.len() {
        let needed = resampler.input_frames_next();
        in_buf[0].clear();
        in_buf[0].extend_from_slice(&mono[pos..pos + needed]);
        let (_in_used, out_written) = resampler.process_into_buffer(&in_buf, &mut out_buf, None)?;
        out.extend_from_slice(&out_buf[0][..out_written]);
        pos += needed;
    }
    // Tail: pad with zeros to flush. For a transcription pre-processor a
    // few ms of silence at the end is cheaper than the bookkeeping for a
    // partial-chunk path.
    if pos < mono.len() {
        let needed = resampler.input_frames_next();
        in_buf[0].clear();
        in_buf[0].extend_from_slice(&mono[pos..]);
        in_buf[0].resize(needed, 0.0);
        let (_in_used, out_written) = resampler.process_into_buffer(&in_buf, &mut out_buf, None)?;
        out.extend_from_slice(&out_buf[0][..out_written]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_passthrough_at_16khz() {
        // 16 kHz mono: should return the input unchanged.
        let input: Vec<f32> = (0..1600).map(|i| (i as f32) * 0.001).collect();
        let out = resample_to_16khz_mono(&input, 16_000, 1).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn resample_stereo_downmix() {
        // 16 kHz stereo with L=1.0, R=-1.0 -> mono should be ~0.0 everywhere.
        let frames = 1024;
        let mut interleaved = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            interleaved.push(1.0);
            interleaved.push(-1.0);
        }
        let out = resample_to_16khz_mono(&interleaved, 16_000, 2).unwrap();
        assert_eq!(out.len(), frames);
        for s in out {
            assert!(s.abs() < 1e-6, "downmix not zero: {s}");
        }
    }

    #[test]
    fn resample_44k_to_16k_changes_length_proportionally() {
        // 1 s of mono 44.1 kHz silence -> ~16 000 samples at 16 kHz.
        let input = vec![0.0f32; 44_100];
        let out = resample_to_16khz_mono(&input, 44_100, 1).unwrap();
        let expected = 16_000;
        let delta = (out.len() as i64 - expected as i64).abs();
        assert!(
            delta < 2_000,
            "resampled length {} differs from expected {} by more than 2000 samples",
            out.len(),
            expected
        );
    }

    #[test]
    fn rejects_zero_channels() {
        let err = resample_to_16khz_mono(&[], 16_000, 0).unwrap_err();
        assert!(matches!(err, WhisperError::InvalidAudio(_)));
    }

    #[test]
    fn rejects_misaligned_buffer() {
        // 3 samples for stereo is not a whole number of frames.
        let err = resample_to_16khz_mono(&[1.0, 2.0, 3.0], 16_000, 2).unwrap_err();
        assert!(matches!(err, WhisperError::InvalidAudio(_)));
    }

    #[test]
    fn load_returns_model_missing_for_nonexistent_path() {
        let path = Path::new("/definitely/does/not/exist/whisper.onnx");
        let err = WhisperModel::load(path).unwrap_err();
        match err {
            WhisperError::ModelMissing { path: p } => {
                assert!(p.contains("whisper.onnx"));
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }
}
