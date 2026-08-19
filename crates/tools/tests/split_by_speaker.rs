//! Splitting one track into a track per speaker (#168 §2).
//!
//! The acceptance criterion that matters most here is the one that is
//! easiest to get subtly wrong: **combined playback is sample-identical
//! to the original when nothing else changed**. An interview split into
//! two tracks that sums to something slightly different from what went
//! in is worse than not splitting it, because the difference is
//! inaudible until it is not, and by then the original arrangement is
//! several edits back.
//!
//! So the central test renders before and after and compares the bytes.
//! Everything else here exists to protect that property against the two
//! things that really break it — audio nobody claimed, and audio two
//! people claimed.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

/// A tone whose frequency climbs with time, so any misplacement of a
/// span shows up as a different pitch rather than as more of the same.
fn write_sweep(path: &Path, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    let total = SR as usize * seconds;
    for n in 0..total {
        let t = n as f32 / SR as f32;
        let freq = 200.0 + 600.0 * (n as f32 / total as f32);
        w.write_sample(((2.0 * std::f32::consts::PI * freq * t).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Session {
    fn new(seconds: usize) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let src = write_sweep(&dir.path().join("interview.wav"), seconds);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": src.to_string_lossy() }));
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("head");
        self.store.get(head).expect("node").state
    }

    /// Render the current head to a WAV and return its samples.
    fn render(&self, tag: &str) -> Vec<f32> {
        let out = self.dir.path().join(format!("render-{tag}.wav"));
        audio_engine::render_state_to_wav(&self.state(), &out, None).expect("render");
        let mut r = hound::WavReader::open(&out).expect("open render");
        r.samples::<i16>()
            .map(|s| s.expect("sample") as f32)
            .collect()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

/// Two speakers alternating, with gaps between turns — the realistic
/// shape, since a diariser marks speech and says nothing about silence.
fn alternating() -> Value {
    json!([
        { "start_sec": 0.0, "end_sec": 2.0, "speaker": "host" },
        { "start_sec": 2.5, "end_sec": 5.0, "speaker": "guest" },
        { "start_sec": 5.5, "end_sec": 7.0, "speaker": "host" },
    ])
}

#[test]
fn a_two_speaker_recording_produces_two_tracks() {
    let mut s = Session::new(10);
    let res = ok(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));
    assert_eq!(res["speakers"], json!(2));

    let state = s.state();
    let names: Vec<&str> = state.tracks.iter().map(|t| t.name.as_str()).collect();
    // Order of first appearance, not alphabetical — the caller listed
    // the host first.
    assert_eq!(names, vec!["host", "guest", "unassigned"]);
}

#[test]
fn combined_playback_is_sample_identical_to_the_original() {
    // The criterion this whole tool is shaped around. Splitting must be
    // a change to the *arrangement* and to nothing you can hear.
    let mut s = Session::new(10);
    let before = s.render("before");

    ok(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));

    let after = s.render("after");
    assert_eq!(
        before.len(),
        after.len(),
        "split changed the length of the render"
    );
    assert_eq!(before, after, "split changed the audio");
}

#[test]
fn audio_no_segment_covers_is_kept_rather_than_dropped() {
    // The gaps hold room tone and breaths. Dropping them is the easy
    // implementation and it silently deletes the parts of a recording
    // nobody thinks to check.
    let mut s = Session::new(10);
    let res = ok(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));

    // 0.5 + 0.5 between turns, plus 3s of tail after the last one.
    let expected = (4.0 * SR as f64).round() as u64;
    assert_eq!(res["unassigned_samples"], json!(expected));

    let state = s.state();
    let unassigned = state
        .tracks
        .iter()
        .find(|t| t.name == "unassigned")
        .expect("unassigned track");
    assert_eq!(unassigned.clips.len(), 3, "two gaps and the tail");
}

#[test]
fn overlapping_turns_are_awarded_once_and_reported() {
    // People talk over each other, so a diariser emits overlap. Copying
    // the shared audio into both tracks would sum to double — audible,
    // and exactly the kind of thing that gets blamed on the mic.
    let mut s = Session::new(10);
    let before = s.render("before");

    let res = ok(s.call(
        "split_by_speaker",
        json!({
            "track": 0,
            "segments": [
                { "start_sec": 0.0, "end_sec": 4.0, "speaker": "host" },
                { "start_sec": 3.0, "end_sec": 6.0, "speaker": "guest" },
            ]
        }),
    ));

    assert_eq!(res["overlapping_segments"], json!(1));
    assert_eq!(res["speakers"], json!(2));
    // Still identical: the contested second went to exactly one of them.
    assert_eq!(before, s.render("after"), "overlap was double-counted");

    let state = s.state();
    let guest = state
        .tracks
        .iter()
        .find(|t| t.name == "guest")
        .expect("guest track");
    // The guest keeps 3s→6s minus the second the host already had.
    let guest_frames: u64 = guest.clips.iter().map(|c| c.length).sum();
    assert_eq!(guest_frames, (2.0 * SR as f64).round() as u64);
}

#[test]
fn a_speaker_with_several_turns_gets_one_track_with_several_clips() {
    // Turns must stay where they were on the timeline. Rebasing each
    // speaker's audio to zero would be a much simpler implementation
    // and would scramble the conversation.
    let mut s = Session::new(10);
    ok(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));

    let state = s.state();
    let host = state
        .tracks
        .iter()
        .find(|t| t.name == "host")
        .expect("host track");
    assert_eq!(host.clips.len(), 2);
    assert_eq!(host.clips[0].start_in_track, 0);
    assert_eq!(
        host.clips[1].start_in_track,
        (5.5 * SR as f64).round() as u64
    );
    // The second turn reads from 5.5s into the source, not from 2s in.
    assert_eq!(
        host.clips[1].source_offset,
        (5.5 * SR as f64).round() as u64
    );
}

#[test]
fn the_split_tracks_replace_the_source_in_place() {
    // Appending them instead would leave the original playing too, so
    // every voice would be heard twice — and the identity test above
    // would be the thing that caught it.
    let mut s = Session::new(10);
    let before_tracks = s.state().tracks.len();
    assert_eq!(before_tracks, 1);

    ok(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));

    let state = s.state();
    assert_eq!(state.tracks.len(), 3);
    assert!(
        !state.tracks.iter().any(|t| t.name == "interview"),
        "the source track is still present"
    );
}

