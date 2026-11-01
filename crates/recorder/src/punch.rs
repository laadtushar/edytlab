//! The native monitor of a punch-in, and the take it records (#203).
//!
//! A punch-in replaces one region of a track with a new performance. The
//! performer has to hear the music around the region to play along with it,
//! and what they play has to land where they meant it to: the take is
//! recorded by the microphone some tens of milliseconds after the sound
//! that cued it left the program (output buffers, converters, the speaker,
//! the air, input buffers), so cutting the take out of the recording at the
//! region's nominal times puts it late by exactly that, on every take.
//! [`crate::latency`] has the clock model and [`crate::calibration`]
//! measures its one constant per pair of devices; this module is where they
//! meet a take.
//!
//! ## What plays
//!
//! Two renders of the mix, which the caller makes, in the order of the
//! performer's ears: the mix as it is, and the mix with the track being
//! replaced muted. [`build_monitor`] plays the first before the region and
//! after it (so the performer hears their old line leading in and the music
//! going on after it) and the second inside it (so the line being replaced
//! is not in their headphones while they play its replacement). The
//! switches are at the region's own edges, in output frames, with a 5 ms
//! crossfade across each so they do not click. Every other
//! track plays throughout.
//!
//! ## What is kept
//!
//! [`extract`] cuts the region out of the capture at
//! [`crate::latency::input_frame_for`]'s frames: sample-exact for the
//! constant the pair was measured to have. Frames the capture does not
//! reach at its start (the region begins before the microphone was running)
//! are silence; a capture that ends before the region does is refused, not
//! padded, because a take that is silent where the performer played is
//! worse than none. A capture that lost frames ([`Capture::xrun`]) is
//! refused whole, never spliced around the gap.
//!
//! The take comes out in the microphone's own rate and channels.
//! [`convert`] puts it in the track's: channels first (N to 1 averages, 1
//! to N copies, otherwise the first channels and silence for any the source
//! does not have), then the rate with the same FFT resampler the mixer
//! uses, with its delay taken out so sample 0 stays at sample 0.
//!
//! ## Clock drift
//!
//! The two streams are clocked by separate devices, so they drift apart by
//! parts per million of the take's length (about 3 ms over 30 s at 100
//! ppm). That is nothing for a line fix and it is why a long punch is the
//! caller's to warn about.

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

use rubato::{FftFixedInOut, Resampler};
use thiserror::Error;

use crate::duplex::{play_and_capture, Capture, DevicePair};
use crate::latency::input_frame_for;
use crate::store::{LatencyStore, StoreError, Stored};
use crate::RecorderError;

/// The part of the monitor that is the punch: the region to replace, in
/// seconds from the monitor's first frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub start_sec: f64,
    pub end_sec: f64,
}

impl Region {
    pub fn len_sec(&self) -> f64 {
        self.end_sec - self.start_sec
    }

    fn check(&self) -> Result<(), PunchError> {
        if self.start_sec.is_finite()
            && self.end_sec.is_finite()
            && self.start_sec >= 0.0
            && self.end_sec > self.start_sec
        {
            Ok(())
        } else {
            Err(PunchError::BadRegion)
        }
    }
}

/// Why a punch-in did not produce a take. The text is for the person.
#[derive(Debug, Error)]
pub enum PunchError {
    #[error(
        "No recording latency is kept for these speakers and this microphone, so a take would \
         land late by the delay between them. Open Settings, then Audio, and either press \
         Measure or type the delay in milliseconds, then punch in again."
    )]
    NotCalibrated,
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Device(#[from] RecorderError),
    #[error("that region cannot be punched in")]
    BadRegion,
    #[error(
        "the audio device dropped out during the take, so it was not used. Close other apps \
         that use audio and try again"
    )]
    Dropout,
    #[error(
        "the speakers never started playing, so the performer heard nothing and the take \
         cannot be placed. Check that the output is not muted, switched off or in use by \
         another app, and try again"
    )]
    OutputNeverStarted,
    #[error("the microphone gave no timing information, so the take cannot be placed")]
    NoClock,
    #[error(
        "the recording ended before the whole region had been heard, so the take was not used. \
         Try again, and if it keeps happening check the microphone"
    )]
    CaptureTooShort,
    #[error("the microphone recorded nothing for this region")]
    NothingHeard,
    #[error("cancelled")]
    Cancelled,
    #[error("could not convert the take: {0}")]
    Convert(String),
    #[error("could not write the take: {0}")]
    Write(String),
}

/// What the person's last word on the delay is, or [`PunchError::NotCalibrated`].
///
/// A measured value and a typed one are equal here: either lets a punch
/// go ahead. There is no default number, because a guess is wrong in both
/// directions on most hardware, and that error is the silent lateness this
/// whole feature exists to prevent.
pub fn latency_for(store: &LatencyStore, pair: &DevicePair) -> Result<Stored, PunchError> {
    store.get(pair)?.ok_or(PunchError::NotCalibrated)
}

// ---------------------------------------------------------------------------
// Converting audio between layouts.
// ---------------------------------------------------------------------------

/// `samples` (interleaved, `from_ch` channels) in `to_ch` channels.
///
/// N to 1 averages, 1 to N copies, and any other change keeps the first
/// channels: a destination channel the source has no counterpart for is
/// silent. Equal counts are returned as they are.
pub fn channel_map(samples: &[f32], from_ch: usize, to_ch: usize) -> Vec<f32> {
    let (from_ch, to_ch) = (from_ch.max(1), to_ch.max(1));
    if from_ch == to_ch {
        return samples.to_vec();
    }
    let frames = samples.len() / from_ch;
    let mut out = Vec::with_capacity(frames * to_ch);
    for frame in samples.chunks_exact(from_ch) {
        if to_ch == 1 {
            out.push(frame.iter().sum::<f32>() / from_ch as f32);
        } else if from_ch == 1 {
            out.extend(std::iter::repeat_n(frame[0], to_ch));
        } else {
            for c in 0..to_ch {
                out.push(frame.get(c).copied().unwrap_or(0.0));
            }
        }
    }
    out
}

