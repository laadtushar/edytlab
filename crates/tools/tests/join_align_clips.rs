//! `join_clips` and `align_end_to_end` (#771 items 1 and 4).
//!
//! `join_clips` merges adjacent clips of one track. Clips that are
//! contiguous in one source become one clip over that source, so nothing is
//! written and a render is what it was; anything else is rendered into a new
//! file, and the answer says so. `align_end_to_end` moves the tracks it is
//! given one after another. Both are counted in the frames of the clips they
//! touch (#234, #673, #678), so each has a mixed-rate case: a 16 kHz voice
//! under a 48 kHz or 44.1 kHz bed.

use serde_json::json;
use session::EnvelopePoint;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_tone, Session};

const VOICE_RATE: u32 = 16_000;
const SESSION_RATES: [u32; 2] = [48_000, 44_100];

impl Session {
    /// A track's clips as (start_in_track, source_offset, length), in the
    /// frames of each clip's own file.
    fn clips(&self, track: usize) -> Vec<(u64, u64, u64)> {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| (c.start_in_track, c.source_offset, c.length))
            .collect()
    }

    /// One track of `seconds` of tone at `rate`.
    fn one_track(rate: u32, seconds: usize) -> Self {
        let mut s = Session::empty();
        let a = write_tone(&s.dir.path().join("a.wav"), rate, seconds);
        s.load(&a);
        s
    }

    /// Track 0 is a bed at `rate` of `bed_secs`; track 1 a 16 kHz voice of
    /// `voice_secs`.
    fn bed_and_voice(rate: u32, bed_secs: usize, voice_secs: usize) -> Self {
        let mut s = Session::empty();
        let bed = write_tone(&s.dir.path().join("bed.wav"), rate, bed_secs);
        let voice = write_tone(&s.dir.path().join("voice.wav"), VOICE_RATE, voice_secs);
        s.load(&bed);
        s.load(&voice);
        assert_eq!(s.state().sample_rate, rate, "the bed sets the rate");
        s
    }

    /// One mono tone track per entry of `seconds`, all at 48 kHz.
    fn tracks_of(seconds: &[usize]) -> Self {
        let mut s = Session::empty();
        for (i, secs) in seconds.iter().enumerate() {
            let p = write_tone(&s.dir.path().join(format!("t{i}.wav")), 48_000, *secs);
            s.load(&p);
        }
        s
    }

    fn length(&self) -> u64 {
        self.state().length_samples
    }

    /// Frames a full render of the head writes.
    fn rendered_frames(&self) -> u64 {
        let out = self.dir.path().join("length-check.wav");
        audio_engine::render_state_to_wav(&self.state(), &out, None)
            .expect("render")
            .frames_written
    }

    /// Files in the project's derived-audio folder.
    fn derived_files(&self) -> usize {
        let dir = tools::provenance::derived_dir(self.store.project_dir());
        std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
    }
}

fn max_diff(a: &[i16], b: &[i16]) -> i32 {
    assert_eq!(a.len(), b.len(), "the renders differ in length");
    a.iter()
        .zip(b)
        .map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs())
        .max()
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// join_clips
// ---------------------------------------------------------------------------

/// Two halves of one file, back to back, become one clip over that file: the
/// same source, no new audio, and a render that is the same bytes.
#[test]
fn join_clips_merges_contiguous_clips_of_one_source_without_rendering() {
    let mut s = Session::one_track(48_000, 2);
    let source = s.state().tracks[0].clips[0].source_path.clone();
    let hash = s.state().tracks[0].clips[0].content_hash;
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 1.0 }),
    );
    assert_eq!(s.clips(0).len(), 2);
    let before = s.render_pcm(None);
    let files = s.derived_files();
    let prior = s.head();

    let v = s.ok("join_clips", json!({ "track": 0, "first_clip": 0 }));

    // The joined state is the loaded state exactly, and a node is named for
    // its state (#398), so the head goes back to the node it was split from.
    assert_ne!(s.head(), prior, "the head moved");
    assert_eq!(s.clips(0), vec![(0, 0, 96_000)]);
    let clip = &s.state().tracks[0].clips[0];
    assert_eq!(clip.source_path, source, "still the original file");
    assert_eq!(clip.content_hash, hash);
    assert_eq!(v["rendered"], json!(false));
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("contiguous"), "{summary}");
    assert_eq!(s.derived_files(), files, "a merge writes no audio");
    assert_eq!(s.render_pcm(None), before, "the render is the same bytes");
}

