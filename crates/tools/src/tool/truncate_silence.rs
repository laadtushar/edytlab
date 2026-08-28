use crate::schema::anthropic_tool;
use std::cell::RefCell;
use std::rc::Rc;

use crate::tool::util::{destructive_edit_then, dropped_labels_field, remap_after_cut};
use crate::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::Value;

/// Returns list of (start_frame, end_frame) silent regions meeting min duration.
pub(crate) fn find_silent_regions(
    samples: &[f32],
    sr: u32,
    channels: usize,
    threshold_db: f32,
    min_silence_ms: f32,
) -> Vec<(usize, usize)> {
    let channels = channels.max(1);
    let threshold_lin = 10.0f32.powf(threshold_db / 20.0);
    let min_frames = ((min_silence_ms * 0.001 * sr as f32) as usize).max(1);
    let n_frames = samples.len() / channels;
    let mut regions = Vec::new();
    let mut silent_start: Option<usize> = None;
    for frame in 0..n_frames {
        let peak = (0..channels)
            .map(|ch| samples[frame * channels + ch].abs())
            .fold(0.0f32, f32::max);
        let is_silent = peak < threshold_lin;
        match (is_silent, silent_start) {
            (true, None) => silent_start = Some(frame),
            (false, Some(start)) => {
                if frame - start >= min_frames {
                    regions.push((start, frame));
                }
                silent_start = None;
            }
            _ => {}
        }
    }
    if let Some(start) = silent_start {
        if n_frames - start >= min_frames {
            regions.push((start, n_frames));
        }
    }
    regions
}

pub(crate) fn apply_truncate_silence(
    samples: Vec<f32>,
    sr: u32,
    channels: usize,
    threshold_db: f32,
    min_silence_ms: f32,
) -> Vec<f32> {
    let channels = channels.max(1);
    let regions = find_silent_regions(&samples, sr, channels, threshold_db, min_silence_ms);
    if regions.is_empty() {
        return samples;
    }
    let n_frames = samples.len() / channels;
    let mut keep = vec![true; n_frames];
    for (s, e) in regions {
        keep[s..e].fill(false);
    }
    let mut out = Vec::with_capacity(samples.len());
    for (frame, &kept) in keep.iter().enumerate() {
        if kept {
            for ch in 0..channels {
                out.push(samples[frame * channels + ch]);
            }
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    threshold_db: f32,
    min_silence_ms: Option<f32>,
}

pub struct TruncateSilenceTool;

impl Tool for TruncateSilenceTool {
    fn name(&self) -> &'static str {
        "truncate_silence"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "truncate_silence",
            "Find and remove silent regions in a track. threshold_db is the silence floor; min_silence_ms is the minimum gap duration to remove. Appends a new session node.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "threshold_db": { "type": "number", "description": "Silence threshold in dBFS (e.g. -60)" },
                    "min_silence_ms": { "type": "number", "default": 500.0, "description": "Minimum silence duration to remove in ms" }
                },
                "required": ["track", "threshold_db"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        let min_ms = args.min_silence_ms.unwrap_or(500.0).max(1.0);
        let channels = {
            let state = match crate::tool::util::load_head_state(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(ToolResult::Error(e)),
            };
            if let Err(e) = crate::tool::util::check_track_index(&state.tracks, args.track) {
                return Ok(ToolResult::Error(e));
            }
            let clip = state.tracks[args.track].clips.first().cloned();
            if let Some(c) = clip {
                audio_decoder::decode_file(&c.source_path)
                    .map(|d| d.channels as usize)
                    .unwrap_or(1)
            } else {
                return Ok(ToolResult::Error(format!(
                    "track {} has no clips",
                    args.track
                )));
            }
        };
        let (thresh, min) = (args.threshold_db, min_ms);

        // The spans have to escape the edit closure (#231/#276).
        // `find_silent_regions` runs inside it, against the flattened
        // buffer the helper produces, and the after-hook is the only
        // place with a `SessionState` to remap — so the two share this.
        // The closure always runs first, so the hook never reads it
        // empty by accident.
        let removed: Rc<RefCell<Vec<(usize, usize)>>> = Rc::new(RefCell::new(Vec::new()));
        let removed_edit = Rc::clone(&removed);
        let removed_hook = Rc::clone(&removed);

        Ok(destructive_edit_then(
            ctx,
            args.track,
            move |samples, sr, chans| {
                *removed_edit.borrow_mut() =
                    find_silent_regions(samples, sr, channels, thresh, min);
                let result = apply_truncate_silence(samples.clone(), sr, channels, thresh, min);
                *samples = result;
                Ok((sr, chans))
            },
            move |state, _| {
                // Labels and words follow the audio out (#231). Applied
                // back to front: each cut renumbers everything after it,
                // so working forwards would leave every later span
                // pointing at coordinates that no longer exist.
                let rate = state.sample_rate.max(1) as f64;
                let mut dropped = 0;
                for (s, e) in removed_hook.borrow().iter().rev() {
                    dropped += remap_after_cut(state, *s as f64 / rate, *e as f64 / rate);
                }
                dropped_labels_field(dropped)
            },
            format!(
                "truncate_silence track {} threshold={}dB min={}ms",
                args.track, args.threshold_db, min_ms
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_truncate_silence, find_silent_regions};

    #[test]
    fn finds_silent_region() {
        let samples: Vec<f32> = [vec![0.5f32; 3], vec![0.0f32; 4], vec![0.5f32; 3]].concat();
        let regions = find_silent_regions(&samples, 10, 1, -60.0, 100.0);
        assert!(!regions.is_empty(), "should find a silent region");
        let (s, e) = regions[0];
        assert_eq!(s, 3);
        assert_eq!(e, 7);
    }

    #[test]
    fn removes_silence() {
        let samples: Vec<f32> = [vec![0.5f32; 3], vec![0.0f32; 4], vec![0.5f32; 3]].concat();
        let result = apply_truncate_silence(samples.clone(), 10, 1, -60.0, 100.0);
        assert_eq!(result.len(), 6, "silent frames removed");
    }
}