/// `samples` (interleaved, `channels` channels) at `to_sr` instead of
/// `from_sr`: the same length in seconds, `round(frames * to / from)` frames,
/// with sample 0 at sample 0. The FFT resampler delays what it outputs by
/// half a chunk; that is cut off here, because a take is placed to a
/// millisecond and the delay is of the same size.
pub fn resample(
    samples: &[f32],
    channels: usize,
    from_sr: u32,
    to_sr: u32,
) -> Result<Vec<f32>, PunchError> {
    let channels = channels.max(1);
    if from_sr == 0 || to_sr == 0 {
        return Err(PunchError::Convert("a sample rate of 0 Hz".into()));
    }
    if from_sr == to_sr || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let frames = samples.len() / channels;
    let wanted =
        ((frames as u64 * u64::from(to_sr) + u64::from(from_sr) / 2) / u64::from(from_sr)) as usize;

    let conv = |e: &dyn std::fmt::Display| PunchError::Convert(e.to_string());
    let mut r = FftFixedInOut::<f32>::new(from_sr as usize, to_sr as usize, 1024, channels)
        .map_err(|e| conv(&e))?;
    let delay = r.output_delay();
    let mut in_buf = vec![vec![0.0f32; r.input_frames_max()]; channels];
    let mut out_buf = vec![vec![0.0f32; r.output_frames_max()]; channels];
    let mut out: Vec<Vec<f32>> =
        vec![Vec::with_capacity(delay + wanted + out_buf[0].len()); channels];

    let mut pos = 0usize;
    while out[0].len() < delay + wanted {
        let needed = r.input_frames_next();
        let available = frames.saturating_sub(pos).min(needed);
        for (c, buf) in in_buf.iter_mut().enumerate() {
            buf.clear();
            buf.extend((0..available).map(|f| samples[(pos + f) * channels + c]));
            buf.resize(needed, 0.0);
        }
        let (_used, written) = r
            .process_into_buffer(&in_buf, &mut out_buf, None)
            .map_err(|e| conv(&e))?;
        for (o, b) in out.iter_mut().zip(&out_buf) {
            o.extend_from_slice(&b[..written]);
        }
        pos += available;
    }

    let mut interleaved = Vec::with_capacity(wanted * channels);
    for f in 0..wanted {
        for o in &out {
            interleaved.push(o[delay + f]);
        }
    }
    Ok(interleaved)
}

/// `samples` in another rate and another channel count: the take into the
/// track's format, or a render into the speakers'.
///
/// Whichever order does the fewer channels' work: a mix-down before the
/// resampler, an up-mix after it.
pub fn convert(
    samples: &[f32],
    from_sr: u32,
    from_ch: usize,
    to_sr: u32,
    to_ch: usize,
) -> Result<Vec<f32>, PunchError> {
    let (from_ch, to_ch) = (from_ch.max(1), to_ch.max(1));
    if to_ch <= from_ch {
        let mapped = channel_map(samples, from_ch, to_ch);
        resample(&mapped, to_ch, from_sr, to_sr)
    } else {
        let resampled = resample(samples, from_ch, from_sr, to_sr)?;
        Ok(channel_map(&resampled, from_ch, to_ch))
    }
}

/// How far below the loudest channel a channel is not counted as carrying
/// signal: 40 dB, as an amplitude ratio.
const SILENT_RATIO: f64 = 0.01;

/// A take's channels in the track's, chosen by what the microphone
/// actually delivered rather than by position (#203).
///
/// [`channel_map`] is right for a mix going to speakers, where every
/// channel is signal: it averages, copies, or keeps the first ones. It is
/// wrong for a *take*: a two-input USB interface with the microphone on
/// input 1 delivers the voice on one channel and silence on the other, so
/// keeping the first channels gives a left-only take in a stereo track and
/// averaging to mono gives one 6 dB down. So:
///
/// - going to mono takes the channel with the highest RMS;
/// - going to several channels while exactly one input channel carries
///   signal (the others at least 40 dB below it) copies that channel to
///   all of them;
/// - anything else (several channels with signal) is [`channel_map`]'s
///   rule, the first channels; and a silent take is mapped the same way.
pub fn take_channel_map(samples: &[f32], from_ch: usize, to_ch: usize) -> Vec<f32> {
    let (from_ch, to_ch) = (from_ch.max(1), to_ch.max(1));
    if from_ch == 1 {
        return channel_map(samples, from_ch, to_ch);
    }
    let mut energy = vec![0.0f64; from_ch];
    for frame in samples.chunks_exact(from_ch) {
        for (e, v) in energy.iter_mut().zip(frame) {
            *e += f64::from(*v) * f64::from(*v);
        }
    }
    let loudest =
        energy.iter().enumerate().fold(
            (0usize, 0.0f64),
            |best, (i, e)| if *e > best.1 { (i, *e) } else { best },
        );
    if loudest.1 <= 0.0 {
        return channel_map(samples, from_ch, to_ch);
    }
    // Energies, so the ratio is squared.
    let carrying = energy
        .iter()
        .filter(|e| **e > loudest.1 * SILENT_RATIO * SILENT_RATIO)
        .count();
    if to_ch == 1 || carrying == 1 {
        let c = loudest.0;
        return samples
            .chunks_exact(from_ch)
            .flat_map(|f| std::iter::repeat_n(f[c], to_ch))
            .collect();
    }
    channel_map(samples, from_ch, to_ch)
}

