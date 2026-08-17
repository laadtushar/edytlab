//! Branching primitives for the session DAG (M24).
//!
//! [`SessionDiff`] represents the structural delta between two
//! [`SessionState`]s. Each [`DiffOp`] names a single targeted location
//! (track id, effect index, master-chain index, etc.) so [`merge`] can
//! detect conflicts as "both branches mutated the same target".
//!
//! ## Design notes
//!
//! Tracks are identified by their stable [`TrackId`] (a UUID), not by
//! position in `SessionState::tracks`. This matters because a fork can
//! re-order tracks (e.g. `remove_track` then `add_track`); using indices
//! would generate spurious "modify" ops where there was really only an
//! identity-preserving move. The exception is [`DiffOp::TrackOrder`],
//! which captures a pure reorder with no field changes.
//!
//! Effects don't have stable ids in the schema, so for chains attached
//! to a track we identify each effect by `(track_id, effect_index)` and
//! conflict on identical pairs. Same for `(bus_id, effect_index)` and
//! `(MasterChain, effect_index)`.
//!
//! Clips also lack ids; the diff treats `Track::clips` as a unit
//! (`ClipsChange { track_id, clips }`). Two branches that both mutate
//! the clip list of the same track therefore conflict, which matches
//! the user's mental model — two parallel "rearrange the clips" passes
//! cannot be auto-merged.
//!
//! ## Determinism
//!
//! Op ordering is deterministic: tracks are sorted by `TrackId` (UUID
//! bytes), effects within a track by their index, top-level scalar ops
//! in a fixed enum-discriminant order. Two `diff(a, b)` calls always
//! produce byte-identical JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::node::{NodeId, SessionNode};
use crate::state::{
    Bus, BusGraph, Clip, EffectInstance, KeyMap, SessionState, TempoMap, Track, TrackId, Transcript,
};
use crate::store::Store;
use crate::{Error, Result};

/// Identifies which scope an effect change applies to. For
/// `TrackEffect`, the track is named by its [`TrackId`] so the op
/// remains stable across reorders.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum EffectScope {
    /// `SessionState::master_chain[index]`.
    Master,
    /// `SessionState::tracks[track_with_id].effects[index]`.
    Track { track_id: TrackId },
    /// `SessionState::bus_routing.buses[bus_with_id].effects[index]`.
    Bus { bus_id: Uuid },
}

/// A single delta operation. Each op carries the new value (for
/// merge/apply); the [`SessionDiff`] separately records whether this op
/// was an addition, removal, or modification relative to a baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DiffOp {
    /// A whole track was added or removed (whichever side it appears on
    /// inside `SessionDiff::added`/`removed`). Position is recorded for
    /// apply but not used for conflict identity (track id is enough).
    Track { position: usize, track: Box<Track> },
    /// Reorder tracks by `TrackId`. Conflicts on any other op that
    /// references a moved track.
    TrackOrder { order: Vec<TrackId> },
    /// Set `Track::name` for the named track.
    TrackName { track_id: TrackId, value: String },
    /// Set `Track::gain_db`.
    TrackGain { track_id: TrackId, value: f32 },
    /// Set `Track::pan`.
    TrackPan { track_id: TrackId, value: f32 },
    /// Set `Track::muted`.
    TrackMuted { track_id: TrackId, value: bool },
    /// Set `Track::soloed`.
    TrackSoloed { track_id: TrackId, value: bool },
    /// Replace the entire `Track::clips` vec. Two diffs that both touch
    /// the same `track_id`'s clips conflict.
    ClipsChange { track_id: TrackId, clips: Vec<Clip> },
    /// A single effect was added/removed/modified at `(scope, index)`.
    /// `effect` is the new value for `added`/`modified`; for `removed`
    /// it carries the *old* value (so apply can reconstruct).
    Effect {
        scope: EffectScope,
        index: usize,
        effect: EffectInstance,
    },
    /// Replace `BusGraph::buses` membership/order. Per-bus effect
    /// changes go via [`DiffOp::Effect`] with `EffectScope::Bus`.
    BusList { buses: Vec<BusMeta> },
    /// Replace the tempo map wholesale.
    TempoMap { value: TempoMap },
    /// Replace the optional key map wholesale.
    KeyMap { value: Option<KeyMap> },
    /// Replace the optional transcript wholesale.
    Transcript { value: Option<Transcript> },
    /// Replace `SessionState::sample_rate`.
    SampleRate { value: u32 },
    /// Replace `SessionState::length_samples`.
    LengthSamples { value: u64 },
}

