//! `SessionContext` — what the agent loop sees about the session and the
//! user's current focus on the timeline. Built per turn from the
//! frontend-pushed selection, the store head's annotations, and the
//! head's tracks.

use session::{Annotation, AnnotationKind, SessionState};
use tools::Range;

#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub selection: Option<Range>,
    pub markers: Vec<Annotation>,
    /// The node the session is at, as hex. Tools that act on a node
    /// (`render_final`, `render_preview`, `fork_node`) take this id, and
    /// the model has no other way to learn it.
    pub head: Option<String>,
    /// The tracks at the head, in the order the tools index them.
    pub tracks: Vec<TrackBrief>,
}

/// One track as the model needs to see it to act on it: which index it
/// is, what it is called, where its audio sits on the timeline, and its
/// mixer state (#408).
///
/// Without this the model knew nothing about the session it was editing.
/// "Fade out the last two seconds" could not be placed, because nothing
/// said how long the track was, and the model went looking for a way to
/// measure it instead of fading.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackBrief {
    pub name: String,
    /// Each clip's `(start, end)` on the session timeline, in seconds.
    pub clips: Vec<(f64, f64)>,
    pub gain_db: f32,
    pub pan: f32,
    pub muted: bool,
    pub soloed: bool,
    /// Effect kinds in chain order, bypassed ones marked.
    pub effects: Vec<String>,
}

