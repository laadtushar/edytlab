//! `join_clips` — make adjacent clips of one track a single clip (#771).
//!
//! Two clips cut from one file and lying back to back are one stretch of
//! that file that happens to be written as two. Those are merged: one clip
//! over the same source, nothing written, and a render that is what it was.
//!
//! Anything else cannot be one clip over a file: a gap between the clips, a
//! jump in the file (the middle was cut out), different files, or clips with
//! different stretch or pitch requests. Those are rendered, once, into a new
//! file under `derived/` (gaps are silence and overlapping clips sum, each
//! with its own volume envelope already applied, as the renderer mixes them),
//! and the answer says that it happened, because the clip then points at new
//! audio and the originals are no longer what it plays. That file is the
//! audio as it sounds, so the clip over it carries no envelope, stretch,
//! pitch or beat grid of its own.
//!
//! Positions are counted in the frames of the clips' own source file (#234),
//! so nothing here turns seconds into frames. The seconds in the answer are
//! the first clip's own rate. In a merge the envelopes are closed at each
//! clip's own ends ([`slice_envelope`]), merged through
//! [`merge_clip_envelopes`] and set with [`Clip::set_envelope`] (#677).

use serde::Deserialize;
use serde_json::{json, Value};
use session::audio_store::{AudioFolder, AudioName, AudioStore};
use session::{Clip, EnvelopePoint};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    append_state, args_and_head, check_track_index, clip_frames_to_seconds, flatten_clips,
    merge_clip_envelopes, set_session_length, slice_envelope, write_cas_wav,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    track: usize,
    first_clip: usize,
    last_clip: Option<usize>,
}

/// `b` carries on exactly where `a` stops, in the timeline and in the file,
/// and asks for nothing `a` does not.
fn continues(a: &Clip, b: &Clip) -> bool {
    a.source_path == b.source_path
        && a.content_hash == b.content_hash
        && a.time_stretch_factor == b.time_stretch_factor
        && a.pitch_shift_semitones == b.pitch_shift_semitones
        && a.beat_grid == b.beat_grid
        && b.start_in_track == a.start_in_track.saturating_add(a.length)
        && b.source_offset == a.source_offset.saturating_add(a.length)
}

/// One clip's curve, closed at both of its own ends: a clip with no curve
/// is held at 0 dB, and points beyond the clip's length (which `set_clip_envelope`
/// accepts, and the renderer never reaches) are dropped, with the gain the
/// curve has at the clip's end written there.
fn closed_curve(c: &Clip) -> Vec<EnvelopePoint> {
    if c.length == 0 {
        return Vec::new();
    }
    let mut points = slice_envelope(&c.volume_envelope, 0, c.length);
    if points.is_empty() {
        points.push(EnvelopePoint {
            time_samples: 0,
            gain_db: 0.0,
        });
    }
    if points.len() == 1 {
        // A flat curve is one point; it still has to be held to the end.
        points.push(EnvelopePoint {
            time_samples: c.length,
            gain_db: points[0].gain_db,
        });
    }
    points
}

/// The clips' automation as one curve for the joined clip, in its frames
/// counted from `origin`, its start.
///
/// The renderer holds a curve's first value before its first point and its
/// last value after its last, and a clip with no curve is at unity. Merging
/// the points as they stand would therefore spread one clip's curve over the
/// clips beside it: a half with no automation would take the other half's
/// first value, and a point past one clip's end would act inside the next.
/// So when any clip is automated, every clip is closed at both of its own
/// ends first ([`closed_curve`]), which leaves each stretch of the joined
/// clip with the gain it had.
fn joined_envelope(group: &[Clip], origin: u64, total: u64) -> Vec<EnvelopePoint> {
    if group.iter().all(|c| c.volume_envelope.is_empty()) {
        return Vec::new();
    }
    let closed: Vec<Clip> = group
        .iter()
        .map(|c| {
            let mut clip = c.clone();
            // Counted from the joined clip's start, which is where
            // `merge_clip_envelopes` expects the collapsed clip to be.
            clip.start_in_track = c.start_in_track - origin;
            clip.set_envelope(closed_curve(c));
            clip
        })
        .collect();
    merge_clip_envelopes(&closed, total, total)
}

pub struct JoinClipsTool;

