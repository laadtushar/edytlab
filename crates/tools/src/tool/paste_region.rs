//! Paste clipboard audio into a track at a given time offset.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    destructive_edit_then, insert_gap_timeline, load_head_state, remap_after_insert,
    seconds_to_frames, sync_other_tracks, timeline_end, track_channels, track_clip_rates,
};
use crate::{Tool, ToolContext, ToolResult};

/// Splice `clipboard` into `samples` at `at_sec`. The insertion point is
/// clamped to `samples.len()` so a value past the end appends rather than
/// panicking.
pub fn apply(
    samples: &mut Vec<f32>,
    sample_rate: u32,
    channels: usize,
    at_sec: f64,
    clipboard: &Option<crate::Clipboard>,
) -> Result<(), PasteError> {
    let clip = clipboard.as_ref().ok_or(PasteError::EmptyClipboard)?;
    let stride = channels.max(1);

    // Conform before splicing (#239). The clipboard used to be a bare
    // sample vector, so this function read it with the *destination's*
    // stride: two seconds of stereo pasted into a mono track became four
    // seconds of alternating left/right, and the tool returned Ok.
    let data = conform(clip, sample_rate, stride)?;

    let total_frames = samples.len() / stride;
    // Splice on a frame boundary: an offset landing mid-frame would
    // shift every following sample by one and swap left and right for
    // the rest of the track.
    let offset = ((at_sec * sample_rate as f64) as usize).min(total_frames) * stride;
    samples.splice(offset..offset, data.iter().copied());
    Ok(())
}

/// Re-interleave `clip` for a destination of `to_channels` at
/// `to_rate`, or explain why it cannot be.
///
/// Channel count is convertible in the two directions that have an
/// unambiguous answer — duplicate a mono source across the
/// destination's channels, or average a multi-channel source down to
/// mono. Anything else (5.1 into stereo, say) has no single right
/// answer, so it is refused by name rather than guessed at.
///
/// Sample rate is refused outright. Resampling here would be a second
/// resampler in a second place with its own latency and quality
/// characteristics, and `resample_track` already exists to make the two
/// tracks agree first. Splicing across rates is the failure this issue
/// is about, so the one thing this must not do is proceed quietly.
pub(crate) fn conform(
    clip: &crate::Clipboard,
    to_rate: u32,
    to_channels: usize,
) -> Result<std::borrow::Cow<'_, [f32]>, PasteError> {
    if clip.sample_rate != to_rate {
        return Err(PasteError::RateMismatch {
            from: clip.sample_rate,
            to: to_rate,
        });
    }

    let from = clip.channels.max(1) as usize;
    let to = to_channels.max(1);
    if from == to {
        return Ok(std::borrow::Cow::Borrowed(&clip.samples));
    }

    if from == 1 {
        // One source sample becomes `to` identical samples: the same
        // signal in every channel, which is what a listener expects
        // from pasting mono into a stereo track.
        let mut out = Vec::with_capacity(clip.samples.len() * to);
        for s in &clip.samples {
            out.extend(std::iter::repeat_n(*s, to));
        }
        return Ok(std::borrow::Cow::Owned(out));
    }

    if to == 1 {
        // Average, not "take the left channel": dropping a channel
        // silently loses anything panned to it.
        let out = clip
            .samples
            .chunks_exact(from)
            .map(|frame| frame.iter().sum::<f32>() / from as f32)
            .collect();
        return Ok(std::borrow::Cow::Owned(out));
    }

    Err(PasteError::ChannelMismatch { from, to })
}

#[derive(Debug, thiserror::Error)]
pub enum PasteError {
    #[error("clipboard is empty; run copy_region first")]
    EmptyClipboard,

    #[error(
        "clipboard was copied at {from} Hz but track {to} Hz; run resample_track \
         on one of them first so the two agree"
    )]
    RateMismatch { from: u32, to: u32 },

    #[error(
        "cannot paste {from}-channel audio into a {to}-channel track: only \
         mono-to-many and many-to-mono have an unambiguous conversion"
    )]
    ChannelMismatch { from: usize, to: usize },
}

pub struct PasteRegionTool;

