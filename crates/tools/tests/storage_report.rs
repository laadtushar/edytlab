//! `storage_report` measures the problem #98 describes.
//!
//! The interesting claim is not "it adds up file sizes" — it is the
//! three-way split. #98's central observation is that undo makes every
//! node reachable by design, so "unreachable from the head" is not the
//! same as "safe to delete" and a naive mark-and-sweep would free
//! nothing. A report that collapsed the categories would make a sweep
//! look safe when it is not, which is the specific mistake that ticket
//! warns about.
//!
//! So each test here pins one boundary: that editing moves bytes from
//! nowhere into *history* rather than making them disappear, that the
//! head's own audio is never counted as reclaimable, and that a file
//! nothing names is found.

use serde_json::{json, Value};

#[path = "support/harness.rs"]
mod harness;
use harness::{write_tone, Session};
use tempfile::TempDir;

const SAMPLE_RATE: u32 = 48_000;

/// Load a file, run `edits` destructive edits, and return the report.
fn report_after(edits: usize) -> (Session, Value) {
    let mut s = Session::with_edits(edits);
    let v = s.ok("storage_report", json!({}));
    (s, v)
}

fn u(v: &Value, path: &[&str]) -> u64 {
    let mut cur = v;
    for k in path {
        cur = &cur[k];
    }
    cur.as_u64()
        .unwrap_or_else(|| panic!("expected a number at {path:?}, got {cur}"))
}

/// **A project contains the audio it points at** (#156).
///
/// Derived files used to be written to `<source_dir>/derived/`, so a
/// project folder held only `project.json` and `.audiograph/` while
/// every sample sat next to whatever the user happened to open. A
/// project was therefore not a thing you could copy, move or back up.
#[test]
fn derived_audio_is_written_inside_the_project() {
    let mut s = Session::empty();
    // The source lives somewhere else entirely, which is the normal
    // case: a user opens a file from their music folder.
    let elsewhere = TempDir::new().expect("source dir");
    let src = write_tone(&elsewhere.path().join("in.wav"), SAMPLE_RATE, 1);

    s.load(&src);
    s.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );

    let node = s.store.get(s.head()).expect("head node");
    let derived = &node.state.tracks[0].clips[0].source_path;

    assert!(
        derived.starts_with(s.dir.path()),
        "derived audio landed outside the project: {}",
        derived.display()
    );
    assert!(
        !derived.starts_with(elsewhere.path()),
        "derived audio was written beside the source: {}",
        derived.display()
    );
    assert!(derived.is_file(), "and it must actually be there");
}

/// A session with no edits has written no derived audio, and the report
/// says so rather than erroring or inventing a number.
#[test]
fn a_fresh_session_reports_nothing_derived() {
    let (_tmp, v) = report_after(0);
    assert_eq!(u(&v, &["total_bytes"]), 0);
    assert_eq!(u(&v, &["history", "files"]), 0);
    assert_eq!(u(&v, &["unreferenced", "files"]), 0);
}

/// **The measurement #98 asks for.**
///
/// Each destructive edit writes a file and none are removed, so the
/// total has to grow with the edit count — and the growth has to land in
/// *history*, not in `unreferenced`. Those older files are exactly what
/// undo is holding onto; calling them unreferenced would be the report
/// telling a sweep it may delete the undo stack.
#[test]
fn every_edit_adds_a_file_that_only_history_needs() {
    let (_a, one) = report_after(1);
    let (_b, four) = report_after(4);

    assert!(
        u(&four, &["total_bytes"]) > u(&one, &["total_bytes"]),
        "four edits should cost more than one: {} vs {}",
        u(&four, &["total_bytes"]),
        u(&one, &["total_bytes"])
    );
    // One edit: its output is the head's audio, so nothing is history
    // yet. Four: three superseded files are.
    assert_eq!(
        u(&one, &["history", "files"]),
        0,
        "the only derived file is the one the head uses"
    );
    assert_eq!(
        u(&four, &["history", "files"]),
        3,
        "three superseded files should be held by history, not by the head"
    );
    assert_eq!(
        u(&four, &["unreferenced", "files"]),
        0,
        "a superseded file is still named by its node — calling it \
         unreferenced would invite a sweep to delete the undo stack"
    );
}

/// The head's audio is never in a reclaimable category, whatever else
/// the session has done. Every option in #98 has this as its one
/// non-negotiable.
#[test]
fn the_heads_own_audio_is_always_live() {
    let (_tmp, v) = report_after(3);
    assert!(
        u(&v, &["live", "bytes"]) > 0,
        "the current version's audio must be counted live"
    );
    assert_eq!(u(&v, &["live", "files"]), 1);
}

