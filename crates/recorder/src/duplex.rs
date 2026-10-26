//! Play one buffer and record the microphone at the same time, on the
//! process clock (#203).
//!
//! This is the native monitor of a punch-in and of a calibration: the
//! webview cannot do it, because its player is in another process with a
//! start delay that changes from run to run, and nothing ties its clock to
//! the capture. Here both streams are cpal's, in this process, and every
//! callback reads `Instant::now()` for [`crate::latency`]'s clock model.
//!
//! ## Callbacks
//!
//! Neither callback locks, allocates or logs. `Recorder`'s capture is a
//! `Mutex<Vec>` that grows; a callback that waits for a lock, or for an
//! allocation, is late, a late input callback drops frames, and a dropped
//! frame moves every frame after it. So:
//!
//! - the output callback copies from an immutable buffer through a cursor
//!   it owns, and plays silence past the end;
//! - the input callback stores into a buffer allocated before the stream
//!   starts, as relaxed atomic stores of the samples' bits (no `unsafe`,
//!   and a store is a plain move on every target we ship), and publishes
//!   how much is valid with one release store;
//! - anything wrong (a stream error, a device that went away) sets a flag
//!   the caller reads afterwards: [`Capture::xrun`]. The caller refuses
//!   the take; it never splices around a gap.
//!
//! The streams are built, run and dropped on the calling thread
//! (`cpal::Stream` is not `Send`), so call this from a blocking task.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::latency::Envelope;
use crate::RecorderError;

/// The default output and the default input, as they are right now: the
/// key a stored latency is filed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePair {
    pub host: String,
    pub output: String,
    pub input: String,
    pub out_sr: u32,
    pub out_ch: u16,
    pub in_sr: u32,
    pub in_ch: u16,
}

impl DevicePair {
    /// `host|output name|input name|out_sr|in_sr`. The rates are in it
    /// because buffer depth changes with them; the channel counts are not,
    /// because they do not move the delay.
    pub fn key(&self) -> String {
        let clean = |s: &str| s.replace('|', "/");
        format!(
            "{}|{}|{}|{}|{}",
            clean(&self.host),
            clean(&self.output),
            clean(&self.input),
            self.out_sr,
            self.in_sr
        )
    }
}

/// What play-and-capture recorded, and how its two streams sit in time.
#[derive(Debug, Clone)]
pub struct Capture {
    /// Interleaved, `in_ch` channels.
    pub samples: Vec<f32>,
    pub in_sr: u32,
    pub in_ch: u16,
    /// The output stream's origin on the process clock, `P0`.
    pub out_origin_ns: Option<i64>,
    /// The input stream's origin on the process clock, `C0`.
    pub in_origin_ns: Option<i64>,
    /// A stream reported an error while running.
    pub xrun: bool,
    /// The caller's cancel fired before the capture was full.
    pub cancelled: bool,
}

impl Capture {
    /// `P0 - C0`, the argument [`crate::latency::input_frame_for`] takes.
    /// `None` when either stream never got past its warm-up.
    pub fn skew_ns(&self) -> Option<i64> {
        Some(self.out_origin_ns? - self.in_origin_ns?)
    }
}

/// The default devices and their configurations.
pub fn default_pair() -> Result<DevicePair, RecorderError> {
    let host = cpal::default_host();
    let out = host
        .default_output_device()
        .ok_or(RecorderError::NoOutputDevice)?;
    let inp = host.default_input_device().ok_or(RecorderError::NoDevice)?;
    let out_cfg = out
        .default_output_config()
        .map_err(|e| RecorderError::Stream(e.to_string()))?;
    let in_cfg = inp
        .default_input_config()
        .map_err(|e| RecorderError::Stream(e.to_string()))?;
    Ok(DevicePair {
        host: host.id().name().to_string(),
        output: out.name().unwrap_or_else(|_| "unknown output".into()),
        input: inp.name().unwrap_or_else(|_| "unknown input".into()),
        out_sr: out_cfg.sample_rate().0,
        out_ch: out_cfg.channels(),
        in_sr: in_cfg.sample_rate().0,
        in_ch: in_cfg.channels(),
    })
}

/// How long to wait for the first input callback before giving up on a
/// device that opened but is not delivering.
const FIRST_CALLBACK_TIMEOUT: Duration = Duration::from_secs(3);
/// Slack past the capture's own length for a slow device to catch up.
const FINISH_SLACK: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(10);

