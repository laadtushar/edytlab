//! Sync-lock: an edit to one track is an edit to the timeline (#170 §3).
//!
//! The case this exists for is an interview recorded one track per
//! speaker. Cutting a sentence out of one of them leaves every later
//! word on that track early by the length of the cut while the other
//! speaker stays put, and the conversation comes apart — silently, with
//! nothing about the edit saying it will.
//!
//! So the tests are about alignment, not about clip arithmetic: after
//! the edit, does a thing that was simultaneous stay simultaneous?
//!
//! `cut_range`, `insert_silence` and `paste_region` read the mode; the last
//! group of tests is the paste's (#572), which splices audio into one track
//! and used to leave the others where they were.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path, seconds: usize) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SAMPLE_RATE as usize * seconds) {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

/// `seconds` of a quiet sine at `rate` Hz with `channels` interleaved
/// channels.
fn write_tone(path: &Path, seconds: usize, rate: u32, channels: u16) -> PathBuf {
    let spec = WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(rate as usize * seconds) {
        let t = n as f32 / rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        for _ in 0..channels {
            w.write_sample((s * 32_767.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    /// Two speakers, ten seconds each, recorded together.
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let a = write_sine(&dir.path().join("host.wav"), 10);
        let b = write_sine(&dir.path().join("guest.wav"), 10);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": a.to_string_lossy() }));
        s.call("load", json!({ "path": b.to_string_lossy() }));
        s
    }

    /// One track per `(seconds, rate, channels)`, in order. The session's
    /// rate is the first one's.
    fn from_specs(specs: &[(usize, u32, u16)]) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        for (i, (seconds, rate, channels)) in specs.iter().enumerate() {
            let wav = write_tone(
                &s._dir.path().join(format!("t{i}.wav")),
                *seconds,
                *rate,
                *channels,
            );
            ok(s.call("load", json!({ "path": wav.to_string_lossy() })));
        }
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn state(&self) -> session::SessionState {
        let head = self.store.head().expect("a head");
        self.store.get(head).expect("head node").state
    }

    /// Where a track's timeline ends, in samples.
    fn track_len(&self, track: usize) -> u64 {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| c.start_in_track + c.length)
            .max()
            .unwrap_or(0)
    }

    fn node_count(&self) -> usize {
        self.store.list_nodes().expect("nodes").len()
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// Off by default, and the old behaviour is exactly what it was.
#[test]
fn without_sync_lock_a_cut_touches_only_its_own_track() {
    let mut s = Session::new();
    assert!(!s.state().sync_lock, "off unless asked for");

    let before = s.track_len(1);
    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 }),
    ));

    assert_eq!(s.track_len(1), before, "the other track must not move");
    assert_eq!(s.track_len(0), before - 48_000, "and this one shortens");
}

/// The interview case: a cut on one speaker takes the same span from
/// the other, so what was simultaneous stays simultaneous.
#[test]
fn with_sync_lock_a_cut_shortens_every_track() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));

    let before = s.track_len(1);
    let v = ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 96_000, "end_sample": 144_000 }),
    ));
    assert_eq!(v["synced_tracks"], json!(1), "{v}");

    assert_eq!(s.track_len(0), before - 48_000);
    assert_eq!(
        s.track_len(1),
        before - 48_000,
        "the second speaker loses the same second, or the two drift apart"
    );
}

/// Inserting has to open the gap everywhere for the same reason.
#[test]
fn with_sync_lock_an_insert_lengthens_every_track() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));

    let before = s.track_len(1);
    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 2.0, "duration": 1.0 }),
    ));

    assert_eq!(s.track_len(0), before + 48_000);
    assert_eq!(s.track_len(1), before + 48_000, "the gap opens on both");
}

/// An insert in the middle splits the other track's clip and moves the
/// tail, rather than moving the whole thing — the audio before the
/// insert point has not moved and must not.
#[test]
fn an_insert_moves_only_what_comes_after_it() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    ok(s.call(
        "insert_silence",
        json!({ "track": 0, "at": 4.0, "duration": 2.0 }),
    ));

    let other = &s.state().tracks[1];
    assert_eq!(other.clips.len(), 2, "the clip is split at the gap");
    assert_eq!(other.clips[0].start_in_track, 0, "the head has not moved");
    assert_eq!(other.clips[0].length, 4 * 48_000);
    assert_eq!(
        other.clips[1].start_in_track,
        6 * 48_000,
        "the tail starts after the two-second gap"
    );
    assert_eq!(
        other.clips[1].source_offset,
        4 * 48_000,
        "and plays from where it left off, not from the beginning",
    );
}