/// Bus metadata without its effect list — effects flow through
/// [`DiffOp::Effect`] with [`EffectScope::Bus`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusMeta {
    pub id: Uuid,
    pub name: String,
}

/// A target key used for conflict detection in [`merge`]. Two ops with
/// identical targets are considered to touch the same logical
/// location; if both sides of a merge contain such pairs the merge
/// fails with [`Error::MergeConflict`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiffTarget {
    Track(TrackId),
    TrackOrder,
    TrackName(TrackId),
    TrackGain(TrackId),
    TrackPan(TrackId),
    TrackMuted(TrackId),
    TrackSoloed(TrackId),
    Clips(TrackId),
    Effect(EffectScope, usize),
    BusList,
    TempoMap,
    KeyMap,
    Transcript,
    SampleRate,
    LengthSamples,
}

impl DiffOp {
    /// Conflict target for [`merge`].
    pub fn target(&self) -> DiffTarget {
        match self {
            DiffOp::Track { track, .. } => DiffTarget::Track(track.id.clone()),
            DiffOp::TrackOrder { .. } => DiffTarget::TrackOrder,
            DiffOp::TrackName { track_id, .. } => DiffTarget::TrackName(track_id.clone()),
            DiffOp::TrackGain { track_id, .. } => DiffTarget::TrackGain(track_id.clone()),
            DiffOp::TrackPan { track_id, .. } => DiffTarget::TrackPan(track_id.clone()),
            DiffOp::TrackMuted { track_id, .. } => DiffTarget::TrackMuted(track_id.clone()),
            DiffOp::TrackSoloed { track_id, .. } => DiffTarget::TrackSoloed(track_id.clone()),
            DiffOp::ClipsChange { track_id, .. } => DiffTarget::Clips(track_id.clone()),
            DiffOp::Effect { scope, index, .. } => DiffTarget::Effect(scope.clone(), *index),
            DiffOp::BusList { .. } => DiffTarget::BusList,
            DiffOp::TempoMap { .. } => DiffTarget::TempoMap,
            DiffOp::KeyMap { .. } => DiffTarget::KeyMap,
            DiffOp::Transcript { .. } => DiffTarget::Transcript,
            DiffOp::SampleRate { .. } => DiffTarget::SampleRate,
            DiffOp::LengthSamples { .. } => DiffTarget::LengthSamples,
        }
    }
}

/// Structural delta between two [`SessionState`]s.
///
/// `added` are ops describing things present in `b` but not `a`;
/// `removed` describe things present in `a` but not `b` (the op carries
/// the *old* value from `a` so the diff is invertible). `modified` is a
/// pair `(from_a, to_b)` for things present in both but with different
/// content.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionDiff {
    pub added: Vec<DiffOp>,
    pub removed: Vec<DiffOp>,
    pub modified: Vec<(DiffOp, DiffOp)>,
}

impl SessionDiff {
    /// Reverse the diff: `swap(diff(a, b)) == diff(b, a)`.
    pub fn swapped(self) -> Self {
        Self {
            added: self.removed,
            removed: self.added,
            modified: self
                .modified
                .into_iter()
                .map(|(from, to)| (to, from))
                .collect(),
        }
    }

    /// All ops in a single iterator, useful for callers that don't care
    /// about the added/removed/modified split (e.g. apply, conflict
    /// detection — which both treat the diff as a flat list of "things
    /// to do to a base state to reach the target").
    pub fn ops_b_side(&self) -> Vec<&DiffOp> {
        let mut out: Vec<&DiffOp> = self.added.iter().collect();
        for (_, to) in &self.modified {
            out.push(to);
        }
        // Removals on the b-side mean "this thing was deleted in b".
        for op in &self.removed {
            out.push(op);
        }
        out
    }

    /// Iterate every conflict target this diff touches. Used by
    /// [`merge`] for conflict detection.
    pub fn targets(&self) -> Vec<DiffTarget> {
        let mut out: Vec<DiffTarget> = Vec::new();
        for op in &self.added {
            out.push(op.target());
        }
        for op in &self.removed {
            out.push(op.target());
        }
        for (from, _to) in &self.modified {
            out.push(from.target());
        }
        out
    }

