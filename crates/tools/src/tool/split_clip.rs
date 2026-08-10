use crate::schema::anthropic_tool;
use crate::tool::util::{append_state, check_track_index, load_head_state, slice_envelope};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
use session::Clip;

/// Split `clip` into two clips at `at_frames` frames from the clip's start_in_track.
/// `at_frames` is relative to the clip's start (i.e. within [1, length-1]).
/// Returns (left, right) or Err if split point is outside the clip.
pub(crate) fn split_at(clip: &Clip, at_frames: u64) -> Result<(Clip, Clip), String> {
    if at_frames == 0 || at_frames >= clip.length {
        return Err(format!(
            "split point {at_frames} must be in (0, {})",
            clip.length
        ));
    }
    let left = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: clip.start_in_track,
        source_offset: clip.source_offset,
        length: at_frames,
        content_hash: clip.content_hash,
        time_stretch_factor: clip.time_stretch_factor,
        pitch_shift_semitones: clip.pitch_shift_semitones,
        beat_grid: clip.beat_grid.clone(),
        volume_envelope: slice_envelope(&clip.volume_envelope, 0, at_frames),
    };
    let right = Clip {
        source_path: clip.source_path.clone(),
        start_in_track: clip.start_in_track + at_frames,
        source_offset: clip.source_offset + at_frames,
        length: clip.length - at_frames,
        content_hash: clip.content_hash,
        time_stretch_factor: clip.time_stretch_factor,
        pitch_shift_semitones: clip.pitch_shift_semitones,
        beat_grid: clip.beat_grid.clone(),
        volume_envelope: slice_envelope(&clip.volume_envelope, at_frames, clip.length - at_frames),
    };
    Ok((left, right))
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    clip_index: usize,
    at_sec: f64,
}

pub struct SplitClipTool;

