//! Tool-level coverage for `reverse`, `fade`, and `insert_silence`.
//!
//! All three had no test of any kind — not a unit test on the DSP, not
//! an integration test through the dispatcher. They appear in
//! `tools_integration.rs` only inside the assertion that lists every
//! registered tool name, which checks that they are *registered*, not
//! that they work. 43 of the 69 registered tools are in that position.
//!
//! Two things are being pinned here.
//!
//! **Clip-awareness.** Seconds and samples are positions on the *track*,
//! and a track is a list of clips. A tool that reaches for `clips.first()`
//! — or that edits the source file a clip points at — is right until the
//! first edit splits the track, and then it is silently wrong. `reverse`
//! and `fade` route through `destructive_edit`, so they should inherit
//! the flatten-first behaviour; nothing proved they did.
//!
//! **Sample-exactness.** The exact-equality assertions here are what
//! found the `32_767.0` quantisation bug in `audio-engine`'s `write_wav`:
//! every destructive edit pulled every sample above half scale one LSB
//! toward zero, and did it again on the next edit. A tolerance-based
//! test would have passed straight over it. That is why these compare
//! whole buffers rather than energy.
//!
//! The source signal is a linear ramp rather than a tone. A ramp makes
//! position self-identifying: every frame's value says where in the
//! timeline it came from, so "did this land in the right place" is an
//! exact equality rather than an energy heuristic — and a ramp sweeps
//! the whole amplitude range, which is what exposed the quantisation
//! bug's dependence on level.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

