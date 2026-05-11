//! M24 DAG-op tests for the session store: fork / diff / merge /
//! revert_to, plus a proptest that random fork/append/revert sequences
//! leave the store in a valid state (parent pointers resolve, hashes
//! match, head reachable).

use std::path::PathBuf;

use chrono::Utc;
use proptest::prelude::*;
use session::{
    BusGraph, Clip, EffectInstance, NodeId, SessionDiff, SessionNode, SessionState, Store,
    TempoMap, Track, TrackId,
};
use tempfile::TempDir;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_state() -> SessionState {
    SessionState {
        tracks: Vec::new(),
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: 48_000,
        length_samples: 0,
        annotations: Vec::new(),
    }
}

fn track(name: &str) -> Track {
    Track {
        id: TrackId(Uuid::new_v4()),
        name: name.into(),
        clips: vec![Clip {
            source_path: PathBuf::from(format!("media/{name}.wav")),
            start_in_track: 0,
            source_offset: 0,
            length: 1_000,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
        }],
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        soloed: false,
        effects: Vec::new(),
    }
}

fn append(store: &mut Store, state: SessionState, label: &str) -> NodeId {
    let node = SessionNode {
        id: NodeId([0; 32]),
        parent: None,
        created_at: Utc::now(),
        label: Some(label.into()),
        reasoning: None,
        state,
    };
    store.append(node).unwrap()
}

// ---------------------------------------------------------------------------
// Acceptance: fork creates a child whose state is byte-identical to parent.
// ---------------------------------------------------------------------------

#[test]
fn fork_creates_byte_equal_child() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let mut s = empty_state();
    s.tracks.push(track("vox"));
    let parent_id = append(&mut store, s.clone(), "seed");

    // Move head somewhere else so fork can prove it returns us.
    let mut s2 = s.clone();
    s2.tracks.push(track("drums"));
    let _other = append(&mut store, s2, "other-branch");

    let branch_head = store.fork(parent_id).unwrap();
    assert_eq!(branch_head, parent_id, "fork returns the branch-point id");
    assert_eq!(store.head(), Some(parent_id));

    let parent_state = store.get(parent_id).unwrap().state;
    let branch_state = store.get(branch_head).unwrap().state;
    let pj = serde_json::to_string(&parent_state).unwrap();
    let bj = serde_json::to_string(&branch_state).unwrap();
    assert_eq!(pj, bj, "fork target state must be byte-equal to parent");
}

// ---------------------------------------------------------------------------
// Acceptance: diff is symmetric under swap.
// ---------------------------------------------------------------------------

#[test]
fn diff_is_symmetric() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let s_a = {
        let mut s = empty_state();
        s.tracks.push(track("vox"));
        s.length_samples = 1_000;
        s
    };
    let s_b = {
        let mut s = s_a.clone();
        s.tracks[0].gain_db = 3.0;
        s.length_samples = 2_000;
        s.master_chain.push(EffectInstance {
            kind: "limiter".into(),
            params: serde_json::json!({"threshold_db": -1.0}),
            bypassed: false,
        });
        s
    };
    let a = append(&mut store, s_a, "a");
    let b = append(&mut store, s_b, "b");

    let diff_ab = store.diff(a, b).unwrap();
    let diff_ba = store.diff(b, a).unwrap();
    let swapped_ab = diff_ab.clone().swapped();

    // Compare via canonical JSON because DiffOp/SessionDiff implement
    // PartialEq but the pair-order in `modified` could differ; we
    // re-sort both to the same canonical order via JSON pretty-print.
    let s = serde_json::to_value(&swapped_ab).unwrap();
    let t = serde_json::to_value(&diff_ba).unwrap();
    assert_eq!(s, t, "swap(diff(a,b)) != diff(b,a)");
}

// ---------------------------------------------------------------------------
// Acceptance: apply(diff(a, b), &mut a_state) == b_state.
// ---------------------------------------------------------------------------