    /// Apply the diff in-place to `base`, producing the target state.
    /// `apply(diff(a, b), &mut a) -> a == b` is the round-trip
    /// invariant.
    ///
    /// The order of application matters: structural changes (track
    /// add/remove, track order, bus list) come first, then field
    /// changes by track, then top-level scalars. Within each category
    /// ops are sorted to keep ordering deterministic.
    pub fn apply(&self, base: &mut SessionState) -> Result<()> {
        // Collect every op into a single flat vec: removed (act as
        // deletions on base), modified (apply b-side), added.
        let mut ops: Vec<DiffOp> = Vec::new();
        // Removals: for tracks/effects/buses, removal means delete.
        // For scalars (key_map, transcript) the removal is "set to
        // None". Each case handled below.
        for op in &self.removed {
            ops.push(op.clone());
        }
        for (_, to) in &self.modified {
            ops.push(to.clone());
        }
        for op in &self.added {
            ops.push(op.clone());
        }

        // We split into phases for predictable behaviour. Phase A:
        // delete tracks (those that appear in `removed` as DiffOp::Track).
        let mut tracks_to_delete: Vec<TrackId> = Vec::new();
        for op in &self.removed {
            if let DiffOp::Track { track, .. } = op {
                tracks_to_delete.push(track.id.clone());
            }
        }
        base.tracks.retain(|t| !tracks_to_delete.contains(&t.id));

        // Phase B: delete effects scheduled for removal. We process by
        // descending index per scope so earlier deletions don't shift
        // later targets.
        let mut effect_removes: Vec<(EffectScope, Vec<usize>)> = Vec::new();
        for op in &self.removed {
            if let DiffOp::Effect { scope, index, .. } = op {
                if let Some((_, idxs)) = effect_removes.iter_mut().find(|(s, _)| s == scope) {
                    idxs.push(*index);
                } else {
                    effect_removes.push((scope.clone(), vec![*index]));
                }
            }
        }
        for (scope, mut idxs) in effect_removes {
            idxs.sort_unstable();
            idxs.dedup();
            idxs.reverse();
            for i in idxs {
                remove_effect_at(base, &scope, i)?;
            }
        }

        // Phase C: track adds. Insert at the recorded position, clamped
        // to current length.
        for op in &self.added {
            if let DiffOp::Track { position, track } = op {
                let pos = (*position).min(base.tracks.len());
                base.tracks.insert(pos, (**track).clone());
            }
        }

        // Phase D: bus-list change (added + modified BusList ops). At
        // most one effective BusList op per side; pick the one from
        // `added` then `modified` then `removed`.
        if let Some(buslist) = pick_buslist(&ops) {
            let existing_effects: BTreeMap<Uuid, Vec<EffectInstance>> = base
                .bus_routing
                .buses
                .iter()
                .map(|b| (b.id, b.effects.clone()))
                .collect();
            base.bus_routing = BusGraph {
                buses: buslist
                    .iter()
                    .map(|m| Bus {
                        id: m.id,
                        name: m.name.clone(),
                        effects: existing_effects.get(&m.id).cloned().unwrap_or_default(),
                    })
                    .collect(),
            };
        }

        // Phase E: per-track field changes (added + modified). Apply
        // every non-structural op now.
        for op in self
            .added
            .iter()
            .chain(self.modified.iter().map(|(_, t)| t))
        {
            apply_op(base, op)?;
        }

        // Phase F: TrackOrder if present in added/modified — applies
        // last so it also reorders any tracks added in phase C.
        let mut order: Option<Vec<TrackId>> = None;
        for op in self
            .added
            .iter()
            .chain(self.modified.iter().map(|(_, t)| t))
        {
            if let DiffOp::TrackOrder { order: o } = op {
                order = Some(o.clone());
            }
        }
        if let Some(order) = order {
            reorder_tracks(base, &order);
        }

        Ok(())
    }
}

fn pick_buslist(ops: &[DiffOp]) -> Option<Vec<BusMeta>> {
    for op in ops {
        if let DiffOp::BusList { buses } = op {
            return Some(buses.clone());
        }
    }
    None
}

