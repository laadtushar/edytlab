//! Per-region export (#766, scope item 3): `export_multiple` with
//! `regions: true` writes one WAV per region annotation, each that region's
//! span of the final mix.
//!
//! What these pin:
//!
//! - each file is the render of the whole session over the region's seconds,
//!   so it holds exactly what `render_final` with that `range` would (mute and
//!   solo included);
//! - files are named `<n>_<sanitised name>.wav`, `n` counting the regions in
//!   time order from 1, so two regions with one name do not collide;
//! - point markers are not regions: they are left out and the result counts
//!   them;
//! - no region is an error, not an empty folder;
//! - a region that runs past the end of the session is cut at the end, and
//!   one that lies wholly past it refuses the call before any file is
//!   written.
//!
//! The project-audio refusal (#653) is in `render_over_source.rs`, beside
//! the other writers'. A region whose time is not finite is skipped and
//! counted too, but a stored node cannot hold one (JSON has no NaN, so the
//! store refuses to read it back), so that case is a unit test of the plan
//! in `export_multiple.rs`.

use std::path::{Path, PathBuf};

use serde_json::json;
use session::{Annotation, AnnotationId, AnnotationKind};

#[path = "support/harness.rs"]
mod harness;
use harness::{read_pcm, Session};

const RATE: u32 = 48_000;

fn region(name: &str, start_sec: f64, end_sec: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.into(),
        kind: AnnotationKind::Region { start_sec, end_sec },
    }
}

fn marker(name: &str, time_sec: f64) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.into(),
        kind: AnnotationKind::Marker { time_sec },
    }
}

impl Session {
    /// Put `labels` on the head as the window or an older build would have
    /// stored them: straight into a node, so a test can hold a label no tool
    /// would write (a region past the end).
    fn plant_labels(&mut self, labels: Vec<Annotation>) {
        let mut state = self.state();
        state.annotations = labels;
        self.append_state("labels", state);
    }

    /// The folder the tool writes into, under the project.
    fn out_dir(&self, name: &str) -> PathBuf {
        self.store.project_dir().join(name)
    }
}

/// Four seconds of two tones, the second muted: a render of the mix is the
/// first alone, so a file holding the second, or the sum, is caught.
fn two_tracks_one_muted() -> Session {
    let mut s = Session::with_tones(RATE, 4 * RATE as usize, &[(440.0, 0.25), (660.0, 0.25)]);
    s.ok("mute_track", json!({ "track": 1, "muted": true }));
    s
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

fn range(start_sec: f64, end_sec: f64) -> audio_engine::TimeRange {
    audio_engine::TimeRange {
        start_frame: (start_sec * f64::from(RATE)) as u64,
        end_frame: (end_sec * f64::from(RATE)) as u64,
    }
}

#[test]
fn writes_one_file_per_region_each_its_span_of_the_mix() {
    let mut s = two_tracks_one_muted();
    // Stored out of time order: the files count in time order.
    s.plant_labels(vec![region("Outro", 2.5, 3.5), region("Intro", 0.0, 1.0)]);

    let out = s.ok(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    let dir = s.out_dir("regions");
    assert_eq!(files_in(&dir), ["1_Intro.wav", "2_Outro.wav"], "{out}");
    assert_eq!(out["count"], 2, "{out}");
    assert_eq!(out["exported"].as_array().unwrap().len(), 2, "{out}");

    // Each file is the render of the session over its region's seconds.
    assert_eq!(
        read_pcm(&dir.join("1_Intro.wav")),
        s.render_pcm(Some(range(0.0, 1.0)))
    );
    assert_eq!(
        read_pcm(&dir.join("2_Outro.wav")),
        s.render_pcm(Some(range(2.5, 3.5)))
    );
    assert_eq!(read_pcm(&dir.join("1_Intro.wav")).len(), RATE as usize);
}

#[test]
fn export_by_region_appends_no_node() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![region("Intro", 0.0, 1.0)]);
    let (head, count, state) = (s.head(), s.node_count(), s.state());

    s.ok(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    s.assert_history_unchanged(head, count, &state);
}

#[test]
fn two_regions_with_one_name_do_not_collide_and_names_are_sanitised() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![
        region("Take", 0.0, 0.5),
        region("Take", 1.0, 1.5),
        region("a/b c.d", 2.0, 2.5),
    ]);

    let out = s.ok(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    assert_eq!(
        files_in(&s.out_dir("regions")),
        ["1_Take.wav", "2_Take.wav", "3_a_b_c_d.wav"],
        "{out}"
    );
}