/// The join may name a run of more than two clips, and leave the others.
#[test]
fn join_clips_joins_a_run_inside_a_longer_track() {
    let mut s = Session::one_track(48_000, 4);
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 1.0 }),
    );
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 1, "at_sec": 2.0 }),
    );
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 2, "at_sec": 3.0 }),
    );
    assert_eq!(s.clips(0).len(), 4);
    let before = s.render_pcm(None);
    let (prior, count) = (s.head(), s.node_count());

    let v = s.ok(
        "join_clips",
        json!({ "track": 0, "first_clip": 1, "last_clip": 2 }),
    );

    s.assert_one_node_appended_after(prior, count);

    assert_eq!(
        s.clips(0),
        vec![
            (0, 0, 48_000),
            (48_000, 48_000, 96_000),
            (144_000, 144_000, 48_000)
        ]
    );
    assert_eq!(v["joined_clips"], json!(2));
    assert_eq!(s.render_pcm(None), before);
}

/// A volume envelope on one half only must not spread to the other when the
/// clips become one: the renderer holds a curve's first value before its
/// first point, so a bare merge would have faded the half that had none.
#[test]
fn join_clips_keeps_each_clips_envelope_where_it_was() {
    let mut s = Session::one_track(48_000, 2);
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 1.0 }),
    );
    // Only the second half is automated: a step down to -12 dB.
    s.ok(
        "set_clip_envelope",
        json!({
            "track_index": 0,
            "clip_index": 1,
            "points": [
                { "time_sec": 0.25, "gain_db": -12.0 },
                { "time_sec": 0.75, "gain_db": -12.0 }
            ]
        }),
    );
    let before = s.render_pcm(None);

    s.ok("join_clips", json!({ "track": 0, "first_clip": 0 }));

    assert_eq!(s.clips(0), vec![(0, 0, 96_000)]);
    let after = s.render_pcm(None);
    assert!(
        max_diff(&before, &after) <= 1,
        "the join changed the render by {} LSB",
        max_diff(&before, &after)
    );
    // The first half is still at unity, the second still lowered.
    let loud = after[10_000..20_000]
        .iter()
        .map(|x| i32::from(*x).abs())
        .max();
    let quiet = after[60_000 + 12_000..60_000 + 30_000]
        .iter()
        .map(|x| i32::from(*x).abs())
        .max();
    assert!(loud.unwrap() > quiet.unwrap() * 2, "{loud:?} vs {quiet:?}");
    // The curve is in the one clip's own frames, written as source frames.
    let env: &Vec<EnvelopePoint> = &s.state().tracks[0].clips[0].volume_envelope;
    assert!(!env.is_empty());
    assert!(env
        .windows(2)
        .all(|w| w[0].time_samples <= w[1].time_samples));
}

/// Clips with a gap between them cannot be one clip over the file, so the
/// join is rendered, the gap is silence, and the answer says so.
#[test]
fn join_clips_renders_a_gap_and_says_so() {
    let mut s = Session::one_track(48_000, 2);
    s.ok(
        "split_clip",
        json!({ "track": 0, "clip_index": 0, "at_sec": 1.0 }),
    );
    s.ok(
        "move_clip",
        json!({ "track": 0, "clip_index": 1, "start_sec": 2.0 }),
    );
    let before = s.render_pcm(None);
    let files = s.derived_files();
    let (prior, count) = (s.head(), s.node_count());

    let v = s.ok("join_clips", json!({ "track": 0, "first_clip": 0 }));

    s.assert_one_node_appended_after(prior, count);
    assert_eq!(v["rendered"], json!(true));
    let summary = v["summary"].as_str().unwrap();
    assert!(summary.contains("rendered"), "{summary}");
    assert_eq!(s.derived_files(), files + 1, "one new file");
    assert_eq!(s.clips(0), vec![(0, 0, 3 * 48_000)]);
    let clip = s.state().tracks[0].clips[0].clone();
    let derived = tools::provenance::derived_dir(s.store.project_dir());
    assert_eq!(clip.source_path.parent().unwrap(), derived);
    let after = s.render_pcm(None);
    assert!(max_diff(&before, &after) <= 1);
    assert_eq!(s.length(), 3 * 48_000);
}

/// Adjacent on the timeline but not in the file (the middle was cut out):
/// the two sides come from different places, so this too is rendered.
#[test]
fn join_clips_renders_clips_that_are_not_contiguous_in_the_source() {
    let mut s = Session::one_track(48_000, 3);
    s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 48_000, "end_sample": 96_000 }),
    );
    assert_eq!(s.clips(0), vec![(0, 0, 48_000), (48_000, 96_000, 48_000)]);
    let before = s.render_pcm(None);

    let v = s.ok("join_clips", json!({ "track": 0, "first_clip": 0 }));

    assert_eq!(v["rendered"], json!(true));
    assert_eq!(s.clips(0), vec![(0, 0, 96_000)]);
    assert!(max_diff(&before, &s.render_pcm(None)) <= 1);
}