/// A mono linear ramp from -0.9 to +0.9 over the whole file.
fn write_ramp_wav(dir: &Path) -> PathBuf {
    let path = dir.join("ramp.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SOURCE_FRAMES {
        let f = n as f32 / (SOURCE_FRAMES - 1) as f32;
        let s = -0.9 + 1.8 * f;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// Report the first index at which two renders differ, with values.
///
/// `assert_eq!` on a 7000-element `Vec<i16>` prints both vectors in
/// full, which buries the one fact you need. This prints the fact.
fn assert_same(actual: &[i16], expected: &[i16], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: length differs");
    if let Some(i) = (0..actual.len()).find(|&i| actual[i] != expected[i]) {
        let n = (0..actual.len())
            .filter(|&i| actual[i] != expected[i])
            .count();
        panic!(
            "{what}: first difference at frame {i} \
             (got {}, expected {}); {n} of {} frames differ",
            actual[i],
            expected[i],
            actual.len()
        );
    }
}

/// Drive the dispatcher: load the ramp, run `steps`, render, return PCM.
///
/// Each step is `(tool, args)`. The node id threads through automatically
/// so callers don't have to unpick it from every result.
fn run(steps: &[(&str, Value)]) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(tmp.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let src = write_ramp_wav(tmp.path());
    let out = tmp.path().join("out.wav");

    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
    };

    let loaded = ok(dispatcher
        .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
        .unwrap());
    let mut node_id = loaded["node_id"].as_str().unwrap().to_string();

    for (tool, args) in steps {
        let res = ok(dispatcher.invoke(tool, args.clone(), &mut ctx).unwrap());
        if let Some(id) = res["node_id"].as_str() {
            node_id = id.to_string();
        }
    }

    ok(dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": node_id,
                "format": "wav",
                "out_path": out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let mut reader = WavReader::open(&out).expect("open out");
    reader
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// Removes frames 2000..3000, leaving a track of two clips.
fn interior_cut() -> (&'static str, Value) {
    (
        "cut_range",
        json!({ "track": 0, "start_sample": 2000, "end_sample": 3000 }),
    )
}

/// A whole-track reverse. Applied an even number of times it is the
/// identity, while still driving a full decode → edit → encode → decode
/// round trip on each application — which is the path under test.
///
/// A zero-length range would be a cheaper no-op, but the range resolver
/// rejects `start == end` before the tool ever runs.
fn reverse_all() -> (&'static str, Value) {
    ("reverse", json!({ "track": 0 }))
}

// ---------------------------------------------------------------------------
// The destructive-edit round trip
// ---------------------------------------------------------------------------

/// An edit that changes nothing must produce bytes identical to no edit.
///
/// This is the invariant `audio-engine::encode`'s module docs already
/// claim — "a destructive edit that happens to be a no-op … produces a
/// file whose decoded samples are bit-equal to what the renderer would
/// have emitted from the source". It was false: `write_wav` scaled by
/// `32_767.0` while the decoder divides by `32_768.0`, so the round trip
/// lost one LSB on every sample above half scale.
///
/// A ramp is the right probe because the error is level-dependent — it
/// appears only above half scale, so a quiet test tone would never have
/// shown it.
#[test]
fn an_edit_and_its_inverse_are_bit_identical_to_no_edit() {
    let plain = run(&[]);
    let there_and_back = run(&[reverse_all(), reverse_all()]);

    assert_same(
        &there_and_back,
        &plain,
        "reversing twice did not return the original audio",
    );
}

/// And the loss must not accumulate. Ten reversals, still bit-exact.
///
/// Accumulation is what made the original bug worth fixing rather than
/// tolerating: one LSB is inaudible, but an editing session is a long
/// chain of destructive edits and each one applied the shift again. At
/// ten edits the old code was ten LSBs low across the loud half of the
/// signal.
#[test]
fn repeated_edits_do_not_drift() {
    let plain = run(&[]);
    let steps: Vec<(&str, Value)> = (0..10).map(|_| reverse_all()).collect();
    let edited = run(&steps);

    assert_same(
        &edited,
        &plain,
        "ten reversals drifted; the per-edit error accumulates",
    );
}

// ---------------------------------------------------------------------------
// reverse
// ---------------------------------------------------------------------------

#[test]
fn reverse_with_no_range_reverses_the_whole_track() {
    let plain = run(&[]);
    let reversed = run(&[("reverse", json!({ "track": 0 }))]);

    assert_eq!(plain.len(), SOURCE_FRAMES);

    let mut expected = plain.clone();
    expected.reverse();
    assert_same(&reversed, &expected, "output is not the reversed input");
}

/// The one that would catch a source-file edit.
///
/// After an interior cut the track is two clips over one source file.
/// Reversing "the track" has to reverse the 7000-frame timeline. A tool
/// that reversed the source, or that only reached the first clip, gets a
/// different answer — and both failure modes still produce plausible
/// audio of the right length, which is why this compares against the
/// timeline rather than checking a length or an RMS.
#[test]
fn reverse_operates_on_the_timeline_not_the_source_file() {
    let plain = run(&[interior_cut()]);
    let reversed = run(&[interior_cut(), ("reverse", json!({ "track": 0 }))]);

    assert_eq!(plain.len(), 7_000, "an interior cut leaves 7000 frames");

    let mut expected = plain.clone();
    expected.reverse();
    assert_same(
        &reversed,
        &expected,
        "reversing a split track must reverse the 7000-frame timeline, \
         not the 8000-frame source it was cut from",
    );
}

#[test]
fn reverse_leaves_material_outside_the_range_untouched() {
    let plain = run(&[]);
    let reversed = run(&[(
        "reverse",
        json!({ "track": 0, "range": { "start_sec": 0.25, "end_sec": 0.75 } }),
    )]);

    assert_eq!(reversed.len(), plain.len());

    let (a, b) = (2_000usize, 6_000usize);
    assert_same(&reversed[..a], &plain[..a], "head was modified");
    assert_same(&reversed[b..], &plain[b..], "tail was modified");

    let mut expected_mid = plain[a..b].to_vec();
    expected_mid.reverse();
    assert_same(
        &reversed[a..b],
        &expected_mid,
        "the selected range is not the reverse of the original range",
    );
}

// ---------------------------------------------------------------------------
// fade
// ---------------------------------------------------------------------------

#[test]
fn fade_in_starts_at_silence_and_leaves_the_rest_alone() {
    let plain = run(&[]);
    let faded = run(&[(
        "fade",
        json!({
            "track": 0,
            "kind": "in",
            "range": { "start_sec": 0.0, "end_sec": 0.5 }
        }),
    )]);

    assert_eq!(faded.len(), plain.len(), "fade must not change length");
    assert!(
        faded[0].abs() <= 1,
        "a fade-in must start at silence, got {}",
        faded[0]
    );

    // Outside the range, byte-for-byte untouched. This assertion is what
    // surfaced the quantisation bug — `apply_fade` writes only inside
    // its range, but the rewrite of the whole track shifted the rest.
    assert_same(
        &faded[4_000..],
        &plain[4_000..],
        "fade changed audio outside its range",
    );
}

/// Monotonic gain across the ramp, checked at coarse checkpoints so
/// quantisation noise near the ramp's zero crossing can't flip a
/// comparison.
#[test]
fn fade_in_gain_increases_across_the_range() {
    let plain = run(&[]);
    let faded = run(&[(
        "fade",
        json!({
            "track": 0,
            "kind": "in",
            "range": { "start_sec": 0.0, "end_sec": 1.0 }
        }),
    )]);

    let ratio_at = |i: usize| (faded[i] as f64).abs() / (plain[i] as f64).abs().max(1.0);
    let (early, mid, late) = (ratio_at(1_000), ratio_at(4_000), ratio_at(7_000));

    assert!(
        early < mid && mid < late,
        "fade-in gain must rise across the range, got {early:.3} → {mid:.3} → {late:.3}"
    );
    assert!(
        late > 0.8,
        "gain should be near unity by the end of the range, got {late:.3}"
    );
}

/// The range is half-open, so the last frame *inside* it gets gain
/// `1/len` rather than exactly 0 — the zero belongs to the frame at
/// `end`, which is outside. That is a legitimate convention, and this
/// pins it: near-silence, not silence.
#[test]
fn fade_out_reaches_near_silence_by_the_end_of_its_range() {
    let plain = run(&[]);
    let faded = run(&[(
        "fade",
        json!({
            "track": 0,
            "kind": "out",
            "range": { "start_sec": 0.5, "end_sec": 1.0 }
        }),
    )]);

    let last = *faded.last().expect("non-empty render") as f64;
    let original = *plain.last().expect("non-empty render") as f64;
    let residual = last.abs() / original.abs();

    assert!(
        residual < 0.01,
        "a fade-out must be within 1% of silence at the end of its range, \
         got {last} against an unfaded {original} ({residual:.4})"
    );
}

/// A fade whose range spans the seam of a split track must ramp across
/// both clips. If it only reached the first, the second half of the
/// range would come back at full amplitude.
#[test]
fn fade_spans_the_seam_of_a_split_track() {
    let plain = run(&[interior_cut()]);
    let faded = run(&[
        interior_cut(),
        (
            "fade",
            json!({
                "track": 0,
                "kind": "in",
                "range": { "start_sec": 0.0, "end_sec": 0.75 }
            }),
        ),
    ]);

    assert_eq!(plain.len(), 7_000);
    assert_eq!(faded.len(), plain.len());

    // Frame 2000 onward comes from the second clip. Sample beyond the
    // seam but still inside the fade range.
    let past_seam = 3_000;
    let ratio = (faded[past_seam] as f64).abs() / (plain[past_seam] as f64).abs().max(1.0);
    assert!(
        ratio < 0.85,
        "the fade stopped at the clip boundary: frame {past_seam} is past \
         the seam and still inside the range, but came back at {ratio:.3}x"
    );
}

// ---------------------------------------------------------------------------
// insert_silence
// ---------------------------------------------------------------------------

#[test]
fn insert_silence_lengthens_the_track_by_exactly_the_requested_duration() {
    let plain = run(&[]);
    let padded = run(&[(
        "insert_silence",
        json!({ "track": 0, "at": 0.5, "duration": 0.25 }),
    )]);

    assert_eq!(
        padded.len(),
        plain.len() + 2_000,
        "0.25s at 8kHz is 2000 frames"
    );
}

/// The inserted span must be silent, and the audio on both sides must
/// survive intact — the head unmoved, the tail shifted by exactly the
/// inserted length.
#[test]
fn insert_silence_preserves_the_audio_on_both_sides() {
    let plain = run(&[]);
    let padded = run(&[(
        "insert_silence",
        json!({ "track": 0, "at": 0.5, "duration": 0.25 }),
    )]);

    let at = 4_000usize;
    let pad = 2_000usize;

    assert_same(&padded[..at], &plain[..at], "audio before the insert moved");

    assert!(
        padded[at..at + pad].iter().all(|s| s.abs() <= 1),
        "the inserted span is not silent"
    );

    assert_same(
        &padded[at + pad..],
        &plain[at..],
        "audio after the insert was not shifted intact",
    );
}