/// Play `monitor` (interleaved, `pair.out_ch` channels at `pair.out_sr`)
/// from the default output while recording `capture_frames` frames from the
/// default input.
///
/// `pair` is what the caller prepared the monitor for; if a device or its
/// format has changed since, this refuses rather than play something at the
/// wrong rate. `cancel` is polled about every 10 ms.
pub fn play_and_capture(
    monitor: &[f32],
    pair: &DevicePair,
    capture_frames: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Capture, RecorderError> {
    let now = default_pair()?;
    if &now != pair {
        return Err(RecorderError::DeviceChanged);
    }
    let host = cpal::default_host();
    let out_dev = host
        .default_output_device()
        .ok_or(RecorderError::NoOutputDevice)?;
    let in_dev = host.default_input_device().ok_or(RecorderError::NoDevice)?;
    let out_cfg: cpal::StreamConfig = out_dev
        .default_output_config()
        .map_err(|e| RecorderError::Stream(e.to_string()))?
        .into();
    let in_cfg: cpal::StreamConfig = in_dev
        .default_input_config()
        .map_err(|e| RecorderError::Stream(e.to_string()))?
        .into();

    let out_ch = pair.out_ch as usize;
    let in_ch = pair.in_ch as usize;
    let capacity = capture_frames * in_ch;

    let epoch = Instant::now();
    let ns = move || epoch.elapsed().as_nanos() as i64;

    let store: Arc<Vec<AtomicU32>> = Arc::new((0..capacity).map(|_| AtomicU32::new(0)).collect());
    let filled = Arc::new(AtomicUsize::new(0));
    let xrun = Arc::new(AtomicBool::new(false));
    let out_env = Arc::new(Envelope::new(pair.out_sr));
    let in_env = Arc::new(Envelope::new(pair.in_sr));
    let monitor: Arc<[f32]> = monitor.into();

    let output = {
        let (monitor, env, xrun) = (
            Arc::clone(&monitor),
            Arc::clone(&out_env),
            Arc::clone(&xrun),
        );
        let mut cursor = 0usize; // in frames
        out_dev
            .build_output_stream(
                &out_cfg,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    env.observe(ns(), cursor as u64);
                    let from = (cursor * out_ch).min(monitor.len());
                    let n = data.len().min(monitor.len() - from);
                    data[..n].copy_from_slice(&monitor[from..from + n]);
                    data[n..].fill(0.0);
                    cursor += data.len() / out_ch;
                },
                move |_| xrun.store(true, Ordering::Relaxed),
                None,
            )
            .map_err(|e| RecorderError::Stream(e.to_string()))?
    };

    let input = {
        let (store, filled, env, xrun) = (
            Arc::clone(&store),
            Arc::clone(&filled),
            Arc::clone(&in_env),
            Arc::clone(&xrun),
        );
        let mut seen = 0usize; // samples delivered, valid or not
        in_dev
            .build_input_stream(
                &in_cfg,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    seen += data.len();
                    env.observe(ns(), (seen / in_ch) as u64);
                    let have = filled.load(Ordering::Relaxed);
                    let n = data.len().min(capacity - have);
                    for (slot, v) in store[have..have + n].iter().zip(data) {
                        slot.store(v.to_bits(), Ordering::Relaxed);
                    }
                    filled.store(have + n, Ordering::Release);
                },
                move |_| xrun.store(true, Ordering::Relaxed),
                None,
            )
            .map_err(|e| RecorderError::Stream(e.to_string()))?
    };

    // The microphone first, so the capture is running before the first
    // monitor frame can be heard.
    input
        .play()
        .map_err(|e| RecorderError::Stream(e.to_string()))?;
    output
        .play()
        .map_err(|e| RecorderError::Stream(e.to_string()))?;

    let started = Instant::now();
    let deadline =
        started + Duration::from_secs_f64(capture_frames as f64 / pair.in_sr as f64) + FINISH_SLACK;
    let mut cancelled = false;
    let mut stalled = false;
    loop {
        if filled.load(Ordering::Acquire) >= capacity {
            break;
        }
        if cancel() {
            cancelled = true;
            break;
        }
        let now = Instant::now();
        if now >= deadline
            || (filled.load(Ordering::Relaxed) == 0 && now - started > FIRST_CALLBACK_TIMEOUT)
        {
            stalled = true;
            break;
        }
        std::thread::sleep(POLL);
    }

    // Dropping a stream stops it and waits for its callback to finish, so
    // everything below is read with no callback running.
    drop(input);
    drop(output);

    if stalled {
        return Err(RecorderError::Stream(
            "the microphone stopped delivering audio".into(),
        ));
    }
    let valid = filled.load(Ordering::Acquire) / in_ch * in_ch;
    let samples = store[..valid]
        .iter()
        .map(|b| f32::from_bits(b.load(Ordering::Relaxed)))
        .collect();
    Ok(Capture {
        samples,
        in_sr: pair.in_sr,
        in_ch: pair.in_ch,
        out_origin_ns: out_env.origin_ns(),
        in_origin_ns: in_env.origin_ns(),
        xrun: xrun.load(Ordering::Relaxed),
        cancelled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> DevicePair {
        DevicePair {
            host: "ALSA".into(),
            output: "Speakers (Realtek)".into(),
            input: "Microphone (USB)".into(),
            out_sr: 48_000,
            out_ch: 2,
            in_sr: 44_100,
            in_ch: 1,
        }
    }

    #[test]
    fn the_key_names_the_pair_and_both_rates() {
        assert_eq!(
            pair().key(),
            "ALSA|Speakers (Realtek)|Microphone (USB)|48000|44100"
        );
    }

    #[test]
    fn a_different_rate_is_a_different_key_but_a_different_channel_count_is_not() {
        let a = pair();
        let mut b = pair();
        b.in_sr = 48_000;
        assert_ne!(a.key(), b.key());
        let mut c = pair();
        c.in_ch = 2;
        c.out_ch = 6;
        assert_eq!(a.key(), c.key());
    }

    #[test]
    fn a_separator_in_a_device_name_cannot_split_the_key() {
        let mut p = pair();
        p.output = "Out | Left".into();
        assert_eq!(p.key().split('|').count(), 5);
    }

    #[test]
    fn skew_is_the_output_origin_less_the_input_origin() {
        let c = Capture {
            samples: vec![],
            in_sr: 48_000,
            in_ch: 1,
            out_origin_ns: Some(5_000_000),
            in_origin_ns: Some(2_000_000),
            xrun: false,
            cancelled: false,
        };
        assert_eq!(c.skew_ns(), Some(3_000_000));
        let none = Capture {
            in_origin_ns: None,
            ..c
        };
        assert_eq!(none.skew_ns(), None);
    }
}
