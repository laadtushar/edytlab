//! Integration tests for the M20 audio-time stub.
//!
//! Until M28 wires up Rubber Band, the contract surface is:
//!
//! * Valid arguments → `Err(NotImplemented)`.
//! * Invalid factor / semitones → the matching `Invalid*` variant.
//! * Bad channel count → `ChannelMismatch`.
//!
//! The plan's quantitative acceptance criteria (frequency tolerance,
//! round-trip RMS, formant preservation) gate the M28 PR.

use audio_time::{pitch_shift, shift, time_stretch, Error};

#[test]
fn time_stretch_validates_factor() {
    let samples = [0.0f32; 1024];
    // Sentinel: valid args produce NotImplemented, not a crash.
    assert_eq!(
        time_stretch(&samples, 48_000, 1, 0.5, false),
        Err(Error::NotImplemented)
    );
    // factor = 0
    assert!(matches!(
        time_stretch(&samples, 48_000, 1, 0.0, false),
        Err(Error::InvalidFactor(_))
    ));
    // negative
    assert!(matches!(
        time_stretch(&samples, 48_000, 1, -1.0, false),
        Err(Error::InvalidFactor(_))
    ));
    // NaN
    assert!(matches!(
        time_stretch(&samples, 48_000, 1, f32::NAN, false),
        Err(Error::InvalidFactor(_))
    ));
    // +inf
    assert!(matches!(
        time_stretch(&samples, 48_000, 1, f32::INFINITY, false),
        Err(Error::InvalidFactor(_))
    ));
}

#[test]
fn pitch_shift_validates_semitones() {
    let samples = [0.0f32; 1024];
    // Valid args → NotImplemented.
    assert_eq!(
        pitch_shift(&samples, 48_000, 1, 12.0, false),
        Err(Error::NotImplemented)
    );
    // Outside ±48
    assert!(matches!(
        pitch_shift(&samples, 48_000, 1, 60.0, false),
        Err(Error::InvalidSemitones(_))
    ));
    assert!(matches!(
        pitch_shift(&samples, 48_000, 1, -60.0, false),
        Err(Error::InvalidSemitones(_))
    ));
    // NaN
    assert!(matches!(
        pitch_shift(&samples, 48_000, 1, f32::NAN, false),
        Err(Error::InvalidSemitones(_))
    ));
    // Boundary value (exactly MAX_SEMITONES) is accepted.
    assert_eq!(
        pitch_shift(&samples, 48_000, 1, shift::MAX_SEMITONES, false),
        Err(Error::NotImplemented)
    );
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

#[test]
fn preserve_formants_flag_is_recorded_not_rejected() {
    // Until M28 the flag is purely informational, but accepting
    // `preserve_formants = true` without changing the error variant is
    // a contract guarantee callers may rely on.
    let samples = [0.0f32; 1024];
    assert_eq!(
        time_stretch(&samples, 48_000, 1, 1.0, true),
        Err(Error::NotImplemented)
    );
    assert_eq!(
        pitch_shift(&samples, 48_000, 1, 0.0, true),
        Err(Error::NotImplemented)
    );
}
