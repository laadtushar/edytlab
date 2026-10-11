//! `SessionContext` — what the agent loop sees about the session and the
//! user's current focus on the timeline. Built per turn from the
//! frontend-pushed selection, the store head's annotations, and the
//! head's tracks, buses and master chain.

use session::{Annotation, AnnotationKind, EffectInstance, SessionState};
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
    /// The effects buses at the head, in the order the session holds them
    /// (#552). Empty when there are none, and then nothing about buses is
    /// said at all.
    pub buses: Vec<BusBrief>,
    /// The master bus's effects at the head, in chain order, bypassed ones
    /// marked (#460). The same words a track's `effects` use. Empty when
    /// the bus has none, and then nothing about it is said at all.
    pub master_effects: Vec<String>,
}

impl SessionContext {
    /// The master chain of `state`, as [`SessionContext::master_effects`]
    /// holds it.
    ///
    /// Without this the model could put an effect on the master but not
    /// see one an earlier turn had put there, so `effect_index` would be
    /// a guess. A track's chain is shown for the same reason.
    pub fn master_effects_from_state(state: &SessionState) -> Vec<String> {
        effect_labels(&state.master_chain)
    }
}

/// Effect kinds in chain order, bypassed ones marked.
fn effect_labels(chain: &[EffectInstance]) -> Vec<String> {
    chain
        .iter()
        .map(|e| {
            if e.bypassed {
                format!("{} (bypassed)", e.kind)
            } else {
                e.kind.clone()
            }
        })
        .collect()
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
                effects: effect_labels(&t.effects),
            })
            .collect()
    }

    fn end(&self) -> f64 {
        self.clips.iter().map(|&(_, end)| end).fold(0.0, f64::max)
    }
}

/// One effects bus as the model needs to see it to use it (#552): its
/// name, the id `set_send` and `remove_send` take, which tracks feed it,
/// and its effect chain.
///
/// Without this a conversation learned a bus only from its own
/// `create_bus` result, or from the error an unknown name got, and two
/// buses sharing a name cost a second refused call.
#[derive(Debug, Clone, PartialEq)]
pub struct BusBrief {
    pub name: String,
    /// The bus id, in full: a name may be shared, the id never is.
    pub id: String,
    /// The tracks that send to this bus, in track order.
    pub sends: Vec<SendBrief>,
    /// Effect kinds in chain order, bypassed ones marked, as a track's.
    pub effects: Vec<String>,
}

/// One track's send to a bus.
#[derive(Debug, Clone, PartialEq)]
pub struct SendBrief {
    /// The sending track's index, which is what a tool's `track` takes.
    pub track: usize,
    pub track_name: String,
    pub level_db: f32,
}