/// Apply a single non-structural op (everything except track
/// add/remove and effect remove, which are handled in earlier phases).
fn apply_op(state: &mut SessionState, op: &DiffOp) -> Result<()> {
    match op {
        DiffOp::Track { .. } => Ok(()),      // handled in phase C
        DiffOp::TrackOrder { .. } => Ok(()), // handled in phase F
        DiffOp::TrackName { track_id, value } => {
            if let Some(t) = find_track_mut(state, track_id) {
                t.name = value.clone();
            }
            Ok(())
        }
        DiffOp::TrackGain { track_id, value } => {
            if let Some(t) = find_track_mut(state, track_id) {
                t.gain_db = *value;
            }
            Ok(())
        }
        DiffOp::TrackPan { track_id, value } => {
            if let Some(t) = find_track_mut(state, track_id) {
                t.pan = *value;
            }
            Ok(())
        }
        DiffOp::TrackMuted { track_id, value } => {
            if let Some(t) = find_track_mut(state, track_id) {
                t.muted = *value;
            }
            Ok(())
        }
        DiffOp::TrackSoloed { track_id, value } => {
            if let Some(t) = find_track_mut(state, track_id) {
                t.soloed = *value;
            }
            Ok(())
        }
        DiffOp::ClipsChange { track_id, clips } => {
            if let Some(t) = find_track_mut(state, track_id) {
                t.clips = clips.clone();
            }
            Ok(())
        }
        DiffOp::Effect {
            scope,
            index,
            effect,
        } => set_effect_at(state, scope, *index, effect.clone()),
        DiffOp::BusList { .. } => Ok(()), // handled in phase D
        DiffOp::TempoMap { value } => {
            state.tempo_map = value.clone();
            Ok(())
        }
        DiffOp::KeyMap { value } => {
            state.key_map = value.clone();
            Ok(())
        }
        DiffOp::Transcript { value } => {
            state.transcript = value.clone();
            Ok(())
        }
        DiffOp::SampleRate { value } => {
            state.sample_rate = *value;
            Ok(())
        }
        DiffOp::LengthSamples { value } => {
            state.length_samples = *value;
            Ok(())
        }
    }
}

fn find_track_mut<'a>(state: &'a mut SessionState, id: &TrackId) -> Option<&'a mut Track> {
    state.tracks.iter_mut().find(|t| &t.id == id)
}

fn find_bus_mut(state: &mut SessionState, id: Uuid) -> Option<&mut Bus> {
    state.bus_routing.buses.iter_mut().find(|b| b.id == id)
}

fn set_effect_at(
    state: &mut SessionState,
    scope: &EffectScope,
    index: usize,
    effect: EffectInstance,
) -> Result<()> {
    let chain: &mut Vec<EffectInstance> = match scope {
        EffectScope::Master => &mut state.master_chain,
        EffectScope::Track { track_id } => match find_track_mut(state, track_id) {
            Some(t) => &mut t.effects,
            None => return Ok(()), // track was removed; drop the op
        },
        EffectScope::Bus { bus_id } => match find_bus_mut(state, *bus_id) {
            Some(b) => &mut b.effects,
            None => return Ok(()),
        },
    };
    if index < chain.len() {
        chain[index] = effect;
    } else if index == chain.len() {
        chain.push(effect);
    } else {
        // Pad with the new effect repeated at the gap if needed; in
        // practice diff never produces sparse indices because we
        // generate them from the source state.
        while chain.len() < index {
            chain.push(effect.clone());
        }
        chain.push(effect);
    }
    Ok(())
}

fn remove_effect_at(state: &mut SessionState, scope: &EffectScope, index: usize) -> Result<()> {
    let chain: &mut Vec<EffectInstance> = match scope {
        EffectScope::Master => &mut state.master_chain,
        EffectScope::Track { track_id } => match find_track_mut(state, track_id) {
            Some(t) => &mut t.effects,
            None => return Ok(()),
        },
        EffectScope::Bus { bus_id } => match find_bus_mut(state, *bus_id) {
            Some(b) => &mut b.effects,
            None => return Ok(()),
        },
    };
    if index < chain.len() {
        chain.remove(index);
    }
    Ok(())
}

fn reorder_tracks(state: &mut SessionState, order: &[TrackId]) {
    let mut by_id: BTreeMap<Vec<u8>, Track> = state
        .tracks
        .drain(..)
        .map(|t| (t.id.0.as_bytes().to_vec(), t))
        .collect();
    let mut new_tracks: Vec<Track> = Vec::with_capacity(order.len());
    for id in order {
        if let Some(t) = by_id.remove(id.0.as_bytes().as_slice()) {
            new_tracks.push(t);
        }
    }
    // Anything not in the requested order goes at the end (sorted by
    // id for determinism).
    let mut leftovers: Vec<Track> = by_id.into_values().collect();
    leftovers.sort_by(|a, b| a.id.0.as_bytes().cmp(b.id.0.as_bytes()));
    new_tracks.extend(leftovers);
    state.tracks = new_tracks;
}

// =============================================================================
// Diff construction
// =============================================================================

