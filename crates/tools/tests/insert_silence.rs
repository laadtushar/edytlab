use tools::tool::insert_silence::{apply_insert_silence, InsertSilenceError};

#[test]
fn insert_silence_extends_buffer_length() {
    let sr = 48_000u32;
    let mut samples = vec![1.0f32; 96_000];
    apply_insert_silence(&mut samples, sr, 1, 0.0, 2.0).unwrap();
    assert_eq!(samples.len(), 96_000 + 2 * sr as usize);
}

#[test]
fn insert_silence_at_zero_prepends() {
    let mut samples = vec![1.0f32, 1.0];
    apply_insert_silence(&mut samples, 1, 1, 0.0, 1.0).unwrap();
    assert_eq!(samples, vec![0.0, 1.0, 1.0]);
}

#[test]
fn insert_silence_negative_duration_rejected() {
    let mut samples = vec![1.0f32; 4];
    let err = apply_insert_silence(&mut samples, 1, 1, 0.0, -1.0).unwrap_err();
    assert!(matches!(err, InsertSilenceError::NegativeDuration(_)));
}

#[test]
fn insert_silence_negative_offset_rejected() {
    let mut samples = vec![1.0f32; 4];
    let err = apply_insert_silence(&mut samples, 1, 1, -0.5, 1.0).unwrap_err();
    assert!(matches!(err, InsertSilenceError::NegativeOffset(_)));
}

#[test]
fn insert_silence_offset_past_end_appends() {
    // at_sec is clamped to samples.len() so silence is appended.
    let mut samples = vec![1.0f32, 2.0];
    apply_insert_silence(&mut samples, 1, 1, 100.0, 2.0).unwrap();
    assert_eq!(samples, vec![1.0, 2.0, 0.0, 0.0]);
}

/// Silence spliced into stereo must land on a frame boundary and be a
/// whole number of frames long.
///
/// The position and length were previously counted in samples, so on
/// stereo the silence landed at half the requested time and ran half as
/// long. Worse, an odd sample count shifts every frame after the splice
/// by one — swapping left and right for the entire rest of the track.
#[test]
fn insert_silence_into_stereo_keeps_channels_paired() {
    // 4 stereo frames at sr=1. Left counts up, right is its negative.
    let mut samples = vec![1.0f32, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0];
    apply_insert_silence(&mut samples, 1, 2, 2.0, 1.0).unwrap();

    assert_eq!(
        samples,
        vec![1.0, -1.0, 2.0, -2.0, 0.0, 0.0, 3.0, -3.0, 4.0, -4.0],
        "one frame of silence at frame 2, channels still paired"
    );
}

/// A duration that is not a whole number of frames must still not
/// desynchronise the interleaving — the length is rounded in frames,
/// so the buffer stays a multiple of the channel count.
#[test]
fn insert_silence_stereo_length_stays_a_multiple_of_channels() {
    let mut samples = vec![1.0f32, -1.0, 2.0, -2.0];
    // 0.5 frames at sr=1 truncates to 0 frames — never to an odd
    // number of interleaved samples.
    apply_insert_silence(&mut samples, 1, 2, 0.0, 0.5).unwrap();
    assert_eq!(
        samples.len() % 2,
        0,
        "an odd sample count would swap L/R for the rest of the track"
    );
    assert_eq!(samples, vec![1.0, -1.0, 2.0, -2.0]);
}
