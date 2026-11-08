use serde::Deserialize;
use serde_json::{json, Value};

use session::{AnnotationKind, SessionState};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    attach_render_warnings, load_head_state, parse_args, render_warnings_for_tracks,
    session_length_frames,
};
use crate::{Tool, ToolContext, ToolResult};

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    track_indices: Option<Vec<usize>>,
    /// One file per region annotation, each its span of the mix (#766).
    #[serde(default)]
    regions: bool,
    output_dir: String,
    format: Option<String>,
}

pub struct ExportMultipleTool;

/// Sanitize a track name for use as a filename component.
/// Keeps alphanumeric chars, underscores, and hyphens; replaces everything
/// else (spaces, dots, slashes, etc.) with `_`.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The file a track is exported to, in the output folder.
fn file_name(index: usize, track_name: &str) -> String {
    format!("{index}_{}.wav", sanitize_name(track_name))
}

/// Characters of a region's name kept in its file name. A label's name has
/// no length limit, and a file name past the file system's (255 bytes on
/// most) would fail the write after the earlier files were written.
const REGION_NAME_CHARS: usize = 60;

/// The file the `n`th region (from 1, in time order) is exported to. The
/// number keeps two regions with one name apart; padded to the widest so
/// the files list in time order.
fn region_file_name(n: usize, width: usize, name: &str) -> String {
    let name: String = sanitize_name(name)
        .chars()
        .take(REGION_NAME_CHARS)
        .collect();
    format!("{n:0width$}_{name}.wav")
}

/// How far a render of `state` reaches, in session frames: the furthest clip
/// end over the tracks the render mixes, so a muted track, or one another
/// track's solo silences, does not lengthen it. Which tracks those are is
/// asked of the renderer's own plan, so the two agree.
fn mix_length_frames(state: &SessionState) -> Result<u64, String> {
    let graph = audio_engine::graph::build(state)
        .map_err(|e| format!("could not plan the mix to export: {e}; nothing was written"))?;
    let mut heard = state.clone();
    let mut index = 0;
    heard.tracks.retain(|_| {
        let mixed = graph
            .tracks
            .iter()
            .any(|plan| plan.track_index == index && plan.contributes);
        index += 1;
        mixed
    });
    Ok(session_length_frames(&heard))
}

/// A region to export: its name, and its frames of the mix.
struct RegionSpan<'a> {
    name: &'a str,
    range: audio_engine::TimeRange,
}

/// What `regions: true` exports, worked out before anything is written.
struct RegionPlan<'a> {
    spans: Vec<RegionSpan<'a>>,
    /// Point markers, which have no span.
    skipped_markers: usize,
    /// Regions with an edge that is not a finite number of seconds.
    skipped_not_finite: usize,
    /// Regions cut at the start or the end of the session.
    clamped: usize,
}

/// The regions of `state` in time order, each clamped to the mix. A region
/// that is empty once clamped (wholly past the end) refuses the call, since
/// the file the user asked for cannot be made.
fn plan_regions(state: &SessionState) -> Result<RegionPlan<'_>, String> {
    let mut skipped_markers = 0;
    let mut skipped_not_finite = 0;
    let mut regions: Vec<(&str, f64, f64)> = Vec::new();
    for label in &state.annotations {
        match label.kind {
            AnnotationKind::Marker { .. } => skipped_markers += 1,
            AnnotationKind::Region { start_sec, end_sec } => {
                if start_sec.is_finite() && end_sec.is_finite() {
                    regions.push((label.name.as_str(), start_sec, end_sec));
                } else {
                    skipped_not_finite += 1;
                }
            }
        }
    }
    if regions.is_empty() {
        let mut msg = "the session has no region to export; add one with add_markers".to_string();
        if skipped_not_finite > 0 {
            msg.push_str(&format!(
                " ({skipped_not_finite} stored region(s) have a time that is not finite)"
            ));
        }
        return Err(msg);
    }
    regions.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)));

    let rate = f64::from(state.sample_rate.max(1));
    // A float-to-integer cast saturates, so a huge time is the largest frame
    // and a negative one frame 0.
    let frame = |sec: f64| (sec * rate) as u64;
    if let Some((name, start_sec, end_sec)) = regions
        .iter()
        .find(|&&(_, start_sec, end_sec)| frame(start_sec) >= frame(end_sec))
    {
        let name: String = name.chars().take(REGION_NAME_CHARS).collect();
        return Err(format!(
            "region {name:?} ({start_sec:.3}-{end_sec:.3} s) is empty: it is shorter \
             than one sample at {rate} Hz. Widen or delete it, then export again; \
             nothing was written"
        ));
    }
    let total = mix_length_frames(state)?;
    let mut clamped = 0;
    let mut spans = Vec::with_capacity(regions.len());
    for (name, start_sec, end_sec) in regions {
        let start = frame(start_sec);
        let end = frame(end_sec);
        let end_frame = end.min(total);
        if start >= end_frame {
            let name: String = name.chars().take(REGION_NAME_CHARS).collect();
            return Err(format!(
                "region {name:?} ({start_sec:.2}-{end_sec:.2} s) has nothing to export: \
                 it lies past the end of the session at {:.2} s. Move or delete it, \
                 then export again; nothing was written",
                total as f64 / rate
            ));
        }
        if start_sec < 0.0 || end > total {
            clamped += 1;
        }
        spans.push(RegionSpan {
            name,
            range: audio_engine::TimeRange {
                start_frame: start,
                end_frame,
            },
        });
    }
    Ok(RegionPlan {
        spans,
        skipped_markers,
        skipped_not_finite,
        clamped,
    })
}