/// Compute the structural delta `b - a`.
pub fn diff_states(a: &SessionState, b: &SessionState) -> SessionDiff {
    let mut diff = SessionDiff::default();

    // ---- Tracks ----
    let a_by_id: BTreeMap<Vec<u8>, (usize, &Track)> = a
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.0.as_bytes().to_vec(), (i, t)))
        .collect();
    let b_by_id: BTreeMap<Vec<u8>, (usize, &Track)> = b
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.0.as_bytes().to_vec(), (i, t)))
        .collect();

    // Removed tracks: present in a, absent in b.
    for (id, (pos, track)) in &a_by_id {
        if !b_by_id.contains_key(id) {
            diff.removed.push(DiffOp::Track {
                position: *pos,
                track: Box::new((*track).clone()),
            });
        }
    }
    // Added tracks: absent in a, present in b.
    for (id, (pos, track)) in &b_by_id {
        if !a_by_id.contains_key(id) {
            diff.added.push(DiffOp::Track {
                position: *pos,
                track: Box::new((*track).clone()),
            });
        }
    }

    // Order change among the common subset.
    let common_a: Vec<&TrackId> = a
        .tracks
        .iter()
        .filter(|t| b_by_id.contains_key(t.id.0.as_bytes().as_slice()))
        .map(|t| &t.id)
        .collect();
    let common_b: Vec<&TrackId> = b
        .tracks
        .iter()
        .filter(|t| a_by_id.contains_key(t.id.0.as_bytes().as_slice()))
        .map(|t| &t.id)
        .collect();
    if common_a != common_b {
        let order: Vec<TrackId> = b.tracks.iter().map(|t| t.id.clone()).collect();
        let from_order: Vec<TrackId> = a.tracks.iter().map(|t| t.id.clone()).collect();
        diff.modified.push((
            DiffOp::TrackOrder { order: from_order },
            DiffOp::TrackOrder { order },
        ));
    }

    // Per-track field diffs (only for tracks present in BOTH).
    let mut common_ids: Vec<&TrackId> = a
        .tracks
        .iter()
        .filter_map(|t| {
            if b_by_id.contains_key(t.id.0.as_bytes().as_slice()) {
                Some(&t.id)
            } else {
                None
            }
        })
        .collect();
    common_ids.sort_by(|x, y| x.0.as_bytes().cmp(y.0.as_bytes()));
    for tid in common_ids {
        let ta = a_by_id[tid.0.as_bytes().as_slice()].1;
        let tb = b_by_id[tid.0.as_bytes().as_slice()].1;
        track_field_diff(tid, ta, tb, &mut diff);
        // Track-effect chain.
        let scope = EffectScope::Track {
            track_id: tid.clone(),
        };
        effect_chain_diff(&scope, &ta.effects, &tb.effects, &mut diff);
    }

    // ---- Master chain ----
    effect_chain_diff(
        &EffectScope::Master,
        &a.master_chain,
        &b.master_chain,
        &mut diff,
    );

    // ---- Bus graph ----
    bus_graph_diff(&a.bus_routing, &b.bus_routing, &mut diff);

    // ---- Top-level scalars ----
    if a.tempo_map_canonical() != b.tempo_map_canonical() {
        diff.modified.push((
            DiffOp::TempoMap {
                value: a.tempo_map.clone(),
            },
            DiffOp::TempoMap {
                value: b.tempo_map.clone(),
            },
        ));
    }
    if a.key_map_canonical() != b.key_map_canonical() {
        diff.modified.push((
            DiffOp::KeyMap {
                value: a.key_map.clone(),
            },
            DiffOp::KeyMap {
                value: b.key_map.clone(),
            },
        ));
    }
    if a.transcript_canonical() != b.transcript_canonical() {
        diff.modified.push((
            DiffOp::Transcript {
                value: a.transcript.clone(),
            },
            DiffOp::Transcript {
                value: b.transcript.clone(),
            },
        ));
    }
    if a.sample_rate != b.sample_rate {
        diff.modified.push((
            DiffOp::SampleRate {
                value: a.sample_rate,
            },
            DiffOp::SampleRate {
                value: b.sample_rate,
            },
        ));
    }
    if a.length_samples != b.length_samples {
        diff.modified.push((
            DiffOp::LengthSamples {
                value: a.length_samples,
            },
            DiffOp::LengthSamples {
                value: b.length_samples,
            },
        ));
    }

    sort_diff_for_determinism(&mut diff);
    diff
}