#[test]
fn the_speakers_name_the_tracks() {
    let mut s = Session::new(10);
    ok(s.call(
        "split_by_speaker",
        json!({
            "track": 0,
            "segments": [
                { "start_sec": 0.0, "end_sec": 3.0, "speaker": "Priya" },
                { "start_sec": 3.0, "end_sec": 6.0, "speaker": "Sam" },
            ],
            "unassigned_name": "room"
        }),
    ));
    let state = s.state();
    let names: Vec<&str> = state.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["Priya", "Sam", "room"]);
}

#[test]
fn a_clip_carrying_a_transform_is_refused_rather_than_guessed_at() {
    // An envelope's points are stated relative to the clip start, so
    // slicing the clip silently reinterprets every one of them.
    // Rendering differently after an edit that claimed to only
    // rearrange is the failure worth refusing over — and this is a
    // realistic order of operations, since ducking (#200) writes
    // exactly this.
    let mut s = Session::new(10);
    ok(s.call(
        "set_clip_envelope",
        json!({
            "track_index": 0,
            "clip_index": 0,
            "points": [
                { "time_sec": 0.0, "gain_db": 0.0 },
                { "time_sec": 4.0, "gain_db": -6.0 },
            ]
        }),
    ));

    let msg = err(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));
    assert!(
        msg.contains("volume envelope"),
        "the refusal should name what blocked it, got: {msg}"
    );
}

#[test]
fn an_empty_segment_list_is_refused() {
    let mut s = Session::new(10);
    let msg = err(s.call("split_by_speaker", json!({ "track": 0, "segments": [] })));
    assert!(msg.contains("segments must not be empty"), "got: {msg}");
}

#[test]
fn segments_past_the_end_are_clamped_rather_than_rejected() {
    // A diariser rounding the last turn past the end of the file is
    // routine. Refusing the whole batch over it would be unhelpful.
    let mut s = Session::new(5);
    let before = s.render("before");
    let res = ok(s.call(
        "split_by_speaker",
        json!({
            "track": 0,
            "segments": [{ "start_sec": 0.0, "end_sec": 900.0, "speaker": "solo" }]
        }),
    ));
    assert_eq!(res["speakers"], json!(1));
    assert_eq!(res["unassigned_samples"], json!(0));
    assert_eq!(before, s.render("after"));
}

#[test]
fn a_zero_length_turn_is_ignored_without_making_a_track() {
    let mut s = Session::new(10);
    let res = ok(s.call(
        "split_by_speaker",
        json!({
            "track": 0,
            "segments": [
                { "start_sec": 0.0, "end_sec": 3.0, "speaker": "host" },
                { "start_sec": 4.0, "end_sec": 4.0, "speaker": "ghost" },
            ]
        }),
    ));
    assert_eq!(res["speakers"], json!(1));
    let state = s.state();
    assert!(!state.tracks.iter().any(|t| t.name == "ghost"));
}

#[test]
fn splitting_appends_an_ordinary_node() {
    // "Both produce ordinary nodes" — undo, branching and provenance
    // all come from that and none of them are special-cased here.
    let mut s = Session::new(10);
    let before = s.store.head().expect("head");
    let res = ok(s.call(
        "split_by_speaker",
        json!({ "track": 0, "segments": alternating() }),
    ));

    let after = s.store.head().expect("head");
    assert_ne!(before, after);
    assert_eq!(res["node_id"], json!(after.to_hex()));

    let node = s.store.get(after).expect("node");
    assert_eq!(node.parent, Some(before));
}

#[test]
fn an_out_of_range_track_is_refused() {
    let mut s = Session::new(10);
    let msg = err(s.call(
        "split_by_speaker",
        json!({ "track": 7, "segments": alternating() }),
    ));
    assert!(msg.contains('7'), "got: {msg}");
}
