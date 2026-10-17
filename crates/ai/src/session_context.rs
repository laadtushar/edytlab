//! `SessionContext` — what the agent loop sees about the session and the
//! user's current focus on the timeline. Built per turn from the
//! frontend-pushed selection, the store head's annotations, and the
//! head's tracks, buses and master chain.
//!
//! # The budget (#573)
//!
//! The block goes into the system prompt on every turn, and the tracks,
//! buses and markers in it grow with the session. So it is written within
//! a byte budget that depends on what the provider is sent
//! ([`block_budget`]):
//!
//! - [`SESSION_BLOCK_MAX_BYTES`] (8,000 B) for a provider sent every tool,
//!   which is every hosted one;
//! - [`SLIM_SESSION_BLOCK_MAX_BYTES`] (2,300 B) for one sent the slim set,
//!   which is Ollama, whose whole first request must fit in 12,288 B. There
//!   the block also shares the request with the tools: what it takes past
//!   `tool_selection::SLIM_SESSION_ALLOWANCE_BYTES` comes off the tool
//!   budget.
//!
//! A session that fits is listed in full. One that does not is cut list by
//! list, and each cut list says how many it left out ([`render_block_within`]).
//! `tests/request_size.rs` holds the requests a huge session makes under
//! the bound with headroom.

use session::{Annotation, AnnotationKind, EffectInstance, SessionState};
use tools::Range;

use crate::provider::ToolSet;

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

/// How many sends a bus lists before the rest are summarised, for the same
/// reason: a reverb that every track feeds would otherwise be one very long
/// line.
const MAX_SENDS_LISTED: usize = 8;

/// How many effects a chain lists before the rest are summarised. Real
/// chains are far shorter. This is what keeps the master line, which is
/// never left out, from growing without bound.
const MAX_EFFECTS_LISTED: usize = 16;

/// The most the block may take, in bytes of its text, for a provider that
/// is sent every tool ([`ToolSet::Full`]: Anthropic and every other hosted
/// provider) (#573).
///
/// Such a provider has the room (Anthropic's first request is about 63 KB
/// before the block), but it pays for the block on every turn, and none
/// of it is cached, because the block changes with every edit. The
/// thirty-track mix with eight buses and forty markers in
/// `tests/request_size.rs` comes to about 5.5 KB and is listed in full;
/// two hundred tracks, fifty buses and a thousand markers came to 47 KB,
/// and are cut to this.
pub const SESSION_BLOCK_MAX_BYTES: usize = 8_000;

/// The most the block may take for a provider sent the slim tool set
/// ([`ToolSet::Slim`]: Ollama), whose whole first request must fit in half
/// of an 8,192-token context, 12,288 B (`tests/request_size.rs`).
///
/// The six-track mix with three buses there comes to about 1.4 KB and is
/// listed in full. The block shares the request with the tool schemas:
/// what it takes past [`SLIM_SESSION_ALLOWANCE_BYTES`] comes off the tool
/// budget ([`slim_tools_budget`]), so a session of any size costs the
/// request about what a one-track one does, and costs instead the tools
/// that no longer fit, from the end of the slim set's order of priority.
/// This cap is what keeps that cost small enough for a message that names
/// nothing to still be sent the whole core, which a test in
/// `tool_selection.rs` holds. Of the huge session in `request_size.rs`
/// (two hundred tracks, fifty buses, a thousand markers) it lists eight
/// tracks, two buses and twenty-two markers, the ones the message names
/// first.
///
/// [`SLIM_SESSION_ALLOWANCE_BYTES`]: crate::tool_selection::SLIM_SESSION_ALLOWANCE_BYTES
/// [`slim_tools_budget`]: crate::tool_selection::slim_tools_budget
pub const SLIM_SESSION_BLOCK_MAX_BYTES: usize = 2_300;

/// The block's budget for a provider sent `tool_set`.
pub fn block_budget(tool_set: ToolSet) -> usize {
    match tool_set {
        ToolSet::Full => SESSION_BLOCK_MAX_BYTES,
        ToolSet::Slim => SLIM_SESSION_BLOCK_MAX_BYTES,
    }
}

