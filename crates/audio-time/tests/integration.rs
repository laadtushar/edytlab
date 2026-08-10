//! Contract tests for the time-stretch / pitch-shift primitives.
//!
//! These used to assert that valid arguments produced
//! `Err(NotImplemented)` — the stub's contract, and the reason the tools
//! above this crate spent so long reporting a change the audio never
//! received. Valid arguments now produce audio, so the argument
//! validation is checked alongside what the functions actually return.
//!
//! Signal-quality assertions (frequency accuracy, length exactness) live
//! next to the implementation in `src/`, where a failure points at the
//! line responsible.

use audio_time::{pitch_shift, shift, time_stretch, Error};

/// One second of a 440 Hz tone at 48 kHz.
fn tone() -> Vec<f32> {
    (0..48_000)
        .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 48_000.0).sin() * 0.5)
        .collect()
}

#[test]
fn time_stretch_validates_factor() {
    let samples = tone();

    // Valid arguments produce audio of the promised length.
    let out = time_stretch(&samples, 48_000, 1, 0.5, false).expect("valid factor");
    assert_eq!(out.len(), samples.len() * 2, "factor 0.5 is twice as long");

    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            matches!(
                time_stretch(&samples, 48_000, 1, bad, false),
                Err(Error::InvalidFactor(_))
            ),
            "factor {bad} should be rejected"
        );
    }
}

#[test]
fn pitch_shift_validates_semitones() {
    let samples = tone();

    let out = pitch_shift(&samples, 48_000, 1, 12.0, false).expect("valid semitones");
    assert_eq!(out.len(), samples.len(), "pitch shift preserves duration");

    for bad in [60.0, -60.0, f32::NAN] {
        assert!(
            matches!(
                pitch_shift(&samples, 48_000, 1, bad, false),
                Err(Error::InvalidSemitones(_))
            ),
            "{bad} semitones should be rejected"
        );
    }

    // Exactly at the boundary is accepted.
    assert!(pitch_shift(&samples, 48_000, 1, shift::MAX_SEMITONES, false).is_ok());
    assert!(pitch_shift(&samples, 48_000, 1, -shift::MAX_SEMITONES, false).is_ok());
}

#[test]
fn channel_mismatch_surfaces_distinct_error() {
    // 2 channels, 5 samples → not divisible.
    assert!(matches!(
        time_stretch(&[0.0; 5], 48_000, 2, 1.0, false),
        Err(Error::ChannelMismatch(_))
    ));
    // 0 channels.
    assert!(matches!(
        pitch_shift(&[0.0; 4], 48_000, 0, 0.0, false),
        Err(Error::ChannelMismatch(_))
    ));
}

/// `preserve_formants` is accepted and currently ignored.
///
/// Ignored is not the same as rejected: the flag is part of the tool
/// schema, so passing it must succeed and return the same audio as
/// passing `false`. When formant preservation lands, this test is the
/// one that should start failing.
#[test]
fn preserve_formants_is_accepted_and_currently_ignored() {
    let samples = tone();

    let with = time_stretch(&samples, 48_000, 1, 1.5, true).expect("flag accepted");
    let without = time_stretch(&samples, 48_000, 1, 1.5, false).expect("flag accepted");
    assert_eq!(
        with, without,
        "the flag is documented as ignored; if this diverges, the docs and \
         the tool description need updating with it"
    );

    assert!(pitch_shift(&samples, 48_000, 1, 3.0, true).is_ok());
}
