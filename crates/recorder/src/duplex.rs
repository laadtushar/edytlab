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
//! cpal's error callback is not a glitch detector on its own. Since cpal
//! 0.17 ALSA reports the overruns and underruns it recovers from
//! (`ErrorKind::Xrun`), and since 0.18.2 CoreAudio (both directions) and
//! WASAPI capture do too, but a sound server behind ALSA's `default` hides its own, and
//! WASAPI playback reports none. The timing check is what catches those,
//! and it sees a loss of more than about one buffer plus 20 ms, not a
//! smaller one; a calibration whose sweeps then disagree is refused for
//! that reason, and a take is only as good as the quiet machine it ran on.
//! Any error the callback is handed counts: this crate does not enable
//! cpal's `realtime` features, the only source of the informational
//! `ErrorKind::RealtimeDenied`.
//!
//! The streams are built, run and dropped on the calling thread, so call
//! this from a blocking task. (`cpal::Stream` was not `Send` before cpal
//! 0.17; it is now, but nothing here needs to move one.)

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

/// Which way this crate's streams are set up, as part of every stored
/// latency's key. The latency is mostly buffers, so a latency measured
/// with streams buffered one way is wrong for streams buffered another,
/// on the same devices at the same rates. Bump it when what the streams
/// are buffered by changes: cpal's defaults, or a `BufferSize` asked for
/// here. A key without it (five fields) is from a build before it, which
/// is never read as this one's.
///
/// `cpal18`: cpal 0.17 stopped imposing its own ALSA buffering (25 ms
/// periods in a 100 ms buffer), and the streams now ask for 50 ms periods
/// in a 100 ms buffer ([`crate::stream_config`]); on the loopback's private
/// PulseAudio daemon the measured latency went from 75.6 ms (cpal 0.15)
/// to 33.4 ms. A calibration from cpal 0.15 applied now would put a take
/// about 42 ms early there, so it reads as not measured and is made again.
/// The other hosts' streams changed under the same versions too: WASAPI
/// opens default devices through the audio-interface activation path
/// instead of `IMMDevice::Activate`, and CoreAudio sets the device's
/// physical format and calls back with fixed-size buffers, so the field
/// applies on every host, not only ALSA.
const STREAM_SETUP: &str = "cpal18";

impl DevicePair {
    /// `host|output name|input name|out_sr|in_sr|stream setup`. The rates
    /// are in it because buffer depth changes with them, and the stream
    /// setup ([`STREAM_SETUP`]) because it changes the buffers too; the
    /// channel counts are not, because they do not move the delay.
    pub fn key(&self) -> String {
        let clean = |s: &str| s.replace('|', "/");
        format!(
            "{}|{}|{}|{}|{}|{}",
            clean(&self.host),
            clean(&self.output),
            clean(&self.input),
            self.out_sr,
            self.in_sr,
            STREAM_SETUP
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

/// The name a device is filed under: what cpal 0.15's `Device::name()`
/// answered, which every stored latency's key holds. cpal 0.18 removed
/// `name()`. On ALSA its replacement, `description().name()`, is the
/// hint's text for people (`Default Audio Device` for `default`, `PulseAudio
/// Sound Server` for `pulse`), not the PCM name [`crate::identify`] reads,
/// so there the name is the device id's PCM name (`default`, `pulse`,
/// `hw:CARD=PCH,DEV=0`), as `name()` was. On CoreAudio and WASAPI
/// `description().name()` reads the property `name()` read (the device
/// name; the endpoint's FriendlyName).
fn device_name(host: cpal::HostId, dev: &cpal::Device) -> Option<String> {
    if host.name() == crate::identify::ALSA {
        dev.id().ok().map(|id| id.id().to_string())
    } else {
        dev.description().ok().map(|d| d.name().to_string())
    }
}

/// The default devices and their configurations.
pub fn default_pair() -> Result<DevicePair, RecorderError> {
    let host = crate::host()?;
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
    let out_name = device_name(host.id(), &out).unwrap_or_else(|| "unknown output".into());
    let in_name = device_name(host.id(), &inp).unwrap_or_else(|| "unknown input".into());
    pair_from(
        host.id().name(),
        Side {
            name: &out_name,
            sr: out_cfg.sample_rate(),
            ch: out_cfg.channels(),
        },
        Side {
            name: &in_name,
            sr: in_cfg.sample_rate(),
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
    let host = crate::host()?;
    let out_dev = host
        .default_output_device()
        .ok_or(RecorderError::NoOutputDevice)?;
    let in_dev = host.default_input_device().ok_or(RecorderError::NoDevice)?;
    let out_cfg = crate::stream_config(
        &out_dev
            .default_output_config()
            .map_err(|e| RecorderError::Stream(e.to_string()))?,
    );
    let in_cfg = crate::stream_config(
        &in_dev
            .default_input_config()
            .map_err(|e| RecorderError::Stream(e.to_string()))?,
    );

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
                out_cfg,
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
                in_cfg,
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
            "ALSA|sysdefault:CARD=PCH|sysdefault:CARD=USB|48000|44100|cpal18"
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
        assert_eq!(p.key().split('|').count(), 6);
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
            format!("ALSA|default ({PCI_SINK})|default ({SOURCE})|48000|48000|cpal18")
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

    /// The names every stored latency is filed under on Linux: cpal's ALSA
    /// defaults must still read as `default`, which [`crate::identify`]
    /// replaces with the sound server's sink and source. cpal 0.18's
    /// `description().name()` would read `Default Audio Device` here and
    /// orphan every stored calibration. Opens no device.
    #[cfg(target_os = "linux")]
    #[test]
    fn alsa_defaults_are_filed_under_the_pcm_name_default() {
        let host = cpal::default_host();
        assert_eq!(host.id().name(), crate::identify::ALSA);
        let out = host
            .default_output_device()
            .expect("ALSA always has a default");
        let inp = host
            .default_input_device()
            .expect("ALSA always has a default");
        assert_eq!(device_name(host.id(), &out).as_deref(), Some("default"));
        assert_eq!(device_name(host.id(), &inp).as_deref(), Some("default"));
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