impl Tool for JoinClipsTool {
    fn name(&self) -> &'static str {
        "join_clips"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "join_clips",
            "Join adjacent clips of one track into one clip. Back-to-back clips of the same source \
             file merge with nothing rendered; any other join (a gap, a jump in the file) is \
             rendered into a new file, and the result says so. Joins first_clip and the next \
             clip, or the run to last_clip. Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "first_clip": { "type": "integer" },
                    "last_clip": {
                        "type": "integer",
                        "description": "Last clip of the run, inclusive. Default first_clip + 1."
                    }
                },
                "required": ["track", "first_clip"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let (args, mut state) = match args_and_head::<Args>(args, ctx) {
            Ok(v) => v,
            Err(r) => return Ok(r),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let clips = state.tracks[args.track].clips.clone();
        if clips.len() < 2 {
            return Ok(ToolResult::Error(format!(
                "track {} has {} clip{}; a join needs at least two",
                args.track,
                clips.len(),
                if clips.len() == 1 { "" } else { "s" },
            )));
        }
        let first = args.first_clip;
        let last = args.last_clip.unwrap_or(first.saturating_add(1));
        for idx in [first, last] {
            if idx >= clips.len() {
                return Ok(ToolResult::Error(format!(
                    "clip index {idx} out of range; track {} has {} clips",
                    args.track,
                    clips.len()
                )));
            }
        }
        if last <= first {
            return Ok(ToolResult::Error(format!(
                "last_clip ({last}) must come after first_clip ({first}); a join needs at least \
                 two clips"
            )));
        }

        let group = &clips[first..=last];
        let origin = group.iter().map(|c| c.start_in_track).min().unwrap_or(0);
        let end = group
            .iter()
            .map(|c| c.start_in_track.saturating_add(c.length))
            .max()
            .unwrap_or(origin);
        let total = end - origin;
        let merge = group.windows(2).all(|w| continues(&w[0], &w[1]));
        // A merge keeps the clips' curves as one. A render has them applied
        // to the audio already, so its clip has none.
        let envelope = if merge {
            joined_envelope(group, origin, total)
        } else {
            Vec::new()
        };

        let mut joined = if merge {
            let mut clip = group[0].clone();
            clip.length = total;
            clip
        } else {
            // Laid out from the joined clip's start, so the render has no
            // lead-in before it.
            let rebased: Vec<Clip> = group
                .iter()
                .map(|c| {
                    let mut c = c.clone();
                    c.start_in_track -= origin;
                    c
                })
                .collect();
            let audio = match flatten_clips(&rebased, true, &|m| {
                format!(
                    "join_clips needs clips of one sample rate and channel count; these differ \
                     ({} Hz / {} ch and {} Hz / {} ch in {}). Bring the clips to one format \
                     first, or leave them as separate clips",
                    m.first.0,
                    m.first.1,
                    m.found.0,
                    m.found.1,
                    m.path.file_name().map_or_else(
                        || m.path.display().to_string(),
                        |n| n.to_string_lossy().into_owned()
                    ),
                )
            }) {
                Ok(a) => a,
                Err(msg) => return Ok(ToolResult::Error(msg)),
            };
            // Named for the samples and their layout (#620); the clip's
            // `content_hash` stays the hash of the samples alone, as every
            // edit's does. The audio store writes it (#719).
            let name = AudioName::of_samples(&audio.window, audio.sample_rate, audio.channels);
            let audio_store = AudioStore::new(ctx.store.project_dir());
            let path = match write_cas_wav(
                &audio_store,
                AudioFolder::Derived,
                &name,
                &audio.window,
                audio.sample_rate,
                audio.channels,
            ) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "failed to write {}: {e}",
                        audio_store.path_of(AudioFolder::Derived, &name)
                    )))
                }
            };
            let frames = (audio.window.len() / audio.channels.max(1) as usize) as u64;
            Clip {
                source_path: path,
                start_in_track: origin,
                source_offset: 0,
                length: frames,
                content_hash: Some(crate::provenance::legacy_sample_hash(&audio.window)),
                // The file is the audio as the renderer plays it. It applies
                // no stretch, pitch or grid itself, so these are recorded
                // requests, and a copy of the first clip's would be a request
                // the new audio never had.
                time_stretch_factor: None,
                pitch_shift_semitones: None,
                beat_grid: None,
                volume_envelope: Vec::new(),
                volume_envelope_frames: session::EnvelopeFrames::default(),
            }
        };
        joined.set_envelope(envelope);
        let seconds = clip_frames_to_seconds(&joined, joined.length).unwrap_or(0.0);

        let n = last - first + 1;
        state.tracks[args.track]
            .clips
            .splice(first..=last, std::iter::once(joined));
        set_session_length(&mut state);

        let how = if merge {
            "they were contiguous in the same source, so they are one clip over it and nothing \
             was rendered"
                .to_string()
        } else {
            "they were not contiguous in one source, so the join was rendered into a new file \
             (gaps are silence, overlapping clips are summed, and each clip's volume envelope \
             is applied to its audio)"
                .to_string()
        };
        let label = format!("join_clips track {} clips {}..{}", args.track, first, last);
        let new_id = match append_state(ctx, state, label) {
            Ok(id) => id,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        Ok(ToolResult::Ok(json!({
            "node_id": new_id.to_hex(),
            "clip_index": first,
            "joined_clips": n,
            "rendered": !merge,
            "summary": format!(
                "Joined clips {first} to {last} of track {} into one clip of {seconds:.3}s: {how}",
                args.track
            ),
        })))
    }
}