/// [`convert`] for a take: the channels by [`take_channel_map`], then the
/// rate.
pub fn convert_take(
    samples: &[f32],
    from_sr: u32,
    from_ch: usize,
    to_sr: u32,
    to_ch: usize,
) -> Result<Vec<f32>, PunchError> {
    let mapped = take_channel_map(samples, from_ch, to_ch);
    resample(&mapped, to_ch.max(1), from_sr, to_sr)
}

// ---------------------------------------------------------------------------
// The monitor.
// ---------------------------------------------------------------------------

/// A rendered mix: interleaved samples in the format the renderer wrote.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// What the performer hears: `as_is` outside the region and `muted_target`
/// inside it, in the speakers' format (`out_sr`, `out_ch`).
///
/// Both renders start at the monitor's first frame. A render shorter than
/// the other (or than the region) is silent after its end.
pub fn build_monitor(
    as_is: &Rendered,
    muted_target: &Rendered,
    region: Region,
    out_sr: u32,
    out_ch: u16,
) -> Result<Vec<f32>, PunchError> {
    region.check()?;
    let ch = out_ch.max(1) as usize;
    let a = convert(
        &as_is.samples,
        as_is.sample_rate,
        as_is.channels as usize,
        out_sr,
        ch,
    )?;
    let m = convert(
        &muted_target.samples,
        muted_target.sample_rate,
        muted_target.channels as usize,
        out_sr,
        ch,
    )?;
    let s = (region.start_sec * f64::from(out_sr)).round() as usize;
    let e = (region.end_sec * f64::from(out_sr)).round() as usize;
    let frames = (a.len() / ch).max(m.len() / ch).max(e);
    let mut out = vec![0.0f32; frames * ch];
    let copy = |out: &mut [f32], src: &[f32], from: usize, to: usize| {
        let (from, to) = (from * ch, (to * ch).min(src.len()).min(out.len()));
        if from < to {
            out[from..to].copy_from_slice(&src[from..to]);
        }
    };
    copy(&mut out, &a, 0, s);
    copy(&mut out, &m, s, e);
    copy(&mut out, &a, e, frames);

    // A hard cut from one render to the other is a step in the signal and
    // is heard as a click. Everything but the target track is the same in
    // both renders, so the switch is a short *amplitude* crossfade
    // (gains summing to one), which leaves that shared audio exactly as it
    // was and moves only the target's part. (An equal-power fade would be
    // right for unrelated signals; for these it would lift the shared part
    // by up to 3 dB for the length of the fade.) Centred on the edge, so
    // the track is gone by the region's first frame plus half the fade.
    let half =
        ((CROSSFADE_SEC * f64::from(out_sr) / 2.0).round() as usize).clamp(1, ((e - s) / 2).max(1));
    let at = |src: &[f32], f: usize, c: usize| src.get(f * ch + c).copied().unwrap_or(0.0);
    for (edge, entering) in [(s, true), (e, false)] {
        let from = edge.saturating_sub(half);
        let to = (edge + half).min(frames);
        for f in from..to {
            let t = ((f + half - edge) as f32 + 0.5) / (2 * half) as f32;
            let (wa, wm) = if entering { (1.0 - t, t) } else { (t, 1.0 - t) };
            for c in 0..ch {
                out[f * ch + c] = at(&a, f, c) * wa + at(&m, f, c) * wm;
            }
        }
    }
    Ok(out)
}

/// Length of the crossfade where the monitor switches between the two
/// renders.
pub const CROSSFADE_SEC: f64 = 0.005;

// ---------------------------------------------------------------------------
// The take.
// ---------------------------------------------------------------------------

/// What the microphone heard of the region.
#[derive(Debug, Clone)]
pub struct Take {
    /// Interleaved, `channels` channels, exactly the region long.
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    /// The delay it was placed with.
    pub latency_ms: f64,
}

/// Cut the region out of `capture`, placed by `latency_sec` and the
/// capture's own clock skew.
pub fn extract(capture: &Capture, region: Region, latency_sec: f64) -> Result<Take, PunchError> {
    region.check()?;
    if capture.cancelled {
        return Err(PunchError::Cancelled);
    }
    if capture.xrun {
        return Err(PunchError::Dropout);
    }
    if capture.out_origin_ns.is_none() {
        return Err(PunchError::OutputNeverStarted);
    }
    let skew = capture.skew_ns().ok_or(PunchError::NoClock)?;

    let ch = capture.in_ch.max(1) as usize;
    let sr = capture.in_sr;
    let have = (capture.samples.len() / ch) as i64;
    let first = input_frame_for(region.start_sec, skew, latency_sec, sr);
    let len = ((region.len_sec() * f64::from(sr)).round() as i64).max(1);
    if first + len > have {
        return Err(PunchError::CaptureTooShort);
    }
    if first + len <= 0 {
        return Err(PunchError::NothingHeard);
    }

    let mut samples = vec![0.0f32; len as usize * ch];
    let from = first.max(0);
    let to = (first + len).min(have);
    let dst = ((from - first) as usize) * ch;
    let n = ((to - from) as usize) * ch;
    samples[dst..dst + n]
        .copy_from_slice(&capture.samples[from as usize * ch..from as usize * ch + n]);
    Ok(Take {
        samples,
        sample_rate: sr,
        channels: capture.in_ch,
        latency_ms: latency_sec * 1_000.0,
    })
}