#[test]
fn join_clips_refuses_what_it_cannot_join() {
    let mut s = Session::bed_and_voice(48_000, 2, 2);
    s.ok(
        "split_clip",
        json!({ "track": 1, "clip_index": 0, "at_sec": 1.0 }),
    );
    let (prior, count, state) = (s.head(), s.node_count(), s.state());

    // A track with one clip has nothing to join.
    let m = s.err("join_clips", json!({ "track": 0, "first_clip": 0 }));
    assert!(m.contains("clip"), "{m}");
    // No such track.
    let m = s.err("join_clips", json!({ "track": 9, "first_clip": 0 }));
    assert!(m.contains("track"), "{m}");
    // No such clip.
    s.err(
        "join_clips",
        json!({ "track": 1, "first_clip": 1, "last_clip": 2 }),
    );
    s.err("join_clips", json!({ "track": 1, "first_clip": 5 }));
    // A run that ends before it starts.
    s.err(
        "join_clips",
        json!({ "track": 1, "first_clip": 1, "last_clip": 0 }),
    );
    // A run of one.
    s.err(
        "join_clips",
        json!({ "track": 1, "first_clip": 0, "last_clip": 0 }),
    );

    s.assert_history_unchanged(prior, count, &state);
}

/// A 16 kHz voice under a bed: the join works in the voice's frames, and the
/// session's length stays in session frames.
#[test]
fn join_clips_on_a_16k_voice_under_a_bed_counts_in_the_voices_frames() {
    for rate in SESSION_RATES {
        // Contiguous: a merge, the clip is the voice's four seconds.
        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok(
            "split_clip",
            json!({ "track": 1, "clip_index": 0, "at_sec": 2.0 }),
        );
        let files = s.derived_files();
        let v = s.ok("join_clips", json!({ "track": 1, "first_clip": 0 }));
        let r = u64::from(VOICE_RATE);
        assert_eq!(v["rendered"], json!(false), "session at {rate}");
        assert_eq!(s.clips(1), vec![(0, 0, 4 * r)], "session at {rate}");
        assert_eq!(s.derived_files(), files, "session at {rate}");
        assert_eq!(s.length(), 6 * u64::from(rate), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");

        // Not contiguous: the second half moves to 3 s, so the joined clip is
        // six seconds of the voice's own frames, in a file at the voice's
        // rate, and the session is as long as it was.
        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok(
            "split_clip",
            json!({ "track": 1, "clip_index": 0, "at_sec": 1.0 }),
        );
        s.ok(
            "move_clip",
            json!({ "track": 1, "clip_index": 1, "start_sec": 3.0 }),
        );
        let v = s.ok("join_clips", json!({ "track": 1, "first_clip": 0 }));
        assert_eq!(v["rendered"], json!(true), "session at {rate}");
        assert_eq!(s.clips(1), vec![(0, 0, 6 * r)], "session at {rate}");
        let clip = s.state().tracks[1].clips[0].clone();
        assert_eq!(
            audio_decoder::probe_sample_rate(&clip.source_path).unwrap(),
            VOICE_RATE,
            "session at {rate}"
        );
        assert_eq!(s.length(), 6 * u64::from(rate), "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

// ---------------------------------------------------------------------------
// align_end_to_end
// ---------------------------------------------------------------------------

/// Three tracks of one, two and three seconds, put one after another: the
/// first stays, the second starts where it ends, the third after that.
#[test]
fn align_end_to_end_places_three_tracks_one_after_another() {
    let mut s = Session::tracks_of(&[1, 2, 3]);
    let (prior, count) = (s.head(), s.node_count());

    let v = s.ok("align_end_to_end", json!({ "tracks": [0, 1, 2] }));

    s.assert_one_node_appended_after(prior, count);
    assert_eq!(s.clips(0), vec![(0, 0, 48_000)]);
    assert_eq!(s.clips(1), vec![(48_000, 0, 96_000)]);
    assert_eq!(s.clips(2), vec![(144_000, 0, 144_000)]);
    assert_eq!(s.length(), 6 * 48_000);
    assert_eq!(s.rendered_frames(), s.length());
    assert!(v["summary"].as_str().unwrap().contains("end to end"));
}

/// The order given is the order placed, and the first track in it stays.
#[test]
fn align_end_to_end_follows_the_order_it_is_given() {
    let mut s = Session::tracks_of(&[1, 2, 3]);

    s.ok("align_end_to_end", json!({ "tracks": [2, 0, 1] }));

    assert_eq!(s.clips(2), vec![(0, 0, 144_000)]);
    assert_eq!(s.clips(0), vec![(144_000, 0, 48_000)]);
    assert_eq!(s.clips(1), vec![(192_000, 0, 96_000)]);
}

/// A gap goes between each track and the next.
#[test]
fn align_end_to_end_leaves_the_gap_it_is_asked_for() {
    let mut s = Session::tracks_of(&[1, 1, 1]);

    s.ok(
        "align_end_to_end",
        json!({ "tracks": [0, 1, 2], "gap_sec": 0.5 }),
    );

    assert_eq!(s.clips(1)[0].0, 72_000);
    assert_eq!(s.clips(2)[0].0, 144_000);
    assert_eq!(s.length(), 192_000);
}

/// A track moves as a whole: its clips keep the spacing they had, and the
/// first one lands where the previous track ends. Tracks not named stay.
#[test]
fn align_end_to_end_moves_a_track_whole_and_leaves_the_others() {
    let mut s = Session::tracks_of(&[1, 2, 1]);
    // Track 1 is split and its second half moved: clips at 0 s and 1.5 s.
    s.ok(
        "split_clip",
        json!({ "track": 1, "clip_index": 0, "at_sec": 1.0 }),
    );
    s.ok(
        "move_clip",
        json!({ "track": 1, "clip_index": 1, "start_sec": 1.5 }),
    );
    let untouched = s.clips(2);

    s.ok("align_end_to_end", json!({ "tracks": [0, 1] }));

    assert_eq!(s.clips(0), vec![(0, 0, 48_000)]);
    // Both clips moved by one second; the half second between them stayed.
    assert_eq!(
        s.clips(1),
        vec![(48_000, 0, 48_000), (48_000 + 72_000, 48_000, 48_000)]
    );
    assert_eq!(s.clips(2), untouched);
}

/// The first track in the list is not pulled back to zero if it starts later.
#[test]
fn align_end_to_end_keeps_the_first_tracks_own_start() {
    let mut s = Session::tracks_of(&[1, 1]);
    s.ok("time_shift", json!({ "track": 0, "offset_sec": 2.0 }));

    s.ok("align_end_to_end", json!({ "tracks": [0, 1] }));

    assert_eq!(s.clips(0)[0].0, 96_000);
    assert_eq!(s.clips(1)[0].0, 144_000, "after the first ends at 3 s");
}

/// A 16 kHz voice and a bed: the voice goes where the bed ends, counted in
/// the voice's frames, and the bed where the voice ends, counted in the bed's.
#[test]
fn align_end_to_end_counts_each_tracks_own_frames() {
    for rate in SESSION_RATES {
        let r = u64::from(VOICE_RATE);
        let b = u64::from(rate);

        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok("align_end_to_end", json!({ "tracks": [0, 1] }));
        assert_eq!(s.clips(0), vec![(0, 0, 6 * b)], "session at {rate}");
        assert_eq!(s.clips(1), vec![(6 * r, 0, 4 * r)], "session at {rate}");
        assert_eq!(s.length(), 10 * b, "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");

        let mut s = Session::bed_and_voice(rate, 6, 4);
        s.ok(
            "align_end_to_end",
            json!({ "tracks": [1, 0], "gap_sec": 1.0 }),
        );
        assert_eq!(s.clips(1), vec![(0, 0, 4 * r)], "session at {rate}");
        assert_eq!(s.clips(0), vec![(5 * b, 0, 6 * b)], "session at {rate}");
        assert_eq!(s.length(), 11 * b, "session at {rate}");
        assert_eq!(s.rendered_frames(), s.length(), "session at {rate}");
    }
}

#[test]
fn align_end_to_end_refuses_what_it_cannot_place() {
    let mut s = Session::tracks_of(&[1, 1]);
    s.ok("add_track", json!({}));
    let (prior, count, state) = (s.head(), s.node_count(), s.state());

    // One track is already end to end with itself.
    // The schema says so before the tool runs.
    for tracks in [json!([0]), json!([])] {
        let e = s
            .try_call("align_end_to_end", json!({ "tracks": tracks }))
            .expect_err("a list shorter than two is rejected");
        assert!(e.to_string().contains("2 items"), "{e}");
    }
    // No such track.
    let m = s.err("align_end_to_end", json!({ "tracks": [0, 7] }));
    assert!(m.contains("track"), "{m}");
    // The same track twice.
    let m = s.err("align_end_to_end", json!({ "tracks": [0, 1, 0] }));
    assert!(m.contains("twice") || m.contains("once"), "{m}");
    // A track with no clips has nothing to place.
    let m = s.err("align_end_to_end", json!({ "tracks": [0, 2] }));
    assert!(m.contains("no clips"), "{m}");
    // A negative gap is refused by the schema too.
    s.try_call(
        "align_end_to_end",
        json!({ "tracks": [0, 1], "gap_sec": -1.0 }),
    )
    .expect_err("a negative gap is rejected");

    s.assert_history_unchanged(prior, count, &state);
}