impl Tool for SplitClipTool {
    fn name(&self) -> &'static str {
        "split_clip"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "split_clip",
            "Split a clip into two at the specified time position. Both resulting clips reference the same source file with adjusted offsets. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "clip_index": {
                        "type": "integer",
                        "description": "Zero-based clip index within the track"
                    },
                    "at_sec": {
                        "type": "number",
                        "description": "Position to split at, in seconds from track start"
                    }
                },
                "required": ["track", "clip_index", "at_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let track = &state.tracks[args.track];
        if args.clip_index >= track.clips.len() {
            return Ok(ToolResult::Error(format!(
                "clip_index {} out of range (track has {} clips)",
                args.clip_index,
                track.clips.len()
            )));
        }
        let clip = track.clips[args.clip_index].clone();
        let at_frames = (args.at_sec * state.sample_rate as f64) as u64;
        // Convert absolute track position to offset within clip
        let clip_at = at_frames.saturating_sub(clip.start_in_track);
        let (left, right) = match split_at(&clip, clip_at) {
            Ok(pair) => pair,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        let track = &mut state.tracks[args.track];
        track.clips.remove(args.clip_index);
        track.clips.insert(args.clip_index, right);
        track.clips.insert(args.clip_index, left);
        let new_id = match append_state(
            ctx,
            state,
            format!(
                "split_clip track {} clip {} at {:.3}s",
                args.track, args.clip_index, args.at_sec
            ),
        ) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "summary": format!(
                "Split track {} clip {} at {:.3}s",
                args.track, args.clip_index, args.at_sec
            )
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::split_at;
    use session::{Clip, EnvelopePoint};
    use std::path::PathBuf;

    fn make_clip(start_in_track: u64, source_offset: u64, length: u64) -> Clip {
        Clip {
            source_path: PathBuf::from("/tmp/test.wav"),
            start_in_track,
            source_offset,
            length,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: vec![],
        }
    }

    #[test]
    fn splits_middle() {
        let clip = make_clip(0, 0, 100);
        let (a, b) = split_at(&clip, 40).unwrap();
        assert_eq!(a.start_in_track, 0);
        assert_eq!(a.source_offset, 0);
        assert_eq!(a.length, 40);
        assert_eq!(b.start_in_track, 40);
        assert_eq!(b.source_offset, 40);
        assert_eq!(b.length, 60);
    }

    #[test]
    fn rejects_split_at_start() {
        let clip = make_clip(0, 0, 100);
        assert!(split_at(&clip, 0).is_err());
    }

    #[test]
    fn rejects_split_at_end() {
        let clip = make_clip(0, 0, 100);
        assert!(split_at(&clip, 100).is_err());
    }

    /// A 1000-frame clip with a linear fade from 0 dB down to -20 dB.
    fn faded_clip() -> Clip {
        let mut clip = make_clip(0, 0, 1000);
        clip.volume_envelope = vec![
            EnvelopePoint {
                time_samples: 0,
                gain_db: 0.0,
            },
            EnvelopePoint {
                time_samples: 1000,
                gain_db: -20.0,
            },
        ];
        clip
    }

    /// Reproduce the engine's interpolation so the assertions can talk
    /// about gain at a frame rather than about point lists.
    fn gain_at(points: &[EnvelopePoint], frame: u64) -> f32 {
        if points.is_empty() {
            return 0.0;
        }
        if frame <= points[0].time_samples {
            return points[0].gain_db;
        }
        let last = &points[points.len() - 1];
        if frame >= last.time_samples {
            return last.gain_db;
        }
        let pos = points.partition_point(|p| p.time_samples <= frame);
        let a = &points[pos - 1];
        let b = &points[pos];
        let alpha = (frame - a.time_samples) as f32 / (b.time_samples - a.time_samples) as f32;
        a.gain_db + alpha * (b.gain_db - a.gain_db)
    }

    /// The two halves of a split must together reproduce the original
    /// curve.
    #[test]
    fn splitting_rebases_the_second_half_of_the_curve() {
        let clip = faded_clip();
        let original = clip.volume_envelope.clone();
        let (left, right) = split_at(&clip, 400).expect("split");

        // The left half is unchanged in absolute terms.
        for frame in [0u64, 100, 399] {
            let want = gain_at(&original, frame);
            let got = gain_at(&left.volume_envelope, frame);
            assert!(
                (want - got).abs() < 0.01,
                "left half at frame {frame}: expected {want} dB, got {got}"
            );
        }

        // The right half's frame 0 is the original's frame 400. Before
        // the fix this read 0.0 dB — the start of the curve all over
        // again.
        let want_at_split = gain_at(&original, 400);
        let got_at_split = gain_at(&right.volume_envelope, 0);
        assert!(
            (want_at_split - got_at_split).abs() < 0.01,
            "right half should resume at {want_at_split} dB, got {got_at_split}"
        );
        assert!(
            got_at_split < -1.0,
            "the split lands well into the fade; {got_at_split} dB looks like a reset \
             to the start of the curve"
        );

        // ...and it keeps ramping to the same endpoint.
        let want_end = gain_at(&original, 1000);
        let got_end = gain_at(&right.volume_envelope, 600);
        assert!(
            (want_end - got_end).abs() < 0.01,
            "right half should still end at {want_end} dB, got {got_end}"
        );
    }

    /// Splitting inside a flat stretch shouldn't manufacture a ramp.
    #[test]
    fn splitting_a_flat_envelope_stays_flat() {
        let mut clip = faded_clip();
        clip.volume_envelope = vec![EnvelopePoint {
            time_samples: 0,
            gain_db: -6.0,
        }];

        let (left, right) = split_at(&clip, 400).expect("split");
        for env in [&left.volume_envelope, &right.volume_envelope] {
            for frame in [0u64, 200, 599] {
                let got = gain_at(env, frame);
                assert!(
                    (got + 6.0).abs() < 0.01,
                    "flat -6 dB expected at frame {frame}, got {got}"
                );
            }
        }
    }

    /// A clip with no automation gains none.
    #[test]
    fn splitting_without_automation_adds_none() {
        let clip = make_clip(0, 0, 1000);
        let (left, right) = split_at(&clip, 400).expect("split");
        assert!(left.volume_envelope.is_empty());
        assert!(right.volume_envelope.is_empty());
    }
}