#[test]
fn point_markers_are_left_out_and_counted() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![
        marker("cue", 0.5),
        region("Intro", 0.0, 1.0),
        marker("cue 2", 2.0),
    ]);

    let out = s.ok(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    assert_eq!(files_in(&s.out_dir("regions")), ["1_Intro.wav"]);
    assert_eq!(out["skipped_markers"], 2, "{out}");
    let summary = out["summary"].as_str().unwrap();
    assert!(summary.contains("2 point markers"), "{summary}");
}

#[test]
fn no_region_is_an_error_and_writes_nothing() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![marker("cue", 0.5)]);

    let msg = s.err(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    assert!(msg.contains("no region"), "{msg}");
    assert!(!s.out_dir("regions").exists());
}

#[test]
fn a_region_past_the_end_is_cut_at_the_end() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![region("Tail", 3.0, 10.0)]);

    let out = s.ok(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    let file = s.out_dir("regions").join("1_Tail.wav");
    assert_eq!(read_pcm(&file).len(), RATE as usize, "{out}");
    assert_eq!(out["clamped"], 1, "{out}");
}

#[test]
fn a_region_wholly_past_the_end_refuses_the_call_before_any_file_is_written() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![region("Intro", 0.0, 1.0), region("Ghost", 5.0, 6.0)]);

    let msg = s.err(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    assert!(msg.contains("Ghost"), "{msg}");
    assert!(msg.contains("end"), "{msg}");
    assert_eq!(files_in(&s.out_dir("regions")), Vec::<String>::new());
}

/// The end of the session is where the render stops: a muted track that
/// runs on further does not make a region past the mix's end exportable.
#[test]
fn the_end_is_the_end_of_what_the_render_mixes() {
    let mut s = Session::with_tones(RATE, 2 * RATE as usize, &[(440.0, 0.25)]);
    let long = harness::write_sine(
        &s.dir.path().join("long.wav"),
        RATE,
        660.0,
        0.25,
        6 * RATE as usize,
    );
    s.load(&long);
    s.ok("mute_track", json!({ "track": 1, "muted": true }));
    s.plant_labels(vec![region("Intro", 0.0, 1.0), region("Late", 3.0, 4.0)]);

    let msg = s.err(
        "export_multiple",
        json!({ "regions": true, "output_dir": "regions" }),
    );

    assert!(msg.contains("Late"), "{msg}");
    assert_eq!(files_in(&s.out_dir("regions")), Vec::<String>::new());
}

#[test]
fn regions_and_track_indices_are_one_or_the_other() {
    let mut s = two_tracks_one_muted();
    s.plant_labels(vec![region("Intro", 0.0, 1.0)]);

    let both = s.err(
        "export_multiple",
        json!({ "regions": true, "track_indices": [0], "output_dir": "regions" }),
    );
    assert!(both.contains("not both"), "{both}");

    let neither = s.err("export_multiple", json!({ "output_dir": "regions" }));
    assert!(neither.contains("track_indices"), "{neither}");
    assert!(neither.contains("regions"), "{neither}");
    assert!(!s.out_dir("regions").exists());
}

/// The per-track export is as it was.
#[test]
fn track_indices_alone_still_export_tracks() {
    let mut s = two_tracks_one_muted();
    let out = s.ok(
        "export_multiple",
        json!({ "track_indices": [0, 1], "output_dir": "stems" }),
    );
    assert_eq!(out["count"], 2, "{out}");
    assert_eq!(files_in(&s.out_dir("stems")).len(), 2);
}
