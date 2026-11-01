//! `snap_to_zero_crossing` on `cut_range`, `split_clip` and `trim` (#771).
//!
//! A 100 Hz sine at 8 kHz is exactly zero every 40 frames, so the nearest
//! crossing to any frame is known by arithmetic. The snap is off unless asked
//! for, and a call that never mentions it gives what it always did.

use serde_json::{json, Value};

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

const RATE: u32 = 8_000;

fn sine() -> Vec<f32> {
    (0..RATE as usize)
        .map(|n| (std::f32::consts::TAU * 100.0 * n as f32 / RATE as f32).sin() * 0.5)
        .collect()
}

fn session() -> Session {
    Session::with_tracks(&[(RATE, sine())])
}

fn clips(s: &Session) -> Vec<(u64, u64, u64)> {
    s.state().tracks[0]
        .clips
        .iter()
        .map(|c| (c.start_in_track, c.source_offset, c.length))
        .collect()
}

/// 1010 is 10 frames from the crossing at 1000 (and 30 from 1040); 2025 is 15
/// from 2040 (and 25 from 2000).
#[test]
fn cut_range_snaps_each_end_to_the_nearest_crossing() {
    let mut s = session();
    let v = s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 1010, "end_sample": 2025, "snap_to_zero_crossing": true }),
    );
    assert_eq!(v["removed_samples"], json!(1040));
    assert_eq!(clips(&s), vec![(0, 0, 1000), (1000, 2040, 5960)]);
}

#[test]
fn trim_snaps_each_end_to_the_nearest_crossing() {
    let mut s = session();
    let v = s.ok(
        "trim",
        json!({ "track": 0, "start_sample": 1010, "end_sample": 2025, "snap_to_zero_crossing": true }),
    );
    assert_eq!(v["kept_samples"], json!(1040));
    assert_eq!(clips(&s), vec![(0, 1000, 1040)]);
}

#[test]
fn split_clip_snaps_the_split_to_the_nearest_crossing() {
    let mut s = session();
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 1010.0 / RATE as f64, "snap_to_zero_crossing": true }),
    );
    assert_eq!(clips(&s), vec![(0, 0, 1000), (1000, 1000, 7000)]);
}

/// Off unless asked for: omitted and `false` both cut exactly where told,
/// and the answer has the keys it always had.
#[test]
fn without_the_flag_every_cut_is_where_it_was_asked() {
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    for extra in [None, Some(false)] {
        let with = |mut a: Value| {
            if let Some(f) = extra {
                a["snap_to_zero_crossing"] = json!(f);
            }
            a
        };
        let mut s = session();
        let v = s.ok(
            "cut_range",
            with(json!({ "track": 0, "start_sample": 1010, "end_sample": 2025 })),
        );
        assert_eq!(v["removed_samples"], json!(1015), "{extra:?}");
        assert_eq!(clips(&s), vec![(0, 0, 1010), (1010, 2025, 5975)]);
        assert_eq!(
            keys(&v),
            [
                "dropped_labels",
                "node_id",
                "removed_samples",
                "summary",
                "synced_tracks"
            ]
        );

        let mut s = session();
        let v = s.ok(
            "trim",
            with(json!({ "track": 0, "start_sample": 1010, "end_sample": 2025 })),
        );
        assert_eq!(v["kept_samples"], json!(1015), "{extra:?}");
        assert_eq!(clips(&s), vec![(0, 1010, 1015)]);

        let mut s = session();
        s.ok(
            "split_clip",
            with(json!({ "track": 0, "clip_index": 0, "at_sec": 1010.0 / RATE as f64 })),
        );
        assert_eq!(clips(&s), vec![(0, 0, 1010), (1010, 1010, 6990)]);
    }
}

/// No crossing within 5 ms (40 frames): the point stays. A constant signal
/// has none anywhere.
#[test]
fn with_no_crossing_in_reach_the_cut_stays() {
    let mut s = Session::with_tracks(&[(RATE, vec![0.25; RATE as usize])]);
    let v = s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 1010, "end_sample": 2025, "snap_to_zero_crossing": true }),
    );
    assert_eq!(v["removed_samples"], json!(1015));
}

/// Two ends that would snap onto one frame leave the request as it was.
#[test]
fn ends_that_would_meet_are_left_alone() {
    let mut s = session();
    let v = s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 1005, "end_sample": 1015, "snap_to_zero_crossing": true }),
    );
    assert_eq!(v["removed_samples"], json!(10));
}