/// A file in `derived/` that no node names — the leftover of an
/// interrupted edit — is the one category any policy could reclaim
/// without an argument, so the report has to actually find it.
#[test]
fn a_file_no_node_names_is_reported_unreferenced() {
    let mut s = Session::with_edits(1);

    // An orphan, as an interrupted edit would leave behind.
    let orphan = s
        .dir
        .path()
        .join(".audiograph")
        .join("derived")
        .join("orphan.wav");
    std::fs::write(&orphan, vec![0u8; 4096]).expect("write orphan");

    let v = s.ok("storage_report", json!({}));
    assert_eq!(u(&v, &["unreferenced", "files"]), 1);
    assert_eq!(u(&v, &["unreferenced", "bytes"]), 4096);
    let largest = v["largest_unreferenced"]
        .as_array()
        .expect("largest_unreferenced is an array");
    assert_eq!(largest.len(), 1);
    assert!(
        largest[0]["path"]
            .as_str()
            .expect("path")
            .ends_with("orphan.wav"),
        "the report should name the file: {}",
        largest[0]["path"]
    );
}

/// It is a report. #98 is explicit that no policy has been chosen and
/// that the wrong one silently loses work, so this tool must not be the
/// thing that quietly starts deleting.
#[test]
fn the_report_deletes_nothing() {
    let mut s = Session::with_edits(3);
    let derived = s.dir.path().join(".audiograph").join("derived");
    let orphan = derived.join("orphan.wav");
    std::fs::write(&orphan, vec![0u8; 1024]).expect("write orphan");

    let listing = || -> Vec<_> {
        std::fs::read_dir(&derived)
            .expect("read derived")
            .flatten()
            .map(|e| e.path())
            .collect()
    };
    let before = listing();
    s.ok("storage_report", json!({}));
    let after = listing();

    assert_eq!(before.len(), after.len(), "the report removed a file");
    assert!(orphan.exists(), "the report deleted the orphan");
}

// =============================================================================
// Timeline lane copies (#374): the flattened `track-<hash>.wav` files the
// timeline shows are named by no clip. They are the head's audio, or a
// cache of an older version's — never "referenced by nothing".
// =============================================================================

impl Session {
    fn report(&mut self) -> Value {
        self.ok("storage_report", json!({}))
    }

    /// The lane copy the timeline would show for `node`'s first track.
    fn lane_copy_of(&self, node: session::NodeId) -> std::path::PathBuf {
        let clips = self.store.get(node).expect("node").state.tracks[0]
            .clips
            .clone();
        tools::flattened_track_wav(self.dir.path(), &clips).expect("lane copy")
    }

    fn parent(&self) -> session::NodeId {
        self.store
            .get(self.head())
            .expect("head")
            .parent
            .expect("parent")
    }

    fn orphan(&self) -> u64 {
        let path = self
            .dir
            .path()
            .join(".audiograph")
            .join("derived")
            .join("left-by-a-failed-edit.wav");
        std::fs::write(&path, vec![0u8; 4096]).expect("orphan");
        4096
    }
}

#[test]
fn the_lane_copy_on_screen_is_live() {
    let mut p = Session::with_edits(2);
    let live_before = u(&p.report(), &["live", "files"]);
    p.lane_copy_of(p.head());

    let v = p.report();
    assert_eq!(u(&v, &["live", "files"]), live_before + 1, "{v}");
    assert_eq!(u(&v, &["unreferenced", "files"]), 0, "{v}");
}

#[test]
fn a_lane_copy_of_an_older_version_is_its_own_category() {
    let mut p = Session::with_edits(2);
    let lane = p.lane_copy_of(p.parent());
    let bytes = std::fs::metadata(&lane).expect("lane").len();

    let v = p.report();
    assert_eq!(u(&v, &["lane_copies", "files"]), 1, "{v}");
    assert_eq!(u(&v, &["lane_copies", "bytes"]), bytes);
    assert_eq!(u(&v, &["unreferenced", "files"]), 0, "{v}");
    assert!(
        v["summary"].as_str().unwrap().contains("timeline copies"),
        "the summary says what they are: {}",
        v["summary"]
    );
}

/// What the report calls unreferenced is exactly what opening the
/// project removes: the report's own summary says so.
#[test]
fn unreferenced_is_exactly_what_opening_the_project_removes() {
    let mut p = Session::with_edits(3);
    p.lane_copy_of(p.head());
    p.lane_copy_of(p.parent());
    let orphan_bytes = p.orphan();

    let v = p.report();
    assert_eq!(u(&v, &["unreferenced", "bytes"]), orphan_bytes, "{v}");

    let swept = tools::reclaim::sweep_orphans(&p.store).expect("sweep");
    assert_eq!(
        swept.removed_files as u64,
        u(&v, &["unreferenced", "files"])
    );
    assert_eq!(swept.freed_bytes, u(&v, &["unreferenced", "bytes"]));
}

/// A project whose clips all point outside `derived/` — a file just
/// loaded — still has a `derived/` holding lane copies and leftovers,
/// and the report has to look there.
#[test]
fn the_projects_own_derived_directory_is_always_counted() {
    let mut p = Session::with_edits(0);
    p.lane_copy_of(p.head());
    let orphan_bytes = p.orphan();

    let v = p.report();
    assert_eq!(u(&v, &["live", "files"]), 1, "the lane copy on screen: {v}");
    assert_eq!(u(&v, &["unreferenced", "bytes"]), orphan_bytes, "{v}");
}
