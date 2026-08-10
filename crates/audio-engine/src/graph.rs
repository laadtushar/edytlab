//! Render graph construction.
//!
//! ### History
//!
//! Phase 1 collapsed the session DAG to a single-track / single-clip linear
//! chain because tracks were guaranteed `len() <= 1`. M21 lifts that:
//! sessions can carry multiple tracks, all of which are summed into a
//! deterministic mixdown at render time. Bus routing and the master chain
//! remain forward-compat fields, ignored here.
//!
//! The single-clip assumption outlived Phase 1 by rather longer, and it
//! was not harmless: `cut_range` with an interior range and `split_clip`
//! both leave a track holding two clips, and the graph read only
//! `clips.first()`. Everything after the cut was dropped from the
//! mixdown with no error anywhere to say so. A track is now flattened
//! into one plan entry *per clip*, each carrying the track index it came
//! from so solo/mute stays a per-track decision.
//!
//! ### Mixdown order
//!
//! Tracks are added to the mix in their `state.tracks` order, and each
//! track's clips in their `Vec<Clip>` order. The session preserves
//! insertion order and downstream tools (`add_track`, `remove_track`)
//! maintain that order, so two byte-identical sessions produce a
//! byte-identical mix even on machines whose `HashMap` iteration order
//! differs. This is the M14 determinism trap; do not reorder tracks or
//! clips inside the render path.
//!
//! ### Solo / mute semantics
//!
//! Solo overrides mute: if any track has `soloed = true`, only soloed
//! tracks contribute samples; muted-but-soloed still plays. If no track is
//! soloed, muted tracks contribute zero samples. Both semantics are
//! finalised at graph-build time so the render loop is a flat sum.

use std::path::PathBuf;

use session::{EnvelopePoint, SessionState};

use crate::Error;

/// One clip, flattened into an entry in the mix.
///
/// Each plan entry carries everything the offline render needs to materialise
/// one clip: where to read samples from, the trim window in source frames,
/// where it sits on the track's timeline, the linear gain (in dB), and
/// whether it will actually contribute (after solo/mute resolution at
/// graph-build time).
///
/// The name is historical — a track used to be guaranteed one clip, so a
/// plan entry and a track were the same thing. They are not any more, which
/// is why [`track_index`](TrackPlan::track_index) has to be carried
/// explicitly: the render loop still needs to reach the owning `Track` for
/// its live solo/mute gate, and the plan index no longer answers that.
#[derive(Debug, Clone)]
pub struct TrackPlan {
    pub source_path: PathBuf,
    /// Index into `state.tracks` of the track this clip belongs to.
    /// Several plan entries share one index when a track has been split.
    pub track_index: usize,
    pub gain_db: f32,
    /// Stereo position, -1.0 hard left to 1.0 hard right. Applied at
    /// emit time; see `pan_gains` in the render module for the law and
    /// why it is the one chosen.
    pub pan: f32,
    /// Frame offset into the decoded source where the clip starts.
    pub source_offset: u64,
    /// Where the clip begins on the track's timeline, in the source's
    /// frame domain. Rendered as leading silence; a gap between two clips
    /// stays a gap rather than being spliced shut.
    pub start_in_track: u64,
    /// Frame length of the clip (counted in the source's sample rate).
    pub length: u64,
    /// Resolved contribution flag: if `false`, the render skips this clip
    /// outright. Captures both mute (no solo present) and "not-soloed"
    /// (some other track has solo).
    pub contributes: bool,
    /// Per-clip volume automation points (sorted by time_samples).
    pub volume_envelope: Vec<EnvelopePoint>,
}

/// The flattened render plan.
///
/// `project_sample_rate` is the rate the mixdown is written at: any track
/// whose source rate differs is resampled at render time via rubato. Channel
/// count of the rendered file is the maximum channel count of any
/// contributing source (mono-on-stereo is upmixed by duplication; stereo
/// stays stereo).
#[derive(Debug, Clone)]
pub struct RenderGraph {
    pub project_sample_rate: u32,
    pub tracks: Vec<TrackPlan>,
    /// Set when the plan reduces to a single contributing track with
    /// unity gain, no effects, and a clip that covers its entire source.
    /// In that one case the render path can copy the source bytes
    /// verbatim — see [`is_unity_passthrough`](crate::render).
    pub unity_passthrough_track: Option<UnityPassthrough>,
}

