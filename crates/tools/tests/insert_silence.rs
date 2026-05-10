use tools::tool::insert_silence::{apply_insert_silence, InsertSilenceError};

#[test]
fn insert_silence_extends_buffer_length() {
    let sr = 48_000u32;
    let mut samples = vec![1.0f32; 96_000];
    apply_insert_silence(&mut samples, sr, 0.0, 2.0).unwrap();
    assert_eq!(samples.len(), 96_000 + 2 * sr as usize);
}

#[test]
fn insert_silence_at_zero_prepends() {
    let mut samples = vec![1.0f32, 1.0];
    apply_insert_silence(&mut samples, 1, 0.0, 1.0).unwrap();
    assert_eq!(samples, vec![0.0, 1.0, 1.0]);
}

#[test]
fn insert_silence_negative_duration_rejected() {
    let mut samples = vec![1.0f32; 4];
    let err = apply_insert_silence(&mut samples, 1, 0.0, -1.0).unwrap_err();
    assert!(matches!(err, InsertSilenceError::NegativeDuration(_)));
}

#[test]
fn insert_silence_negative_offset_rejected() {
    let mut samples = vec![1.0f32; 4];
    let err = apply_insert_silence(&mut samples, 1, -0.5, 1.0).unwrap_err();
    assert!(matches!(err, InsertSilenceError::NegativeOffset(_)));
}

#[test]
fn insert_silence_offset_past_end_appends() {
    // at_sec is clamped to samples.len() so silence is appended.
    let mut samples = vec![1.0f32, 2.0];
    apply_insert_silence(&mut samples, 1, 100.0, 2.0).unwrap();
    assert_eq!(samples, vec![1.0, 2.0, 0.0, 0.0]);
}