/// Where the take is, for the progress strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The lead-in: the performer hears the track as it is.
    Preroll,
    /// Inside the region: the track is muted and the take is the punch.
    Recording,
    /// After the region, until the monitor ends.
    Postroll,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Preroll => "preroll",
            Phase::Recording => "recording",
            Phase::Postroll => "postroll",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// Seconds to the end of this phase.
    pub remaining_sec: f64,
}

/// What `elapsed_sec` after the monitor began is in: the phase, and how long
/// it has to run. `total_sec` is the monitor's length. It is the clock of
/// the caller's, not the devices': good for a countdown, not for placing
/// anything.
pub fn progress_at(elapsed_sec: f64, region: Region, total_sec: f64) -> Progress {
    let e = elapsed_sec.max(0.0);
    if e < region.start_sec {
        Progress {
            phase: Phase::Preroll,
            remaining_sec: region.start_sec - e,
        }
    } else if e < region.end_sec {
        Progress {
            phase: Phase::Recording,
            remaining_sec: region.end_sec - e,
        }
    } else {
        Progress {
            phase: Phase::Postroll,
            remaining_sec: (total_sec - e).max(0.0),
        }
    }
}

/// Room after the monitor's end (and the latency) for the capture to keep
/// running: the microphone starts before the speakers, and a region heard
/// back `skew + latency` after it was played has to be inside the capture.
pub const CAPTURE_MARGIN_SEC: f64 = 1.0;
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// How many microphone frames a take of `monitor_sec` needs to cover
/// `region` heard back after `latency_sec`.
pub fn capture_frames(monitor_sec: f64, region: Region, latency_sec: f64, in_sr: u32) -> usize {
    let sec = monitor_sec.max(region.end_sec) + latency_sec.max(0.0) + CAPTURE_MARGIN_SEC;
    (sec * f64::from(in_sr)).ceil() as usize
}

/// Play `monitor` to the performer and record them; returns the region.
///
/// `cancel` is polled about every 10 ms and `on_progress` called about four
/// times a second, both from this thread. Like
/// [`crate::duplex::play_and_capture`], which does the work, call it from a
/// blocking task: the streams are not `Send`.
pub fn record(
    pair: &DevicePair,
    monitor: &[f32],
    region: Region,
    latency_sec: f64,
    cancel: &dyn Fn() -> bool,
    on_progress: &dyn Fn(Progress),
) -> Result<Take, PunchError> {
    region.check()?;
    let out_frames = monitor.len() / pair.out_ch.max(1) as usize;
    let total_sec = out_frames as f64 / f64::from(pair.out_sr.max(1));
    let frames = capture_frames(total_sec, region, latency_sec, pair.in_sr);

    // The first poll is right after the streams start, which is the
    // countdown's zero.
    let started: Cell<Option<Instant>> = Cell::new(None);
    let last: Cell<Option<Instant>> = Cell::new(None);
    let tick = || {
        let now = Instant::now();
        let t0 = started.get().unwrap_or_else(|| {
            started.set(Some(now));
            now
        });
        if last.get().is_none_or(|l| now - l >= PROGRESS_EVERY) {
            last.set(Some(now));
            on_progress(progress_at((now - t0).as_secs_f64(), region, total_sec));
        }
        cancel()
    };

    let capture = play_and_capture(monitor, pair, frames, &tick)?;
    extract(&capture, region, latency_sec)
}

/// Read a WAV (16, 24 or 32-bit integer, or 32-bit float) as interleaved
/// `f32`: what the renderer wrote, for [`build_monitor`].
pub fn read_wav(path: &Path) -> Result<Rendered, PunchError> {
    let rerr =
        |e: &dyn std::fmt::Display| PunchError::Convert(format!("could not read the mix: {e}"));
    let mut r = hound::WavReader::open(path).map_err(|e| rerr(&e))?;
    let spec = r.spec();
    let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => r
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| rerr(&e))?,
        (hound::SampleFormat::Int, bits @ 8..=32) => {
            let scale = 1.0 / (1u64 << (bits - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| rerr(&e))?
        }
        (fmt, bits) => {
            return Err(rerr(&format!("a {bits}-bit {fmt:?} WAV")));
        }
    };
    Ok(Rendered {
        samples,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
    })
}