impl Tool for PasteRegionTool {
    fn name(&self) -> &'static str {
        "paste_region"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "paste_region",
            "Paste the clipboard audio (set by copy_region) into a track at a given offset. \
             The clipboard audio is spliced in; samples after the insertion point are shifted right. \
             With sync-lock on (see `set_sync_lock`) the gap opens on every other track too, \
             so the tracks stay aligned: the call then moves clips on tracks it does not name, \
             and a clip that straddles the insertion point is split in two. \
             Appends a new session node.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Zero-based track index" },
                    "at":    {
                        "type": "number",
                        "description": "Insertion point in seconds; past the end appends"
                    }
                },
                "required": ["track", "at"],
                "additionalProperties": false
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            track: usize,
            at: f64,
        }

        let parsed: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        // Snapshot clipboard now so the closure captures it by value.
        // We check for empty before the closure to give an early error.
        let clipboard_snap = ctx.clipboard.clone();
        if clipboard_snap.is_none() {
            return Ok(ToolResult::Error(
                "clipboard is empty; run copy_region first".into(),
            ));
        }
        let at = parsed.at;
        let track = parsed.track;
        let channels = match track_channels(ctx, track) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        // The clipboard is the hidden input that made this op
        // unreplayable (#163): it lived in memory and was never
        // persisted, so after a paste the audio existed only inside the
        // derived file. `copy_region` writes a CAS blob; find it by
        // content, and write it here if the copy could not (an older
        // session, or a failed write) so the op still closes over what
        // it read.
        //
        // Hashed with the *clipboard's* own rate and channel count, not
        // the destination's (#239). `audio_hash` mixes both into the
        // digest, so hashing a stereo capture as if it were mono
        // produced a second, mono-headered duplicate blob — and the op's
        // `inputs.clipboard` then pointed at audio `copy_region` never
        // wrote. Replaying that provenance would have reproduced the
        // wrong sound.
        let pasted = match clipboard_snap.as_ref() {
            Some(c) => c,
            // Unreachable: emptiness was rejected above. Returning
            // rather than defaulting keeps it that way if the guard
            // above ever moves.
            None => {
                return Ok(ToolResult::Error(
                    "clipboard is empty; run copy_region first".into(),
                ))
            }
        };
        let project_dir = ctx.store.project_dir().to_path_buf();
        let blob = crate::provenance::store_clipboard_blob(
            &project_dir,
            &pasted.samples,
            pasted.sample_rate,
            pasted.channels,
        )
        .ok();

        // What the paste puts in, in the clipboard's own terms. A splice
        // keeps the frame count whatever the channel conversion does
        // (`conform` duplicates or averages within a frame), and it only
        // succeeds when the clipboard's rate is the track's, so these two
        // numbers are also the length of the gap it opens.
        let (clip_rate, clip_frames) = (
            pasted.sample_rate.max(1),
            pasted.samples.len() / pasted.channels.max(1) as usize,
        );

        // Each other track's own rate, for sync-lock below (#234, #673), asked
        // before anything is edited so a track with no single rate refuses
        // the whole paste.
        let rates = match load_head_state(ctx) {
            Ok(state) if state.sync_lock => match track_clip_rates(&state) {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::Error(e)),
            },
            Ok(_) => Vec::new(),
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        let label = format!("paste clipboard at {at:.2}s on track {track}");
        let summary = label.clone();
        let result = destructive_edit_then(
            ctx,
            track,
            move |samples, sample_rate, chans| {
                // A refused paste (another rate, a channel layout with no
                // unambiguous conversion) ends the edit here. It used to
                // be dropped, which left a new node holding the track
                // unchanged under the summary "paste clipboard at …" —
                // and would now have moved the labels for audio that never
                // went in.
                apply(samples, sample_rate, channels, at, &clipboard_snap)
                    .map_err(|e| e.to_string())?;
                Ok((sample_rate, chans))
            },
            move |state, edited| {
                // A paste is an insert: everything after the point is
                // later by the clipboard's length, and the labels and
                // words that name those moments have to be too (#203).
                // `apply` clamps the point to the end of the track, so
                // the same clamp is applied here; the track is already
                // the longer one, hence the subtraction.
                let rate = f64::from(clip_rate);
                let old_end =
                    timeline_end(&state.tracks[edited].clips).saturating_sub(clip_frames as u64);
                let at_frames = ((at.max(0.0) * rate) as u64).min(old_end);
                let at_sec = at_frames as f64 / rate;
                let len_sec = clip_frames as f64 / rate;
                remap_after_insert(state, at_sec, len_sec);

                // Sync-lock (#170 §3, #572): the track the paste names got
                // longer by the clipboard, and every other track has to open
                // the same gap or everything after it drifts by exactly what
                // was pasted. Same node as the sample edit, so undo puts all
                // of it back, and the same timeline shift `insert_silence`
                // makes, which costs no derived file.
                //
                // The gap is where the audio actually went in (`at` clamped
                // to the end of the track) and as long as the clipboard
                // plays, in seconds, then counted in each other track's own
                // frames (#234, #673): the paste only succeeds when the
                // clipboard is at the *pasted track's* rate, and the others
                // may be at that rate, at the session's, or at a third, so
                // neither the clipboard's frame count nor the session's rate
                // is their unit. Labels and words are not touched again
                // here; they are the session's, not a track's, and moved once
                // above.
                let mut extras = serde_json::Map::new();
                if !state.sync_lock {
                    return extras;
                }
                let synced = if len_sec <= 0.0 {
                    0
                } else {
                    sync_other_tracks(state, edited, &rates, |clips, rate| {
                        let gap_len = seconds_to_frames(len_sec, rate);
                        if gap_len == 0 {
                            return clips.to_vec();
                        }
                        insert_gap_timeline(clips, seconds_to_frames(at_sec, rate), gap_len)
                    })
                };
                extras.insert("synced_tracks".into(), json!(synced));
                if synced > 0 {
                    // The node keeps the plain label; the answer says what
                    // else moved, as `cut_range`'s does.
                    extras.insert(
                        "summary".into(),
                        json!(format!(
                            "{summary} and {synced} other track(s), sync-locked"
                        )),
                    );
                }
                extras
            },
            label,
        );

        // Record the op here rather than letting the dispatcher's default
        // apply: the default marks every paste unreplayable, which was
        // true before the blob existed and is not now.
        if let (Some(hash), Some(head)) = (blob, ctx.store.head()) {
            let op = session::NodeOp::new(
                "paste_region".to_string(),
                json!({ "track": track, "at": at }),
                env!("CARGO_PKG_VERSION").to_string(),
            )
            .with_inputs(json!({ "clipboard": hash }));
            if let Err(e) = ctx.store.set_op(head, op) {
                tracing::warn!(error = %e, "failed to record paste provenance");
            }
        }

        Ok(result)
    }
}