fn track_field_diff(tid: &TrackId, a: &Track, b: &Track, diff: &mut SessionDiff) {
    if a.name != b.name {
        diff.modified.push((
            DiffOp::TrackName {
                track_id: tid.clone(),
                value: a.name.clone(),
            },
            DiffOp::TrackName {
                track_id: tid.clone(),
                value: b.name.clone(),
            },
        ));
    }
    if a.gain_db.to_bits() != b.gain_db.to_bits() {
        diff.modified.push((
            DiffOp::TrackGain {
                track_id: tid.clone(),
                value: a.gain_db,
            },
            DiffOp::TrackGain {
                track_id: tid.clone(),
                value: b.gain_db,
            },
        ));
    }
    if a.pan.to_bits() != b.pan.to_bits() {
        diff.modified.push((
            DiffOp::TrackPan {
                track_id: tid.clone(),
                value: a.pan,
            },
            DiffOp::TrackPan {
                track_id: tid.clone(),
                value: b.pan,
            },
        ));
    }
    if a.muted != b.muted {
        diff.modified.push((
            DiffOp::TrackMuted {
                track_id: tid.clone(),
                value: a.muted,
            },
            DiffOp::TrackMuted {
                track_id: tid.clone(),
                value: b.muted,
            },
        ));
    }
    if a.soloed != b.soloed {
        diff.modified.push((
            DiffOp::TrackSoloed {
                track_id: tid.clone(),
                value: a.soloed,
            },
            DiffOp::TrackSoloed {
                track_id: tid.clone(),
                value: b.soloed,
            },
        ));
    }
    if !clips_equal(&a.clips, &b.clips) {
        diff.modified.push((
            DiffOp::ClipsChange {
                track_id: tid.clone(),
                clips: a.clips.clone(),
            },
            DiffOp::ClipsChange {
                track_id: tid.clone(),
                clips: b.clips.clone(),
            },
        ));
    }
}

fn clips_equal(a: &[Clip], b: &[Clip]) -> bool {
    // Compare by canonical JSON to dodge `f32` PartialEq vs Eq concerns
    // and the optional fields with `skip_serializing_if`.
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

fn effect_chain_diff(
    scope: &EffectScope,
    a: &[EffectInstance],
    b: &[EffectInstance],
    diff: &mut SessionDiff,
) {
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        match (a.get(i), b.get(i)) {
            (Some(ea), Some(eb)) if !effect_equal(ea, eb) => {
                diff.modified.push((
                    DiffOp::Effect {
                        scope: scope.clone(),
                        index: i,
                        effect: ea.clone(),
                    },
                    DiffOp::Effect {
                        scope: scope.clone(),
                        index: i,
                        effect: eb.clone(),
                    },
                ));
            }
            (Some(_), Some(_)) => {}
            (Some(ea), None) => diff.removed.push(DiffOp::Effect {
                scope: scope.clone(),
                index: i,
                effect: ea.clone(),
            }),
            (None, Some(eb)) => diff.added.push(DiffOp::Effect {
                scope: scope.clone(),
                index: i,
                effect: eb.clone(),
            }),
            (None, None) => {}
        }
    }
}

fn effect_equal(a: &EffectInstance, b: &EffectInstance) -> bool {
    a.kind == b.kind && a.bypassed == b.bypassed && a.params == b.params
}

fn bus_graph_diff(a: &BusGraph, b: &BusGraph, diff: &mut SessionDiff) {
    let a_meta: Vec<BusMeta> = a
        .buses
        .iter()
        .map(|x| BusMeta {
            id: x.id,
            name: x.name.clone(),
        })
        .collect();
    let b_meta: Vec<BusMeta> = b
        .buses
        .iter()
        .map(|x| BusMeta {
            id: x.id,
            name: x.name.clone(),
        })
        .collect();
    if a_meta != b_meta {
        diff.modified.push((
            DiffOp::BusList { buses: a_meta },
            DiffOp::BusList { buses: b_meta },
        ));
    }
    // Per-bus effect chains (common ids only).
    let a_by_id: BTreeMap<Uuid, &Bus> = a.buses.iter().map(|x| (x.id, x)).collect();
    let b_by_id: BTreeMap<Uuid, &Bus> = b.buses.iter().map(|x| (x.id, x)).collect();
    let mut common: Vec<Uuid> = a_by_id
        .keys()
        .filter(|k| b_by_id.contains_key(k))
        .copied()
        .collect();
    common.sort();
    for id in common {
        let scope = EffectScope::Bus { bus_id: id };
        effect_chain_diff(&scope, &a_by_id[&id].effects, &b_by_id[&id].effects, diff);
    }
}

