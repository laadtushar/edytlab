//! Integration tests for `audio-io`.
//!
//! These tests open the host's default output device, so they only run on
//! platforms where cpal can find one (macOS, Windows). On headless CI Linux
//! runners without ALSA configured they will be skipped via
//! `#[cfg(not(target_os = "linux"))]`. CI runs the matrix on macOS + Windows
//! per the Phase 1 plan.

#![cfg(not(target_os = "linux"))]

use std::f32::consts::TAU;
use std::thread;
use std::time::Duration;

use audio_io::{default_output, Result};

const CHANNELS: u16 = 2;
const SAMPLE_RATE: u32 = 48_000;
const FREQ: f32 = 440.0;

fn sine_buffer(sr: u32, channels: u16, seconds: f32) -> Vec<f32> {
    let frames = (sr as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * channels as usize);
    for f in 0..frames {
        let t = f as f32 / sr as f32;
        let s = (TAU * FREQ * t).sin() * 0.2;
        for _ in 0..channels {
            out.push(s);
        }
    }
    out
}

#[test]
fn plays_one_second_sine_and_advances_played_counter() -> Result<()> {
    let mut stream = default_output(SAMPLE_RATE, CHANNELS)?;
    let buf = sine_buffer(SAMPLE_RATE, CHANNELS, 1.0);
    stream.write_samples(&buf)?;
    stream.play()?;

    thread::sleep(Duration::from_millis(1100));

    let played = stream.samples_played();
    let expected = SAMPLE_RATE as u64;
    let low = (expected as f64 * 0.95) as u64;
    let high = (expected as f64 * 1.05) as u64;
    assert!(
        played >= low && played <= high,
        "samples_played={played} not within 5% of {expected}"
    );
    Ok(())
}

#[test]
fn opens_at_44100_even_if_device_runs_at_a_different_rate() -> Result<()> {
    // Exercises the rubato resampling path: we ask for 44.1 kHz and let the
    // crate insert a resampler if the device runs at 48 kHz (typical on
    // modern macOS / Windows).
    let mut stream = default_output(44_100, CHANNELS)?;
    let buf = sine_buffer(44_100, CHANNELS, 0.5);
    stream.write_samples(&buf)?;
    stream.play()?;

    thread::sleep(Duration::from_millis(700));

    // The device-side counter ticks at the device's sample rate, which we
    // don't expose here. Just assert the callback ran (counter advanced) —
    // the exact rate is verified in unit tests.
    assert!(stream.samples_played() > 0);
    Ok(())
}

#[test]
fn underrun_writes_silence_without_panicking() -> Result<()> {
    let mut stream = default_output(SAMPLE_RATE, CHANNELS)?;
    stream.play()?;

    // Starve the stream: don't write anything for >50 ms, then check the
    // played counter has advanced — meaning the audio callback wrote silence
    // rather than crashing or stalling.
    thread::sleep(Duration::from_millis(100));
    let mid = stream.samples_played();
    assert!(mid > 0, "expected silence frames to be counted on underrun");

    thread::sleep(Duration::from_millis(100));
    let later = stream.samples_played();
    assert!(
        later > mid,
        "samples_played stalled across underrun: {mid} -> {later}"
    );

    // Recovery: writing real audio after starvation should still work.
    let buf = sine_buffer(SAMPLE_RATE, CHANNELS, 0.2);
    stream.write_samples(&buf)?;
    thread::sleep(Duration::from_millis(250));
    assert!(stream.samples_played() > later);
    Ok(())
}