impl TrackBrief {
    /// The tracks of `state`, in index order.
    ///
    /// Clip frames are counted in the clip's own source rate (#234), as
    /// `list_tracks` counts them, so the model and the timeline agree on
    /// where everything is. A source whose header cannot be read falls
    /// back to the session rate, as the timeline does.
    pub fn from_state(state: &SessionState) -> Vec<TrackBrief> {
        let session_rate = state.sample_rate.max(1) as f64;
        state
            .tracks
            .iter()
            .map(|t| TrackBrief {
                name: t.name.clone(),
                clips: t
                    .clips
                    .iter()
                    .map(|c| {
                        let rate = tools::clip_source_rate(c)
                            .map(f64::from)
                            .unwrap_or(session_rate);
                        let start = c.start_in_track as f64 / rate;
                        (start, start + c.length as f64 / rate)
                    })
                    .collect(),
                gain_db: t.gain_db,
                pan: t.pan,
                muted: t.muted,
                soloed: t.soloed,
                effects: t
                    .effects
                    .iter()
                    .map(|e| {
                        if e.bypassed {
                            format!("{} (bypassed)", e.kind)
                        } else {
                            e.kind.clone()
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    fn end(&self) -> f64 {
        self.clips.iter().map(|&(_, end)| end).fold(0.0, f64::max)
    }
}

/// How many clips a track lists before the rest are summarised. A track
/// cut into hundreds of pieces would otherwise fill the prompt.
const MAX_CLIPS_LISTED: usize = 8;

/// Render the context as a deterministic block to splice into the
/// system prompt. Returns an empty string when context is empty so
/// callers can splice unconditionally.
pub fn render_block(ctx: &SessionContext) -> String {
    if ctx.selection.is_none()
        && ctx.markers.is_empty()
        && ctx.head.is_none()
        && ctx.tracks.is_empty()
    {
        return String::new();
    }
    let mut out = String::new();
    if ctx.head.is_some() || !ctx.tracks.is_empty() {
        out.push_str("## session\n");
        if let Some(head) = &ctx.head {
            out.push_str(&format!("head node: {head}\n"));
        }
        let length = ctx.tracks.iter().map(TrackBrief::end).fold(0.0, f64::max);
        out.push_str(&format!("length: {length:.2}s\n\n"));
    }
    if !ctx.tracks.is_empty() {
        out.push_str("## tracks\n");
        out.push_str(
            "The index is what a tool's `track` argument takes. Times are seconds on the session timeline.\n",
        );
        for (i, t) in ctx.tracks.iter().enumerate() {
            out.push_str(&format!("- {i} \"{}\"", t.name));
            let mut flags = Vec::new();
            if t.muted {
                flags.push("muted");
            }
            if t.soloed {
                flags.push("soloed");
            }
            if !flags.is_empty() {
                out.push_str(&format!(" ({})", flags.join(", ")));
            }
            out.push_str(": ");
            match t.clips.as_slice() {
                [] => out.push_str("no audio"),
                [(start, end)] => out.push_str(&format!("{start:.2}-{end:.2}s")),
                clips => {
                    let listed: Vec<String> = clips
                        .iter()
                        .take(MAX_CLIPS_LISTED)
                        .map(|(s, e)| format!("{s:.2}-{e:.2}s"))
                        .collect();
                    out.push_str(&format!("{} clips: {}", clips.len(), listed.join(", ")));
                    if clips.len() > MAX_CLIPS_LISTED {
                        out.push_str(&format!(", and {} more", clips.len() - MAX_CLIPS_LISTED));
                    }
                }
            }
            out.push_str(&format!(
                "; gain {:+.1} dB; pan {}",
                t.gain_db,
                pan_text(t.pan)
            ));
            if !t.effects.is_empty() {
                out.push_str(&format!("; effects: {}", t.effects.join(", ")));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if let Some(sel) = ctx.selection {
        out.push_str("## current_selection\n");
        out.push_str(&format!(
            "start: {:.2}s  end: {:.2}s\n\n",
            sel.start_sec, sel.end_sec
        ));
    }
    if !ctx.markers.is_empty() {
        let mut sorted: Vec<&Annotation> = ctx.markers.iter().collect();
        sorted.sort_by(|a, b| {
            time_of(a)
                .partial_cmp(&time_of(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.push_str("## markers\n");
        for a in sorted {
            match &a.kind {
                AnnotationKind::Marker { time_sec } => {
                    out.push_str(&format!("- {} @ {:.2}s\n", a.name, time_sec));
                }
                AnnotationKind::Region { start_sec, end_sec } => {
                    out.push_str(&format!(
                        "- {} @ {:.2}s-{:.2}s\n",
                        a.name, start_sec, end_sec
                    ));
                }
            }
        }
    }
    out
}

/// `-1.0` hard left, `0.0` centre, `1.0` hard right, as a person says it.
fn pan_text(pan: f32) -> String {
    let pct = (pan.abs() * 100.0).round() as i32;
    if pct == 0 {
        "centre".to_string()
    } else if pan < 0.0 {
        format!("{pct}% left")
    } else {
        format!("{pct}% right")
    }
}

fn time_of(a: &Annotation) -> f64 {
    match a.kind {
        AnnotationKind::Marker { time_sec } => time_sec,
        AnnotationKind::Region { start_sec, .. } => start_sec,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn wav(path: &Path, rate: u32, seconds: f64) -> PathBuf {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).expect("wav");
        for _ in 0..((rate as f64 * seconds) as usize) {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        path.to_path_buf()
    }

    fn brief(name: &str, clips: &[(f64, f64)]) -> TrackBrief {
        TrackBrief {
            name: name.into(),
            clips: clips.to_vec(),
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
        }
    }

    #[test]
    fn an_empty_context_renders_nothing() {
        assert_eq!(render_block(&SessionContext::default()), "");
    }

    /// What the model needs to place "the last two seconds" and to name
    /// the node it is exporting.
    #[test]
    fn the_block_names_each_track_by_index_with_its_span_and_the_head() {
        let mut drums = brief("drums", &[(0.0, 16.0)]);
        drums.gain_db = -3.0;
        drums.pan = -1.0;
        drums.muted = true;
        drums.effects = vec!["low_pass".into(), "eq (bypassed)".into()];
        let ctx = SessionContext {
            head: Some("ab12".into()),
            tracks: vec![drums, brief("bass", &[(8.0, 12.5), (13.0, 25.07)])],
            ..Default::default()
        };

        let text = render_block(&ctx);

        assert!(text.contains("head node: ab12"), "{text}");
        assert!(text.contains("length: 25.07s"), "{text}");
        assert!(
            text.contains("- 0 \"drums\" (muted): 0.00-16.00s; gain -3.0 dB; pan 100% left; effects: low_pass, eq (bypassed)"),
            "{text}"
        );
        assert!(
            text.contains(
                "- 1 \"bass\": 2 clips: 8.00-12.50s, 13.00-25.07s; gain +0.0 dB; pan centre"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_track_cut_into_many_clips_is_summarised() {
        let clips: Vec<(f64, f64)> = (0..20).map(|i| (i as f64, i as f64 + 0.5)).collect();
        let ctx = SessionContext {
            tracks: vec![brief("chopped", &clips)],
            ..Default::default()
        };
        let text = render_block(&ctx);
        assert!(text.contains("20 clips:"), "{text}");
        assert!(text.contains("and 12 more"), "{text}");
        assert!(!text.contains("19.00-19.50s"), "{text}");
    }

    /// The model and the timeline must agree on where a clip is: both
    /// count a clip's frames in its own source's rate (#234).
    #[test]
    fn clip_times_use_the_clips_own_rate_not_the_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = wav(&dir.path().join("bed.wav"), 44_100, 2.0);
        let state = SessionState {
            tracks: vec![session::Track {
                id: session::TrackId::new(),
                name: "bed".into(),
                clips: vec![session::Clip {
                    source_path: src,
                    start_in_track: 44_100,
                    source_offset: 0,
                    length: 88_200,
                    content_hash: None,
                    time_stretch_factor: None,
                    pitch_shift_semitones: None,
                    beat_grid: None,
                    volume_envelope: Vec::new(),
                }],
                gain_db: 1.5,
                pan: 0.25,
                muted: false,
                soloed: true,
                effects: vec![],
                sends: vec![],
            }],
            bus_routing: session::BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
            sync_lock: false,
        };

        let tracks = TrackBrief::from_state(&state);

        assert_eq!(tracks.len(), 1);
        let (start, end) = tracks[0].clips[0];
        assert!(
            (start - 1.0).abs() < 1e-9 && (end - 3.0).abs() < 1e-9,
            "{:?}",
            tracks[0].clips
        );
        assert!(tracks[0].soloed);
        assert_eq!(pan_text(tracks[0].pan), "25% right");
    }
}
