//! A transcript made from a track goes with the track when a tool moves it
//! (#899).
//!
//! The words of a transcript made from a track are seconds of that track as
//! it sits on the timeline (#384). `time_shift` (and `move_clip`) moved the
//! clips and left the words, so `cut_words` still accepted the track and cut
//! the wrong audio. `align_end_to_end` already moved them (#892); all three
//! now share `util::shift_track_transcript`.
//!
//! A shift is counted in the frames of the track's own clips, the words in
//! seconds: they are moved by the seconds the clips actually moved, so a
//! shift the track clamps at 0 moves the words by the clamp, not by the
//! request.

use serde_json::json;
use session::TranscriptSource;

#[path = "support/harness.rs"]
mod harness;
use harness::{write_tone, Session};

const RATE: u32 = 48_000;

impl Session {
    /// One mono tone track per `(rate, seconds)`, in order; the first sets the
    /// session's rate.
    fn tracks_at(specs: &[(u32, usize)]) -> Self {
        let mut s = Session::empty();
        for (i, (rate, secs)) in specs.iter().enumerate() {
            let p = write_tone(&s.dir.path().join(format!("t{i}.wav")), *rate, *secs);
            s.load(&p);
        }
        s
    }

    /// The issue's session: track 0 is 4 s, track 1 is 2 s with a transcript
    /// of track 1, "hello" at 0.5 to 1.0 s.
    fn issue() -> Self {
        let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 2)]);
        s.with_track_transcript(1, &[("hello", 0.5, 1.0)]);
        s
    }

    fn clips(&self, track: usize) -> Vec<(u64, u64, u64)> {
        self.state().tracks[track]
            .clips
            .iter()
            .map(|c| (c.start_in_track, c.source_offset, c.length))
            .collect()
    }

    fn times(&self) -> Vec<(f32, f32)> {
        self.state()
            .transcript
            .expect("a transcript")
            .words
            .iter()
            .map(|w| (w.start_s, w.end_s))
            .collect()
    }
}

/// The issue's repro: shift track 1 by +4 s and the word follows, so cutting
/// it cuts the audio that is now at 4.5 to 5.0 s.
#[test]
fn time_shift_moves_the_words_and_cut_words_cuts_the_audio_they_name() {
    let mut s = Session::issue();

    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    assert_eq!(s.times(), vec![(4.5, 5.0)]);
    assert_eq!(s.clips(1), vec![(4 * 48_000, 0, 2 * 48_000)]);

    s.ok(
        "cut_words",
        json!({ "from_word": 0, "to_word": 1, "track": 1 }),
    );
    // Half a second came out of the middle of the clip: a 0.5 s head at 4 s
    // and a 1 s tail from the file's 1.0 s, closed up behind it.
    assert_eq!(
        s.clips(1),
        vec![
            (4 * 48_000, 0, 24_000),
            (4 * 48_000 + 24_000, 48_000, 48_000)
        ],
        "the audio at 4.5 to 5.0 s was cut"
    );
}

/// A backward shift moves them back.
#[test]
fn time_shift_backwards_moves_the_words_back() {
    let mut s = Session::issue();
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    s.ok("time_shift", json!({ "track": 1, "offset_sec": -4.0 }));
    assert_eq!(s.times(), vec![(0.5, 1.0)]);
}

/// The rule before 0 s is the clips' own: a shift is clamped for the whole
/// track so no clip goes negative, and the words move by what the clips
/// moved, so they stay on their audio and are never before 0.
#[test]
fn a_clamped_shift_moves_the_words_by_the_clamp() {
    let mut s = Session::issue();
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 1.0 }));
    assert_eq!(s.times(), vec![(1.5, 2.0)]);

    let v = s.ok("time_shift", json!({ "track": 1, "offset_sec": -5.0 }));
    assert_eq!(v["clamped"], json!(true));
    assert_eq!(v["applied_sec"], json!(-1.0));
    assert_eq!(s.clips(1)[0].0, 0, "the clip stopped at 0");
    assert_eq!(
        s.times(),
        vec![(0.5, 1.0)],
        "the word moved the 1 s the clip moved, not the 5 s asked for"
    );
}

/// Seconds are frames of the track's own clips: a 16 kHz voice moved 4 s is
/// 64 000 frames, and its words move 4 s.
#[test]
fn a_track_at_another_rate_moves_its_words_by_seconds() {
    let mut s = Session::tracks_at(&[(RATE, 6), (16_000, 2)]);
    s.with_track_transcript(1, &[("hello", 0.5, 1.0)]);

    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    assert_eq!(s.clips(1)[0].0, 64_000);
    assert_eq!(s.times(), vec![(4.5, 5.0)]);

    s.ok(
        "cut_words",
        json!({ "from_word": 0, "to_word": 1, "track": 1 }),
    );
    assert_eq!(
        s.clips(1),
        vec![(64_000, 0, 8_000), (72_000, 16_000, 16_000)],
        "0.5 s head at 4 s, then the file from 1.0 s"
    );
}

