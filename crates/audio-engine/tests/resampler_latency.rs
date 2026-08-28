//! An off-rate track lines up with an on-rate one (#242).
//!
//! `FftFixedInOut` has a latency of `chunk_size_out / 2`, and nothing
//! ever asked it for the number — `grep -rn output_delay` matched
//! nothing. So every clip whose source rate differed from the project's
//! came out that much late relative to every track already at the
//! project rate: 11.7 ms for 48k↔44.1k, 5.3 ms for 96k→48k, 65 ms for
//! 8k→44.1k. And because the render caps each track at its expected
//! frame count, the delayed head pushed an equal slice off the end.
//!
//! In a mix that is an audible flam between a 48 kHz overdub and a
//! 44.1 kHz bed. On a lone off-rate track it clips the last word or the
//! decay of the last note. Nothing reported it: `frames_written` still
//! matched the expected length, because the length was right and only
//! the contents were shifted.
//!
//! The only cross-rate test asserted RMS and peak, never alignment or
//! what was at the end — which is exactly why a whole-track time shift
//! went unnoticed.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const PROJECT_RATE: u32 = 44_100;

/// One second of silence with a full-scale impulse at each named frame.
///
/// Impulses rather than a tone because the question is *where*, and an
/// impulse has one unambiguous answer.
fn write_impulses(dir: &Path, name: &str, rate: u32, at: &[u32]) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..rate {
        let s = if at.contains(&n) { 30_000 } else { 0 };
        w.write_sample(s as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn track(name: &str, source: &Path, length: u64) -> Track {
    Track {
        id: TrackId::new(),
        name: name.into(),
        clips: vec![Clip {
            source_path: source.to_path_buf(),
            start_in_track: 0,
            source_offset: 0,
            length,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
        }],
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        soloed: false,
        effects: Vec::new(),
        sends: Vec::new(),
    }
}

fn state(tracks: Vec<Track>, length_samples: u64) -> SessionState {
    SessionState {
        tracks,
        bus_routing: BusGraph { buses: Vec::new() },
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: PROJECT_RATE,
        length_samples,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

/// The peak frame of each impulse in the render.
///
/// Resampling smears an impulse across several frames — a sinc kernel's
/// ringing — so "every frame above the threshold" reports one impulse as
/// four, and the *first* such frame sits a few frames before the real
/// centre. Runs of adjacent loud frames are therefore grouped, and each
/// group reports its own maximum. Measuring the leading edge instead
/// made a correctly-placed impulse read as 9 frames early.
fn peaks(path: &Path) -> Vec<usize> {
    let mut reader = WavReader::open(path).expect("open render");
    let channels = reader.spec().channels as usize;

    // Per-frame magnitude, across channels.
    let mut mag: Vec<u16> = Vec::new();
    for (i, s) in reader.samples::<i16>().enumerate() {
        let v = s.expect("sample").unsigned_abs();
        let frame = i / channels;
        if frame == mag.len() {
            mag.push(v);
        } else {
            mag[frame] = mag[frame].max(v);
        }
    }

    let mut out = Vec::new();
    let mut group: Option<(usize, u16)> = None; // (best frame, its value)
    let mut gap = 0usize;
    for (f, &v) in mag.iter().enumerate() {
        if v > 4_000 {
            gap = 0;
            group = Some(match group {
                Some((bf, bv)) if bv >= v => (bf, bv),
                _ => (f, v),
            });
        } else if let Some((bf, _)) = group {
            gap += 1;
            // 32 quiet frames ends an impulse. The ringing decays well
            // inside that, and the two impulses under test are seconds
            // apart.
            if gap > 32 {
                out.push(bf);
                group = None;
            }
        }
    }
    if let Some((bf, _)) = group {
        out.push(bf);
    }
    out
}

#[test]
fn an_off_rate_track_lands_on_the_same_frame_as_an_on_rate_one() {
    let tmp = TempDir::new().expect("tempdir");
    // Same impulse position in seconds on both, at different rates.
    let anchor = write_impulses(tmp.path(), "anchor.wav", PROJECT_RATE, &[10]);
    let off = write_impulses(tmp.path(), "off.wav", 48_000, &[11]); // ≈10 at 44.1k

    let out_anchor = tmp.path().join("anchor-out.wav");
    render_state_to_wav(
        &state(
            vec![track("anchor", &anchor, u64::from(PROJECT_RATE))],
            u64::from(PROJECT_RATE),
        ),
        &out_anchor,
        None,
    )
    .expect("render anchor");

    let out_off = tmp.path().join("off-out.wav");
    render_state_to_wav(
        &state(vec![track("off", &off, 48_000)], 48_000),
        &out_off,
        None,
    )
    .expect("render off-rate");

    let anchor_at = *peaks(&out_anchor).first().expect("anchor impulse");
    let off_at = *peaks(&out_off).first().expect("off-rate impulse");

    let skew = anchor_at.abs_diff(off_at);
    assert!(
        skew <= 2,
        "the off-rate track's impulse landed at frame {off_at} while the \
         on-rate one landed at {anchor_at} — {skew} frames of skew. \
         Uncompensated, this was 514 frames."
    );
}

/// The other half: the delay used to push the end of the track off the
/// edge, because the output is capped at the expected frame count.
#[test]
fn the_end_of_an_off_rate_track_survives() {
    let tmp = TempDir::new().expect("tempdir");
    // 48 kHz, impulses near both ends. 47_990 of 48_000 is 10 frames
    // from the end — well inside the 514-frame slice the latency used
    // to consume.
    let src = write_impulses(tmp.path(), "ends.wav", 48_000, &[10, 47_990]);

    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state(vec![track("t", &src, 48_000)], 48_000), &out, None)
        .expect("render");

    let found = peaks(&out);
    assert_eq!(
        found.len(),
        2,
        "expected both impulses in the render, found {found:?} — the \
         closing one used to be truncated with the resampler's delay"
    );

    // 47_990 source frames at 48 kHz is 44_090 project frames at 44.1k.
    let expected_last = 44_090usize;
    let last = *found.last().unwrap();
    assert!(
        last.abs_diff(expected_last) <= 2,
        "the closing impulse landed at {last}, expected about {expected_last}"
    );
}

/// The ratio with the largest latency in the report — 8k → 44.1k is
/// 2866 frames, 65 ms — so a small delay is not passing by accident.
#[test]
fn a_large_ratio_change_is_compensated_too() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_impulses(tmp.path(), "slow.wav", 8_000, &[80]);

    let out = tmp.path().join("out.wav");
    render_state_to_wav(&state(vec![track("t", &src, 8_000)], 8_000), &out, None).expect("render");

    // 80 frames at 8 kHz is 10 ms, which is 441 frames at 44.1 kHz.
    let at = *peaks(&out).first().expect("impulse");
    assert!(
        at.abs_diff(441) <= 3,
        "an 8 kHz source's impulse landed at {at}, expected about 441 — \
         uncompensated this was 2866 frames late"
    );
}