/// Write `samples` (interleaved) as a 32-bit float WAV.
pub fn write_wav(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), PunchError> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let werr = |e: &dyn std::fmt::Display| PunchError::Write(e.to_string());
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| werr(&e))?;
    for s in samples {
        w.write_sample(*s).map_err(|e| werr(&e))?;
    }
    w.finalize().map_err(|e| werr(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Source;

    const REGION: Region = Region {
        start_sec: 1.0,
        end_sec: 1.5,
    };

    /// A recognisable ramp: sample `i` is `i / 1e6`, so a take's first
    /// sample says which capture frame it was cut from.
    fn ramp(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels)
            .map(|i| (i / channels) as f32 / 1e6)
            .collect()
    }

    fn capture(samples: Vec<f32>, in_sr: u32, in_ch: u16, skew_ns: i64) -> Capture {
        Capture {
            samples,
            in_sr,
            in_ch,
            out_origin_ns: Some(skew_ns + 1_000_000),
            in_origin_ns: Some(1_000_000),
            xrun: false,
            cancelled: false,
        }
    }

    fn frame_of(v: f32) -> i64 {
        (v * 1e6).round() as i64
    }

    #[test]
    fn the_take_starts_where_the_region_is_heard_not_where_it_is_played() {
        // Played at 1.0 s of the monitor, the output started 30 ms after
        // the microphone, and the pair's constant is 45 ms: heard at
        // 1.075 s of the capture, frame 51 600 at 48 kHz.
        let c = capture(ramp(48_000 * 3, 1), 48_000, 1, 30_000_000);
        let take = extract(&c, REGION, 0.045).unwrap();
        assert_eq!(frame_of(take.samples[0]), 51_600);
        assert_eq!(take.samples.len(), 24_000, "exactly the region's length");
        assert_eq!(frame_of(*take.samples.last().unwrap()), 51_600 + 23_999);
        assert!((take.latency_ms - 45.0).abs() < 1e-9);

        // The naive splice (no skew, no latency) cuts at the nominal time,
        // 75 ms early: the performer's line would land that much late in
        // the session. This is the failure the whole feature is for.
        let naive = extract(&capture(ramp(48_000 * 3, 1), 48_000, 1, 0), REGION, 0.0).unwrap();
        assert_eq!(frame_of(naive.samples[0]), 48_000);
        assert_eq!(
            frame_of(take.samples[0]) - frame_of(naive.samples[0]),
            3_600,
            "75 ms at 48 kHz"
        );
    }

    #[test]
    fn a_region_that_begins_before_the_capture_is_silence_up_to_it() {
        // The microphone started 0.2 s after the output (negative skew) and
        // there is no latency: the region's first 0.2 s was not recorded.
        let c = capture(ramp(48_000 * 3, 1), 48_000, 1, -(200_000_000));
        // Region start 0.1 s: frame (0.1 - 0.2) * 48000 = -4800.
        let region = Region {
            start_sec: 0.1,
            end_sec: 0.6,
        };
        let take = extract(&c, region, 0.0).unwrap();
        assert_eq!(take.samples.len(), 24_000);
        assert!(
            take.samples[..4_800].iter().all(|s| *s == 0.0),
            "zeros first"
        );
        assert_eq!(
            frame_of(take.samples[4_800]),
            0,
            "then the capture from its start"
        );
        assert_eq!(frame_of(take.samples[4_801]), 1);
        assert_eq!(frame_of(*take.samples.last().unwrap()), 24_000 - 4_800 - 1);
    }

    #[test]
    fn a_region_entirely_before_the_capture_is_refused() {
        let c = capture(ramp(48_000, 1), 48_000, 1, -(2_000_000_000));
        let r = Region {
            start_sec: 0.0,
            end_sec: 0.5,
        };
        assert!(matches!(extract(&c, r, 0.0), Err(PunchError::NothingHeard)));
    }

    #[test]
    fn a_capture_that_ends_before_the_region_does_is_refused_not_padded() {
        let c = capture(ramp(48_000, 1), 48_000, 1, 0);
        // The region ends at 1.5 s; one second was recorded.
        assert!(matches!(
            extract(&c, REGION, 0.0),
            Err(PunchError::CaptureTooShort)
        ));
    }

    #[test]
    fn a_stereo_microphone_keeps_its_channels_in_order() {
        let c = capture(ramp(48_000 * 2, 2), 48_000, 2, 0);
        let take = extract(&c, REGION, 0.0).unwrap();
        assert_eq!(take.channels, 2);
        assert_eq!(take.samples.len(), 24_000 * 2);
        assert_eq!(take.samples[0], take.samples[1], "one frame, both channels");
        assert_eq!(frame_of(take.samples[0]), 48_000);
        assert_eq!(frame_of(take.samples[2]), 48_001);
    }

    #[test]
    fn a_take_is_refused_when_frames_were_lost_or_the_streams_have_no_clock() {
        let ok = capture(ramp(48_000 * 3, 1), 48_000, 1, 0);
        let mut c = ok.clone();
        c.xrun = true;
        assert!(matches!(extract(&c, REGION, 0.0), Err(PunchError::Dropout)));
        let mut c = ok.clone();
        c.cancelled = true;
        assert!(matches!(
            extract(&c, REGION, 0.0),
            Err(PunchError::Cancelled)
        ));
        let mut c = ok.clone();
        c.out_origin_ns = None;
        assert!(matches!(
            extract(&c, REGION, 0.0),
            Err(PunchError::OutputNeverStarted)
        ));
        let mut c = ok.clone();
        c.in_origin_ns = None;
        assert!(matches!(extract(&c, REGION, 0.0), Err(PunchError::NoClock)));
        let bad = Region {
            start_sec: 2.0,
            end_sec: 1.0,
        };
        assert!(matches!(extract(&ok, bad, 0.0), Err(PunchError::BadRegion)));
    }

    #[test]
    fn channels_map_by_averaging_copying_or_keeping_the_first() {
        // Stereo to mono averages.
        assert_eq!(channel_map(&[1.0, 3.0, -1.0, 1.0], 2, 1), vec![2.0, 0.0]);
        // Mono to stereo and to five channels copies.
        assert_eq!(channel_map(&[0.5, -0.5], 1, 2), vec![0.5, 0.5, -0.5, -0.5]);
        assert_eq!(channel_map(&[0.25], 1, 5), vec![0.25; 5]);
        // Six to two keeps the first two.
        let six: Vec<f32> = (0..12).map(|i| i as f32).collect();
        assert_eq!(channel_map(&six, 6, 2), vec![0.0, 1.0, 6.0, 7.0]);
        // Two to three keeps both and is silent in the third.
        assert_eq!(channel_map(&[1.0, 2.0], 2, 3), vec![1.0, 2.0, 0.0]);
        // Equal counts are unchanged.
        assert_eq!(
            channel_map(&[1.0, 2.0, 3.0, 4.0], 2, 2),
            vec![1.0, 2.0, 3.0, 4.0]
        );
    }

    /// A Hann-windowed burst of `hz`, centred on `at` frames, `len` frames long.
    fn burst(frames: usize, at: usize, sr: u32, hz: f64) -> Vec<f32> {
        let half = 200usize;
        let mut v = vec![0.0f32; frames];
        for i in 0..2 * half {
            let w = 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / half as f64).cos();
            let t = i as f64 / f64::from(sr);
            v[at - half + i] = (w * (2.0 * std::f64::consts::PI * hz * t).sin()) as f32;
        }
        v
    }

    fn peak_frame(x: &[f32]) -> usize {
        x.iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0
    }

    #[test]
    fn a_mono_take_becomes_a_stereo_take_with_both_channels_the_same() {
        let mono = burst(44_100, 22_050, 44_100, 2_000.0);
        let stereo = convert(&mono, 44_100, 1, 48_000, 2).unwrap();
        assert_eq!(stereo.len(), 48_000 * 2, "one second, two channels");
        assert!(stereo.chunks_exact(2).all(|f| f[0] == f[1]));
    }

    #[test]
    fn a_44_1k_take_lands_at_the_same_time_in_a_48k_session() {
        // The burst is half a second in. In 48 kHz it must still be: any
        // resampler delay left in would move it by tens of frames.
        let take = burst(44_100, 22_050, 44_100, 2_000.0);
        let out = convert(&take, 44_100, 1, 48_000, 1).unwrap();
        assert_eq!(out.len(), 48_000);
        let at = peak_frame(&out);
        assert!(
            (at as i64 - 24_000).abs() <= 24,
            "the burst's loudest sample is at {at}, wanted 24000 within a burst's width"
        );
        // Tighter: the centre of energy, which does not depend on which
        // lobe of the burst is the loudest.
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (i, v) in out.iter().enumerate() {
            num += i as f64 * f64::from(*v) * f64::from(*v);
            den += f64::from(*v) * f64::from(*v);
        }
        let centre = num / den;
        assert!(
            (centre - 24_000.0).abs() < 1.0,
            "centre of energy at {centre}"
        );
    }

    #[test]
    fn resampling_down_and_up_keeps_the_length_in_seconds() {
        let x = vec![0.1f32; 44_100 * 2];
        let up = resample(&x, 2, 44_100, 48_000).unwrap();
        assert_eq!(up.len(), 48_000 * 2);
        let down = resample(&up, 2, 48_000, 44_100).unwrap();
        assert_eq!(down.len(), 44_100 * 2);
        assert_eq!(resample(&x, 2, 44_100, 44_100).unwrap(), x);
        assert!(resample(&[], 1, 44_100, 48_000).unwrap().is_empty());
        assert!(resample(&x, 1, 0, 48_000).is_err());
    }

    fn lr(frames: usize, left: f32, right: f32) -> Vec<f32> {
        (0..frames).flat_map(|_| [left, right]).collect()
    }

    #[test]
    fn a_mono_microphone_on_one_of_two_inputs_fills_a_stereo_track() {
        // Input 1 has the voice, input 2 is silent: both track channels get
        // the voice, not a left-only take.
        let left_only = lr(100, 0.5, 0.0);
        let out = take_channel_map(&left_only, 2, 2);
        assert_eq!(out, lr(100, 0.5, 0.5), "the voice on both, not left only");
        // Going to three channels with one carrying signal copies it.
        let out = take_channel_map(&left_only, 2, 3);
        assert!(out.chunks_exact(3).all(|f| f == [0.5, 0.5, 0.5]));
        // The microphone on input 2.
        let right_only = lr(100, 0.0, 0.25);
        let out = take_channel_map(&right_only, 2, 6);
        assert!(out.chunks_exact(6).all(|f| f.iter().all(|v| *v == 0.25)));
    }

    #[test]
    fn a_stereo_take_to_mono_takes_the_loudest_channel_not_the_average() {
        // The average of a voice on one input is 6 dB down.
        let out = take_channel_map(&lr(100, 0.5, 0.0), 2, 1);
        assert!(out.iter().all(|v| *v == 0.5), "{:?}", &out[..3]);
        let out = take_channel_map(&lr(100, 0.1, 0.4), 2, 1);
        assert!(out.iter().all(|v| *v == 0.4));
        // Both inputs carry signal: still the louder one, not a mix.
        let both: Vec<f32> = (0..200)
            .flat_map(|i| [(i as f32 * 0.3).sin() * 0.3, (i as f32 * 0.7).sin() * 0.5])
            .collect();
        let out = take_channel_map(&both, 2, 1);
        assert_eq!(
            out,
            both.iter().skip(1).step_by(2).copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn one_carrying_channel_among_six_is_found_wherever_it_is() {
        let mut six = vec![0.0f32; 6 * 50];
        for f in 0..50 {
            six[f * 6 + 3] = 0.2;
        }
        let stereo = take_channel_map(&six, 6, 2);
        assert!(stereo.chunks_exact(2).all(|f| f == [0.2, 0.2]));
        let mono = take_channel_map(&six, 6, 1);
        assert!(mono.iter().all(|v| *v == 0.2));
    }

    #[test]
    fn two_channels_that_both_carry_signal_keep_the_first_channels_rule() {
        let both = lr(100, 0.3, 0.2);
        assert_eq!(take_channel_map(&both, 2, 2), both);
        let six = take_channel_map(&both, 2, 6);
        assert_eq!(&six[..6], &[0.3, 0.2, 0.0, 0.0, 0.0, 0.0]);
        // 6 in, 2 out, with several carrying signal: the first two.
        let many: Vec<f32> = (0..6 * 20).map(|i| (i % 6) as f32 + 1.0).collect();
        let first_two = take_channel_map(&many, 6, 2);
        assert_eq!(&first_two[..2], &[1.0, 2.0]);
    }

    #[test]
    fn a_channel_forty_decibels_down_is_silence_and_thirty_nine_is_not() {
        let down = |db: f32| lr(100, 0.5, 0.5 * 10f32.powf(-db / 20.0));
        // 41 dB down: the left is copied to the right.
        let out = take_channel_map(&down(41.0), 2, 3);
        assert!(out.chunks_exact(3).all(|f| f == [0.5, 0.5, 0.5]));
        // 39 dB down still carries signal: the first channels, as before.
        let out = take_channel_map(&down(39.0), 2, 3);
        assert!(out
            .chunks_exact(3)
            .all(|f| f[0] == 0.5 && f[1] > 0.0 && f[1] < 0.01 && f[2] == 0.0));
    }

    #[test]
    fn a_silent_take_maps_without_dividing_by_nothing() {
        let silent = lr(10, 0.0, 0.0);
        assert_eq!(take_channel_map(&silent, 2, 1), vec![0.0; 10]);
        assert_eq!(take_channel_map(&silent, 2, 4).len(), 40);
        assert!(take_channel_map(&[], 2, 1).is_empty());
    }

    #[test]
    fn a_mono_microphone_on_input_one_lands_in_a_44_1k_to_48k_stereo_track_on_both_channels() {
        let mono_of_two: Vec<f32> = burst(44_100, 22_050, 44_100, 2_000.0)
            .into_iter()
            .flat_map(|v| [v, 0.0])
            .collect();
        let out = convert_take(&mono_of_two, 44_100, 2, 48_000, 2).unwrap();
        assert_eq!(out.len(), 48_000 * 2);
        assert!(
            out.chunks_exact(2).all(|f| (f[0] - f[1]).abs() < 1e-6),
            "both channels the voice"
        );
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!((peak_frame(&left) as i64 - 24_000).abs() <= 24);
    }

    #[test]
    fn a_stereo_render_is_played_by_a_mono_device_and_a_surround_one() {
        let stereo: Vec<f32> = [1.0f32, 0.0].repeat(1_000);
        let to_mono = convert(&stereo, 48_000, 2, 48_000, 1).unwrap();
        assert!(to_mono.iter().all(|s| (*s - 0.5).abs() < 1e-6));
        let to_six = convert(&stereo, 48_000, 2, 48_000, 6).unwrap();
        assert_eq!(to_six.len(), 6_000);
        assert_eq!(&to_six[..6], &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    fn flat(v: f32, frames: usize, ch: u16) -> Rendered {
        Rendered {
            samples: vec![v; frames * ch as usize],
            sample_rate: 48_000,
            channels: ch,
        }
    }

    #[test]
    fn the_monitor_plays_the_mix_around_the_region_and_the_muted_mix_inside_it() {
        let as_is = flat(0.9, 48_000 * 3, 2);
        let muted = flat(0.1, 48_000 * 3, 2);
        let m = build_monitor(&as_is, &muted, REGION, 48_000, 2).unwrap();
        let at = |sec: f64| m[(sec * 48_000.0) as usize * 2];
        assert_eq!(at(0.5), 0.9, "pre-roll: the track is heard");
        assert_eq!(at(0.99), 0.9, "up to the fade before the region");
        assert_eq!(
            at(1.01),
            0.1,
            "from just after the region's first frame it is muted"
        );
        assert_eq!(at(1.49), 0.1);
        assert_eq!(at(1.51), 0.9, "post-roll: it is back");
        assert_eq!(at(2.9), 0.9);
        assert_eq!(m.len(), 48_000 * 3 * 2);
    }

    #[test]
    fn the_monitor_switches_by_a_short_fade_not_a_step() {
        let as_is = flat(0.9, 48_000 * 3, 2);
        let muted = flat(0.1, 48_000 * 3, 2);
        let m = build_monitor(&as_is, &muted, REGION, 48_000, 2).unwrap();
        let left: Vec<f32> = m.iter().step_by(2).copied().collect();
        // A hard switch steps by 0.8 between two frames; the fade is 5 ms,
        // 240 frames, so no frame moves by more than a few thousandths.
        let biggest = left
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(biggest < 0.01, "a step of {biggest} at a switch");
        // It does fall from one render to the other across the edge, and
        // monotonically, and is centred on it.
        let (s, e) = (48_000usize, 72_000usize);
        assert!((left[s] - 0.5).abs() < 0.01, "{}", left[s]);
        assert!((left[e] - 0.5).abs() < 0.01, "{}", left[e]);
        assert!(left[s - 120..s + 120].windows(2).all(|w| w[1] <= w[0]));
        assert!(left[e - 120..e + 120].windows(2).all(|w| w[1] >= w[0]));
        // What both renders share is not touched by it: a bed in both is
        // the same level all the way through (gains sum to one).
        let bed = |v: f32| Rendered {
            samples: vec![v; 48_000 * 3 * 2],
            sample_rate: 48_000,
            channels: 2,
        };
        let same = build_monitor(&bed(0.4), &bed(0.4), REGION, 48_000, 2).unwrap();
        assert!(
            same.iter().all(|v| (*v - 0.4).abs() < 1e-6),
            "the shared audio dipped"
        );
    }

    #[test]
    fn a_region_shorter_than_the_fade_still_switches() {
        let tiny = Region {
            start_sec: 1.0,
            end_sec: 1.001,
        };
        let m = build_monitor(
            &flat(0.9, 96_000, 1),
            &flat(0.1, 96_000, 1),
            tiny,
            48_000,
            1,
        )
        .unwrap();
        assert_eq!(m.len(), 96_000);
        assert!(m.iter().all(|v| v.is_finite() && (0.1..=0.9).contains(v)));
    }

    #[test]
    fn the_monitor_follows_the_speakers_rate_and_channels() {
        let as_is = Rendered {
            samples: vec![0.5; 44_100 * 3],
            sample_rate: 44_100,
            channels: 1,
        };
        let muted = Rendered {
            samples: vec![0.0; 44_100 * 3],
            sample_rate: 44_100,
            channels: 1,
        };
        let m = build_monitor(&as_is, &muted, REGION, 48_000, 2).unwrap();
        assert_eq!(m.len(), 48_000 * 3 * 2);
        // Away from the resampler's edges.
        assert!((m[48_000 / 2 * 2] - 0.5).abs() < 1e-3);
        assert!((m[48_000 / 2 * 2 + 1] - 0.5).abs() < 1e-3);
        assert!(
            m[(1.25 * 48_000.0) as usize * 2].abs() < 1e-3,
            "muted inside"
        );
    }

    #[test]
    fn a_render_that_ends_early_is_silent_after_its_end() {
        let as_is = flat(0.9, 48_000, 1); // ends at 1 s, before the region
        let muted = flat(0.1, 48_000 * 2, 1);
        let m = build_monitor(&as_is, &muted, REGION, 48_000, 1).unwrap();
        assert_eq!(m.len(), 48_000 * 2);
        assert_eq!(m[(1.25 * 48_000.0) as usize], 0.1);
        assert_eq!(
            m[(1.75 * 48_000.0) as usize],
            0.0,
            "after the region the short render is over"
        );
    }

    #[test]
    fn the_progress_walks_through_preroll_the_region_and_postroll() {
        let p = |t| progress_at(t, REGION, 3.0);
        assert_eq!(
            p(0.0),
            Progress {
                phase: Phase::Preroll,
                remaining_sec: 1.0
            }
        );
        assert_eq!(p(0.25).phase, Phase::Preroll);
        assert!((p(0.25).remaining_sec - 0.75).abs() < 1e-9);
        assert_eq!(p(1.0).phase, Phase::Recording);
        assert!((p(1.2).remaining_sec - 0.3).abs() < 1e-9);
        assert_eq!(p(1.5).phase, Phase::Postroll);
        assert!((p(2.0).remaining_sec - 1.0).abs() < 1e-9);
        assert_eq!(p(9.0).remaining_sec, 0.0, "never negative");
        assert_eq!(p(-1.0).phase, Phase::Preroll, "a clock a hair early");
        assert_eq!(Phase::Recording.as_str(), "recording");
    }

    #[test]
    fn the_capture_runs_past_the_monitor_by_the_latency_and_a_margin() {
        let n = capture_frames(3.0, REGION, 0.05, 48_000);
        assert_eq!(n, ((3.0 + 0.05 + CAPTURE_MARGIN_SEC) * 48_000.0) as usize);
        // A short monitor still covers the region.
        let short = capture_frames(0.5, REGION, 0.0, 44_100);
        assert!(short as f64 / 44_100.0 >= REGION.end_sec + CAPTURE_MARGIN_SEC);
    }

    fn pair() -> DevicePair {
        DevicePair {
            host: "ALSA".into(),
            output: "Speakers".into(),
            input: "Mic".into(),
            out_sr: 48_000,
            out_ch: 2,
            in_sr: 44_100,
            in_ch: 1,
        }
    }

    #[test]
    fn a_punch_without_any_stored_latency_is_refused_with_both_ways_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = LatencyStore::at(dir.path().join("audio-latency.json"));
        let e = latency_for(&store, &pair()).unwrap_err();
        assert!(matches!(e, PunchError::NotCalibrated));
        let text = e.to_string();
        assert!(text.contains("Settings"), "{text}");
        assert!(text.contains("Audio"), "{text}");
        assert!(text.contains("Measure"), "{text}");
        assert!(text.contains("type the delay"), "{text}");
    }

    #[test]
    fn a_measured_value_and_a_typed_one_both_count_as_calibrated() {
        let dir = tempfile::tempdir().unwrap();
        let store = LatencyStore::at(dir.path().join("audio-latency.json"));
        store
            .put(
                &pair(),
                Stored {
                    latency_ms: 41.0,
                    spread_ms: Some(0.2),
                    measured_at: 1,
                    source: Source::Measured,
                },
            )
            .unwrap();
        assert_eq!(latency_for(&store, &pair()).unwrap().latency_ms, 41.0);
        store.put_manual(&pair(), 60.0).unwrap();
        let kept = latency_for(&store, &pair()).unwrap();
        assert_eq!((kept.latency_ms, kept.source), (60.0, Source::Manual));
        // Another pair is still not calibrated.
        let mut other = pair();
        other.output = "Headset".into();
        assert!(matches!(
            latency_for(&store, &other),
            Err(PunchError::NotCalibrated)
        ));
    }

    #[test]
    fn a_rendered_16_bit_mix_reads_as_floats_in_full_scale_units() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mix.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for v in [16_384i16, -16_384, 32_767, -32_768] {
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
        let r = read_wav(&path).unwrap();
        assert_eq!((r.sample_rate, r.channels), (44_100, 2));
        assert_eq!(r.samples[..2], [0.5, -0.5]);
        assert!((r.samples[2] - 0.99997).abs() < 1e-4);
        assert_eq!(r.samples[3], -1.0);
        assert!(read_wav(&dir.path().join("missing.wav")).is_err());
    }

    #[test]
    fn a_take_is_written_as_a_float_wav_that_reads_back_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let samples: Vec<f32> = (0..2_000).map(|i| (i as f32 * 0.01).sin()).collect();
        write_wav(&path, &samples, 48_000, 2).unwrap();
        let mut r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.spec().channels, 2);
        assert_eq!(r.spec().sample_rate, 48_000);
        assert_eq!(r.spec().sample_format, hound::SampleFormat::Float);
        let back: Vec<f32> = r.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(back, samples);
    }
}
