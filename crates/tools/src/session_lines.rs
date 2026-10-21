//! The lines that describe a session to the model: one per track, bus and
//! marker (#408, #552, #618).
//!
//! Two things write them. The session block in the system prompt
//! (`ai::session_context`) lists as many as its byte budget allows and says
//! how many it left out; `list_session` lists the rest, a page at a time.
//! A line has to read the same in both, so it is written here, once, and
//! both call this module. `ai` imports the briefs and the functions below;
//! it does not write a line of its own.
//!
//! What a line says:
//!
//! - a track: `- <index> "<name>" (muted, soloed): <clips>; gain <db> dB;
//!   pan <where>; effects: <chain>`. The index is what a tool's `track`
//!   argument takes, and times are seconds on the session timeline;
//! - a bus: `- "<name>" (id <id>): from <sends>; effects: <chain>`. The id
//!   is what `set_send` and `remove_send` take;
//! - a marker: `- <name> @ <time>s`, a region `- <name> @ <start>s-<end>s`,
//!   in time order.
//!
//! Every list a line holds is bounded ([`MAX_CLIPS_LISTED`],
//! [`MAX_SENDS_LISTED`], [`MAX_EFFECTS_LISTED`]), so the one part of a line
//! that can grow without limit is a name.

use session::{Annotation, AnnotationKind, Bus, EffectInstance, SessionState, Track};

use crate::tool::util::clip_source_rate;

/// How many clips a track lists before the rest are summarised. A track
/// cut into hundreds of pieces would otherwise fill the prompt.
pub const MAX_CLIPS_LISTED: usize = 8;

/// How many sends a bus lists before the rest are summarised, for the same
/// reason: a reverb that every track feeds would otherwise be one very long
/// line.
pub const MAX_SENDS_LISTED: usize = 8;

/// How many effects a chain lists before the rest are summarised. Real
/// chains are far shorter. This is what keeps the master line, which is
/// never left out, from growing without bound.
pub const MAX_EFFECTS_LISTED: usize = 16;

/// Effect kinds in chain order, bypassed ones marked.
pub fn effect_labels(chain: &[EffectInstance]) -> Vec<String> {
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
    /// `track`, with its clips placed in seconds. `session_rate` is the
    /// rate a clip is counted in when its source's header cannot be read.
    ///
    /// Clip frames are counted in the clip's own source rate (#234), as
    /// `list_tracks` counts them, so the model and the timeline agree on
    /// where everything is. Reading a header opens the source file, which
    /// is why `list_session` builds only the tracks of the page it writes.
    pub fn of(track: &Track, session_rate: f64) -> TrackBrief {
        TrackBrief {
            name: track.name.clone(),
            clips: track
                .clips
                .iter()
                .map(|c| {
                    let rate = clip_source_rate(c).map(f64::from).unwrap_or(session_rate);
                    let start = c.start_in_track as f64 / rate;
                    (start, start + c.length as f64 / rate)
                })
                .collect(),
            gain_db: track.gain_db,
            pan: track.pan,
            muted: track.muted,
            soloed: track.soloed,
            effects: effect_labels(&track.effects),
        }
    }

    /// The tracks of `state`, in index order. A source whose header cannot
    /// be read falls back to the session rate, as the timeline does.
    pub fn from_state(state: &SessionState) -> Vec<TrackBrief> {
        let session_rate = session_rate(state);
        state
            .tracks
            .iter()
            .map(|t| TrackBrief::of(t, session_rate))
            .collect()
    }

    /// Where the last of its clips ends, in seconds; 0 for a track with no
    /// audio.
    pub fn end(&self) -> f64 {
        self.clips.iter().map(|&(_, end)| end).fold(0.0, f64::max)
    }
}

/// The session's rate, never 0.
pub fn session_rate(state: &SessionState) -> f64 {
    state.sample_rate.max(1) as f64
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
    /// `bus`, with the sends of `state`'s tracks that name it.
    pub fn of(state: &SessionState, bus: &Bus) -> BusBrief {
        BusBrief {
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
        }
    }

    /// The buses of `state`, in the order it holds them.
    ///
    /// A send to a bus the session does not hold is dropped: it names
    /// nothing the model could use.
    pub fn from_state(state: &SessionState) -> Vec<BusBrief> {
        state
            .bus_routing
            .buses
            .iter()
            .map(|bus| BusBrief::of(state, bus))
            .collect()
    }
}

/// The markers and regions of `annotations` in time order: a marker by its
/// time, a region by its start. Ties keep the order they are held in.
pub fn markers_by_time(annotations: &[Annotation]) -> Vec<&Annotation> {
    let mut sorted: Vec<&Annotation> = annotations.iter().collect();
    sorted.sort_by(|a, b| {
        time_of(a)
            .partial_cmp(&time_of(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
}

fn time_of(a: &Annotation) -> f64 {
    match a.kind {
        AnnotationKind::Marker { time_sec } => time_sec,
        AnnotationKind::Region { start_sec, .. } => start_sec,
    }
}

/// The line for track `i`, ending in a newline.
pub fn track_line(i: usize, t: &TrackBrief) -> String {
    let mut out = format!("- {i} \"{}\"", t.name);
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
        out.push_str(&format!("; effects: {}", chain_text(&t.effects)));
    }
    out.push('\n');
    out
}

/// The line for a bus, ending in a newline.
pub fn bus_line(b: &BusBrief) -> String {
    let mut out = format!("- \"{}\" (id {}): ", b.name, b.id);
    if b.sends.is_empty() {
        out.push_str("no sends");
    } else {
        let sends: Vec<String> = b
            .sends
            .iter()
            .take(MAX_SENDS_LISTED)
            .map(|s| format!("{} \"{}\" ({:+.1} dB)", s.track, s.track_name, s.level_db))
            .collect();
        out.push_str(&format!("from {}", sends.join(", ")));
        if b.sends.len() > MAX_SENDS_LISTED {
            out.push_str(&format!(", and {} more", b.sends.len() - MAX_SENDS_LISTED));
        }
    }
    if !b.effects.is_empty() {
        out.push_str(&format!("; effects: {}", chain_text(&b.effects)));
    }
    out.push('\n');
    out
}

/// The line for a marker or a region, ending in a newline.
pub fn marker_line(a: &Annotation) -> String {
    match &a.kind {
        AnnotationKind::Marker { time_sec } => format!("- {} @ {:.2}s\n", a.name, time_sec),
        AnnotationKind::Region { start_sec, end_sec } => {
            format!("- {} @ {:.2}s-{:.2}s\n", a.name, start_sec, end_sec)
        }
    }
}

/// A chain's effects, in order, at most [`MAX_EFFECTS_LISTED`] of them.
pub fn chain_text(effects: &[String]) -> String {
    let mut text = effects
        .iter()
        .take(MAX_EFFECTS_LISTED)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if effects.len() > MAX_EFFECTS_LISTED {
        text.push_str(&format!(
            ", and {} more",
            effects.len() - MAX_EFFECTS_LISTED
        ));
    }
    text
}

/// `-1.0` hard left, `0.0` centre, `1.0` hard right, as a person says it.
pub fn pan_text(pan: f32) -> String {
    let pct = (pan.abs() * 100.0).round() as i32;
    if pct == 0 {
        "centre".to_string()
    } else if pan < 0.0 {
        format!("{pct}% left")
    } else {
        format!("{pct}% right")
    }
}
