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
use tools::ToolResult;

#[path = "support/harness.rs"]
mod harness;
use harness::{ok, write_exact, Session};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;

/// A mono linear ramp from -0.9 to +0.9 over the whole file.
fn write_ramp_wav(dir: &Path) -> PathBuf {
    write_exact(&dir.join("ramp.wav"), SAMPLE_RATE, SOURCE_FRAMES, |n| {
        let f = n as f32 / (SOURCE_FRAMES - 1) as f32;
        -0.9 + 1.8 * f
    })
}

// The shared exact writer is mono. Keep this signed, unequal stereo
// fixture local so frame/channel mistakes cannot hide behind equal inputs.
fn write_ramp_channels(dir: &Path, channels: u16) -> PathBuf {
    if channels == 1 {
        return write_ramp_wav(dir);
    }
    let path = dir.join("ramp.wav");
    let spec = WavSpec {
        channels,
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
        for ch in 0..channels {
            writer
                .write_sample(if ch == 0 { q } else { -q / 2 })
                .unwrap();
        }
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
    run_channels(steps, 1)
}

fn run_channels(steps: &[(&str, Value)], channels: u16) -> Vec<i16> {
    let mut s = Session::empty();
    let src = write_ramp_channels(s.dir.path(), channels);
    let out = s.dir.path().join("out.wav");

    let loaded = s.ok("load", json!({ "path": src.to_string_lossy() }));
    let mut node_id = loaded["node_id"].as_str().unwrap().to_string();

    for (tool, args) in steps {
        let res = s.ok(tool, args.clone());
        if let Some(id) = res["node_id"].as_str() {
            node_id = id.to_string();
        }
    }

    s.ok(
        "render_final",
        json!({
            "node_id": node_id,
            "format": "wav",
            "out_path": out.to_string_lossy(),
        }),
    );

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

#[test]
fn fade_exponential_argument_reaches_the_dsp() {
    let plain = run(&[]);
    let faded = run(&[(
        "fade",
        json!({
            "track": 0,
            "kind": "in",
            "curve": "exponential",
            "range": { "start_sec": 0.25, "end_sec": 0.75 }
        }),
    )]);
    assert_eq!(faded.len(), plain.len());
    assert_eq!(faded[2_000], 0);
    assert_eq!(faded[3_000], (plain[3_000] as f32 * 0.0625).round() as i16);
    assert_eq!(faded[5_000], (plain[5_000] as f32 * 0.5625).round() as i16);
    assert_same(&faded[..2_000], &plain[..2_000], "prefix changed");
    assert_same(&faded[6_000..], &plain[6_000..], "tail changed");
}

#[test]
fn fade_curve_matrix_renders_exact_samples_on_mono_stereo_and_split_tracks() {
    // Quartiles are literal mathematical gains; the final gains use the
    // existing f32 grid at 3999/4000, rather than an N-1 endpoint.
    let cases = [
        (
            "linear",
            [0.0, 0.25, 0.5, 0.75, 1.0],
            [0.99975, 0.00024998188],
        ),
        (
            "exponential",
            [0.0, 0.0625, 0.25, 0.5625, 1.0],
            [0.9995001, 6.249094e-8],
        ),
        (
            "logarithmic",
            [0.0, 0.5, std::f32::consts::FRAC_1_SQRT_2, 0.8660254, 1.0],
            [0.999875, 0.015810816],
        ),
        (
            "s_curve",
            [0.0, 0.15625, 0.5, 0.84375, 1.0],
            [0.9999998, 1.8744157e-7],
        ),
    ];
    for channels in [1, 2] {
        for split in [false, true] {
            let before = if split { vec![interior_cut()] } else { vec![] };
            let plain = run_channels(&before, channels);
            for (curve, gains, last_gains) in cases {
                for (kind_index, kind) in ["in", "out"].into_iter().enumerate() {
                    let mut steps = before.clone();
                    steps.push((
                        "fade",
                        json!({"track": 0, "kind": kind, "curve": curve,
                        "range": {"start_sec": 0.125, "end_sec": 0.625}}),
                    ));
                    let faded = run_channels(&steps, channels);
                    let stride = channels as usize;
                    assert_eq!(faded.len(), plain.len());
                    assert_same(
                        &faded[..1000 * stride],
                        &plain[..1000 * stride],
                        "curve prefix",
                    );
                    assert_same(
                        &faded[5000 * stride..],
                        &plain[5000 * stride..],
                        "curve tail",
                    );
                    for (index, frame) in [1000, 2000, 3000, 4000, 4999].into_iter().enumerate() {
                        let gain: f32 = if index == 4 {
                            last_gains[kind_index]
                        } else {
                            gains[if kind == "in" { index } else { 4 - index }]
                        };
                        for ch in 0..stride {
                            let i = frame * stride + ch;
                            assert_eq!(
                                faded[i],
                                (plain[i] as f32 * gain).round() as i16,
                                "{curve} {kind}, split={split}, frame {frame} channel {ch}"
                            );
                        }
                    }
                }
            }
        }
    }
}

// Extend the shared session only with fade-specific byte snapshots.
impl Session {
    fn audio(&self) -> (PathBuf, Vec<u8>) {
        let node = self.store.get(self.store.head().unwrap()).unwrap();
        let path = node.state.tracks[0].clips[0].source_path.clone();
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    }

    fn derived(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let directory = match std::fs::read_dir(self.dir.path().join(".audiograph/derived")) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return vec![],
            Err(error) => panic!("read derived directory: {error}"),
        };
        let mut files: Vec<_> = directory
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }
}

#[test]
fn fade_default_and_explicit_linear_have_identical_wav_bytes_and_names() {
    for channels in [1, 2] {
        for kind in [None, Some("in"), Some("out")] {
            let mut s = Session::empty();
            let source = write_ramp_channels(s.dir.path(), channels);
            s.load(&source);
            let loaded = s.store.head().unwrap();
            let mut args = json!({"track": 0, "range": {"start_sec": 0.125, "end_sec": 0.625}});
            if let Some(kind) = kind {
                args["kind"] = kind.into();
            }
            ok(s.try_call_saying("fade", args.clone(), "").unwrap());
            let (default_path, default_bytes) = s.audio();
            s.store.set_head(loaded).unwrap();
            args["curve"] = "linear".into();
            ok(s.try_call_saying("fade", args, "").unwrap());
            assert_eq!(s.audio(), (default_path, default_bytes));
        }
    }
}

#[test]
fn fade_refuses_invalid_curves_and_arguments_without_changing_history_or_files() {
    let mut s = Session::empty();
    let source = write_ramp_wav(s.dir.path());
    s.load(&source);
    ok(s.try_call_saying(
        "fade",
        json!({"track": 0, "curve": "exponential",
        "range": {"start_sec": 0.125, "end_sec": 0.625}}),
        "",
    )
    .unwrap());
    let head = s.store.head();
    let nodes = s.store.list_nodes().unwrap();
    let source_bytes = std::fs::read(&source).unwrap();
    let files = s.derived();
    assert!(
        !files.is_empty(),
        "refusals must preserve existing derived audio"
    );
    let range = json!({"start_sec": 0.125, "end_sec": 0.625});
    let mut cases = Vec::new();
    for curve in [
        json!("unknown"),
        json!("S_curve"),
        json!(null),
        json!(42),
        json!({}),
        json!([]),
    ] {
        cases.push(json!({"track": 0, "range": range, "curve": curve}));
    }
    cases.extend([
        json!({"track": 0, "curve": "exponential"}),
        json!({"track": 0, "curve": "s_curve", "range": {"start_sec": 0.75, "end_sec": 0.25}}),
        json!({"curve": "logarithmic", "range": range}),
        json!({"track": 99, "curve": "exponential", "range": range}),
    ]);
    for args in cases {
        let result = s.try_call_saying("fade", args.clone(), "");
        assert!(
            matches!(result, Err(_) | Ok(ToolResult::Error(_))),
            "accepted {args}"
        );
        assert_eq!(s.store.head(), head);
        assert_eq!(s.store.list_nodes().unwrap().len(), nodes.len());
        assert_eq!(s.derived(), files);
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    }
}

#[test]
fn fade_history_recipe_and_rebuild_preserve_curves_and_legacy_defaults() {
    for curve in [
        None,
        Some("exponential"),
        Some("logarithmic"),
        Some("s_curve"),
    ] {
        let mut original = Session::empty();
        let source = write_ramp_channels(original.dir.path(), 2);
        original.load(&source);
        let source_bytes = std::fs::read(&source).unwrap();
        let mut args = json!({"track": 0, "kind": "out"});
        if let Some(curve) = curve {
            args["curve"] = curve.into();
        }
        ok(original
            .try_call_saying("fade", args, "fade [apply to 0:00.125-0:00.625]")
            .unwrap());
        let head = original.store.head().unwrap();
        let node = original.store.get(head).unwrap();
        let op = node.op.unwrap();
        assert!(op.reproducible);
        let expected_curve = curve.map(|curve| json!(curve));
        assert_eq!(op.params.get("curve"), expected_curve.as_ref());
        assert_eq!(
            op.params["range"],
            json!({"start_sec": 0.125, "end_sec": 0.625})
        );
        let (path, bytes) = original.audio();
        let nodes = original.store.list_nodes().unwrap().len();
        let replay = tools::rederive::replay(&original.store, head).unwrap();
        assert!(replay.produced().contains(path.file_name().unwrap()));
        assert_eq!(original.store.head(), Some(head));
        assert_eq!(original.store.list_nodes().unwrap().len(), nodes);
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);

        let recipe = tools::recipe::export(&original.store, head).unwrap();
        assert!(recipe.blockers().is_empty());
        assert_eq!(recipe.steps.last().unwrap().params, op.params);
        let recipe_path = original.dir.path().join("fade-recipe.json");
        ok(original
            .try_call_saying("export_recipe", json!({"out_path": recipe_path}), "")
            .unwrap());
        let mut fresh = Session::empty();
        ok(fresh
            .try_call_saying("apply_recipe", json!({"path": recipe_path}), "")
            .unwrap());
        let (fresh_path, fresh_bytes) = fresh.audio();
        assert_eq!(fresh_path.file_name(), path.file_name());
        assert_eq!(fresh_bytes, bytes);

        // Advance the head so the real history sweep can remove this file.
        ok(original
            .try_call_saying(
                "silence_region",
                json!({"track": 0, "start_sec": 0.0, "end_sec": 0.1}),
                "",
            )
            .unwrap());
        let current_head = original.store.head();
        let current_nodes = original.store.list_nodes().unwrap().len();
        let report = tools::reclaim::sweep(&original.store, 0).unwrap();
        assert!(report.removed_files > 0);
        assert!(
            !path.exists(),
            "the fade output must actually be swept before rebuild"
        );
        tools::rederive::ensure_present(&original.store, head, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(original.store.head(), current_head);
        assert_eq!(original.store.list_nodes().unwrap().len(), current_nodes);
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    }
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
