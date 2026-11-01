//! `cut_range` and `trim` operate on the track's timeline, so they have to
//! see every clip on it.
//!
//! Both used to read `clips.first()`, rewrite that one clip, and assign the
//! result over `track.clips` — which threw away every other clip on the
//! track. A track split by an earlier interior cut therefore lost its tail
//! the moment you cut or trimmed it again, with no error to say so.
//!
//! Both also measured the track as `max(clip.length)` rather than the
//! furthest clip *end*, so a range past the longest single clip was
//! rejected as out of bounds even though the timeline reached it.

use std::path::{Path, PathBuf};

use hound::WavReader;
use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_half_ramp, Session};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;

/// A project holding `in.wav`, a ramp, so every rendered frame names the
/// source frame it came from. `out.wav` is where a test renders to.
fn fixture() -> Session {
    let s = Session::empty();
    write_half_ramp(&s.dir.path().join("in.wav"), SAMPLE_RATE, SOURCE_FRAMES);
    s
}

impl Session {
    fn src(&self) -> PathBuf {
        self.dir.path().join("in.wav")
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("out.wav")
    }
}

/// Which source frame the rendered frame at `i` came from, recovered from
/// the ramp's amplitude.
fn source_frame_of(sample: i16) -> f32 {
    (sample as f32 / 32_768.0) / 0.5 * SOURCE_FRAMES as f32
}

fn read_samples(path: &Path) -> Vec<i16> {
    let mut reader = WavReader::open(path).expect("open output wav");
    reader
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// Cut twice. The second cut must remove its own range and leave the rest
/// of the track — including everything after the first cut — intact.
#[test]
fn a_second_cut_keeps_the_rest_of_a_split_track() {
    let mut f = fixture();

    f.ok("load", json!({ "path": f.src().to_string_lossy() }));

    // First cut: [1000, 2000) out of the middle. Timeline is now 7000
    // frames, in two clips.
    f.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 1000, "end_sample": 2000 }),
    );

    // Second cut: [5000, 6000) — which lands in the *second* clip, past
    // the end of the first. The old code measured the track as
    // max(clip.length) = 5000 and would reject or mis-place this.
    let cut = f.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 5000, "end_sample": 6000 }),
    );
    let node_id = cut["node_id"].as_str().unwrap().to_string();

    f.ok(
        "render_final",
        json!({
            "node_id": node_id,
            "format": "wav",
            "out_path": f.out().to_string_lossy(),
        }),
    );

    let got = read_samples(&f.out());
    assert_eq!(
        got.len(),
        6000,
        "two 1000-frame cuts from an 8000-frame track leave 6000"
    );

    // Timeline frame 0 is source 0; frame 1000 is source 2000 (first cut);
    // frame 4000 is source 5000; frame 5000 is source 7000 (second cut).
    for (timeline, expected_source) in [(0usize, 0.0), (999, 999.0), (1000, 2000.0), (3999, 4999.0)]
    {
        let got_source = source_frame_of(got[timeline]);
        assert!(
            (got_source - expected_source).abs() < 20.0,
            "timeline frame {timeline} should be source ~{expected_source}, got {got_source}"
        );
    }
    // The frame right after the second cut is the assertion the old code
    // could not pass: it had deleted everything past the first clip.
    let after_second_cut = source_frame_of(got[5000]);
    assert!(
        (after_second_cut - 7000.0).abs() < 20.0,
        "after the second cut the track should resume at source ~7000, got {after_second_cut}"
    );
}

/// Trimming a split track keeps the requested window of the *timeline*,
/// not just whatever fell inside the first clip.
#[test]
fn trimming_a_split_track_keeps_the_whole_window() {
    let mut f = fixture();

    f.ok("load", json!({ "path": f.src().to_string_lossy() }));
    f.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 1000, "end_sample": 2000 }),
    );

    // Keep timeline [500, 4500) — 4000 frames spanning the join at 1000.
    let trim = f.ok(
        "trim",
        json!({ "track": 0, "start_sample": 500, "end_sample": 4500 }),
    );
    let node_id = trim["node_id"].as_str().unwrap().to_string();

    f.ok(
        "render_final",
        json!({
            "node_id": node_id,
            "format": "wav",
            "out_path": f.out().to_string_lossy(),
        }),
    );

    let got = read_samples(&f.out());
    assert_eq!(got.len(), 4000, "trim keeps exactly the requested window");

    // Timeline 500 was source 500; timeline 1000 was source 2000. After
    // the trim those become output frames 0 and 500.
    let first = source_frame_of(got[0]);
    assert!(
        (first - 500.0).abs() < 20.0,
        "trim should start at source ~500, got {first}"
    );
    let across_join = source_frame_of(got[500]);
    assert!(
        (across_join - 2000.0).abs() < 20.0,
        "the trimmed window should carry the second clip across the join \
         (source ~2000), got {across_join}"
    );
    let last = source_frame_of(got[3999]);
    assert!(
        (last - 5499.0).abs() < 20.0,
        "trim should end at source ~5499, got {last}"
    );
}