/// A transcript of another track, of a file, or of no recorded source is not
/// seconds of the shifted track and is left alone.
#[test]
fn a_transcript_of_another_track_or_a_file_does_not_move() {
    // Another track.
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 2)]);
    s.with_track_transcript(0, &[("hello", 0.5, 1.0)]);
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    assert_eq!(s.times(), vec![(0.5, 1.0)]);

    // A file.
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 2)]);
    s.with_transcript(&[("hello", 0.5, 1.0)]);
    let mut state = s.state();
    state.transcript.as_mut().unwrap().source = Some(TranscriptSource::File);
    s.append_state("file transcript", state);
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    assert_eq!(s.times(), vec![(0.5, 1.0)]);

    // No source.
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 2)]);
    s.with_transcript(&[("hello", 0.5, 1.0)]);
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    assert_eq!(s.times(), vec![(0.5, 1.0)]);

    // And no transcript at all is no trouble.
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 2)]);
    s.ok("time_shift", json!({ "track": 1, "offset_sec": 4.0 }));
    assert!(s.state().transcript.is_none());
}

/// `align_end_to_end` moves track 1 to 4 s; its words go with it, through the
/// same helper.
#[test]
fn align_end_to_end_moves_the_words() {
    let mut s = Session::issue();
    s.ok("align_end_to_end", json!({ "tracks": [0, 1] }));
    assert_eq!(s.clips(1)[0].0, 4 * 48_000);
    assert_eq!(s.times(), vec![(4.5, 5.0)]);

    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 2)]);
    s.with_track_transcript(0, &[("hello", 0.5, 1.0)]);
    s.ok("align_end_to_end", json!({ "tracks": [0, 1] }));
    assert_eq!(s.times(), vec![(0.5, 1.0)], "track 0 stayed");
}

/// Sync-lock: an insert into track 0 pushes track 1's later audio along, and
/// the transcript of track 1 is moved with it, word by word.
#[test]
fn a_sync_locked_insert_moves_the_words_after_it() {
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 4)]);
    s.with_track_transcript(1, &[("one", 0.5, 1.0), ("two", 2.0, 2.5)]);
    s.ok("set_sync_lock", json!({ "enabled": true }));

    s.ok(
        "insert_silence",
        json!({ "track": 0, "at": 1.5, "duration": 2.0 }),
    );

    // Track 1 was split at 1.5 s and its second half is 2 s later.
    assert_eq!(s.clips(1)[1].0, 3 * 48_000 + 24_000);
    assert_eq!(s.times(), vec![(0.5, 1.0), (4.0, 4.5)]);

    // The word is where its audio is: cutting "two" removes 4.0 to 4.5 s.
    s.ok(
        "cut_words",
        json!({ "from_word": 1, "to_word": 2, "track": 1 }),
    );
    let end: u64 = s.clips(1).iter().map(|c| c.0 + c.2).max().unwrap();
    assert_eq!(end, 6 * 48_000 - 24_000);
}

/// Sync-lock: a cut from track 0 pulls track 1's later audio back, and the
/// words with it.
#[test]
fn a_sync_locked_cut_moves_the_words_after_it() {
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 4)]);
    s.with_track_transcript(1, &[("one", 0.5, 1.0), ("two", 2.0, 2.5)]);
    s.ok("set_sync_lock", json!({ "enabled": true }));

    s.ok(
        "cut_range",
        json!({ "track": 0, "start_sample": 48_000, "end_sample": 72_000 }),
    );

    assert_eq!(s.times(), vec![(0.5, 1.0), (1.5, 2.0)]);
}

/// `move_clip` moves one clip: the words in that clip go with it and the
/// words in the other clips stay.
#[test]
fn move_clip_moves_the_words_that_sit_in_the_clip() {
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 4)]);
    s.with_track_transcript(1, &[("one", 0.5, 1.0), ("two", 2.5, 3.0)]);
    s.ok(
        "split_clip",
        json!({ "track": 1, "clip_index": 0, "at_sec": 2.0 }),
    );

    // The second clip, 2 to 4 s, to 5 s: 3 s later.
    s.ok(
        "move_clip",
        json!({ "track": 1, "clip_index": 1, "start_sec": 5.0 }),
    );
    assert_eq!(s.clips(1)[1].0, 5 * 48_000);
    assert_eq!(s.times(), vec![(0.5, 1.0), (5.5, 6.0)]);

    // Another track's transcript is not touched by it.
    let mut s = Session::tracks_at(&[(RATE, 4), (RATE, 4)]);
    s.with_track_transcript(0, &[("one", 0.5, 1.0)]);
    s.ok(
        "move_clip",
        json!({ "track": 1, "clip_index": 0, "start_sec": 2.0 }),
    );
    assert_eq!(s.times(), vec![(0.5, 1.0)]);
}