/// "One undoable node, not one per track" — the acceptance says so, and
/// it is the difference between undo restoring the edit and undo
/// restoring half of it.
#[test]
fn a_sync_locked_edit_is_a_single_node() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));

    let before = s.node_count();
    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 }),
    ));
    assert_eq!(s.node_count(), before + 1, "one node, both tracks");
}

/// Undo is the whole point of it being one node.
#[test]
fn undoing_a_sync_locked_cut_restores_both_tracks() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    let (a, b) = (s.track_len(0), s.track_len(1));

    let before_head = s.store.head().expect("head");
    ok(s.call(
        "cut_range",
        json!({ "track": 0, "start_sample": 0, "end_sample": 48_000 }),
    ));
    assert_ne!(s.track_len(0), a);

    s.store.set_head(before_head).expect("undo");
    assert_eq!(s.track_len(0), a, "the cut track is back");
    assert_eq!(s.track_len(1), b, "and so is the one it dragged along");
}

/// The mode travels with the session, so a project re-opened later
/// still edits the way it was left.
#[test]
fn the_mode_is_part_of_the_session() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    assert!(s.state().sync_lock);

    ok(s.call("set_sync_lock", json!({ "enabled": false })));
    assert!(!s.state().sync_lock);
}

/// Setting it to what it already is should not cost the user an undo
/// step that does nothing.
#[test]
fn setting_it_to_what_it_already_is_appends_nothing() {
    let mut s = Session::new();
    let before = s.node_count();
    let v = ok(s.call("set_sync_lock", json!({ "enabled": false })));
    assert_eq!(v["changed"], json!(false), "{v}");
    assert_eq!(s.node_count(), before, "no node for a no-op");
}

// =============================================================================
// paste_region (#572)
// =============================================================================
//
// A paste splices the clipboard into one track, which makes that track longer
// by the clipboard's length. Everything after the point on that track is later;
// with sync-lock on, everything after the point on every other track has to be
// later by the same amount, or the conversation comes apart exactly as it does
// for an insert. The paste used to move the labels and the words and stop
// there, so these tests are the same alignment question `insert_silence`'s are.

/// Copy `[from, to)` of `track` to the clipboard.
fn copy(s: &mut Session, track: usize, from: f64, to: f64) {
    ok(s.call(
        "copy_region",
        json!({ "track": track, "range": { "start_sec": from, "end_sec": to } }),
    ));
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

/// `(start_in_track, source_offset, length)` of each of a track's clips.
fn clips(s: &Session, track: usize) -> Vec<(u64, u64, u64)> {
    s.state().tracks[track]
        .clips
        .iter()
        .map(|c| (c.start_in_track, c.source_offset, c.length))
        .collect()
}

/// **The ticket.** Five seconds go into the host's track at 2 s; the guest's
/// second five seconds onwards are five seconds later, in the same node, and
/// the part before the point has not moved.
#[test]
fn with_sync_lock_a_paste_opens_the_gap_on_every_other_track() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    copy(&mut s, 0, 1.0, 6.0);

    let nodes = s.node_count();
    let v = ok(s.call("paste_region", json!({ "track": 0, "at": 2.0 })));

    assert_eq!(s.track_len(0), 15 * 48_000, "the host grows by the paste");
    assert_eq!(
        s.track_len(1),
        15 * 48_000,
        "and the guest by the same five seconds, or the two drift apart"
    );
    assert_eq!(
        clips(&s, 1),
        vec![
            (0, 0, 2 * 48_000),
            // Plays on from where it left off, after the gap.
            (7 * 48_000, 2 * 48_000, 8 * 48_000),
        ],
        "the guest is split at the point; its head has not moved"
    );
    assert_eq!(s.node_count(), nodes + 1, "one node for both tracks");
    assert_eq!(v["synced_tracks"], json!(1), "{v}");
    assert!(
        v["summary"].as_str().unwrap().contains("1 other track"),
        "the summary says how many tracks were shifted: {v}"
    );
}

/// "One undoable node" is the point: undo puts both tracks back.
#[test]
fn undoing_a_sync_locked_paste_restores_both_tracks() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    copy(&mut s, 0, 1.0, 6.0);
    let (host, guest) = (clips(&s, 0), clips(&s, 1));
    let before = s.store.head().expect("head");

    ok(s.call("paste_region", json!({ "track": 0, "at": 2.0 })));
    assert_ne!(clips(&s, 1), guest, "the guest moved");
    assert_ne!(clips(&s, 0), host, "and the host grew");

    s.store.set_head(before).expect("undo");
    assert_eq!(clips(&s, 0), host, "the host is back");
    assert_eq!(clips(&s, 1), guest, "and so is the guest it dragged along");
}