/// Side-channel data for the byte-copy fast path, populated only when the
/// session genuinely reduces to a single unity-gain pass-through.
#[derive(Debug, Clone)]
pub struct UnityPassthrough {
    pub source_path: PathBuf,
}

pub fn build(state: &SessionState) -> Result<RenderGraph, Error> {
    if state.tracks.is_empty() {
        return Err(Error::NoTrack);
    }

    // Solo overrides mute: precompute whether any track is soloed so we can
    // resolve `contributes` in one pass without re-scanning `state.tracks`
    // inside the loop.
    let any_soloed = state.tracks.iter().any(|t| t.soloed);

    let mut plans: Vec<TrackPlan> = Vec::with_capacity(state.tracks.len());
    for (track_index, track) in state.tracks.iter().enumerate() {
        if !track.effects.is_empty() {
            return Err(Error::EffectsUnsupportedInPhase1);
        }
        // Tracks with zero clips are valid but contribute nothing; the
        // session-state model lets a freshly added empty track sit in the
        // tree before any `load`/`cut_range` populates it. They still get a
        // placeholder entry so an empty track is visible in the plan.
        if track.clips.is_empty() {
            plans.push(TrackPlan {
                source_path: PathBuf::new(),
                track_index,
                gain_db: track.gain_db,
                pan: 0.0,
                source_offset: 0,
                start_in_track: 0,
                length: 0,
                contributes: false,
                volume_envelope: Vec::new(),
            });
            continue;
        }

        let contributes = if any_soloed {
            track.soloed
        } else {
            !track.muted
        };

        for clip in &track.clips {
            plans.push(TrackPlan {
                source_path: clip.source_path.clone(),
                track_index,
                gain_db: track.gain_db,
                pan: track.pan.clamp(-1.0, 1.0),
                source_offset: clip.source_offset,
                start_in_track: clip.start_in_track,
                length: clip.length,
                contributes,
                volume_envelope: clip.volume_envelope.clone(),
            });
        }
    }

    // Decide whether the whole graph reduces to a unity byte-copy. The
    // criteria are conservative on purpose: any deviation (multi-track,
    // non-zero gain, mute, solo, partial clip, sample-rate mismatch with
    // the project) forces the f32 mix path so we never silently bypass
    // downstream logic.
    let unity_passthrough_track = single_track_unity(state, &plans)?;

    Ok(RenderGraph {
        project_sample_rate: state.sample_rate,
        tracks: plans,
        unity_passthrough_track,
    })
}

fn single_track_unity(
    state: &SessionState,
    plans: &[TrackPlan],
) -> Result<Option<UnityPassthrough>, Error> {
    // `plans.len() == state.tracks.len()` by construction; a single check
    // is enough. `track.muted` is implied by `!plan.contributes` (resolved
    // at graph-build time), so we only need to keep `track.soloed` —
    // sole-soloed tracks are still unity-eligible in principle, but we
    // keep the guard to avoid surprising the caller in mixed-solo
    // sessions that happen to collapse to one contributing track.
    if state.tracks.len() != 1 {
        return Ok(None);
    }
    let track = &state.tracks[0];
    let plan = &plans[0];
    if !plan.contributes
        || track.gain_db != 0.0
        || track.pan != 0.0
        || track.soloed
        || !track.effects.is_empty()
    {
        return Ok(None);
    }
    // A byte-copy can only stand in for a track that *is* exactly one
    // source file. A split track is two clips whose concatenation is not
    // any single file on disk, and a clip that starts late needs leading
    // silence the source doesn't contain.
    if track.clips.len() != 1 {
        return Ok(None);
    }
    let clip = &track.clips[0];
    if clip.start_in_track != 0 {
        return Ok(None);
    }
    let (source_frames, source_rate) = peek_source_spec(&clip.source_path)?;
    if clip.source_offset != 0 || clip.length != source_frames {
        return Ok(None);
    }
    if source_rate != state.sample_rate {
        return Ok(None);
    }
    Ok(Some(UnityPassthrough {
        source_path: clip.source_path.clone(),
    }))
}

/// Cheaply read the source's frame count and sample rate from the WAV
/// header without decoding samples. Used by the unity-passthrough check.
fn peek_source_spec(path: &std::path::Path) -> Result<(u64, u32), Error> {
    let reader = hound::WavReader::open(path)?;
    Ok((reader.duration() as u64, reader.spec().sample_rate))
}
