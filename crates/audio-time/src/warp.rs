//! Warp audio from one beat grid onto another.
//!
//! `align_to_beat` recorded a `beat_grid` on every clip and nothing read
//! it — the last tool in the repo that reported success without changing
//! any audio. Warping onto a grid is variable-rate time-stretching: for
//! each pair of consecutive beats, stretch that segment by
//! `target_interval / source_interval`.
//!
//! ## Why this is not a stretch per segment
//!
//! The obvious implementation cuts at each beat, calls `time_stretch` on
//! each piece and concatenates. That puts an audible seam at *every
//! beat*: each call has its own overlap-add ramp at both ends, and its
//! own phase state starting from zero, so partials restart out of step
//! at each join.
//!
//! Instead this builds a ratio schedule and hands it to a single vocoder
//! pass whose synthesis hop varies per frame
//! ([`crate::vocoder::stretch_varying`]). Phase state runs continuously
//! across the whole signal, so a segment boundary is not something the
//! reconstruction can see. `no_discontinuity_at_segment_boundaries`
//! pins that.

use crate::vocoder::{deinterleave, interleave, stretch_varying};
use crate::{check_channels, Error, Result};

/// Build the per-sample ratio schedule from two grids.
///
/// Returns a function of input sample position. Between the *i*th and
/// *(i+1)*th beat the ratio is `target_interval / source_interval`;
/// before the first beat and after the last it is the ratio of the
/// nearest segment, so the head and tail move with the music rather
/// than staying put while everything around them shifts.
///
/// Both grids are in samples and must be the same length.
pub(crate) fn ratio_schedule(source: &[u64], target: &[u64]) -> Vec<(u64, f32)> {
    let mut out = Vec::with_capacity(source.len().saturating_sub(1));
    for i in 0..source.len().saturating_sub(1) {
        let src_span = source[i + 1].saturating_sub(source[i]);
        let tgt_span = target[i + 1].saturating_sub(target[i]);
        // A zero-length source segment would divide by zero; a zero
        // target segment would ask for silence of zero length. Both mean
        // two beats landed on the same sample, which `warp_to_grid`
        // rejects — this is the belt to that braces.
        let ratio = if src_span == 0 {
            1.0
        } else {
            (tgt_span as f32 / src_span as f32).clamp(MIN_RATIO, MAX_RATIO)
        };
        out.push((source[i], ratio));
    }
    out
}

/// Bounds on any single segment's stretch.
///
/// A grid that puts two source beats 5 ms apart and their targets 2 s
/// apart asks for a 400x stretch of that segment, which is not a warp,
/// it is a drone. Clamping keeps one bad beat from swallowing the
/// output — the surrounding segments still land where they should.
const MIN_RATIO: f32 = 0.1;
const MAX_RATIO: f32 = 10.0;

/// Look the schedule up at `pos`, holding the first and last values out
/// past the ends.
fn ratio_at(schedule: &[(u64, f32)], pos: usize) -> f32 {
    if schedule.is_empty() {
        return 1.0;
    }
    let p = pos as u64;
    if p < schedule[0].0 {
        return schedule[0].1;
    }
    // Linear scan backwards from the end. Schedules are one entry per
    // beat — hundreds, not millions — and the caller walks positions
    // forward, so this is not worth a binary search.
    for &(start, ratio) in schedule.iter().rev() {
        if p >= start {
            return ratio;
        }
    }
    schedule[0].1
}