/// Off, the other tracks are not touched, and the result has no sync report.
#[test]
fn without_sync_lock_a_paste_touches_only_its_own_track() {
    let mut s = Session::new();
    assert!(!s.state().sync_lock, "off unless asked for");
    copy(&mut s, 0, 1.0, 6.0);
    let guest = clips(&s, 1);

    let v = ok(s.call("paste_region", json!({ "track": 0, "at": 2.0 })));

    assert_eq!(clips(&s, 1), guest, "the other track must not move");
    assert_eq!(s.track_len(0), 15 * 48_000, "and this one grows");
    assert!(v.get("synced_tracks").is_none(), "{v}");
    assert!(
        !v["summary"].as_str().unwrap().contains("other track"),
        "{v}"
    );
}

/// The host is whichever track the call names, not the first: with three
/// tracks, pasting into the middle one moves the other two and leaves its own
/// timeline to the paste.
#[test]
fn every_track_but_the_host_moves() {
    let mut s = Session::from_specs(&[(10, 48_000, 1), (10, 48_000, 1), (10, 48_000, 1)]);
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    copy(&mut s, 1, 0.0, 3.0);

    let v = ok(s.call("paste_region", json!({ "track": 1, "at": 4.0 })));

    assert_eq!(v["synced_tracks"], json!(2), "{v}");
    for track in 0..3 {
        assert_eq!(
            s.track_len(track),
            13 * 48_000,
            "track {track} is three seconds longer, once"
        );
    }
    for track in [0, 2] {
        assert_eq!(
            clips(&s, track),
            vec![(0, 0, 4 * 48_000), (7 * 48_000, 4 * 48_000, 6 * 48_000)],
            "track {track}: split at 4 s, tail three seconds later"
        );
    }
    // The host's own audio went in through the sample edit, not the timeline
    // shift: still one clip, not split a second time.
    assert_eq!(clips(&s, 1).len(), 1, "{:?}", clips(&s, 1));
}

