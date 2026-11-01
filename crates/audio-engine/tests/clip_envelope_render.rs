//! A clip's volume envelope reaches the audio (#230).
//!
//! `render_state_to_wav` has a byte-copy fast path for a track that
//! *is* exactly one untouched source file. It is worth having — but it
//! was choosing itself without looking at `Clip::volume_envelope`, so
//! the automation the streaming path applies per frame was skipped
//! entirely.
//!
//! The failure was silent and it landed on the most common session
//! shape there is: one track, one clip. A user drew a fade, watched the
//! automation lane draw the curve, exported, and got a file
//! bit-identical to the source. `duck_under_speech` was defeated the
//! same way, since an envelope is its entire output.
//!
//! This is the third time a new processing knob has fallen through this
//! path — #110 was the master chain, #111 was sends — which is why the
//! guard is now an exhaustive destructure of `Clip` rather than a list
//! of remembered fields.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use session::{EnvelopePoint, SessionState};
use tempfile::TempDir;

mod support;
use support::{peak, render_i16, single_track_state, write_sine_trunc};

const SAMPLE_RATE: u32 = 8_000;
const FRAMES: usize = 8_000;

/// A full-scale-ish steady tone, so "did the gain apply" is unambiguous.
fn write_tone(dir: &Path) -> PathBuf {
    write_sine_trunc(dir, "tone.wav", SAMPLE_RATE, FRAMES, 440.0, 0.5)
}

/// One track, one clip covering the whole source at project rate, unity
/// gain, no pan, no effects — precisely the shape that qualifies for the
/// byte copy.
fn with_envelope(source: &Path, envelope: Vec<EnvelopePoint>) -> SessionState {
    let mut state = single_track_state(source);
    state.tracks[0].clips[0].volume_envelope = envelope;
    state
}

fn render(envelope: Vec<EnvelopePoint>) -> Vec<i16> {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    render_i16(&with_envelope(&src, envelope))
}

/// The reported repro: −60 dB across the second half.
///
/// Before the guard this rendered byte-identical to the source, with a
/// second-half peak of 0.5 — the untouched tone.
#[test]
fn an_envelope_on_a_single_track_session_reaches_the_samples() {
    let half = FRAMES / 2;
    let pcm = render(vec![
        EnvelopePoint {
            time_samples: 0,
            gain_db: 0.0,
        },
        EnvelopePoint {
            time_samples: half as u64,
            gain_db: -60.0,
        },
        EnvelopePoint {
            time_samples: FRAMES as u64,
            gain_db: -60.0,
        },
    ]);

    assert_eq!(pcm.len(), FRAMES, "output length must be unchanged");

    let second_half = peak(&pcm[half..]);
    assert!(
        second_half < 0.01,
        "the second half should be near silence at -60 dB, peak is {second_half} \
         — the byte-copy path skipped the envelope"
    );

    // The first half is untouched by the curve at its start, so a fix
    // that simply silenced everything would not pass.
    let first_half = peak(&pcm[..half]);
    assert!(
        first_half > 0.4,
        "the first half starts at 0 dB and should be intact, peak is {first_half}"
    );
}

/// The fast path must still be taken when it is genuinely equivalent —
/// the fix is one more guard, not the removal of the optimisation.
#[test]
fn an_empty_envelope_still_renders_the_untouched_source() {
    let tmp = TempDir::new().expect("tempdir");
    let src = write_tone(tmp.path());
    let out = tmp.path().join("out.wav");
    let state = with_envelope(&src, Vec::new());
    render_state_to_wav(&state, &out, None).expect("render");

    let source_bytes = std::fs::read(&src).expect("read source");
    let out_bytes = std::fs::read(&out).expect("read out");
    assert_eq!(
        source_bytes, out_bytes,
        "an untouched single-track session should still byte-copy"
    );
}

/// A flat 0 dB envelope is not "no envelope": it is automation the user
/// drew, and it must go down the streaming path so that editing a point
/// later behaves consistently. The audio is equivalent, so this asserts
/// the samples rather than the bytes.
#[test]
fn a_flat_envelope_renders_equivalent_audio() {
    let flat = render(vec![
        EnvelopePoint {
            time_samples: 0,
            gain_db: 0.0,
        },
        EnvelopePoint {
            time_samples: FRAMES as u64,
            gain_db: 0.0,
        },
    ]);
    let none = render(Vec::new());

    assert_eq!(flat.len(), none.len());
    let worst = flat
        .iter()
        .zip(&none)
        .map(|(a, b)| (*a as i32 - *b as i32).abs())
        .max()
        .unwrap_or(0);
    assert!(
        worst <= 1,
        "a 0 dB envelope should not change the audio; worst sample delta {worst}"
    );
}
