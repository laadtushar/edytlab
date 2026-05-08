//! Offline + (stub) realtime audio rendering for edytlab Phase 1.
//!
//! Phase 1 scope (per `docs/superpowers/plans/2026-05-05-phase-1-edit-single-track.md`,
//! M06):
//! * Single track, single clip, optional track gain (in dB).
//! * No effects, no bus routing, no master chain — those fields exist in
//!   [`session::SessionState`] for forward compatibility but are ignored.
//! * Render is fully deterministic across platforms; see `render.rs`.
//!
//! The realtime [`play_state`] entry point is a thin stub: it opens an
//! [`audio_io::OutputStream`], pushes the rendered samples, and returns a
//! handle whose `Drop` pauses the stream. Frame-accurate transport, scrubbing,
//! and seeking arrive in Phase 2.

pub mod graph;
pub mod mixer;
pub mod render;

use std::path::Path;

use audio_io::OutputStream;
use session::SessionState;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session has no tracks")]
    NoTrack,
    #[error("track has no clips")]
    NoClip,
    #[error("track effects are not supported in Phase 1")]
    EffectsUnsupportedInPhase1,
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
    #[error("audio output error: {0}")]
    AudioIo(#[from] audio_io::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Inclusive-start, exclusive-end frame range relative to the source clip.
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

/// Stateless wrapper around the engine entry points so callers can hold a
/// single object instead of free functions.
///
/// Phase 1 deliberately keeps this empty: there is no internal cache, no
/// thread pool, no preallocated buffer, and no decoder pool. Phase 2's
/// effects graph and Phase 3's mix pipelines are expected to grow this
/// type with owned state, so call sites — including the M07 tool
/// dispatcher — should reach for `Engine` rather than the bare functions.
///
/// [`play_state`] is intentionally NOT mirrored on `Engine`: it borrows an
/// `OutputStream` whose lifetime the engine does not own. Callers that
/// need realtime preview should keep using the free function for now.
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
}

/// Realtime preview entry point.
///
/// M21 caveat: the realtime preview path does NOT yet do multi-track
/// mixdown. It plays back the FIRST contributing track only. Offline
/// `render_state_to_wav` is the multi-track mix path. The realtime mixer
/// arrives in a later milestone (M22+); until then this is the pragmatic
/// stub that preserves Phase 1 demo behaviour.
// TODO(M22+): wire the multi-track offline mix into the realtime preview
// stream so play and render stay in semantic lockstep.
pub fn play_state<'a>(
    state: &SessionState,
    output: &'a mut dyn OutputStream,
    range: Option<TimeRange>,
) -> Result<PlayHandle<'a>> {
    let graph = graph::build(state)?;
    let plan = graph
        .tracks
        .iter()
        .find(|t| t.contributes && t.length > 0)
        .ok_or(Error::NoClip)?;
    let mut decoded = audio_decoder::decode_file(&plan.source_path)?;
    mixer::apply_gain_db(&mut decoded.samples, plan.gain_db);

    let chans = decoded.channels as usize;
    let total_frames = decoded.samples.len() / chans;
    let (start, end) = render::resolve_range(range, total_frames)?;

    let slice = &decoded.samples[start * chans..end * chans];
    output.play()?;
    output.write_samples(slice)?;

    Ok(PlayHandle { output })
}

/// Owned handle to a running playback stream. Pauses the stream on drop.
/// Phase 1 has no transport controls beyond that.
pub struct PlayHandle<'a> {
    output: &'a mut dyn OutputStream,
}

impl PlayHandle<'_> {
    pub fn pause(&mut self) -> Result<()> {
        self.output.pause()?;
        Ok(())
    }
}

impl Drop for PlayHandle<'_> {
    fn drop(&mut self) {
        // Best-effort: pause errors during teardown have no caller to surface
        // to. The audio-io layer logs them.
        let _ = self.output.pause();
    }
}
