//! Render graph construction.
//!
//! Phase 1 collapses the session DAG to a single linear chain because:
//! * Sessions only ever have 0 or 1 tracks, with 0 or 1 clips.
//! * Track effects are always empty (Phase 1 has no built-in effects).
//! * `bus_routing` and `master_chain` are forward-compat fields, ignored here.
//!
//! Phase 2 will turn this into a real DAG; keep the public surface narrow.

use std::path::PathBuf;

use session::SessionState;

use crate::Error;

/// A flattened, single-track render plan. Phase 1 has at most one of these.
///
/// Clip trimming via `source_offset` / `length` is honored by the offline
/// render path: the rendered output is the slice
/// `source[source_offset .. source_offset + length]` (further restricted by
/// any caller-supplied [`crate::TimeRange`]). Sessions produced by the M08
/// `cut_range`/`trim` tools set these fields to represent sub-ranges of the
/// source file without rewriting the underlying audio. The fast path's
/// [`Self::clip_covers_full_source`] flag stays correct by construction:
/// any non-trivial trim flips it to false and forces the f32 render.
#[derive(Debug, Clone)]
pub struct RenderGraph {
    pub source_path: PathBuf,
    pub track_gain_db: f32,
    pub track_pan: f32,
    pub muted: bool,
    pub soloed: bool,
    pub effects_empty: bool,
    /// Frame offset into the decoded source where the clip starts.
    pub source_offset: u64,
    /// Frame length of the clip (counted in the source's sample rate).
    pub length: u64,
    /// `true` when the clip's `source_offset == 0` AND its `length` equals the
    /// source file's total frame count. Lets the fast path skip clip trimming
    /// without re-decoding to verify.
    pub clip_covers_full_source: bool,
}

pub fn build(state: &SessionState) -> Result<RenderGraph, Error> {
    let track = state.tracks.first().ok_or(Error::NoTrack)?;
    let clip = track.clips.first().ok_or(Error::NoClip)?;

    if !track.effects.is_empty() {
        return Err(Error::EffectsUnsupportedInPhase1);
    }

    let source_frames = peek_source_frames(&clip.source_path)?;
    let clip_covers_full_source = clip.source_offset == 0 && clip.length == source_frames;

    Ok(RenderGraph {
        source_path: clip.source_path.clone(),
        track_gain_db: track.gain_db,
        track_pan: track.pan,
        muted: track.muted,
        soloed: track.soloed,
        effects_empty: track.effects.is_empty(),
        source_offset: clip.source_offset,
        length: clip.length,
        clip_covers_full_source,
    })
}

/// Cheaply read the source's frame count from the WAV header without decoding
/// samples. Used by [`build`] to derive [`RenderGraph::clip_covers_full_source`].
fn peek_source_frames(path: &std::path::Path) -> Result<u64, Error> {
    let reader = hound::WavReader::open(path)?;
    Ok(reader.duration() as u64)
}
