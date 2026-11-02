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

/// A time-stretched or pitch-shifted clip plays other samples than its file
/// holds, so the file's crossings are not where it plays: the cut stays, and
/// the answer says why.
#[test]
fn a_stretched_clip_is_not_snapped_and_the_answer_says_so() {
    for (stretch, pitch) in [(Some(1.5), None), (None, Some(2.0))] {
        let mut s = session();
        let mut state = s.state();
        state.tracks[0].clips[0].time_stretch_factor = stretch;
        state.tracks[0].clips[0].pitch_shift_semitones = pitch;
        s.append_state("stretch", state);
        let v = s.ok(
            "cut_range",
            json!({ "track": 0, "start_sample": 1010, "end_sample": 2025, "snap_to_zero_crossing": true }),
        );
        assert_eq!(v["removed_samples"], json!(1015));
        assert!(
            v["snap_note"].as_str().unwrap().contains("not snapped"),
            "{v}"
        );
    }
}

/// A track whose clips differ in channel count still cuts: a point away from
/// the other clip snaps as usual (only the clips at the point are read), and
/// a point where both play stays where it was, with a note, rather than
/// failing the cut.
#[test]
fn clips_of_different_formats_never_fail_a_snapped_cut() {
    let mut s = session();
    let stereo = s.dir.path().join("stereo.wav");
    harness::write_frames(&stereo, RATE, 2, 4000, |_| 0.25);
    let mut state = s.state();
    state.tracks[0].clips[0].length = 4000;
    state.tracks[0]
        .clips
        .push(harness::clip(&stereo, 4000, 0, 4000));
    s.append_state("two formats", state);

    let v = s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 1010, "end_sample": 2025, "snap_to_zero_crossing": true }),
    );
    assert_eq!(v["removed_samples"], json!(1040));
    assert!(v.get("snap_note").is_none(), "{v}");

    let mut s2 = session();
    let mut state = s2.state();
    let stereo = s2.dir.path().join("stereo.wav");
    harness::write_frames(&stereo, RATE, 2, 4000, |_| 0.25);
    state.tracks[0].clips[0].length = 4000;
    state.tracks[0]
        .clips
        .push(harness::clip(&stereo, 4000, 0, 4000));
    s2.append_state("two formats", state);
    let v = s2.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 3995, "end_sample": 4005, "snap_to_zero_crossing": true }),
    );
    assert_eq!(v["removed_samples"], json!(10));
    assert!(
        v["snap_note"].as_str().unwrap().contains("not snapped"),
        "{v}"
    );
}

/// A split looks for a crossing in the clip's own audio. The silence before a
/// clip that starts later is not a crossing: with the clip at 1000, a split
/// at 1003 used to find the lead-in at 999, throw it away as outside the
/// clip, and not snap; the crossing at 1040 is inside it.
#[test]
fn a_split_ignores_the_silence_before_the_clip() {
    let mut s = session();
    let mut state = s.state();
    state.tracks[0].clips[0].start_in_track = 1000;
    s.append_state("lead-in", state);
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 1003.0 / RATE as f64, "snap_to_zero_crossing": true }),
    );
    assert_eq!(clips(&s), vec![(1000, 0, 40), (1040, 40, 7960)]);
}