fn sort_diff_for_determinism(diff: &mut SessionDiff) {
    diff.added.sort_by_key(op_sort_key);
    diff.removed.sort_by_key(op_sort_key);
    diff.modified.sort_by_key(|(_, to)| op_sort_key(to));
}

fn op_sort_key(op: &DiffOp) -> Vec<u8> {
    // Build a byte key that orders ops first by enum-discriminant
    // family, then by their target identity. Ordering across enum
    // variants is fixed by the `disc` byte below; identity bytes use
    // the same canonical-JSON-of-target-fields trick.
    let mut key: Vec<u8> = Vec::with_capacity(64);
    let disc: u8 = match op {
        DiffOp::TrackOrder { .. } => 0,
        DiffOp::Track { .. } => 1,
        DiffOp::TrackName { .. } => 2,
        DiffOp::TrackGain { .. } => 3,
        DiffOp::TrackPan { .. } => 4,
        DiffOp::TrackMuted { .. } => 5,
        DiffOp::TrackSoloed { .. } => 6,
        DiffOp::ClipsChange { .. } => 7,
        DiffOp::Effect { .. } => 8,
        DiffOp::BusList { .. } => 9,
        DiffOp::TempoMap { .. } => 10,
        DiffOp::KeyMap { .. } => 11,
        DiffOp::Transcript { .. } => 12,
        DiffOp::SampleRate { .. } => 13,
        DiffOp::LengthSamples { .. } => 14,
    };
    key.push(disc);
    match op {
        DiffOp::Track { track, .. } => key.extend_from_slice(track.id.0.as_bytes()),
        DiffOp::TrackName { track_id, .. }
        | DiffOp::TrackGain { track_id, .. }
        | DiffOp::TrackPan { track_id, .. }
        | DiffOp::TrackMuted { track_id, .. }
        | DiffOp::TrackSoloed { track_id, .. }
        | DiffOp::ClipsChange { track_id, .. } => key.extend_from_slice(track_id.0.as_bytes()),
        DiffOp::Effect { scope, index, .. } => {
            match scope {
                EffectScope::Master => key.push(0),
                EffectScope::Track { track_id } => {
                    key.push(1);
                    key.extend_from_slice(track_id.0.as_bytes());
                }
                EffectScope::Bus { bus_id } => {
                    key.push(2);
                    key.extend_from_slice(bus_id.as_bytes());
                }
            }
            key.extend_from_slice(&(*index as u64).to_be_bytes());
        }
        _ => {}
    }
    key
}

// Helpers on SessionState to compute canonical equivalence for the
// optional/defaulted fields. We round-trip through JSON to apply the
// same `skip_serializing_if = "Option::is_none"` treatment as on disk.
trait SessionStateCanonical {
    fn tempo_map_canonical(&self) -> serde_json::Value;
    fn key_map_canonical(&self) -> serde_json::Value;
    fn transcript_canonical(&self) -> serde_json::Value;
}

impl SessionStateCanonical for SessionState {
    fn tempo_map_canonical(&self) -> serde_json::Value {
        serde_json::to_value(&self.tempo_map).unwrap_or(serde_json::Value::Null)
    }
    fn key_map_canonical(&self) -> serde_json::Value {
        serde_json::to_value(&self.key_map).unwrap_or(serde_json::Value::Null)
    }
    fn transcript_canonical(&self) -> serde_json::Value {
        serde_json::to_value(&self.transcript).unwrap_or(serde_json::Value::Null)
    }
}

// =============================================================================
// Free functions for store integration (re-exported from store.rs)
// =============================================================================

/// Look up a node by id and return its `state`.
pub(crate) fn load_state(store: &Store, id: NodeId) -> Result<SessionState> {
    Ok(store.get(id)?.state)
}

/// Compute `diff(a, b)` from the store.
pub fn diff(store: &Store, a: NodeId, b: NodeId) -> Result<SessionDiff> {
    let sa = load_state(store, a)?;
    let sb = load_state(store, b)?;
    Ok(diff_states(&sa, &sb))
}