/// Warp `samples` so that the events at `source_beats` land on
/// `target_beats`.
///
/// Both grids are sample positions into the *input*, ascending and
/// strictly increasing. Pitch is unchanged.
///
/// The output length is the target grid's span extended by however much
/// audio sits outside the grid at each end, scaled by the adjacent
/// segment's ratio — so a warp of a whole track keeps its lead-in and
/// tail rather than cropping to the first and last beat.
///
/// # Errors
///
/// * [`Error::ChannelMismatch`] — fewer than two beats in either grid,
///   the two grids differ in length, or a grid is not strictly
///   increasing. Mismatched lengths are an error rather than a truncate:
///   silently dropping beats is exactly the class of bug this codebase
///   has spent its history removing, and "warped to your grid, but only
///   the first eight beats of it" is not something a caller can notice.
///   Also `channels` is zero, or `samples.len()` does not divide evenly
///   by `channels`.
pub fn warp_to_grid(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    source_beats: &[u64],
    target_beats: &[u64],
) -> Result<Vec<f32>> {
    check_channels(samples.len(), channels)?;
    if source_beats.len() < 2 || target_beats.len() < 2 {
        return Err(Error::ChannelMismatch(format!(
            "need at least two beats in each grid to warp between; got {} source and {} target",
            source_beats.len(),
            target_beats.len()
        )));
    }
    if source_beats.len() != target_beats.len() {
        return Err(Error::ChannelMismatch(format!(
            "grids must have the same number of beats; got {} source and {} target. \
             Truncating to the shorter would silently drop the rest of the arrangement",
            source_beats.len(),
            target_beats.len()
        )));
    }
    for (name, grid) in [("source", source_beats), ("target", target_beats)] {
        if let Some(i) = (1..grid.len()).find(|&i| grid[i] <= grid[i - 1]) {
            return Err(Error::ChannelMismatch(format!(
                "{name} beats must strictly increase; beat {i} at {} does not follow {}",
                grid[i],
                grid[i - 1]
            )));
        }
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    tracing::trace!(
        sample_rate,
        channels,
        beats = source_beats.len(),
        "audio-time::warp_to_grid"
    );

    let schedule = ratio_schedule(source_beats, target_beats);
    let frames = samples.len() / channels as usize;

    // Exact output length. Inside the grid it is the target span; the
    // audio before the first beat and after the last is scaled by the
    // segment nearest it, which is the same rule `ratio_at` uses so the
    // length and the content agree.
    let head_in = source_beats[0] as usize;
    let head_out = (head_in as f32 * schedule[0].1).round() as usize;
    let tail_in = frames.saturating_sub(*source_beats.last().unwrap() as usize);
    let tail_out = (tail_in as f32 * schedule.last().unwrap().1).round() as usize;
    let grid_span = (target_beats.last().unwrap() - target_beats[0]) as usize;
    let target_len = (head_out + grid_span + tail_out).max(1);

    let planes: Vec<Vec<f32>> = deinterleave(samples, channels as usize)
        .iter()
        .map(|p| stretch_varying(p, &|pos| ratio_at(&schedule, pos), target_len, true))
        .collect();
    Ok(interleave(&planes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_needs_two_beats() {
        let e = warp_to_grid(&[0.0; 8], 44_100, 1, &[0], &[0]).expect_err("one beat");
        assert!(format!("{e}").contains("at least two beats"), "{e}");
    }

    #[test]
    fn mismatched_grids_are_an_error_not_a_truncation() {
        let e = warp_to_grid(&[0.0; 8], 44_100, 1, &[0, 100, 200], &[0, 100])
            .expect_err("length mismatch");
        let msg = format!("{e}");
        assert!(msg.contains("same number of beats"), "{msg}");
        assert!(
            msg.contains("silently drop"),
            "the error should say why: {msg}"
        );
    }

    #[test]
    fn a_non_increasing_grid_is_rejected() {
        let e = warp_to_grid(&[0.0; 8], 44_100, 1, &[0, 200, 100], &[0, 100, 200])
            .expect_err("out of order");
        assert!(format!("{e}").contains("strictly increase"), "{e}");
    }

    #[test]
    fn ratios_come_from_the_interval_pairs() {
        // Source beats every 100, target every 200 — everything twice as
        // long.
        let s = ratio_schedule(&[0, 100, 200, 300], &[0, 200, 400, 600]);
        assert_eq!(s.len(), 3);
        for (_, r) in &s {
            assert!((r - 2.0).abs() < 1e-6, "expected 2.0, got {r}");
        }
    }

    /// A tempo that changes partway. The first half compresses, the
    /// second stretches, and the schedule has to say so per segment
    /// rather than averaging.
    #[test]
    fn a_varying_grid_gives_a_varying_schedule() {
        let s = ratio_schedule(&[0, 100, 200], &[0, 50, 400]);
        assert!((s[0].1 - 0.5).abs() < 1e-6, "{:?}", s[0]);
        assert!((s[1].1 - 3.5).abs() < 1e-6, "{:?}", s[1]);
    }

    #[test]
    fn an_absurd_segment_is_clamped_rather_than_swallowing_the_output() {
        // Two source beats 1 sample apart, targets 10_000 apart.
        let s = ratio_schedule(&[0, 1], &[0, 10_000]);
        assert_eq!(s[0].1, MAX_RATIO);
    }

    #[test]
    fn the_schedule_holds_its_ends() {
        let s = ratio_schedule(&[100, 200, 300], &[100, 400, 500]);
        // Before the first beat, the first segment's ratio.
        assert_eq!(ratio_at(&s, 0), s[0].1);
        // After the last, the last segment's.
        assert_eq!(ratio_at(&s, 10_000), s[s.len() - 1].1);
    }
}