#[test]
fn diff_then_apply_reproduces_target() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let s_a = {
        let mut s = empty_state();
        s.tracks.push(track("vox"));
        s.tracks.push(track("drums"));
        s
    };
    let s_b = {
        let mut s = s_a.clone();
        s.tracks[0].gain_db = -6.0;
        s.tracks[1].name = "kick+snare".into();
        s.tracks[0].effects.push(EffectInstance {
            kind: "compressor".into(),
            params: serde_json::json!({"ratio": 4.0}),
            bypassed: false,
        });
        s.length_samples = 12_345;
        s.transcript = Some(session::Transcript::default());
        s
    };
    let a = append(&mut store, s_a.clone(), "a");
    let _ = append(&mut store, s_b.clone(), "b-snapshot-for-id");
    let b_id = NodeId::from_state(&s_b).unwrap();

    let diff = store.diff(a, b_id).unwrap();
    let mut reconstructed = s_a;
    diff.apply(&mut reconstructed).unwrap();

    let r = serde_json::to_value(&reconstructed).unwrap();
    let want = serde_json::to_value(&s_b).unwrap();
    assert_eq!(r, want, "apply(diff(a,b), a) did not reach b");
}

// ---------------------------------------------------------------------------
// Acceptance: empty diff between identical states.
// ---------------------------------------------------------------------------

#[test]
fn diff_of_identical_states_is_empty() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let mut s = empty_state();
    s.tracks.push(track("vox"));
    let id = append(&mut store, s, "only");
    let diff = store.diff(id, id).unwrap();
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
}

// ---------------------------------------------------------------------------
// Acceptance: merge non-conflicting changes succeeds.
// ---------------------------------------------------------------------------

#[test]
fn merge_non_conflicting_combines() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let mut base = empty_state();
    base.tracks.push(track("vox"));
    base.tracks.push(track("drums"));
    let base_id = append(&mut store, base.clone(), "base");

    // Branch A: change track 0's gain.
    let mut a_state = base.clone();
    a_state.tracks[0].gain_db = -3.0;
    let a = append(&mut store, a_state, "branch-a");

    // Reset head to base for branch B.
    store.set_head(base_id).unwrap();

    // Branch B: change track 1's pan (different track, no conflict).
    let mut b_state = base.clone();
    b_state.tracks[1].pan = 0.5;
    let b = append(&mut store, b_state, "branch-b");

    let merged_id = store.merge(a, b).unwrap();
    let merged = store.get(merged_id).unwrap();
    assert_eq!(merged.state.tracks[0].gain_db, -3.0);
    assert_eq!(merged.state.tracks[1].pan, 0.5);
}

// ---------------------------------------------------------------------------
// Acceptance: merge conflicting changes returns MergeConflict.
// ---------------------------------------------------------------------------

#[test]
fn merge_conflicting_returns_err() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let mut base = empty_state();
    let mut t = track("vox");
    t.effects.push(EffectInstance {
        kind: "eq".into(),
        params: serde_json::json!({"hi_db": 0.0}),
        bypassed: false,
    });
    base.tracks.push(t);
    let base_id = append(&mut store, base.clone(), "base");

    // Branch A: tweak the (track 0, effect 0).
    let mut a_state = base.clone();
    a_state.tracks[0].effects[0].params = serde_json::json!({"hi_db": 3.0});
    let a = append(&mut store, a_state, "a");

    store.set_head(base_id).unwrap();

    // Branch B: also tweak the same (track 0, effect 0) — conflict.
    let mut b_state = base.clone();
    b_state.tracks[0].effects[0].params = serde_json::json!({"hi_db": -3.0});
    let b = append(&mut store, b_state, "b");

    let err = store.merge(a, b).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("merge conflict"),
        "expected conflict error, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance: revert_to appends a node with the target state.
// ---------------------------------------------------------------------------

#[test]
fn revert_to_appends_node_with_target_state() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();

    let mut s0 = empty_state();
    s0.tracks.push(track("vox"));
    let v0 = append(&mut store, s0.clone(), "v0");

    let mut s1 = s0.clone();
    s1.tracks[0].gain_db = -6.0;
    let v1 = append(&mut store, s1, "v1");

    let mut s2 = store.get(v1).unwrap().state.clone();
    s2.tracks[0].gain_db = 6.0;
    let _v2 = append(&mut store, s2, "v2");

    // Now revert head to v0's state.
    let new_head = store.revert_to(v0).unwrap();
    assert_eq!(store.head(), Some(new_head));
    let r = store.get(new_head).unwrap();
    let r_json = serde_json::to_value(&r.state).unwrap();
    let v0_json = serde_json::to_value(&store.get(v0).unwrap().state).unwrap();
    assert_eq!(r_json, v0_json, "reverted state must match target state");
    // Content addressing means the new node id IS v0's id (same state).
    assert_eq!(new_head, v0);
}