impl BusBrief {
    /// The buses of `state`, in the order it holds them.
    ///
    /// A send to a bus the session does not hold is dropped: it names
    /// nothing the model could use.
    pub fn from_state(state: &SessionState) -> Vec<BusBrief> {
        state
            .bus_routing
            .buses
            .iter()
            .map(|bus| BusBrief {
                name: bus.name.clone(),
                id: bus.id.to_string(),
                sends: state
                    .tracks
                    .iter()
                    .enumerate()
                    .flat_map(|(index, track)| {
                        track
                            .sends
                            .iter()
                            .filter(|send| send.bus_id == bus.id)
                            .map(move |send| SendBrief {
                                track: index,
                                track_name: track.name.clone(),
                                level_db: send.level_db,
                            })
                    })
                    .collect(),
                effects: effect_labels(&bus.effects),
            })
            .collect()
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
        && ctx.buses.is_empty()
        && ctx.master_effects.is_empty()
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
    if !ctx.buses.is_empty() {
        out.push_str("## buses\n");
        out.push_str(
            "Tracks send a parallel copy to a bus; `set_send` and `remove_send` take its id. The effect tools edit its chain with scope \"bus\" and `bus` (name or id); `effect_index` counts from 0 in the order listed.\n",
        );
        for b in &ctx.buses {
            out.push_str(&format!("- \"{}\" (id {}): ", b.name, b.id));
            if b.sends.is_empty() {
                out.push_str("no sends");
            } else {
                let sends: Vec<String> = b
                    .sends
                    .iter()
                    .map(|s| format!("{} \"{}\" ({:+.1} dB)", s.track, s.track_name, s.level_db))
                    .collect();
                out.push_str(&format!("from {}", sends.join(", ")));
            }
            if !b.effects.is_empty() {
                out.push_str(&format!("; effects: {}", b.effects.join(", ")));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    if !ctx.master_effects.is_empty() {
        out.push_str("## master\n");
        out.push_str(
            "The master bus sums every track. The effect tools edit its chain with scope \"master\" and no `track`; `effect_index` counts from 0 in this order.\n",
        );
        out.push_str(&format!("effects: {}\n\n", ctx.master_effects.join(", ")));
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

    /// The model can edit the master chain only if it can see it: the
    /// index it passes is a position in this list (#460).
    #[test]
    fn the_block_lists_the_master_chain_and_how_to_edit_it() {
        let ctx = SessionContext {
            tracks: vec![brief("drums", &[(0.0, 4.0)])],
            master_effects: vec!["gain".into(), "limiter (bypassed)".into()],
            ..Default::default()
        };

        let text = render_block(&ctx);

        assert!(text.contains("## master\n"), "{text}");
        assert!(text.contains("scope \"master\""), "{text}");
        assert!(
            text.contains("effects: gain, limiter (bypassed)\n"),
            "{text}"
        );
        // After the tracks, so the track lines the model already reads do
        // not move.
        assert!(text.find("## tracks").unwrap() < text.find("## master").unwrap());
    }

    /// A session with nothing on the master says nothing about it: the
    /// block, and so the request, is exactly what it was.
    #[test]
    fn an_empty_master_chain_adds_nothing() {
        let ctx = SessionContext {
            tracks: vec![brief("drums", &[(0.0, 4.0)])],
            ..Default::default()
        };
        assert!(!render_block(&ctx).contains("master"));
    }

    /// A session that is only a master chain still has something to say.
    #[test]
    fn a_master_chain_alone_is_not_an_empty_context() {
        let ctx = SessionContext {
            master_effects: vec!["limiter".into()],
            ..Default::default()
        };
        assert!(render_block(&ctx).contains("effects: limiter"));
    }

    #[test]
    fn the_master_chain_is_read_from_the_state_with_bypass_marked() {
        let mut state = SessionState {
            tracks: vec![],
            bus_routing: session::BusGraph::default(),
            master_chain: vec![
                session::EffectInstance {
                    kind: "gain".into(),
                    params: serde_json::json!({ "db": -3.0 }),
                    bypassed: false,
                },
                session::EffectInstance {
                    kind: "limiter".into(),
                    params: serde_json::json!({}),
                    bypassed: true,
                },
            ],
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
            sync_lock: false,
        };
        assert_eq!(
            SessionContext::master_effects_from_state(&state),
            ["gain", "limiter (bypassed)"]
        );
        state.master_chain.clear();
        assert!(SessionContext::master_effects_from_state(&state).is_empty());
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

    // ---------------------------------------------------------------
    // Buses (#552)
    // ---------------------------------------------------------------

    const REVERB_ID: &str = "3f2a8c10-5b7e-4d21-9a6c-0e1d2f3a4b5c";
    const DELAY_ID: &str = "9c4e1b72-0a3d-4f58-8b26-7d1e5c9a2f40";

    fn bus_brief(name: &str, id: &str) -> BusBrief {
        BusBrief {
            name: name.into(),
            id: id.into(),
            sends: vec![],
            effects: vec![],
        }
    }

    fn send_brief(track: usize, name: &str, level_db: f32) -> SendBrief {
        SendBrief {
            track,
            track_name: name.into(),
            level_db,
        }
    }

    /// A bus built the way the session file holds it, so the test needs
    /// no `uuid` of its own.
    fn state_bus(id: &str, name: &str, effects: Vec<EffectInstance>) -> session::Bus {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "effects": effects,
        }))
        .expect("a bus")
    }

    fn state_send(bus_id: &str, level_db: f32) -> session::Send {
        serde_json::from_value(serde_json::json!({
            "bus_id": bus_id,
            "level_db": level_db,
        }))
        .expect("a send")
    }

    fn state_effect(kind: &str, bypassed: bool) -> EffectInstance {
        EffectInstance {
            kind: kind.into(),
            params: serde_json::json!({}),
            bypassed,
        }
    }

    fn state_track(name: &str, sends: Vec<session::Send>) -> session::Track {
        session::Track {
            id: session::TrackId::new(),
            name: name.into(),
            clips: vec![],
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects: vec![],
            sends,
        }
    }

    fn state_with(tracks: Vec<session::Track>, buses: Vec<session::Bus>) -> SessionState {
        SessionState {
            tracks,
            bus_routing: session::BusGraph { buses },
            master_chain: Vec::new(),
            tempo_map: session::TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples: 0,
            annotations: Vec::new(),
            sync_lock: false,
        }
    }

    /// The model names a bus by what it reads here: a bus it was never
    /// told about costs a refused tool call to find (#552).
    #[test]
    fn the_block_lists_each_bus_with_its_id_its_sends_and_its_chain() {
        let mut reverb = bus_brief("Reverb", REVERB_ID);
        reverb.sends = vec![send_brief(0, "drums", -6.0), send_brief(2, "vocal", 0.0)];
        reverb.effects = vec!["reverb".into(), "eq (bypassed)".into()];
        let ctx = SessionContext {
            tracks: vec![
                brief("drums", &[(0.0, 4.0)]),
                brief("bass", &[(0.0, 4.0)]),
                brief("vocal", &[(0.0, 4.0)]),
            ],
            buses: vec![reverb],
            ..Default::default()
        };

        let text = render_block(&ctx);

        assert!(text.contains("## buses\n"), "{text}");
        assert!(
            text.contains(&format!(
                "- \"Reverb\" (id {REVERB_ID}): from 0 \"drums\" (-6.0 dB), 2 \"vocal\" (+0.0 dB); effects: reverb, eq (bypassed)\n"
            )),
            "{text}"
        );
        // The tools the id is for are named, so the model knows what to do
        // with it.
        assert!(text.contains("set_send"), "{text}");
    }

    /// Two buses may share a name; the id is what tells them apart, so
    /// both are shown in full.
    #[test]
    fn two_buses_with_one_name_are_told_apart_by_id() {
        let ctx = SessionContext {
            buses: vec![bus_brief("Verb", REVERB_ID), bus_brief("Verb", DELAY_ID)],
            ..Default::default()
        };

        let text = render_block(&ctx);

        assert!(text.contains(&format!("(id {REVERB_ID})")), "{text}");
        assert!(text.contains(&format!("(id {DELAY_ID})")), "{text}");
    }

    /// A bus nobody sends to, with nothing on it, is still a bus the model
    /// can name; it is said once and briefly.
    #[test]
    fn a_bus_with_no_sends_and_no_effects_says_only_that() {
        let ctx = SessionContext {
            tracks: vec![brief("drums", &[(0.0, 4.0)])],
            buses: vec![bus_brief("Delay", DELAY_ID)],
            ..Default::default()
        };

        let text = render_block(&ctx);

        assert!(
            text.contains(&format!("- \"Delay\" (id {DELAY_ID}): no sends\n")),
            "{text}"
        );
        assert!(!text.contains("effects:"), "{text}");
    }

    /// The header says how to edit a bus's chain, in the words `## master`
    /// uses for the master's: the scope, the argument that picks the bus,
    /// and what `effect_index` counts (#552, #533).
    #[test]
    fn the_buses_header_says_how_to_edit_a_bus_chain() {
        let ctx = SessionContext {
            buses: vec![bus_brief("Reverb", REVERB_ID)],
            ..Default::default()
        };

        let text = render_block(&ctx);

        let header = text
            .lines()
            .skip_while(|l| *l != "## buses")
            .nth(1)
            .expect("a header line under ## buses");
        assert!(header.contains("scope \"bus\""), "{header}");
        assert!(header.contains("`bus`"), "{header}");
        assert!(header.contains("name or id"), "{header}");
        assert!(header.contains("`effect_index`"), "{header}");
        assert!(header.contains("in the order listed"), "{header}");
        // The id tools are still named.
        assert!(
            header.contains("`set_send`") && header.contains("`remove_send`"),
            "{header}"
        );
    }

    /// The header words are the tools' own: if the effect tools stop taking
    /// `scope: "bus"` with a `bus` argument, this fails instead of the model
    /// being told a call that is refused.
    #[test]
    fn the_buses_header_matches_what_the_effect_tools_take() {
        let schemas = tools::ToolDispatcher::default_dispatcher().tool_schemas();
        for name in ["add_effect", "remove_effect", "set_effect_params"] {
            let schema = schemas
                .as_array()
                .and_then(|all| all.iter().find(|t| t["name"] == name))
                .unwrap_or_else(|| panic!("{name} is a registered tool"));
            let props = &schema["input_schema"]["properties"];
            let scopes = props["scope"]["enum"].as_array().expect("a scope enum");
            assert!(scopes.iter().any(|v| v == "bus"), "{name}: {scopes:?}");
            assert!(props.get("bus").is_some(), "{name}: no `bus` argument");
            if name != "add_effect" {
                assert!(
                    props.get("effect_index").is_some(),
                    "{name}: no effect_index"
                );
            }
        }
    }

    /// A session with no buses says nothing about them: the block, and so
    /// the request, is exactly what it was.
    #[test]
    fn no_buses_adds_no_section() {
        let ctx = SessionContext {
            head: Some("ab12".into()),
            tracks: vec![brief("drums", &[(0.0, 4.0)])],
            master_effects: vec!["limiter".into()],
            ..Default::default()
        };

        let text = render_block(&ctx);

        assert!(!text.contains("## buses"), "{text}");
        assert!(!text.contains("set_send"), "{text}");
        assert!(!text.contains("scope \"bus\""), "{text}");
    }

    /// A session that is only a bus still has something to say.
    #[test]
    fn a_bus_alone_is_not_an_empty_context() {
        let ctx = SessionContext {
            buses: vec![bus_brief("Reverb", REVERB_ID)],
            ..Default::default()
        };
        assert!(render_block(&ctx).contains(REVERB_ID));
    }

    /// Signal flow order: tracks, then the buses they feed, then the
    /// master that sums it all. The track lines the model already reads do
    /// not move.
    #[test]
    fn the_buses_come_after_the_tracks_and_before_the_master() {
        let ctx = SessionContext {
            tracks: vec![brief("drums", &[(0.0, 4.0)])],
            buses: vec![bus_brief("Reverb", REVERB_ID)],
            master_effects: vec!["limiter".into()],
            ..Default::default()
        };

        let text = render_block(&ctx);

        let tracks = text.find("## tracks").unwrap();
        let buses = text.find("## buses").unwrap();
        let master = text.find("## master").unwrap();
        assert!(tracks < buses && buses < master, "{text}");
    }

    #[test]
    fn the_buses_are_read_from_the_state_with_their_sends_and_bypass_marked() {
        let state = state_with(
            vec![
                state_track("drums", vec![state_send(REVERB_ID, -6.0)]),
                state_track("bass", vec![]),
                state_track(
                    "vocal",
                    vec![state_send(DELAY_ID, -12.0), state_send(REVERB_ID, 0.0)],
                ),
            ],
            vec![
                state_bus(
                    REVERB_ID,
                    "Reverb",
                    vec![state_effect("reverb", false), state_effect("eq", true)],
                ),
                state_bus(DELAY_ID, "Delay", vec![]),
            ],
        );

        let buses = BusBrief::from_state(&state);

        assert_eq!(
            buses,
            vec![
                BusBrief {
                    name: "Reverb".into(),
                    id: REVERB_ID.into(),
                    sends: vec![send_brief(0, "drums", -6.0), send_brief(2, "vocal", 0.0)],
                    effects: vec!["reverb".into(), "eq (bypassed)".into()],
                },
                BusBrief {
                    name: "Delay".into(),
                    id: DELAY_ID.into(),
                    sends: vec![send_brief(2, "vocal", -12.0)],
                    effects: vec![],
                },
            ]
        );
    }

    /// A send whose bus the session does not hold names nothing the model
    /// could use, so it is not listed under a bus it does not belong to.
    #[test]
    fn a_send_to_a_bus_the_session_does_not_hold_is_left_out() {
        let state = state_with(
            vec![state_track("drums", vec![state_send(DELAY_ID, -3.0)])],
            vec![state_bus(REVERB_ID, "Reverb", vec![])],
        );

        let buses = BusBrief::from_state(&state);

        assert_eq!(buses.len(), 1);
        assert!(buses[0].sends.is_empty(), "{:?}", buses[0].sends);
    }

    #[test]
    fn a_state_without_buses_has_no_bus_briefs() {
        assert!(
            BusBrief::from_state(&state_with(vec![state_track("a", vec![])], vec![])).is_empty()
        );
    }
}
