//! A track's effects see what the track produces (#313).
//!
//! Follow-up to #243. Before that fix each clip streamer ran its own
//! copy of the track's chain and the results were summed into master;
//! now the clips are summed first and one chain runs on the sum.
//!
//! For *adjacent* clips through a linear filter the two are
//! indistinguishable — superposition — which is why #243's own tests
//! barely move when the grouping is removed. For **overlapping** clips
//! through a **nonlinear** effect they are not:
//!
//! * before: `limit(a) + limit(b)`
//! * after:  `limit(a + b)`
//!
//! Only the second is a limiter. A limiter exists to guarantee a
//! ceiling, and limiting two overlapping clips independently lets their
//! sum sail straight past it — which is the one thing the effect is
//! for.
//!
//! #243 scoped this out as "related but separate", so the corrected
//! behaviour shipped resting on nothing. This is that test.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const SAMPLE_RATE: u32 = 44_100;
const FRAMES: u64 = SAMPLE_RATE as u64; // one second

/// Each clip's amplitude. Two of them overlapping sum to ~1.0 FS,
/// which is comfortably over the ceiling below while each one alone is
/// comfortably under it — that gap is what separates the two orders.
const CLIP_AMP: f32 = 0.5;

/// −6 dB. Above `CLIP_AMP` so a single clip is untouched, well below
/// their sum so the overlap must be caught.
const CEILING_DB: f32 = -6.0;

fn ceiling_linear() -> f32 {
    10.0f32.powf(CEILING_DB / 20.0)
}

/// The ceiling in 16-bit LSB, with a little slack for quantisation.
fn ceiling_lsb() -> i32 {
    (ceiling_linear() * 32_767.0).ceil() as i32 + 2
}

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
        let s = (2.0 * std::f32::consts::PI * 220.0 * t).sin() * CLIP_AMP;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn limiter() -> Vec<EffectInstance> {
    vec![EffectInstance {
        kind: "limiter".to_string(),
        params: serde_json::json!({ "ceiling_db": CEILING_DB }),
        bypassed: false,
    }]
}

fn clip(source: &Path) -> Clip {
    Clip {
        source_path: source.to_path_buf(),
        start_in_track: 0,
        source_offset: 0,
        length: FRAMES,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
    }
}

/// One track holding `n` copies of the same clip, all starting at zero
/// — so they overlap completely and their contributions add.
fn state(source: &Path, n: usize, effects: Vec<EffectInstance>) -> SessionState {
    SessionState {
        tracks: vec![Track {
            id: TrackId::new(),
            name: "t1".into(),
            clips: (0..n).map(|_| clip(source)).collect(),
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
}

fn render(n: usize, effects: Vec<EffectInstance>) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let st = state(&src, n, effects);
    render_state_to_wav(&st, &out, None).expect("render");
    WavReader::open(&out)
        .expect("open out")
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

fn peak(x: &[i16]) -> i32 {
    x.iter().map(|s| (*s as i32).abs()).max().unwrap_or(0)
}

/// The premise, in two halves.
///
/// Without it the real test below could pass for either of two wrong
/// reasons: because only one clip is actually reaching the mix, or
/// because a single clip was already over the ceiling and the overlap
/// is not what is being measured.
#[test]
fn the_two_clips_really_do_overlap_and_one_alone_is_under_the_ceiling() {
    let one = peak(&render(1, Vec::new()));
    let two = peak(&render(2, Vec::new()));

    assert!(
        one < ceiling_lsb(),
        "a single clip peaks at {one} LSB, already at or over the {} LSB ceiling — the test \
         below would pass without the overlap mattering",
        ceiling_lsb()
    );
    assert!(
        two > one + one / 2,
        "two fully overlapping clips peaked at {two} LSB against {one} for one clip; they are \
         not both reaching the mix, so there is no overlap to test"
    );
    assert!(
        two > ceiling_lsb(),
        "the unlimited sum ({two} LSB) does not exceed the ceiling ({} LSB), so a limiter has \
         nothing to do and the test below is vacuous",
        ceiling_lsb()
    );
}

/// The point. A limiter guarantees a ceiling, and it can only do that
/// if it sees the track's whole output.
///
/// Per-clip chains gave `limit(a) + limit(b)`: each clip passes through
/// untouched at 0.5 FS, and their sum lands at ~1.0 FS — twice the
/// ceiling the user set, from an effect whose entire job was to prevent
/// exactly that.
#[test]
fn a_limiter_holds_its_ceiling_across_overlapping_clips() {
    let limited = render(2, limiter());
    let p = peak(&limited);
    assert!(
        p <= ceiling_lsb(),
        "the mix peaks at {p} LSB against a {CEILING_DB} dB ceiling ({} LSB). The track's \
         effect chain is running per clip, so each clip was limited on its own and the sum went \
         straight past the ceiling — which is the one thing a limiter is for.",
        ceiling_lsb()
    );
}

/// And it did not get there by silencing the track.
///
/// Clamping everything to zero would satisfy the ceiling test above
/// perfectly, so the output has to be shown to be real audio that has
/// been limited rather than removed.
#[test]
fn the_limiter_does_not_simply_mute_the_track() {
    let limited = render(2, limiter());
    let p = peak(&limited);
    // A clipped sine spends most of its time at the ceiling, so the
    // peak should be right at it rather than merely under it.
    assert!(
        p > ceiling_lsb() / 2,
        "the limited mix peaks at only {p} LSB, far below the {} LSB ceiling — the audio was \
         removed rather than limited",
        ceiling_lsb()
    );
}