/// `n thing` or `n things`.
fn count(n: usize, thing: &str) -> String {
    format!("{n} {thing}{}", if n == 1 { "" } else { "s" })
}

impl Tool for ExportMultipleTool {
    fn name(&self) -> &'static str {
        "export_multiple"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "export_multiple",
            "Export selected tracks (or regions) as individual WAV files to a directory. Does not modify the session. Returns list of exported file paths.",
            json!({
                "type": "object",
                "properties": {
                    "track_indices": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Indices of tracks to export"
                    },
                    "regions": {
                        "type": "boolean",
                        "description": "Instead of track_indices: one file per region marker, each its span of the full mix"
                    },
                    "output_dir": {
                        "type": "string",
                        "description": "Directory to write exported files into"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["wav"],
                        "default": "wav"
                    }
                },
                "required": ["output_dir"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };

        // Validate format field.
        if let Some(ref fmt) = args.format {
            if fmt != "wav" {
                return Ok(ToolResult::Error(
                    "only 'wav' format is currently supported".to_string(),
                ));
            }
        }

        let track_indices = match (args.track_indices, args.regions) {
            (Some(_), true) => {
                return Ok(ToolResult::Error(
                    "pass track_indices or regions: true, not both".to_string(),
                ))
            }
            (None, false) => {
                return Ok(ToolResult::Error(
                    "pass track_indices (one file per track) or regions: true \
                     (one file per region marker)"
                        .to_string(),
                ))
            }
            (None, true) => None,
            (Some(indices), false) => Some(indices),
        };

        if track_indices.as_ref().is_some_and(Vec::is_empty) {
            return Ok(ToolResult::Error(
                "track_indices must not be empty".to_string(),
            ));
        }

        // Reject duplicates — would produce identical output filenames and lie about count.
        let mut seen = std::collections::HashSet::new();
        for &idx in track_indices.iter().flatten() {
            if !seen.insert(idx) {
                return Ok(ToolResult::Error(format!(
                    "track_indices contains duplicate index {idx}"
                )));
            }
        }

        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        // Validate all indices before doing any work.
        for &idx in track_indices.iter().flatten() {
            if idx >= state.tracks.len() {
                return Ok(ToolResult::Error(format!(
                    "track {idx} is out of range; session has {} track{}",
                    state.tracks.len(),
                    if state.tracks.len() == 1 { "" } else { "s" },
                )));
            }
        }

        // Restrict output_dir to a relative path inside the project directory.
        let raw_out = std::path::Path::new(&args.output_dir);

        // Reject absolute paths and parent-traversal components.
        if raw_out.is_absolute()
            || raw_out
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Ok(ToolResult::Error(
                "output_dir must be a relative path with no '..' components".into(),
            ));
        }

        let out_dir = ctx.store.project_dir().join(raw_out);

        let Some(track_indices) = track_indices else {
            return Ok(export_regions(ctx, &state, &out_dir));
        };

        // Each file's name is known now, so none of them is rendered onto
        // audio the project reads (#653) before the first is written.
        let planned: Vec<std::path::PathBuf> = track_indices
            .iter()
            .map(|&idx| out_dir.join(file_name(idx, &state.tracks[idx].name)))
            .collect();
        if let Err(e) = crate::destination::refuse_project_audio(ctx.store, &planned) {
            return Ok(ToolResult::Error(e));
        }

        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            return Ok(ToolResult::Error(format!(
                "failed to create output_dir {}: {e}",
                out_dir.display()
            )));
        }

        let mut exported: Vec<String> = Vec::with_capacity(track_indices.len());

        for &idx in &track_indices {
            let track = &state.tracks[idx];
            let out_path = out_dir.join(file_name(idx, &track.name));

            // Build a single-track state: one track, unmuted, unsoloed.
            let mut single_state = state.clone();
            single_state.tracks = {
                let mut t = track.clone();
                t.muted = false;
                t.soloed = false;
                vec![t]
            };

            match audio_engine::render_state_to_wav(&single_state, &out_path, None) {
                Ok(_report) => {
                    exported.push(out_path.to_string_lossy().into_owned());
                }
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "render failed for track {idx}: {e}"
                    )));
                }
            }
        }

        let count = exported.len();
        let mut out = json!({
            "exported": exported,
            "count": count,
            "summary": format!(
                "Exported {count} track{} to {}",
                if count == 1 { "" } else { "s" },
                out_dir.display()
            )
        });
        // Each file is a render of one track, so the reports' warnings
        // would repeat the master's once per file and number the track 0
        // every time. Asked of the session instead: each exported track by
        // its own index, the master and the buses once.
        attach_render_warnings(
            &mut out,
            &render_warnings_for_tracks(&state, &track_indices),
        );
        Ok(ToolResult::Ok(out))
    }
}