/// The labels follow the audio, once. They are the session's, not a track's,
/// so each other track being shifted must not shift them again.
#[test]
fn labels_follow_a_paste_once_whatever_the_mode_and_the_track_count() {
    for locked in [false, true] {
        let mut s = Session::from_specs(&[(10, 48_000, 1), (10, 48_000, 1), (10, 48_000, 1)]);
        if locked {
            ok(s.call("set_sync_lock", json!({ "enabled": true })));
        }
        for (name, at) in [("before", 1.0), ("after", 4.0), ("later", 9.0)] {
            ok(s.call("label", json!({ "name": name, "time": at })));
        }
        copy(&mut s, 0, 1.0, 6.0);
        ok(s.call("paste_region", json!({ "track": 0, "at": 2.0 })));

        let at = |name: &str| -> f64 {
            s.state()
                .annotations
                .iter()
                .find_map(|a| match a.kind {
                    session::AnnotationKind::Marker { time_sec } if a.name == name => {
                        Some(time_sec)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no marker {name}"))
        };
        assert_eq!(at("before"), 1.0, "locked={locked}");
        assert_eq!(at("after"), 9.0, "locked={locked}: five seconds later");
        assert_eq!(at("later"), 14.0, "locked={locked}: five, not ten");
    }
}

/// A clipboard in another frame domain than the session's still opens the
/// same gap in time. The host here is a 44.1 kHz bed in a 48 kHz project
/// (#234: a clip counts its own source's frames), the clipboard is two
/// seconds of it, and the 48 kHz tracks have to move by two seconds, which is
/// 96 000 of their frames and not the clipboard's 88 200.
#[test]
fn a_clipboard_at_another_rate_than_the_session_shifts_the_others_by_the_same_seconds() {
    let mut s = Session::from_specs(&[(10, 48_000, 1), (10, 48_000, 1), (4, 44_100, 1)]);
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    copy(&mut s, 2, 0.0, 2.0);
    assert_eq!(
        s.clipboard
            .as_ref()
            .map(|c| (c.sample_rate, c.samples.len())),
        Some((44_100, 88_200))
    );

    let v = ok(s.call("paste_region", json!({ "track": 2, "at": 1.0 })));

    assert_eq!(v["synced_tracks"], json!(2), "{v}");
    assert_eq!(
        s.track_len(2),
        6 * 44_100,
        "the bed grows by two seconds of its own frames"
    );
    for track in [0, 1] {
        assert_eq!(
            clips(&s, track),
            vec![
                (0, 0, 48_000),
                // 1 s + 2 s, in the session's frames.
                (3 * 48_000, 48_000, 9 * 48_000),
            ],
            "track {track}: two seconds later, counted at 48 kHz"
        );
    }
}

/// A stereo clipboard in a mono track opens a gap of its frames, as it does
/// for the labels: one second, not two.
#[test]
fn a_stereo_clipboard_opens_a_gap_of_its_frames_on_the_other_tracks() {
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    s.clipboard = Some(tools::Clipboard {
        samples: vec![0.25; 48_000 * 2],
        sample_rate: 48_000,
        channels: 2,
    });

    ok(s.call("paste_region", json!({ "track": 0, "at": 3.0 })));

    assert_eq!(s.track_len(0), 11 * 48_000);
    assert_eq!(s.track_len(1), 11 * 48_000, "one second, not two");
}

/// `at` past the end appends (the tool's contract), so the audio went in at
/// the host's end, and that is where the gap opens on the others. The host's
/// 10 s are all before it; the guest's second ten seconds are after it.
#[test]
fn a_paste_past_the_end_opens_the_gap_where_it_landed() {
    let mut s = Session::from_specs(&[(10, 48_000, 1), (20, 48_000, 1)]);
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    copy(&mut s, 0, 0.0, 2.0);

    ok(s.call("paste_region", json!({ "track": 0, "at": 99.0 })));

    assert_eq!(s.track_len(0), 12 * 48_000, "appended at the end");
    assert_eq!(
        clips(&s, 1),
        vec![(0, 0, 10 * 48_000), (12 * 48_000, 10 * 48_000, 10 * 48_000)],
        "the guest opens the gap at the host's end, not at 99 s where nothing is"
    );
}

/// A refused paste put nothing in, so nothing else may move either: an error,
/// no node, no track shifted. Both refusals the tool has, with sync-lock on.
#[test]
fn a_refused_paste_shifts_nothing_under_sync_lock() {
    // Another rate than the host's.
    let mut s = Session::new();
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    s.clipboard = Some(tools::Clipboard {
        samples: vec![0.5; 44_100 * 2],
        sample_rate: 44_100,
        channels: 1,
    });
    let (head, guest) = (s.store.head(), clips(&s, 1));
    let msg = err(s.call("paste_region", json!({ "track": 0, "at": 2.0 })));
    assert!(msg.contains("44100") && msg.contains("48000"), "{msg}");
    assert_eq!(s.store.head(), head, "a refused paste writes no node");
    assert_eq!(clips(&s, 1), guest, "and shifts no track");

    // A channel layout with no unambiguous conversion: three channels into a
    // stereo host.
    let mut s = Session::from_specs(&[(10, 48_000, 2), (10, 48_000, 1)]);
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    s.clipboard = Some(tools::Clipboard {
        samples: vec![0.5; 48_000 * 3],
        sample_rate: 48_000,
        channels: 3,
    });
    let (head, guest) = (s.store.head(), clips(&s, 1));
    let msg = err(s.call("paste_region", json!({ "track": 0, "at": 2.0 })));
    assert!(msg.contains("3-channel"), "{msg}");
    assert_eq!(s.store.head(), head, "a refused paste writes no node");
    assert_eq!(clips(&s, 1), guest, "and shifts no track");
}

/// A lone track has nobody to drag along: the lock changes nothing, and the
/// result does not claim it moved anyone.
#[test]
fn a_sync_locked_paste_on_a_lone_track_moves_no_one_and_says_so() {
    let mut s = Session::from_specs(&[(10, 48_000, 1)]);
    ok(s.call("set_sync_lock", json!({ "enabled": true })));
    copy(&mut s, 0, 0.0, 2.0);

    let v = ok(s.call("paste_region", json!({ "track": 0, "at": 1.0 })));

    assert_eq!(s.track_len(0), 12 * 48_000);
    assert_eq!(v["synced_tracks"], json!(0), "{v}");
    assert!(
        !v["summary"].as_str().unwrap().contains("other track"),
        "{v}"
    );
}

/// The description the model reads names every tool the mode reaches, so it
/// can tell what a lock-on paste does before it makes one.
#[test]
fn the_descriptions_name_paste_region() {
    let d = ToolDispatcher::default_dispatcher();
    let schemas = d.tool_schemas();
    let description = |name: &str| -> String {
        schemas
            .as_array()
            .expect("an array")
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is registered"))["description"]
            .as_str()
            .expect("a description")
            .to_string()
    };
    for tool in ["set_sync_lock", "paste_region"] {
        let text = description(tool);
        assert!(
            text.contains("paste_region") || tool == "paste_region",
            "{tool}: {text}"
        );
        assert!(
            text.contains("sync-lock"),
            "{tool} should mention sync-lock: {text}"
        );
    }
    let lock = description("set_sync_lock");
    for tool in ["cut_range", "insert_silence", "paste_region"] {
        assert!(lock.contains(tool), "set_sync_lock omits {tool}: {lock}");
    }
}
