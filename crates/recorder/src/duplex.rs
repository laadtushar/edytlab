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
//! - a stream error callback (a device that went away) sets a flag, and
//!   afterwards the callbacks' timing is checked for lost frames
//!   ([`crate::latency::Envelope::lost_frames`]); either sets
//!   [`Capture::xrun`] and the caller refuses the take, it never splices
//!   around a gap.
//!
//! cpal's error callback is not a glitch detector: ALSA recovers an
//! overrun or underrun itself and calls back as if nothing happened, and
//! CoreAudio and WASAPI report none. The timing check is what catches
//! those, and it sees a loss of more than about one buffer plus 20 ms, not
//! a smaller one; a calibration whose sweeps then disagree is refused for
//! that reason, and a take is only as good as the quiet machine it ran on.
//!
//! The streams are built, run and dropped on the calling thread
//! (`cpal::Stream` is not `Send`), so call this from a blocking task.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::identify::{identify, pactl, Role};
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
    /// A stream reported an error while running, or its callbacks' timing
    /// shows frames were lost.
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

/// One side of a pair, as cpal reports it.
pub(crate) struct Side<'a> {
    pub name: &'a str,
    pub sr: u32,
    pub ch: u16,
}

/// A pair from what cpal reported, with `default`, `pulse` and `pipewire`
/// replaced by the sound server's own name for the default
/// ([`crate::identify`]). A pair that cannot be named is an error, so it
/// is never filed under a name that fits another pair too.
pub(crate) fn pair_from(
    host: &str,
    out: Side<'_>,
    inp: Side<'_>,
    ask: &dyn Fn(&[&str]) -> Result<String, String>,
) -> Result<DevicePair, RecorderError> {
    Ok(DevicePair {
        host: host.to_string(),
        output: identify(host, out.name, Role::Output, ask)?,
        input: identify(host, inp.name, Role::Input, ask)?,
        out_sr: out.sr,
        out_ch: out.ch,
        in_sr: inp.sr,
        in_ch: inp.ch,
    })
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
    let out_name = out.name().unwrap_or_else(|_| "unknown output".into());
    let in_name = inp.name().unwrap_or_else(|_| "unknown input".into());
    pair_from(
        host.id().name(),
        Side {
            name: &out_name,
            sr: out_cfg.sample_rate().0,
            ch: out_cfg.channels(),
        },
        Side {
            name: &in_name,
            sr: in_cfg.sample_rate().0,
            ch: in_cfg.channels(),
        },
        &pactl,
    )
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
        xrun: xrun.load(Ordering::Relaxed) || in_env.lost_frames() || out_env.lost_frames(),
        cancelled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> DevicePair {
        DevicePair {
            host: "ALSA".into(),
            output: "sysdefault:CARD=PCH".into(),
            input: "sysdefault:CARD=USB".into(),
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
            "ALSA|sysdefault:CARD=PCH|sysdefault:CARD=USB|48000|44100"
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

    const PCI_SINK: &str = "alsa_output.pci-0000_00_1f.3.analog-stereo";
    const BT_SINK: &str = "bluez_output.AA_BB_CC_DD_EE_FF.1";
    const SOURCE: &str = "alsa_input.pci-0000_00_1f.3.analog-stereo";

    /// What cpal's ALSA host reports for the defaults: both are `default`.
    fn alsa_defaults(sink: &'static str) -> Result<DevicePair, RecorderError> {
        pair_from(
            "ALSA",
            Side {
                name: "default",
                sr: 48_000,
                ch: 2,
            },
            Side {
                name: "default",
                sr: 48_000,
                ch: 1,
            },
            &move |args| match args {
                ["get-default-sink"] => Ok(format!("{sink}\n")),
                ["get-default-source"] => Ok(format!("{SOURCE}\n")),
                other => panic!("unexpected question {other:?}"),
            },
        )
    }

    #[test]
    fn speakers_and_a_headset_that_alsa_both_calls_default_are_two_pairs() {
        let speakers = alsa_defaults(PCI_SINK).unwrap().key();
        let headset = alsa_defaults(BT_SINK).unwrap().key();
        assert_ne!(
            speakers, headset,
            "a headset must not reuse the speakers' latency"
        );
        assert_eq!(
            speakers,
            format!("ALSA|default ({PCI_SINK})|default ({SOURCE})|48000|48000")
        );
    }

    #[test]
    fn a_pair_that_cannot_be_named_is_refused_with_the_reason() {
        let e = pair_from(
            "ALSA",
            Side {
                name: "default",
                sr: 48_000,
                ch: 2,
            },
            Side {
                name: "default",
                sr: 48_000,
                ch: 1,
            },
            &|_| Err("pactl is not installed".into()),
        )
        .unwrap_err();
        assert!(e.to_string().contains("pactl is not installed"), "{e}");
    }

    #[test]
    fn a_pair_of_named_cards_needs_no_sound_server() {
        let p = pair_from(
            "ALSA",
            Side {
                name: "hw:CARD=PCH,DEV=0",
                sr: 48_000,
                ch: 2,
            },
            Side {
                name: "hw:CARD=USB,DEV=0",
                sr: 44_100,
                ch: 1,
            },
            &|_| panic!("nothing to look up"),
        )
        .unwrap();
        assert_eq!(p.output, "hw:CARD=PCH,DEV=0");
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