/// `regions: true`: one WAV per region annotation of the head, each the
/// render of the whole session (mute, solo, buses and master as they are)
/// over that region's seconds, as `render_final` with that `range` writes
/// it (#766). Every file is planned, and every destination checked against
/// the project's audio (#653), before the first is written.
fn export_regions(
    ctx: &mut ToolContext,
    state: &SessionState,
    out_dir: &std::path::Path,
) -> ToolResult {
    let plan = match plan_regions(state) {
        Ok(p) => p,
        Err(e) => return ToolResult::Error(e),
    };
    let width = plan.spans.len().to_string().len();
    let planned: Vec<std::path::PathBuf> = plan
        .spans
        .iter()
        .enumerate()
        .map(|(i, span)| out_dir.join(region_file_name(i + 1, width, span.name)))
        .collect();
    if let Err(e) = crate::destination::refuse_project_audio(ctx.store, &planned) {
        return ToolResult::Error(e);
    }
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        return ToolResult::Error(format!(
            "failed to create output_dir {}: {e}",
            out_dir.display()
        ));
    }

    let mut exported: Vec<String> = Vec::with_capacity(planned.len());
    for (span, out_path) in plan.spans.iter().zip(&planned) {
        if let Err(e) = ctx.engine.render_to_wav(state, out_path, Some(span.range)) {
            return ToolResult::Error(format!(
                "render failed for {}: {e}; {} written before it",
                out_path.display(),
                count(exported.len(), "file"),
            ));
        }
        exported.push(out_path.to_string_lossy().into_owned());
    }

    let n = exported.len();
    let mut summary = format!("Exported {} to {}", count(n, "region"), out_dir.display());
    if plan.skipped_markers > 0 {
        summary.push_str(&format!(
            "; left out {} (they have no span)",
            count(plan.skipped_markers, "point marker")
        ));
    }
    if plan.skipped_not_finite > 0 {
        summary.push_str(&format!(
            "; skipped {} with a time that is not finite",
            count(plan.skipped_not_finite, "region")
        ));
    }
    if plan.clamped > 0 {
        summary.push_str(&format!(
            "; cut {} at the session's edge",
            count(plan.clamped, "region")
        ));
    }
    let mut out = json!({
        "exported": exported,
        "count": n,
        "skipped_markers": plan.skipped_markers,
        "skipped_not_finite": plan.skipped_not_finite,
        "clamped": plan.clamped,
        "summary": summary,
    });
    // Every file renders the same session, so the warnings are the
    // session's, said once rather than once a file.
    attach_render_warnings(&mut out, &audio_engine::render_warnings(state));
    ToolResult::Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{plan_regions, ExportMultipleTool};
    use crate::Tool;
    use session::{Annotation, AnnotationId, AnnotationKind, SessionState};

    #[test]
    fn tool_name() {
        assert_eq!(ExportMultipleTool.name(), "export_multiple");
    }

    fn state_with(regions: &[(f64, f64)]) -> SessionState {
        SessionState {
            tracks: Vec::new(),
            bus_routing: session::BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 1_000,
            length_samples: 0,
            annotations: regions
                .iter()
                .map(|&(start_sec, end_sec)| Annotation {
                    id: AnnotationId::new(),
                    name: "r".into(),
                    kind: AnnotationKind::Region { start_sec, end_sec },
                })
                .collect(),
            sync_lock: false,
        }
    }

    // A stored node cannot hold a time that is not finite (JSON has no
    // NaN, so the store refuses to read one back), which is why these are
    // asked of the plan and not through the store.

    #[test]
    fn a_region_shorter_than_one_sample_is_refused_as_empty_not_past_the_end() {
        // 1.0000 to 1.0004 s is the same frame at 1 kHz.
        let err = plan_regions(&state_with(&[(1.0, 1.0004)]))
            .err()
            .expect("an empty region is refused");
        assert!(err.contains("is empty"), "{err}");
        assert!(!err.contains("past the end"), "{err}");
    }

    #[test]
    fn a_region_whose_time_is_not_finite_is_skipped_and_counted() {
        let state = state_with(&[(f64::NAN, 1.0), (1.0, f64::INFINITY), (0.5, 1.0)]);
        // The third is finite: it goes on to the mix plan, which a session
        // with no tracks refuses, which shows the first two were skipped
        // rather than refused.
        let err = plan_regions(&state)
            .err()
            .expect("a session with no tracks refuses it");
        assert!(err.contains("could not plan the mix"), "{err}");

        let only_broken = state_with(&[(f64::NAN, 1.0), (f64::NEG_INFINITY, 2.0)]);
        let err = plan_regions(&only_broken).err().expect("nothing to export");
        assert!(err.contains("no region"), "{err}");
        assert!(err.contains("2 stored region(s)"), "{err}");
    }
}