/// Fork: append a child of `parent` whose state is byte-identical to
/// `parent`'s. Updates head. The new node has the same content hash as
/// the parent (content addressing); the resulting `parent` pointer of
/// the new on-disk node still points at `parent`. To keep the on-disk
/// store invariant — one node file per content hash — fork uses
/// [`Store::set_head`] to point at `parent` (the byte-identical child
/// is the parent; that's the whole point of content addressing). For
/// callers that need a *distinct* node id (different label, different
/// timestamp) the AI tool layer either adds a `name_node`-style
/// modification (which is metadata-only and *doesn't* change the id),
/// or follows the fork up with the first real edit. See M24 plan
/// §"fork semantics" for the rationale.
pub fn fork(store: &mut Store, parent: NodeId) -> Result<NodeId> {
    // Verify the parent exists.
    let _ = store.get(parent)?;
    // Move head to parent. Subsequent `append` calls will parent off
    // this node. Content addressing means a "fork copy" with identical
    // state has the same id as the parent — there's no way (and no
    // need) to materialise a distinct duplicate.
    store.set_head(parent)?;
    Ok(parent)
}

/// Append a node whose state matches `target`'s, parented to the
/// current head. Useful for a "revert" UX: leaves the full history in
/// place but moves head to a state equivalent to a past node.
pub fn revert_to(store: &mut Store, target: NodeId) -> Result<NodeId> {
    let target_node = store.get(target)?;
    let new_node = SessionNode {
        id: NodeId([0; 32]),
        parent: None,
        created_at: chrono::Utc::now(),
        label: Some(format!("revert to {}", target.to_hex())),
        reasoning: None,
        state: target_node.state,
        op: None,
    };
    store.append(new_node)
}

/// Merge two nodes by computing both diffs against their (synthetic)
/// common base — Phase 2 takes the base as the lower of the two ids
/// when one is an ancestor of the other, otherwise as `a` itself.
///
/// This intentionally simple semantics is enough for the M26 "3 forks
/// of the drop, A/B compare, accept v2" workflow: the typical merge is
/// "a was the fork point, b is the candidate". A future milestone can
/// upgrade the LCA computation; the public signature stays the same.
///
/// Returns the new merge node's id, or
/// [`Error::MergeConflict`] when both branches mutated the same
/// [`DiffTarget`].
pub fn merge(store: &mut Store, a: NodeId, b: NodeId) -> Result<NodeId> {
    // Pick a base. If a is an ancestor of b (walk b's parents to find
    // a), then base = a, "branch_a" delta is empty, and the merge is
    // just b. Symmetrically for b ancestor of a.
    let base = lowest_common_ancestor(store, a, b)?;
    let base_state = load_state(store, base)?;
    let a_state = load_state(store, a)?;
    let b_state = load_state(store, b)?;

    let diff_a = diff_states(&base_state, &a_state);
    let diff_b = diff_states(&base_state, &b_state);

    // Conflict detection: any target shared between the two.
    let a_targets: std::collections::HashSet<DiffTarget> = diff_a.targets().into_iter().collect();
    let b_targets: std::collections::HashSet<DiffTarget> = diff_b.targets().into_iter().collect();
    let mut conflicts: Vec<DiffTarget> = a_targets.intersection(&b_targets).cloned().collect();
    if !conflicts.is_empty() {
        // Sort for deterministic error messages.
        conflicts.sort_by_key(|t| format!("{t:?}"));
        return Err(Error::MergeConflict {
            targets: conflicts.into_iter().map(|t| format!("{t:?}")).collect(),
        });
    }

    // Apply both diffs to the base state. Order: a then b. Since the
    // diffs are non-conflicting, the result is order-independent.
    let mut merged = base_state.clone();
    diff_a.apply(&mut merged)?;
    diff_b.apply(&mut merged)?;

    let new_node = SessionNode {
        id: NodeId([0; 32]),
        parent: None,
        created_at: chrono::Utc::now(),
        label: Some(format!("merge {} + {}", a.to_hex(), b.to_hex())),
        reasoning: None,
        state: merged,
        op: None,
    };
    store.append(new_node)
}

/// Walk parent chains from `a` and `b` and return their lowest common
/// ancestor by id. Falls back to `a` if no ancestor is shared (which
/// can happen in synthetic test fixtures where two roots exist; the
/// merge will still treat `a` as the base).
fn lowest_common_ancestor(store: &Store, a: NodeId, b: NodeId) -> Result<NodeId> {
    if a == b {
        return Ok(a);
    }
    let mut a_chain: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
    let mut cur: Option<NodeId> = Some(a);
    while let Some(id) = cur {
        a_chain.insert(id);
        let n = store.get(id)?;
        cur = n.parent;
    }
    let mut cur: Option<NodeId> = Some(b);
    while let Some(id) = cur {
        if a_chain.contains(&id) {
            return Ok(id);
        }
        let n = store.get(id)?;
        cur = n.parent;
    }
    Ok(a)
}
