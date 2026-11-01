//! Offline rendering for edytlab: builds a render graph from a [`session::SessionState`] and renders it to WAV ([`render_state_to_wav`] / [`Engine`]), with WAV, FLAC and MP3 encoders and tag writers alongside.
//!
//! There is no native playback. The app plays audio in the webview — WaveSurfer and `<audio>` elements reading rendered files over the asset protocol — so this crate opens no output device and links no audio backend (#388; `tests/no_native_output.rs` holds that).

pub mod effect_chain;
pub mod encode;
pub mod graph;
pub mod metadata;
pub mod mixer;
pub mod render;

pub use encode::{
    write_flac, write_mp3, write_wav, write_wav_f32, WavChunkWriter, MP3_DEFAULT_KBPS,
};
pub use metadata::{read_flac_tags, tag_flac, tag_mp3, Chapter, Tags};
pub use render::render_warnings;

use std::path::Path;

use session::SessionState;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session has no tracks")]
    NoTrack,
    #[error("track has no clips")]
    NoClip,
    #[error("a track sends to bus '{0}', which the session does not define")]
    UnknownBus(String),
    #[error("render range end is before start")]
    InvalidRange,
    #[error("unsupported channel map: source has {from} channels, render target has {to}")]
    UnsupportedChannelMap { from: u16, to: u16 },
    #[error("rubato resampler construction failed: {0}")]
    ResamplerInit(rubato::ResamplerConstructionError),
    #[error("rubato resampler process failed: {0}")]
    ResamplerProcess(rubato::ResampleError),
    #[error("decode error: {0}")]
    Decode(#[from] audio_decoder::DecodeError),
    #[error("wav writer error: {0}")]
    Wav(#[from] hound::Error),
    #[error("encoder error: {0}")]
    Encode(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Inclusive-start, exclusive-end range on the project mixdown timeline,
/// measured in frames at the project's sample rate.
#[derive(Debug, Clone, Copy)]
pub struct TimeRange {
    pub start_frame: u64,
    pub end_frame: u64,
}

#[derive(Debug, Clone)]
pub struct RenderReport {
    pub frames_written: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub peak_dbfs: f32,
    /// Things the render did that its caller should hear about, one
    /// sentence each; empty when there is nothing to say.
    ///
    /// Today that is one thing: a stored effect the renderer cannot run
    /// (a kind that is not streamable yet, or one that does not exist) is
    /// left out of its chain, and the render succeeds without it (#520).
    /// Each such entry gets a warning naming the chain it is in (a track
    /// by index and name, a bus, or the master), its position in that
    /// chain and its kind. The same list is available without rendering
    /// from [`render_warnings`].
    pub warnings: Vec<String>,
}

/// Completion of a controlled range render. Cancellation is not a render
/// failure and carries no report for a partial output.
#[derive(Debug)]
pub enum RenderOutcome {
    Complete(RenderReport),
    Cancelled,
}

/// Render `state` to a 16-bit PCM WAV at `out`. Always uses the source
/// sample rate and channel count.
pub fn render_state_to_wav(
    state: &SessionState,
    out: &Path,
    range: Option<TimeRange>,
) -> Result<RenderReport> {
    render::render(state, out, range)
}

/// Render a range to 16-bit PCM WAV, checking `cancel` before setup,
/// before each existing prefix-discard and output block, and after the
/// final block. Blocks contain at most one second of project frames;
/// setup and an in-progress block are not interrupted.
///
/// A pre-cancelled call leaves `out` untouched. Later cancellation may
/// leave a partial WAV at `out`; the caller owns its cleanup and must
/// publish the file only after [`RenderOutcome::Complete`]. A callback
/// that always returns false preserves the legacy range render's bytes
/// and report. Whole renders retain their existing byte-copy fast path
/// through [`render_state_to_wav`].
pub fn render_state_range_to_wav_cancellable(
    state: &SessionState,
    out: &Path,
    range: TimeRange,
    cancel: &dyn Fn() -> bool,
) -> Result<RenderOutcome> {
    render::render_range_cancellable(state, out, range, cancel)
}

/// Stateless wrapper around the engine entry points so callers can hold a
/// single object instead of free functions.
///
/// Phase 1 deliberately keeps this empty: there is no internal cache, no
/// thread pool, no preallocated buffer, and no decoder pool. Phase 2's
/// effects graph and Phase 3's mix pipelines are expected to grow this
/// type with owned state, so call sites — including the M07 tool
/// dispatcher — should reach for `Engine` rather than the bare functions.
#[derive(Debug, Default)]
pub struct Engine;

impl Engine {
    pub fn new() -> Self {
        Self
    }

    /// See [`render_state_to_wav`].
    pub fn render_to_wav(
        &self,
        state: &SessionState,
        out: &Path,
        range: Option<TimeRange>,
    ) -> Result<RenderReport> {
        render_state_to_wav(state, out, range)
    }

    /// See [`render_state_range_to_wav_cancellable`].
    pub fn render_range_to_wav_cancellable(
        &self,
        state: &SessionState,
        out: &Path,
        range: TimeRange,
        cancel: &dyn Fn() -> bool,
    ) -> Result<RenderOutcome> {
        render_state_range_to_wav_cancellable(state, out, range, cancel)
    }
}
