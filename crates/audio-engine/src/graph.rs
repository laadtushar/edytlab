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
#[derive(Debug, Clone)]
pub struct RenderGraph {
    pub source_path: PathBuf,
    pub track_gain_db: f32,
    /// Sample-frame range of the *clip's source file* to render. Inclusive
    /// start, exclusive end. `None` means render the whole clip.
    pub source_offset: u64,
    pub source_length: u64,
}

pub fn build(state: &SessionState) -> Result<RenderGraph, Error> {
    let track = state.tracks.first().ok_or(Error::NoTrack)?;
    let clip = track.clips.first().ok_or(Error::NoClip)?;

    if !track.effects.is_empty() {
        return Err(Error::EffectsUnsupportedInPhase1);
    }

    Ok(RenderGraph {
        source_path: clip.source_path.clone(),
        track_gain_db: track.gain_db,
        source_offset: clip.source_offset,
        source_length: clip.length,
    })
}
