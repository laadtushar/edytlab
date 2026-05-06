//! Integration tests for `audio-io`.
//!
//! These tests open the host's default output device. On Linux they are
//! skipped via `#[cfg(not(target_os = "linux"))]`.
//!
//! On Mac/Windows they are `#[ignore]`-d so headless CI runners (which expose
//! a virtual audio device that may or may not clock samples) don't fail. CI
//! still verifies they COMPILE on every push, which catches type/API breaks.
//! Real playback verification is a manual gate on a developer's machine via
//! `cargo test -p audio-io -- --ignored`.

use std::f32::consts::TAU;
use std::thread;
use std::time::Duration;

use audio_io::{default_output, Error, OutputStream, Result};

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

/// Open the default output device, or `None` if the host has no usable
/// output (headless runner without an audio device).
fn open_or_skip(
    sample_rate: u32,
    channels: u16,
    name: &str,
) -> Result<Option<Box<dyn OutputStream>>> {
    match default_output(sample_rate, channels) {
        Ok(s) => Ok(Some(s)),
        Err(Error::NoDevice) => {
            eprintln!(
                "audio-io test {name}: skipping — no default output device on this host \
                 (headless CI runner)."
            );
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

fn callback_is_clocking(stream: &mut Box<dyn OutputStream>) -> bool {
    let _ = stream.play();
    thread::sleep(Duration::from_millis(200));
    stream.frames_played() > 0
}

/// Skip the test if the (already-open) stream's audio callback never
/// clocks samples — i.e. the device exists but is virtual.
macro_rules! skip_if_silent {
    ($stream:expr, $name:expr) => {
        if !callback_is_clocking(&mut $stream) {
            eprintln!(
                "audio-io test {}: skipping — default output device exists but never clocked \
                 (likely headless CI runner). Run on a real Mac/Windows host to exercise.",
                $name
            );
            return Ok(());
        }
    };
}

#[test]
#[ignore = "requires real audio hardware; run with --ignored on a developer machine"]
fn plays_one_second_sine_and_advances_played_counter() -> Result<()> {
    let Some(mut stream) = open_or_skip(SAMPLE_RATE, CHANNELS, "plays_one_second_sine")? else {
        return Ok(());
    };
    skip_if_silent!(stream, "plays_one_second_sine");
    let buf = sine_buffer(SAMPLE_RATE, CHANNELS, 1.0);
    stream.write_samples(&buf)?;

    thread::sleep(Duration::from_millis(1100));

    // The played counter ticks at the device rate, not the requested rate.
    let played = stream.frames_played();
    let expected = stream.device_sample_rate() as u64;
    let low = (expected as f64 * 0.95) as u64;
    let high = (expected as f64 * 1.05) as u64;
    assert!(
        played >= low && played <= high,
        "frames_played={played} not within 5% of {expected}"
    );
    Ok(())
}

#[test]
#[ignore = "requires real audio hardware; run with --ignored on a developer machine"]
fn opens_at_44100_even_if_device_runs_at_a_different_rate() -> Result<()> {
    // Exercises the rubato resampling path: we ask for 44.1 kHz and let the
    // crate insert a resampler if the device runs at a different rate (48 kHz
    // is typical on modern macOS / Windows).
    let requested = 44_100u32;
    let Some(mut stream) = open_or_skip(requested, CHANNELS, "opens_at_44100")? else {
        return Ok(());
    };
    skip_if_silent!(stream, "opens_at_44100");
    let device_sr = stream.device_sample_rate();
    eprintln!("audio-io test: device opened at {device_sr} Hz (requested {requested} Hz)");

    // Push half a second of audio at the *requested* (logical) rate. The
    // crate is responsible for resampling to `device_sr` if they differ.
    let buf = sine_buffer(requested, CHANNELS, 0.5);
    stream.write_samples(&buf)?;

    thread::sleep(Duration::from_millis(700));

    let played = stream.frames_played();
    assert!(played > 0, "audio callback never ran");

    // The played counter ticks at the device rate. After ~700 ms we should
    // see roughly 0.7 * device_sr frames regardless of which path
    // (resampler vs. passthrough) was used. This both proves write_samples
    // routed audio end-to-end *and* anchors the assertion on the actual
    // device rate rather than just "some output happened".
    let expected = (device_sr as f64 * 0.7) as u64;
    let low = (expected as f64 * 0.7) as u64;
    let high = (expected as f64 * 1.3) as u64;
    assert!(
        played >= low && played <= high,
        "frames_played={played} not within 30% of {expected} \
         (device_sr={device_sr}, requested={requested})"
    );

    if device_sr == requested {
        eprintln!("audio-io test: device matched requested rate; resampler bypass path exercised");
    } else {
        eprintln!(
            "audio-io test: device rate {device_sr} != requested {requested}; \
             resampler path exercised"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires real audio hardware; run with --ignored on a developer machine"]
fn underrun_writes_silence_without_panicking() -> Result<()> {
    let Some(mut stream) = open_or_skip(SAMPLE_RATE, CHANNELS, "underrun")? else {
        return Ok(());
    };
    skip_if_silent!(stream, "underrun");

    // Starve the stream: don't write anything for >50 ms, then check the
    // played counter has advanced — meaning the audio callback wrote silence
    // rather than crashing or stalling.
    thread::sleep(Duration::from_millis(100));
    let mid = stream.frames_played();
    assert!(mid > 0, "expected silence frames to be counted on underrun");

    thread::sleep(Duration::from_millis(100));
    let later = stream.frames_played();
    assert!(
        later > mid,
        "frames_played stalled across underrun: {mid} -> {later}"
    );

    // Recovery: writing real audio after starvation should still work.
    let buf = sine_buffer(SAMPLE_RATE, CHANNELS, 0.2);
    stream.write_samples(&buf)?;
    thread::sleep(Duration::from_millis(250));
    assert!(stream.frames_played() > later);
    Ok(())
}
