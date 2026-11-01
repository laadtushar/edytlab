//! The raw-podcast fixture the pipeline tests start from.
//!
//! A 15 s, 44.1 kHz mono 16-bit WAV with 2 s of silence followed by 13 s of
//! a 440 Hz sine at -6 dBFS, generated into the test's own temp dir so the
//! binary fixture never lands in git. Both the deterministic test and the
//! AI-driven one assert against the same numbers, so they live here.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub const SAMPLE_RATE: u32 = 44_100;
pub const TOTAL_SECONDS: u32 = 15;
pub const SILENCE_SECONDS: u32 = 2;
pub const SINE_HZ: f32 = 440.0;
/// -6 dBFS in linear amplitude. The exact target the fixture is meant
/// to hit; the tests assert against this in the pre-normalize sanity check.
pub const SINE_AMPLITUDE: f32 = 0.501_187_2; // 10^(-6/20)

/// Write the fixture WAV. Returns the path and the precomputed silence
/// length in frames so callers can pass it straight to `cut_range`.
pub fn write_fixture(dir: &Path) -> (PathBuf, u64) {
    let path = dir.join("raw_podcast_intro.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("create wav");
    let total_frames = SAMPLE_RATE * TOTAL_SECONDS;
    let silence_frames = SAMPLE_RATE * SILENCE_SECONDS;
    for i in 0..total_frames {
        let s = if i < silence_frames {
            0.0
        } else {
            let t = (i - silence_frames) as f32 / SAMPLE_RATE as f32;
            (t * SINE_HZ * std::f32::consts::TAU).sin() * SINE_AMPLITUDE
        };
        // Symmetric round-to-i16. The simple `* MAX as f32 as i16` cast
        // would saturate above +1.0 and we don't want fixture jitter
        // muddying the peak assertion.
        let q = (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32);
        writer.write_sample(q as i16).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
    (path, silence_frames as u64)
}