/// [`render_block_within`] at [`SESSION_BLOCK_MAX_BYTES`], for a message
/// that names nothing.
pub fn render_block(ctx: &SessionContext) -> String {
    render_block_within(ctx, SESSION_BLOCK_MAX_BYTES, "")
}

/// Render the context as a deterministic block to splice into the system
/// prompt, in at most `max_bytes` bytes of text. Returns an empty string
/// when the context is empty so callers can splice unconditionally.
///
/// A session that fits is listed in full, exactly as it always was. One
/// that does not keeps every fixed part (the head and length, the master
/// chain, the selection, the headers) and cuts the three lists that grow
/// with the session: tracks, buses and markers (#573). Each list is given
/// a share of the room that is left, tracks twice what buses and markers
/// are, and a list that needs less than its share passes the rest on. Within
/// a list what is shown first is
///
/// - for tracks, the ones `message` names, by name or as "track 3", then
///   the rest in index order;
/// - for buses, the ones `message` names, then the ones a listed track
///   sends to, then the ones any track sends to, then the rest;
/// - for markers, the ones `message` names, then the rest in time order.
///
/// What is shown stays in its list's own order. A list that was cut ends
/// with a line that says how many it left out of how many, and what can
/// reach the rest: any index for a track, and `export_labels` for the
/// markers. No tool lists the tracks or the buses, so for those the line
/// gives the count and, for tracks, the index range a tool's `track`
/// takes.
///
/// The fixed parts are bounded (a chain lists at most
/// [`MAX_EFFECTS_LISTED`] effects, and a selection and a head are one line
/// each), and come to well under [`SLIM_SESSION_BLOCK_MAX_BYTES`]; a test
/// holds the worst case there. Below what they take the block is longer
/// than `max_bytes`, with every list cut to its count.
pub fn render_block_within(ctx: &SessionContext, max_bytes: usize, message: &str) -> String {
    if ctx.selection.is_none()
        && ctx.markers.is_empty()
        && ctx.head.is_none()
        && ctx.tracks.is_empty()
        && ctx.buses.is_empty()
        && ctx.master_effects.is_empty()
    {
        return String::new();
    }

    let mut markers_by_time: Vec<&Annotation> = ctx.markers.iter().collect();
    markers_by_time.sort_by(|a, b| {
        time_of(a)
            .partial_cmp(&time_of(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut tracks = List::new(
        TRACKS_HEADER,
        "\n",
        tracks_left_out,
        ctx.tracks
            .iter()
            .enumerate()
            .map(|(i, t)| track_line(i, t))
            .collect(),
    );
    let mut buses = List::new(
        BUSES_HEADER,
        "\n",
        buses_left_out,
        ctx.buses.iter().map(bus_line).collect(),
    );
    let mut markers = List::new(
        MARKERS_HEADER,
        "",
        markers_left_out,
        markers_by_time.iter().map(|a| marker_line(a)).collect(),
    );

    let session = session_part(ctx);
    let master = master_part(ctx);
    let selection = selection_part(ctx);
    let fixed = session.len()
        + master.len()
        + selection.len()
        + tracks.frame()
        + buses.frame()
        + markers.frame();

    if fixed + tracks.need() + buses.need() + markers.need() <= max_bytes {
        tracks.show_all();
        buses.show_all();
        markers.show_all();
    } else {
        let mentions = Mentions::of(message);
        let track_order = priority(ctx.tracks.len(), |i| {
            usize::from(!mentions.names_track(i, &ctx.tracks[i].name))
        });
        let marker_order = priority(markers_by_time.len(), |i| {
            usize::from(!mentions.names(&markers_by_time[i].name))
        });

        // Room for the line that says what a list left out is set aside
        // first, at its longest, so the block cannot pass `max_bytes` for
        // having to say so.
        let reserved = tracks.reserve() + buses.reserve() + markers.reserve();
        let room = max_bytes.saturating_sub(fixed + reserved);
        let [for_tracks, for_buses, for_markers] = shares(
            [tracks.need(), buses.need(), markers.need()],
            [2, 1, 1],
            room,
        );

        let mut spare = tracks.fill(&track_order, for_tracks);
        // Which buses come first depends on which tracks are listed.
        let bus_order = priority(ctx.buses.len(), |i| {
            let bus = &ctx.buses[i];
            if mentions.names(&bus.name) {
                0
            } else if bus.sends.iter().any(|s| tracks.is_shown(s.track)) {
                1
            } else if !bus.sends.is_empty() {
                2
            } else {
                3
            }
        });
        spare += buses.fill(&bus_order, for_buses);
        spare += markers.fill(&marker_order, for_markers);

        // A list shown in full has nothing to say it left out, and what a
        // list did not use goes to the others, in the same order.
        for list in [&tracks, &buses, &markers] {
            if list.hidden() == 0 {
                spare += list.reserve();
            }
        }
        spare = tracks.fill(&track_order, spare);
        spare = buses.fill(&bus_order, spare);
        markers.fill(&marker_order, spare);
    }

    let mut out = session;
    tracks.render(&mut out);
    buses.render(&mut out);
    out.push_str(&master);
    out.push_str(&selection);
    markers.render(&mut out);
    out
}

const TRACKS_HEADER: &str = "## tracks\nThe index is what a tool's `track` argument takes. Times are seconds on the session timeline.\n";

const BUSES_HEADER: &str = "## buses\nTracks send a parallel copy to a bus; `set_send` and `remove_send` take its id. The effect tools edit its chain with scope \"bus\" and `bus` (name or id); `effect_index` counts from 0 in the order listed.\n";

const MARKERS_HEADER: &str = "## markers\n";

/// What a cut track list says it left out. Every index still works: a
/// tool's `track` reaches a track whether or not it is listed.
fn tracks_left_out(more: usize, total: usize) -> String {
    format!(
        "…and {more} more tracks ({total} in all), not listed here. A tool's `track` takes any index up to {}.\n",
        total.saturating_sub(1)
    )
}

fn buses_left_out(more: usize, total: usize) -> String {
    format!("…and {more} more buses ({total} in all), not listed here.\n")
}

/// `export_labels` reads only, takes nothing, and returns every marker
/// and region.
fn markers_left_out(more: usize, total: usize) -> String {
    format!(
        "…and {more} more markers ({total} in all), not listed here; `export_labels` lists every one.\n"
    )
}

/// One list of the block: its header, one line per item in the list's own
/// order, and which of them are shown.
struct List {
    header: &'static str,
    /// What ends the section: a blank line, or nothing for the last one.
    trailer: &'static str,
    left_out: fn(usize, usize) -> String,
    lines: Vec<String>,
    shown: Vec<bool>,
}

impl List {
    fn new(
        header: &'static str,
        trailer: &'static str,
        left_out: fn(usize, usize) -> String,
        lines: Vec<String>,
    ) -> Self {
        let shown = vec![false; lines.len()];
        Self {
            header,
            trailer,
            left_out,
            lines,
            shown,
        }
    }

    /// What the section costs before any item: nothing when it has none,
    /// because then it is not written at all.
    fn frame(&self) -> usize {
        if self.lines.is_empty() {
            0
        } else {
            self.header.len() + self.trailer.len()
        }
    }

    /// What every item would cost.
    fn need(&self) -> usize {
        self.lines.iter().map(String::len).sum()
    }

    /// The longest the line that says what was left out can be.
    fn reserve(&self) -> usize {
        if self.lines.is_empty() {
            0
        } else {
            (self.left_out)(self.lines.len(), self.lines.len()).len()
        }
    }

    fn hidden(&self) -> usize {
        self.shown.iter().filter(|shown| !**shown).count()
    }

    fn is_shown(&self, i: usize) -> bool {
        self.shown.get(i).copied().unwrap_or(false)
    }

    fn show_all(&mut self) {
        self.shown.fill(true);
    }

    /// Show the items of `order`, in that order, while they fit in `room`,
    /// and return what is left of it. An item too long for what is left is
    /// skipped, not the end of the list: a shorter one after it may fit.
    fn fill(&mut self, order: &[usize], mut room: usize) -> usize {
        for &i in order {
            let len = self.lines[i].len();
            if !self.shown[i] && len <= room {
                self.shown[i] = true;
                room -= len;
            }
        }
        room
    }

    fn render(&self, out: &mut String) {
        if self.lines.is_empty() {
            return;
        }
        out.push_str(self.header);
        for (line, _) in self.lines.iter().zip(&self.shown).filter(|(_, s)| **s) {
            out.push_str(line);
        }
        let hidden = self.hidden();
        if hidden > 0 {
            out.push_str(&(self.left_out)(hidden, self.lines.len()));
        }
        out.push_str(self.trailer);
    }
}

/// The items `0..len`, by `rank` (lowest first) and then in their own
/// order.
fn priority(len: usize, rank: impl Fn(usize) -> usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    order.sort_by_key(|&i| rank(i));
    order
}

/// Split `room` between lists that `need` so much each, in proportion to
/// `weight`. A list never gets more than it needs: what it does not need
/// is shared again among the others.
fn shares<const N: usize>(need: [usize; N], weight: [usize; N], mut room: usize) -> [usize; N] {
    let mut share = [0; N];
    loop {
        let open: Vec<usize> = (0..N).filter(|&i| share[i] < need[i]).collect();
        let total: usize = open.iter().map(|&i| weight[i]).sum();
        if room == 0 || total == 0 {
            break;
        }
        let mut given = 0;
        for &i in &open {
            let part = (room * weight[i] / total).min(need[i] - share[i]);
            share[i] += part;
            given += part;
        }
        if given == 0 {
            // Less room than weights to split it by: the first open list
            // takes it.
            let i = open[0];
            let part = room.min(need[i] - share[i]);
            share[i] += part;
            given = part;
        }
        room -= given;
    }
    share
}

/// What the user's message names, for putting it first in a list that is
/// cut.
struct Mentions {
    /// The message, lowercased.
    text: String,
    /// The numbers that follow "track" or "tracks" ("track 3", "tracks 3
    /// and 4").
    track_indices: Vec<usize>,
}

impl Mentions {
    fn of(message: &str) -> Self {
        let text = message.to_lowercase();
        let words: Vec<&str> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let mut track_indices = Vec::new();
        for (at, word) in words.iter().enumerate() {
            if !matches!(*word, "track" | "tracks") {
                continue;
            }
            for next in &words[at + 1..] {
                if let Ok(index) = next.parse::<usize>() {
                    track_indices.push(index);
                } else if !matches!(*next, "and" | "or") {
                    break;
                }
            }
        }
        Self {
            text,
            track_indices,
        }
    }

    /// Whether the message has `name` in it as words of their own, in any
    /// case: "the Lead Vox" names "lead vox", "boxes" does not name "box".
    fn names(&self, name: &str) -> bool {
        let name = name.trim().to_lowercase();
        if name.is_empty() {
            return false;
        }
        self.text.match_indices(&name).any(|(at, found)| {
            let before = self.text[..at].chars().next_back();
            let after = self.text[at + found.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    }

    fn names_track(&self, index: usize, name: &str) -> bool {
        self.track_indices.contains(&index) || self.names(name)
    }
}

fn session_part(ctx: &SessionContext) -> String {
    if ctx.head.is_none() && ctx.tracks.is_empty() {
        return String::new();
    }
    let mut out = String::from("## session\n");
    if let Some(head) = &ctx.head {
        out.push_str(&format!("head node: {head}\n"));
    }
    let length = ctx.tracks.iter().map(TrackBrief::end).fold(0.0, f64::max);
    out.push_str(&format!("length: {length:.2}s\n\n"));
    out
}

fn master_part(ctx: &SessionContext) -> String {
    if ctx.master_effects.is_empty() {
        return String::new();
    }
    format!(
        "## master\nThe master bus sums every track. The effect tools edit its chain with scope \"master\" and no `track`; `effect_index` counts from 0 in this order.\neffects: {}\n\n",
        chain_text(&ctx.master_effects)
    )
}

fn selection_part(ctx: &SessionContext) -> String {
    match ctx.selection {
        Some(sel) => format!(
            "## current_selection\nstart: {:.2}s  end: {:.2}s\n\n",
            sel.start_sec, sel.end_sec
        ),
        None => String::new(),
    }
}

fn track_line(i: usize, t: &TrackBrief) -> String {
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

fn bus_line(b: &BusBrief) -> String {
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

fn marker_line(a: &Annotation) -> String {
    match &a.kind {
        AnnotationKind::Marker { time_sec } => format!("- {} @ {:.2}s\n", a.name, time_sec),
        AnnotationKind::Region { start_sec, end_sec } => {
            format!("- {} @ {:.2}s-{:.2}s\n", a.name, start_sec, end_sec)
        }
    }
}

/// A chain's effects, in order, at most [`MAX_EFFECTS_LISTED`] of them.
fn chain_text(effects: &[String]) -> String {
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

    // ---------------------------------------------------------------
    // The budget (#573)
    // ---------------------------------------------------------------

    fn marker(name: &str, time_sec: f64) -> Annotation {
        Annotation {
            id: session::AnnotationId::new(),
            name: name.into(),
            kind: AnnotationKind::Marker { time_sec },
        }
    }

    /// `n` tracks named "track <i>" unless `name` says otherwise.
    fn tracks(n: usize, name: impl Fn(usize) -> String) -> Vec<TrackBrief> {
        (0..n)
            .map(|i| {
                let mut t = brief(&name(i), &[(0.0, 184.5)]);
                t.gain_db = -2.5;
                t.effects = vec!["compressor".into(), "eq".into()];
                t
            })
            .collect()
    }

    fn bus(i: usize, sends: Vec<SendBrief>) -> BusBrief {
        BusBrief {
            name: format!("Bus {i}"),
            id: format!("{i:08x}-5b7e-4d21-9a6c-0e1d2f3a4b5c"),
            sends,
            effects: vec!["reverb".into()],
        }
    }

    /// Two hundred tracks, fifty buses with three sends each, a thousand
    /// markers, a master chain and a selection.
    fn huge() -> SessionContext {
        SessionContext {
            selection: Some(Range {
                start_sec: 1.0,
                end_sec: 2.0,
            }),
            head: Some("a".repeat(64)),
            tracks: tracks(200, |i| format!("strings {i}")),
            buses: (0..50)
                .map(|b| {
                    bus(
                        b,
                        (0..3)
                            .map(|k| {
                                let t = b * 3 + k;
                                send_brief(t, &format!("strings {t}"), -6.0)
                            })
                            .collect(),
                    )
                })
                .collect(),
            markers: (0..1_000)
                .map(|m| marker(&format!("cue {m}"), m as f64 * 0.18))
                .collect(),
            master_effects: vec!["limiter".into()],
        }
    }

    /// The lines of `## <name>`, up to the blank line that ends it.
    fn section<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
        let heading = format!("## {name}");
        text.lines()
            .skip_while(|l| *l != heading)
            .skip(1)
            .take_while(|l| !l.is_empty())
            .collect()
    }

    fn listed<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
        section(text, name)
            .into_iter()
            .filter(|l| l.starts_with("- "))
            .collect()
    }

    fn left_out_line<'a>(text: &'a str, name: &str) -> Option<&'a str> {
        section(text, name)
            .into_iter()
            .find(|l| l.starts_with("…and "))
    }

    /// A session that fits is written exactly as it was before there was a
    /// budget: the cap changes nothing until it has to.
    #[test]
    fn a_session_that_fits_is_written_as_it_always_was() {
        let ctx = SessionContext {
            selection: Some(Range {
                start_sec: 1.0,
                end_sec: 2.0,
            }),
            head: Some("ab12".into()),
            tracks: tracks(6, |i| format!("t{i}")),
            buses: vec![bus(0, vec![send_brief(0, "t0", -6.0)]), bus(1, vec![])],
            markers: vec![marker("drop", 78.5), marker("chorus", 42.0)],
            master_effects: vec!["limiter".into()],
        };
        let unbounded = render_block_within(&ctx, usize::MAX, "");

        for budget in [SLIM_SESSION_BLOCK_MAX_BYTES, SESSION_BLOCK_MAX_BYTES] {
            assert_eq!(
                render_block_within(&ctx, budget, "make t3 louder"),
                unbounded
            );
        }
        assert_eq!(render_block(&ctx), unbounded);
        assert!(!unbounded.contains("…and "), "{unbounded}");
        assert!(
            unbounded.len() <= SLIM_SESSION_BLOCK_MAX_BYTES,
            "{}",
            unbounded.len()
        );
    }

    /// The budget holds for every provider, and each list that was cut says
    /// how many of how many it left out.
    #[test]
    fn a_huge_session_is_cut_to_each_budget_and_says_what_it_left_out() {
        let ctx = huge();
        for budget in [SLIM_SESSION_BLOCK_MAX_BYTES, SESSION_BLOCK_MAX_BYTES] {
            let text = render_block_within(&ctx, budget, "");
            assert!(text.len() <= budget, "{} B over {budget} B", text.len());

            // Every fixed part is still there.
            assert!(text.contains("head node: "), "{text}");
            assert!(text.contains("length: 184.50s"), "{text}");
            assert!(text.contains("## master\n"), "{text}");
            assert!(text.contains("## current_selection\n"), "{text}");

            for (name, total) in [("tracks", 200), ("buses", 50), ("markers", 1_000)] {
                let shown = listed(&text, name).len();
                assert!(shown > 0, "{budget} B: nothing listed under ## {name}");
                let line = left_out_line(&text, name)
                    .unwrap_or_else(|| panic!("{budget} B: ## {name} says nothing was left out"));
                let expected = format!("…and {} more {name} ({total} in all)", total - shown);
                assert!(line.starts_with(&expected), "{line:?} is not {expected:?}");
            }
        }
    }

    /// The lines themselves, word for word: what each list says it left
    /// out, and what reaches the rest.
    #[test]
    fn the_left_out_lines_name_the_count_and_what_reaches_the_rest() {
        // A budget with room for the header and the line, and no item.
        let only_tracks = SessionContext {
            tracks: tracks(30, |i| format!("t{i}")),
            ..Default::default()
        };
        let tiny = session_part(&only_tracks).len() + TRACKS_HEADER.len() + 1 + 120;
        assert_eq!(
            left_out_line(&render_block_within(&only_tracks, tiny, ""), "tracks"),
            Some("…and 30 more tracks (30 in all), not listed here. A tool's `track` takes any index up to 29.")
        );

        let only_markers = SessionContext {
            markers: (0..40)
                .map(|m| marker(&format!("cue {m}"), m as f64))
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            left_out_line(&render_block_within(&only_markers, 100, ""), "markers"),
            Some("…and 40 more markers (40 in all), not listed here; `export_labels` lists every one.")
        );

        let only_buses = SessionContext {
            buses: (0..12).map(|b| bus(b, vec![])).collect(),
            ..Default::default()
        };
        assert_eq!(
            left_out_line(&render_block_within(&only_buses, 300, ""), "buses"),
            Some("…and 12 more buses (12 in all), not listed here.")
        );
    }

    /// The tool the markers line sends the model to lists every marker and
    /// region, reads only, and takes nothing, so the call cannot go wrong.
    #[test]
    fn the_tool_the_markers_line_names_lists_them_and_reads_only() {
        let dispatcher = tools::ToolDispatcher::default_dispatcher();
        let tool = dispatcher
            .get("export_labels")
            .expect("export_labels is a registered tool");
        assert!(!tool.mutates(), "export_labels must be read-only");
        let schema = tool.schema();
        assert_eq!(
            schema["input_schema"]["required"],
            serde_json::json!([]),
            "{schema}"
        );
        assert!(markers_left_out(1, 2).contains("`export_labels`"));
    }

    /// Whatever the session holds, the block stays within the slim budget:
    /// names far longer than a line's share, chains and send lists longer
    /// than anyone builds, and the fixed parts at their longest.
    #[test]
    fn no_session_takes_the_block_past_the_slim_budget() {
        let long = "x".repeat(3_000);
        let chain: Vec<String> = (0..100)
            .map(|_| "stereo_widener (bypassed)".into())
            .collect();
        let ctx = SessionContext {
            selection: Some(Range {
                start_sec: 123_456.789,
                end_sec: 234_567.891,
            }),
            head: Some("f".repeat(64)),
            tracks: (0..300)
                .map(|i| {
                    let mut t = brief(
                        &if i % 2 == 0 {
                            long.clone()
                        } else {
                            format!("t{i}")
                        },
                        &(0..40)
                            .map(|c| (c as f64, c as f64 + 0.5))
                            .collect::<Vec<_>>(),
                    );
                    t.muted = true;
                    t.soloed = true;
                    t.effects = chain.clone();
                    t
                })
                .collect(),
            buses: (0..100)
                .map(|_| BusBrief {
                    name: long.clone(),
                    id: long.clone(),
                    sends: (0..200).map(|t| send_brief(t, &long, -6.0)).collect(),
                    effects: chain.clone(),
                })
                .collect(),
            markers: (0..2_000)
                .map(|m| marker(if m % 3 == 0 { &long } else { "cue" }, m as f64))
                .collect(),
            master_effects: chain.clone(),
        };

        let text = render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, &long);

        assert!(
            text.len() <= SLIM_SESSION_BLOCK_MAX_BYTES,
            "{} B, over {SLIM_SESSION_BLOCK_MAX_BYTES} B",
            text.len()
        );
        // The chain is summarised, not dropped: its count is said.
        assert!(text.contains(", and 84 more\n\n"), "{text}");
        for name in ["tracks", "buses", "markers"] {
            assert!(left_out_line(&text, name).is_some(), "## {name}: {text}");
        }
    }

    /// A line too long for what is left is passed over, not the end of the
    /// list: the shorter ones after it are still shown.
    #[test]
    fn an_item_too_long_for_its_share_is_skipped_and_counted() {
        let mut ctx = SessionContext {
            tracks: tracks(5, |i| format!("t{i}")),
            ..Default::default()
        };
        ctx.tracks[0].name = "x".repeat(5_000);

        let text = render_block_within(&ctx, 1_000, "");

        assert_eq!(listed(&text, "tracks").len(), 4, "{text}");
        assert!(text.contains("- 1 \"t1\""), "{text}");
        assert!(
            left_out_line(&text, "tracks")
                .is_some_and(|l| l.starts_with("…and 1 more tracks (5 in all)")),
            "{text}"
        );
    }

    /// The tracks the message names, by name or by index, are listed even
    /// when they are far down the list, and in index order with the rest.
    #[test]
    fn the_tracks_the_message_names_are_listed_first() {
        let mut ctx = SessionContext {
            head: Some("a".repeat(64)),
            tracks: tracks(200, |i| format!("strings {i}")),
            ..Default::default()
        };
        ctx.tracks[137].name = "Lead Vox".into();

        let text = render_block_within(
            &ctx,
            SLIM_SESSION_BLOCK_MAX_BYTES,
            "make the lead vox louder and pan track 150 left",
        );
        let lines = listed(&text, "tracks");

        let at = |prefix: &str| lines.iter().position(|l| l.starts_with(prefix));
        let vox = at("- 137 \"Lead Vox\"").unwrap_or_else(|| panic!("{text}"));
        let by_index = at("- 150 ").unwrap_or_else(|| panic!("{text}"));
        assert!(at("- 0 ").unwrap() < vox && vox < by_index, "{lines:#?}");
        assert_eq!(by_index, lines.len() - 1, "{lines:#?}");
        // Without the message they are not reached.
        assert!(!render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, "").contains("Lead Vox"));
    }

    /// A bus a listed track sends to is listed before one only an unlisted
    /// track feeds, and a bus the message names before either.
    #[test]
    fn the_buses_listed_tracks_send_to_come_first() {
        let ctx = SessionContext {
            tracks: tracks(100, |i| format!("t{i}")),
            buses: (0..40)
                .map(|b| match b {
                    // Fed by track 0, which is listed.
                    33 => bus(b, vec![send_brief(0, "t0", -6.0)]),
                    // Fed by track 99, which is not.
                    34 => bus(b, vec![send_brief(99, "t99", -6.0)]),
                    _ => bus(b, vec![]),
                })
                .collect(),
            ..Default::default()
        };

        let text = render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, "");
        let lines = listed(&text, "buses");
        let has = |lines: &[&str], name: &str| {
            lines
                .iter()
                .any(|l| l.starts_with(&format!("- \"{name}\"")))
        };
        assert!(
            lines.len() < 40,
            "the list must be cut, or this proves nothing"
        );
        assert!(has(&lines, "Bus 33") && has(&lines, "Bus 34"), "{lines:#?}");
        assert!(!has(&lines, "Bus 39"), "{lines:#?}");
        // Shown in the session's order, whatever put them on the list.
        assert!(
            lines.last().unwrap().starts_with("- \"Bus 34\""),
            "{lines:#?}"
        );

        let named = render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, "add eq to bus 39");
        let lines = listed(&named, "buses");
        assert!(has(&lines, "Bus 39") && has(&lines, "Bus 33"), "{lines:#?}");

        // Every bus is fed, all but the last by a track that is not listed:
        // the last, fed by a listed one, goes ahead of all of them.
        let ctx = SessionContext {
            buses: (0..40)
                .map(|b| {
                    let from = if b == 39 { 0 } else { 99 };
                    bus(b, vec![send_brief(from, &format!("t{from}"), -6.0)])
                })
                .collect(),
            ..ctx
        };
        let text = render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, "");
        let lines = listed(&text, "buses");
        assert!(has(&lines, "Bus 39"), "{lines:#?}");
        assert!(!has(&lines, "Bus 38"), "{lines:#?}");
    }

    /// A marker the message names is listed, among the rest in time order.
    #[test]
    fn the_markers_the_message_names_are_listed_first() {
        let mut markers: Vec<Annotation> = (0..500)
            .map(|m| marker(&format!("cue {m}"), m as f64))
            .collect();
        markers.push(marker("big drop", 900.0));
        let ctx = SessionContext {
            markers,
            ..Default::default()
        };

        let text = render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, "cut at the Big Drop");
        let lines = listed(&text, "markers");
        assert_eq!(lines.last(), Some(&"- big drop @ 900.00s"), "{lines:#?}");
        assert_eq!(lines[0], "- cue 0 @ 0.00s");
        assert!(!render_block_within(&ctx, SLIM_SESSION_BLOCK_MAX_BYTES, "").contains("big drop"));
    }

    #[test]
    fn a_message_names_whole_words_in_any_case_and_track_numbers() {
        let m = Mentions::of("Make the Lead Vox louder, then tracks 3 and 12, and Track #7.");
        assert!(m.names("lead vox"));
        assert!(m.names("LEAD VOX"));
        assert!(!m.names("vo"), "a part of a word is not a name");
        assert!(!m.names("ox"));
        assert!(!m.names(""));
        assert!(!m.names("   "));
        for i in [3, 12, 7] {
            assert!(m.names_track(i, "nothing said"), "track {i}");
        }
        assert!(!m.names_track(4, "nothing said"));
    }

    /// The room is split two to one to one, and a list that needs less than
    /// its share passes the rest on; nothing is given that is not there.
    #[test]
    fn the_room_is_shared_by_weight_and_passed_on() {
        assert_eq!(
            shares([1_000, 1_000, 1_000], [2, 1, 1], 1_000),
            [500, 250, 250]
        );
        assert_eq!(
            shares([100, 1_000, 1_000], [2, 1, 1], 1_000),
            [100, 450, 450]
        );
        assert_eq!(shares([0, 0, 5_000], [2, 1, 1], 1_000), [0, 0, 1_000]);
        assert_eq!(shares([10, 10, 10], [2, 1, 1], 1_000), [10, 10, 10]);
        let tight = shares([5, 5, 5], [2, 1, 1], 2);
        assert_eq!(tight.iter().sum::<usize>(), 2, "{tight:?}");
    }

    /// A chain and a send list longer than the cap say how many more there
    /// are, as a track's clips do.
    #[test]
    fn a_long_chain_and_a_long_send_list_are_summarised() {
        let effects: Vec<String> = (0..20).map(|i| format!("fx{i}")).collect();
        let mut t = brief("t", &[(0.0, 1.0)]);
        t.effects = effects.clone();
        let line = track_line(0, &t);
        assert!(line.contains("fx15, and 4 more\n"), "{line}");
        assert!(!line.contains("fx16"), "{line}");

        let b = bus(
            0,
            (0..12)
                .map(|i| send_brief(i, &format!("t{i}"), -6.0))
                .collect(),
        );
        let line = bus_line(&b);
        assert!(line.contains("7 \"t7\" (-6.0 dB), and 4 more;"), "{line}");
        assert!(!line.contains("\"t8\""), "{line}");
    }

    /// What each provider gets.
    #[test]
    fn the_budget_follows_the_tool_set() {
        assert_eq!(block_budget(ToolSet::Full), SESSION_BLOCK_MAX_BYTES);
        assert_eq!(block_budget(ToolSet::Slim), SLIM_SESSION_BLOCK_MAX_BYTES);
    }
}
