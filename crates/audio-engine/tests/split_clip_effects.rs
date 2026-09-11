//! A split must be a render no-op, even through a stateful filter (#243).
//!
//! `split_clip` guarantees the two clips it produces are adjacent and
//! concatenate to the original stream, so splitting a track and
//! rendering it must produce the same audio as rendering it unsplit.
//!
//! It did not. The render plan carries one entry per *clip*, and the
//! effect chain was built inside `TrackStreamer::open` — one streamer
//! per plan entry — so a track split into N clips got N independent
//! chains, each starting from a zeroed biquad delay line. Every split
//! seam restarted the filter.
//!
//! That is the same failure `TrackStreamer::effects` documents itself as
//! preventing across *chunk* boundaries, one level up: the guarantee
//! held across chunks and not across clips.
//!
//! The tone here is deliberately well below the cutoff. A 40 Hz tone
//! through a 120 Hz low-pass is almost unchanged in steady state, so
//! the filter's *output* is a poor place to look for the bug — what
//! gives it away is the transient a restarted delay line produces, and
//! a passband tone makes that transient the only difference there is.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: u64 = SAMPLE_RATE as u64; // one second
/// Where the split lands. Mid-file, and not on a master-chunk boundary
/// (the master chunk is one second), so the seam under test is the
/// clip seam and nothing else.
const SPLIT_AT: u64 = 22_050;

fn write_tone(dir: &Path) -> PathBuf {
    let path = dir.join("tone.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..FRAMES {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 40.0 * t).sin() * 0.8;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn low_pass() -> Vec<EffectInstance> {
    vec![EffectInstance {
        kind: "low_pass_filter".to_string(),
        params: serde_json::json!({ "cutoff_hz": 120.0 }),
        bypassed: false,
    }]
}

/// One track carrying `clips`, with `effects`.
fn state(source: &Path, clips: Vec<Clip>, effects: Vec<EffectInstance>) -> SessionState {
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "t1".into(),
            clips,
            gain_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
            effects,
            sends: Vec::new(),
        }],
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: SAMPLE_RATE,
        length_samples: FRAMES,
        annotations: Vec::new(),
        sync_lock: false,
    }
    .tap_source(source)
}

/// `SessionState` has no builder, and every clip needs the same source
/// path; this keeps the two constructions below from repeating it.
trait TapSource {
    fn tap_source(self, source: &Path) -> Self;
}

impl TapSource for SessionState {
    fn tap_source(mut self, source: &Path) -> Self {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                clip.source_path = source.to_path_buf();
            }
        }
        self
    }
}

fn clip(start_in_track: u64, source_offset: u64, length: u64) -> Clip {
    Clip {
        source_path: PathBuf::new(),
        start_in_track,
        source_offset,
        length,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
    }
}

fn render(clips: Vec<Clip>, effects: Vec<EffectInstance>) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let st = state(&src, clips, effects);
    render_state_to_wav(&st, &out, None).expect("render");
    WavReader::open(&out)
        .expect("open out")
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// The whole clip, and the same audio cut in two at `SPLIT_AT` — what
/// `split_clip` produces.
fn unsplit() -> Vec<Clip> {
    vec![clip(0, 0, FRAMES)]
}

fn split() -> Vec<Clip> {
    vec![
        clip(0, 0, SPLIT_AT),
        clip(SPLIT_AT, SPLIT_AT, FRAMES - SPLIT_AT),
    ]
}

/// Largest absolute sample difference, and where it is.
fn worst_diff(a: &[i16], b: &[i16]) -> (i32, usize) {
    assert_eq!(a.len(), b.len(), "renders differ in length");
    let mut worst = 0i32;
    let mut at = 0usize;
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        let d = (*x as i32 - *y as i32).abs();
        if d > worst {
            worst = d;
            at = i;
        }
    }
    (worst, at)
}

/// The premise: without an effect, a split already renders identically.
///
/// If this ever fails, the test below is measuring clip assembly rather
/// than filter state and proves nothing about #243.
#[test]
fn a_split_is_a_render_no_op_without_effects() {
    let (worst, at) = worst_diff(&render(unsplit(), Vec::new()), &render(split(), Vec::new()));
    assert_eq!(
        worst, 0,
        "splitting a clip changed the render even with no effects \
         (worst diff {worst} at frame {at}); the fixture is wrong"
    );
}

/// The bug. One chain per track, not per clip, so the filter's delay
/// line carries across the seam.
#[test]
fn a_split_is_a_render_no_op_through_a_stateful_filter() {
    let one = render(unsplit(), low_pass());
    let two = render(split(), low_pass());
    let (worst, at) = worst_diff(&one, &two);
    // Bit-identical is what we want and what the fix delivers; the
    // epsilon is here only so a future change to summation order can't
    // fail this on a 1-LSB rounding difference. The bug it is guarding
    // against is ~12000 LSB.
    assert!(
        worst <= 1,
        "splitting a clip changed the filtered render: worst diff {worst} LSB at frame {at} \
         (split seam is frame {SPLIT_AT}). The effect chain is being rebuilt per clip, so the \
         filter restarts from a zeroed delay line at the seam."
    );
}

/// The seam specifically: a restarted filter shows up as a step between
/// two adjacent samples that the unsplit render does not have.
///
/// `worst_diff` above would also catch a wholesale level change, so this
/// pins the *discontinuity* — the thing that is audible as a click.
#[test]
fn no_step_at_the_split_seam() {
    let two = render(split(), low_pass());
    let seam = SPLIT_AT as usize;
    let step = (two[seam] as i32 - two[seam - 1] as i32).abs();

    // What a sample-to-sample step looks like away from the seam, on the
    // same material: the largest step over the preceding tenth of a
    // second. A 40 Hz tone at 44.1 kHz moves slowly, so this is small.
    let mut typical = 0i32;
    for i in (seam - 4_410)..(seam - 1) {
        typical = typical.max((two[i + 1] as i32 - two[i] as i32).abs());
    }

    assert!(
        step <= typical * 4,
        "step of {step} LSB across the split seam at frame {seam}, against a typical step of \
         {typical} LSB on the same material — the filter is restarting at the clip boundary"
    );
}