// ---------------------------------------------------------------------------
// Proptest: random fork/append/revert sequences leave the store valid.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
enum Op {
    AppendGain(usize, f32), // increase gain on a known track
    AppendAddTrack,
    Fork,       // fork at a random known id
    Revert,     // revert to a random known id
    AppendNoOp, // append base again
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0usize..4, -12.0f32..12.0).prop_map(|(t, g)| Op::AppendGain(t, g)),
        Just(Op::AppendAddTrack),
        Just(Op::Fork),
        Just(Op::Revert),
        Just(Op::AppendNoOp),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        max_shrink_iters: 64,
        .. ProptestConfig::default()
    })]

    #[test]
    fn dag_invariants(ops in prop::collection::vec(arb_op(), 1..32)) {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(dir.path()).unwrap();

        // Seed: a state with one track.
        let mut state = empty_state();
        state.tracks.push(track("seed"));
        let seed = append(&mut store, state, "seed");
        let mut known: Vec<NodeId> = vec![seed];

        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::AppendGain(t, g) => {
                    let head = store.head().unwrap();
                    let mut s = store.get(head).unwrap().state;
                    if !s.tracks.is_empty() {
                        let idx = t % s.tracks.len();
                        s.tracks[idx].gain_db += *g;
                        let id = append(&mut store, s, &format!("op{i}-gain"));
                        known.push(id);
                    }
                }
                Op::AppendAddTrack => {
                    let head = store.head().unwrap();
                    let mut s = store.get(head).unwrap().state;
                    s.tracks.push(track(&format!("t{i}")));
                    let id = append(&mut store, s, &format!("op{i}-addtrack"));
                    known.push(id);
                }
                Op::Fork => {
                    let target = known[i % known.len()];
                    let id = store.fork(target).unwrap();
                    prop_assert_eq!(id, target);
                }
                Op::Revert => {
                    let target = known[i % known.len()];
                    let new_head = store.revert_to(target).unwrap();
                    known.push(new_head);
                }
                Op::AppendNoOp => {
                    let head = store.head().unwrap();
                    let s = store.get(head).unwrap().state;
                    let id = append(&mut store, s, &format!("op{i}-noop"));
                    known.push(id);
                }
            }

            // INVARIANT 1: every known id resolves on disk.
            for id in &known {
                let n = store.get(*id).unwrap();
                // INVARIANT 2: each node's id matches its state's canonical hash.
                let recomputed = NodeId::from_state(&n.state).unwrap();
                prop_assert_eq!(n.id, recomputed);
                // INVARIANT 3: parent pointer (if any) resolves.
                if let Some(p) = n.parent {
                    let _ = store.get(p).unwrap();
                }
            }

            // INVARIANT 4: head is reachable via parent chain to a root.
            let mut cur = store.head();
            let mut steps = 0;
            while let Some(id) = cur {
                let n = store.get(id).unwrap();
                cur = n.parent;
                steps += 1;
                prop_assert!(steps < 10_000, "parent chain too long; possible cycle");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sanity: SessionDiff JSON round-trips byte-equal (deterministic order).
// ---------------------------------------------------------------------------

#[test]
fn session_diff_json_roundtrips() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let mut s_a = empty_state();
    s_a.tracks.push(track("vox"));
    s_a.tracks.push(track("drums"));
    let mut s_b = s_a.clone();
    s_b.tracks[0].gain_db = -3.0;
    s_b.tracks[1].pan = 0.25;
    s_b.master_chain.push(EffectInstance {
        kind: "limiter".into(),
        params: serde_json::json!({"x": 1}),
        bypassed: false,
    });

    let a = append(&mut store, s_a, "a");
    let b = append(&mut store, s_b, "b");

    let d1 = store.diff(a, b).unwrap();
    let json1 = serde_json::to_string_pretty(&d1).unwrap();
    let parsed: SessionDiff = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string_pretty(&parsed).unwrap();
    assert_eq!(json1, json2, "SessionDiff JSON not stable across roundtrip");
}
