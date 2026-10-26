//! Quality-control measurements of a finished mix or recording (#763):
//! loudness (EBU R128 / ITU-R BS.1770-4), loudness range (EBU Tech 3342),
//! sample and true peak, RMS, noise floor, clipping and the contrast between
//! speech and background.
//!
//! The meter is a stream: [`QcMeter::push`] takes interleaved `f32` frames in
//! chunks of any size and [`QcMeter::finish`] reads the answers, so an hour of
//! audio is measured in the memory of one chunk. The loudness numbers come
//! from the `ebur128` crate (a port of `libebur128`, as in [`crate::loudness`]);
//! everything else is here.
//!
//! ## Where the numbers come from
//!
//! * **Integrated loudness**: BS.1770-4 K-weighting, 400 ms blocks at 75 %
//!   overlap, the absolute gate at -70 LUFS and the relative gate 10 LU below
//!   it (Tech 3341 section 2.3).
//! * **Momentary max**: the 400 ms sliding window, not gated, read every
//!   20 ms. **Short-term max**: the 3 s window, read every 100 ms (Tech 3341
//!   section 2.2 asks live meters for at least 10 Hz). A read is made only
//!   once the window is full, so the first 400 ms (or 3 s) never lowers or
//!   raises the maximum.
//! * **Loudness range**: Tech 3342, the 10th to the 95th percentile of the
//!   short-term loudness above the absolute gate (-70 LUFS) and the relative
//!   gate (-20 LU below the absolute-gated mean). Audio shorter than 3 s has
//!   no short-term block, so it has no range.
//! * **Sample peak**: the largest absolute sample value of any channel.
//!   **True peak**: BS.1770-4 Annex 2, the crate's polyphase interpolator
//!   (4x for rates under 96 kHz, 2x under 192 kHz), never below the sample
//!   peak.
//! * **RMS**: the root mean square of every sample of every channel, over the
//!   whole measured audio, silence included. That is what ACX's own checker
//!   and Audacity's RMS report for a selection.
//! * **Noise floor**: the RMS of the quietest 500 ms stretch (windows start
//!   every 100 ms). On audio with no pause that is the quietest passage, and
//!   the report says where it is.
//! * **Clipping**: a run of at least [`CLIP_MIN_RUN`] consecutive samples of
//!   one channel at or beyond [`CLIP_THRESHOLD`] of full scale (Audacity's
//!   Find Clipping default). Runs on several channels that overlap in time
//!   are one event.
//! * **Contrast**: the speech level (the 95th percentile of the 500 ms window
//!   levels) less the background (the noise floor). WCAG 2.0 success
//!   criterion 1.4.7 asks background sound to be at least 20 dB lower than
//!   the speech.

use ebur128::{EbuR128, Mode};

use crate::{Error, Result};

/// A level at or below this is reported as exactly this: digital silence, or
/// a number so small that no real signal sits there. Finite on purpose, so a
/// report never holds `-inf`.
pub const DB_FLOOR: f64 = -120.0;

/// A sample this close to full scale or beyond counts as clipped
/// (-0.0009 dBFS). A 16-bit file's top code, 32767/32768, is above it.
pub const CLIP_THRESHOLD: f32 = 0.9999;

/// Consecutive clipped samples that make a clip (Audacity's Find Clipping
/// start threshold). One full-scale sample is a peak; three in a row is a
/// flat top.
pub const CLIP_MIN_RUN: usize = 3;

/// How many clip times a report lists when the caller does not say.
pub const DEFAULT_CLIP_TIMES: usize = 10;

/// The most clip times a report may list.
pub const MAX_CLIP_TIMES: usize = 50;

/// The WCAG 2.0 SC 1.4.7 minimum gap between speech and background, in dB.
pub const WCAG_CONTRAST_DB: f64 = 20.0;

/// Momentary loudness is read this often (frames are `rate / 50`).
const HOPS_PER_SECOND: u32 = 50;

/// Hops per 100 ms block, which is also how often short-term is read.
const HOPS_PER_BLOCK: usize = 5;

/// A short-term window, in 100 ms blocks.
const SHORT_TERM_BLOCKS: usize = 30;

/// A momentary window, in 100 ms blocks.
const MOMENTARY_BLOCKS: usize = 4;

/// What [`QcMeter`] is asked for.
#[derive(Debug, Clone, Copy)]
pub struct QcOptions {
    /// How many clip start times to keep (at most [`MAX_CLIP_TIMES`]).
    pub max_clip_times: usize,
}

impl Default for QcOptions {
    fn default() -> Self {
        Self {
            max_clip_times: DEFAULT_CLIP_TIMES,
        }
    }
}

/// Clipping found in the audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Clipping {
    /// Separate clipped stretches.
    pub events: usize,
    /// Clipped samples in all, every channel counted.
    pub samples: usize,
    /// Start of each of the first events, in seconds from the start of the
    /// measured audio.
    pub first_times_s: Vec<f64>,
}

/// Speech against background.
#[derive(Debug, Clone, PartialEq)]
pub struct Contrast {
    /// The speech level, dBFS RMS.
    pub speech_dbfs: f64,
    /// The background level, dBFS RMS (the noise floor).
    pub background_dbfs: f64,
    /// `speech - background`, in dB.
    pub difference_db: f64,
    /// Whether the difference is at least [`WCAG_CONTRAST_DB`].
    pub wcag_pass: bool,
}

/// Everything the meter measures. Levels are dB; a level below
/// [`DB_FLOOR`] reads as [`DB_FLOOR`].
#[derive(Debug, Clone, PartialEq)]
pub struct QcReport {
    pub duration_s: f64,
    pub sample_rate: u32,
    pub channels: u16,
    /// LUFS; `None` for under 400 ms of audio or audio quieter than the
    /// absolute gate.
    pub integrated_lufs: Option<f64>,
    /// LUFS; `None` for under 400 ms of audio.
    pub momentary_max_lufs: Option<f64>,
    /// LUFS; `None` for under 3 s of audio.
    pub short_term_max_lufs: Option<f64>,
    /// LU; `None` for under 3 s of audio or audio quieter than the gate.
    pub loudness_range_lu: Option<f64>,
    pub sample_peak_dbfs: f64,
    pub true_peak_dbtp: f64,
    pub rms_dbfs: f64,
    pub noise_floor_dbfs: f64,
    /// Where the quietest stretch starts, in seconds.
    pub noise_floor_at_s: f64,
    pub clipping: Clipping,
    pub contrast: Contrast,
}

/// A streaming meter. See the module docs.
pub struct QcMeter {
    rate: u32,
    channels: usize,
    max_clip_times: usize,
    ebu: EbuR128,
    /// Frames per momentary read (`rate / 50`).
    hop_frames: usize,
    /// Frames in the 400 ms and 3 s windows, as the loudness crate counts
    /// them, which is when a read is first allowed.
    momentary_frames: u64,
    short_term_frames: u64,
    frames_in_hop: usize,
    hops: usize,
    frames: u64,
    momentary_max: f64,
    short_term_max: f64,
    peak: f32,
    sum_sq: f64,
    /// Sum of squares of the block being filled (all channels), and the mean
    /// power of each finished 100 ms block.
    block_sq: f64,
    block_powers: Vec<f64>,
    /// Consecutive clipped samples so far, per channel.
    runs: Vec<usize>,
    in_clip: bool,
    clip_events: usize,
    clip_samples: usize,
    clip_times: Vec<f64>,
}

/// 100 ms blocks in one level window (500 ms).
const WINDOW_BLOCKS: usize = 5;

fn amplitude_db(amplitude: f64) -> f64 {
    if amplitude.is_nan() || amplitude <= 1e-6 {
        DB_FLOOR
    } else {
        (20.0 * amplitude.log10()).max(DB_FLOOR)
    }
}

fn power_db(power: f64) -> f64 {
    if power.is_nan() || power <= 1e-12 {
        DB_FLOOR
    } else {
        (10.0 * power.log10()).max(DB_FLOOR)
    }
}

fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

impl QcMeter {
    pub fn new(sample_rate: u32, channels: u16, options: QcOptions) -> Result<Self> {
        if sample_rate == 0 {
            return Err(Error::Analysis("sample rate is zero".into()));
        }
        if channels == 0 {
            return Err(Error::Analysis("channel count is zero".into()));
        }
        let ebu = EbuR128::new(
            channels as u32,
            sample_rate,
            Mode::I | Mode::LRA | Mode::TRUE_PEAK,
        )
        .map_err(|e| Error::Lufs(format!("ebur128 init: {e:?}")))?;
        let in_100ms = (sample_rate as u64 + 5) / 10;
        Ok(Self {
            rate: sample_rate,
            channels: channels as usize,
            max_clip_times: options.max_clip_times.min(MAX_CLIP_TIMES),
            ebu,
            hop_frames: ((sample_rate / HOPS_PER_SECOND) as usize).max(1),
            momentary_frames: in_100ms * MOMENTARY_BLOCKS as u64,
            short_term_frames: in_100ms * SHORT_TERM_BLOCKS as u64,
            frames_in_hop: 0,
            hops: 0,
            frames: 0,
            momentary_max: f64::NEG_INFINITY,
            short_term_max: f64::NEG_INFINITY,
            peak: 0.0,
            sum_sq: 0.0,
            block_sq: 0.0,
            block_powers: Vec::new(),
            runs: vec![0; channels as usize],
            in_clip: false,
            clip_events: 0,
            clip_samples: 0,
            clip_times: Vec::new(),
        })
    }

    /// Add interleaved frames. A buffer that is not a whole number of frames
    /// is refused, whatever its length.
    pub fn push(&mut self, interleaved: &[f32]) -> Result<()> {
        if !interleaved.len().is_multiple_of(self.channels) {
            return Err(Error::Analysis(format!(
                "{} samples is not a whole number of {}-channel frames",
                interleaved.len(),
                self.channels
            )));
        }
        let mut rest = interleaved;
        while !rest.is_empty() {
            let room = self.hop_frames - self.frames_in_hop;
            let n = room.min(rest.len() / self.channels);
            let (segment, tail) = rest.split_at(n * self.channels);
            self.consume(segment)?;
            self.frames_in_hop += n;
            self.frames += n as u64;
            if self.frames_in_hop == self.hop_frames {
                self.frames_in_hop = 0;
                self.end_hop()?;
            }
            rest = tail;
        }
        Ok(())
    }

    /// Everything that needs no hop boundary: the loudness filter, the peak,
    /// the sums and the clip runs.
    fn consume(&mut self, segment: &[f32]) -> Result<()> {
        self.ebu
            .add_frames_f32(segment)
            .map_err(|e| Error::Lufs(format!("ebur128 add_frames: {e:?}")))?;
        let channels = self.channels;
        let first_frame = self.frames;
        for (i, frame) in segment.chunks_exact(channels).enumerate() {
            let at = first_frame + i as u64;
            let mut clipped_now = false;
            let mut earliest = u64::MAX;
            for (c, &x) in frame.iter().enumerate() {
                let a = x.abs();
                if a > self.peak {
                    self.peak = a;
                }
                let x = x as f64;
                self.sum_sq += x * x;
                self.block_sq += x * x;
                if a >= CLIP_THRESHOLD {
                    self.runs[c] += 1;
                    if self.runs[c] == CLIP_MIN_RUN {
                        self.clip_samples += CLIP_MIN_RUN;
                    } else if self.runs[c] > CLIP_MIN_RUN {
                        self.clip_samples += 1;
                    }
                } else {
                    self.runs[c] = 0;
                }
                if self.runs[c] >= CLIP_MIN_RUN {
                    clipped_now = true;
                    earliest = earliest.min(at + 1 - self.runs[c] as u64);
                }
            }
            if clipped_now && !self.in_clip {
                self.clip_events += 1;
                if self.clip_times.len() < self.max_clip_times {
                    self.clip_times.push(earliest as f64 / self.rate as f64);
                }
            }
            self.in_clip = clipped_now;
        }
        Ok(())
    }

    fn end_hop(&mut self) -> Result<()> {
        self.hops += 1;
        if self.frames >= self.momentary_frames {
            let m = self
                .ebu
                .loudness_momentary()
                .map_err(|e| Error::Lufs(format!("ebur128 momentary: {e:?}")))?;
            if m > self.momentary_max {
                self.momentary_max = m;
            }
        }
        if self.hops.is_multiple_of(HOPS_PER_BLOCK) {
            let block_frames = (self.hop_frames * HOPS_PER_BLOCK) as f64;
            self.block_powers
                .push(self.block_sq / (block_frames * self.channels as f64));
            self.block_sq = 0.0;
            if self.frames >= self.short_term_frames {
                let s = self
                    .ebu
                    .loudness_shortterm()
                    .map_err(|e| Error::Lufs(format!("ebur128 short-term: {e:?}")))?;
                if s > self.short_term_max {
                    self.short_term_max = s;
                }
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<QcReport> {
        if self.frames == 0 {
            return Err(Error::Analysis("no audio to measure".into()));
        }
        let total_samples = self.frames as f64 * self.channels as f64;

        let integrated = if self.frames >= self.momentary_frames {
            self.ebu.loudness_global().ok().and_then(finite)
        } else {
            None
        };
        let loudness_range = if self.frames >= self.short_term_frames && integrated.is_some() {
            self.ebu.loudness_range().ok().and_then(finite)
        } else {
            None
        };
        let mut true_peak = self.peak as f64;
        for c in 0..self.channels as u32 {
            if let Ok(tp) = self.ebu.true_peak(c) {
                true_peak = true_peak.max(tp);
            }
        }

        // Window powers: the mean of five 100 ms blocks, every 100 ms; the
        // whole audio as one window when it is under 500 ms.
        let block_s = (self.hop_frames * HOPS_PER_BLOCK) as f64 / self.rate as f64;
        let mut windows: Vec<(f64, f64)> = if self.block_powers.len() >= WINDOW_BLOCKS {
            self.block_powers
                .windows(WINDOW_BLOCKS)
                .enumerate()
                .map(|(i, w)| {
                    (
                        w.iter().sum::<f64>() / WINDOW_BLOCKS as f64,
                        i as f64 * block_s,
                    )
                })
                .collect()
        } else {
            vec![(self.sum_sq / total_samples, 0.0)]
        };
        let (quiet_power, quiet_at) = windows.iter().copied().fold(
            (f64::INFINITY, 0.0),
            |best, w| if w.0 < best.0 { w } else { best },
        );
        windows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // The 95th percentile, indexed as Tech 3342 indexes its percentiles.
        let speech_power = windows[((windows.len() - 1) as f64 * 0.95).round() as usize].0;
        let noise_floor = power_db(quiet_power);
        let speech = power_db(speech_power);
        let difference = speech - noise_floor;

        Ok(QcReport {
            duration_s: self.frames as f64 / self.rate as f64,
            sample_rate: self.rate,
            channels: self.channels as u16,
            integrated_lufs: integrated,
            momentary_max_lufs: finite(self.momentary_max),
            short_term_max_lufs: finite(self.short_term_max),
            loudness_range_lu: loudness_range,
            sample_peak_dbfs: amplitude_db(self.peak as f64),
            true_peak_dbtp: amplitude_db(true_peak),
            rms_dbfs: power_db(self.sum_sq / total_samples),
            noise_floor_dbfs: noise_floor,
            noise_floor_at_s: quiet_at,
            clipping: Clipping {
                events: self.clip_events,
                samples: self.clip_samples,
                first_times_s: self.clip_times,
            },
            contrast: Contrast {
                speech_dbfs: speech,
                background_dbfs: noise_floor,
                difference_db: difference,
                wcag_pass: difference >= WCAG_CONTRAST_DB,
            },
        })
    }
}

/// Measure a whole buffer in one call.
pub fn measure(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    options: QcOptions,
) -> Result<QcReport> {
    let mut meter = QcMeter::new(sample_rate, channels, options)?;
    meter.push(samples)?;
    meter.finish()
}

#[cfg(test)]
mod tests {
    //! Each measurement against a reference that can be rebuilt here.
    //!
    //! * Loudness: the test signals of EBU Tech 3341 (2023), table 1, which are
    //!   defined as stereo 1 kHz sines at stated levels, durations and
    //!   orders, with the expected values and tolerances of the table.
    //! * Loudness range: EBU Tech 3342 (2023), table 1, cases 1 to 4.
    //! * True peak: Tech 3341 cases 15 to 19, sines at fs/4, fs/6 and fs/8 whose
    //!   samples read lower than their peak.
    //! * Clipping, RMS, noise floor and contrast: signals built to a known
    //!   level, with the answer worked out by hand in the test.
    use super::*;

    const SR: u32 = 48_000;

    fn db_to_lin(db: f64) -> f64 {
        10f64.powf(db / 20.0)
    }

    /// `seconds` of a stereo 1 kHz sine (the same in both channels) whose
    /// per-channel peak is `dbfs`.
    fn tone(dbfs: f64, seconds: f64) -> Vec<f32> {
        let n = (seconds * SR as f64).round() as usize;
        let a = db_to_lin(dbfs);
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = (a * (std::f64::consts::TAU * 1000.0 * i as f64 / SR as f64).sin()) as f32;
            out.push(s);
            out.push(s);
        }
        out
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; (seconds * SR as f64).round() as usize * 2]
    }

    fn concat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.iter().flatten().copied().collect()
    }

    fn stereo(samples: &[f32]) -> QcReport {
        measure(samples, SR, 2, QcOptions::default()).expect("measure")
    }

    fn near(what: &str, got: Option<f64>, want: f64, tol: f64) {
        let got = got.unwrap_or_else(|| panic!("{what}: no value, wanted {want}"));
        eprintln!("{what}: measured {got:.3}, reference {want} +/- {tol}");
        assert!(
            (got - want).abs() <= tol,
            "{what}: got {got:.3}, wanted {want} +/- {tol}"
        );
    }

    // ---- Tech 3341 table 1: loudness -------------------------------------

    /// Case 1: stereo 1 kHz, -23.0 dBFS per channel, 20 s: M, S, I = -23.0 +/- 0.1.
    #[test]
    fn ebu_3341_case_1_minus_23() {
        let r = stereo(&tone(-23.0, 20.0));
        near("I", r.integrated_lufs, -23.0, 0.1);
        near("M max", r.momentary_max_lufs, -23.0, 0.1);
        near("S max", r.short_term_max_lufs, -23.0, 0.1);
    }

    /// Case 2: as case 1 at -33.0 dBFS: M, S, I = -33.0 +/- 0.1.
    #[test]
    fn ebu_3341_case_2_minus_33() {
        let r = stereo(&tone(-33.0, 20.0));
        near("I", r.integrated_lufs, -33.0, 0.1);
        near("M max", r.momentary_max_lufs, -33.0, 0.1);
        near("S max", r.short_term_max_lufs, -33.0, 0.1);
    }

    /// Case 3: 10 s at -36, 60 s at -23, 10 s at -36: I = -23.0 +/- 0.1. The
    /// relative gate has to drop the -36 stretches.
    #[test]
    fn ebu_3341_case_3_relative_gate() {
        let r = stereo(&concat(&[
            tone(-36.0, 10.0),
            tone(-23.0, 60.0),
            tone(-36.0, 10.0),
        ]));
        near("I", r.integrated_lufs, -23.0, 0.1);
    }

    /// Case 4: 10 s at -72, 10 s at -36, 60 s at -23, 10 s at -36, 10 s at
    /// -72: I = -23.0 +/- 0.1. The -72 stretches are under the absolute gate.
    #[test]
    fn ebu_3341_case_4_absolute_gate() {
        let r = stereo(&concat(&[
            tone(-72.0, 10.0),
            tone(-36.0, 10.0),
            tone(-23.0, 60.0),
            tone(-36.0, 10.0),
            tone(-72.0, 10.0),
        ]));
        near("I", r.integrated_lufs, -23.0, 0.1);
    }

    /// Case 5: 20 s at -26, 20.1 s at -20, 20 s at -26: I = -23.0 +/- 0.1.
    #[test]
    fn ebu_3341_case_5_two_levels() {
        let r = stereo(&concat(&[
            tone(-26.0, 20.0),
            tone(-20.0, 20.1),
            tone(-26.0, 20.0),
        ]));
        near("I", r.integrated_lufs, -23.0, 0.1);
    }

    /// Case 9: (1.34 s at -20, 1.66 s at -30) five times: S = -23.0 +/- 0.1,
    /// constant after 3 s, so its maximum is -23.0.
    #[test]
    fn ebu_3341_case_9_short_term_constant() {
        let cycle = concat(&[tone(-20.0, 1.34), tone(-30.0, 1.66)]);
        let r = stereo(&concat(&[
            cycle.clone(),
            cycle.clone(),
            cycle.clone(),
            cycle.clone(),
            cycle,
        ]));
        near("S max", r.short_term_max_lufs, -23.0, 0.1);
    }

    /// Case 10 (file-based): i * 0.15 s of silence, 3 s at -23.0 dBFS, 1 s of
    /// silence, for i = 0..19, each a file of its own: max S = -23.0 +/- 0.1.
    /// Short-term is read every 100 ms, so an odd i (0.15 s steps) starts off
    /// the grid by 50 ms and reads 0.07 LU low; the checked i are the first
    /// few, one in the middle and the last (all 20 pass, 0.07 LU worst; a
    /// debug build takes two minutes for the rest).
    #[test]
    fn ebu_3341_case_10_short_term_max_per_segment() {
        for i in [0, 1, 2, 3, 10, 19] {
            let r = stereo(&concat(&[
                silence(i as f64 * 0.15),
                tone(-23.0, 3.0),
                silence(1.0),
            ]));
            near(
                &format!("S max, i = {i}"),
                r.short_term_max_lufs,
                -23.0,
                0.1,
            );
        }
    }

    /// Case 12: (0.18 s at -20, 0.22 s at -30) 25 times: M = -23.0 +/- 0.1,
    /// constant after 1 s, so its maximum is -23.0.
    #[test]
    fn ebu_3341_case_12_momentary_constant() {
        let cycle = concat(&[tone(-20.0, 0.18), tone(-30.0, 0.22)]);
        let mut all = Vec::new();
        for _ in 0..25 {
            all.extend_from_slice(&cycle);
        }
        let r = stereo(&all);
        near("M max", r.momentary_max_lufs, -23.0, 0.1);
    }

    /// Case 13 (file-based): i * 20 ms of silence, 400 ms at -23.0 dBFS, 1 s
    /// of silence, for i = 0..19: max M = -23.0 +/- 0.1. The tone starts off
    /// the 100 ms grid for most i, which is why momentary is read every 20 ms.
    /// The checked i are the first few, the middle and the last (all 20 pass,
    /// 0.006 LU worst; the rest only costs debug-build time).
    #[test]
    fn ebu_3341_case_13_momentary_max_per_segment() {
        for i in [0, 1, 2, 5, 10, 19] {
            let r = stereo(&concat(&[
                silence(i as f64 * 0.02),
                tone(-23.0, 0.4),
                silence(1.0),
            ]));
            near(&format!("M max, i = {i}"), r.momentary_max_lufs, -23.0, 0.1);
        }
    }

    // ---- Tech 3342 table 1: loudness range -------------------------------

    fn range_of(levels: &[f64]) -> Option<f64> {
        let parts: Vec<Vec<f32>> = levels.iter().map(|&l| tone(l, 20.0)).collect();
        stereo(&concat(&parts)).loudness_range_lu
    }

    /// Case 1: 20 s at -20 then 20 s at -30 dBFS: LRA = 10 +/- 1 LU.
    #[test]
    fn ebu_3342_case_1_ten_lu() {
        near("LRA", range_of(&[-20.0, -30.0]), 10.0, 1.0);
    }

    /// Case 2: -20 then -15 dBFS: LRA = 5 +/- 1 LU.
    #[test]
    fn ebu_3342_case_2_five_lu() {
        near("LRA", range_of(&[-20.0, -15.0]), 5.0, 1.0);
    }

    /// Case 3: -40 then -20 dBFS: LRA = 20 +/- 1 LU.
    #[test]
    fn ebu_3342_case_3_twenty_lu() {
        near("LRA", range_of(&[-40.0, -20.0]), 20.0, 1.0);
    }

    /// Case 4: -50, -35, -20, -35, -50 dBFS, 20 s each: LRA = 15 +/- 1 LU.
    #[test]
    fn ebu_3342_case_4_fifteen_lu() {
        near(
            "LRA",
            range_of(&[-50.0, -35.0, -20.0, -35.0, -50.0]),
            15.0,
            1.0,
        );
    }

    /// The expected response is unchanged if the signal is repeated.
    #[test]
    fn ebu_3342_repeating_the_signal_changes_nothing() {
        near("LRA", range_of(&[-20.0, -30.0, -20.0, -30.0]), 10.0, 1.0);
    }

    // ---- Tech 3341 table 1: true peak ------------------------------------

    /// A stereo sine at `fs / divisor` with the stated amplitude (fraction of
    /// full scale) and phase in degrees, tapered by a 10 ms fade in and out
    /// as the cases ask.
    fn peak_signal(divisor: f64, amplitude: f64, phase_deg: f64) -> Vec<f32> {
        let n = SR as usize; // one second
        let fade = (SR as f64 * 0.010) as usize;
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let env = if i < fade {
                i as f64 / fade as f64
            } else if i >= n - fade {
                (n - 1 - i) as f64 / fade as f64
            } else {
                1.0
            };
            let phase = std::f64::consts::TAU * i as f64 / divisor + phase_deg.to_radians();
            let s = (amplitude * env * phase.sin()) as f32;
            out.push(s);
            out.push(s);
        }
        out
    }

    /// The answer is `want` dBTP, +0.2/-0.4 (table 1, cases 15 to 19).
    fn true_peak_case(what: &str, divisor: f64, amplitude: f64, phase: f64, want: f64) {
        let r = stereo(&peak_signal(divisor, amplitude, phase));
        let got = r.true_peak_dbtp;
        eprintln!(
            "{what}: true peak {got:.3} dBTP (sample peak {:.3} dBFS), reference {want} +0.2/-0.4",
            r.sample_peak_dbfs
        );
        assert!(
            got <= want + 0.2 && got >= want - 0.4,
            "{what}: true peak {got:.3} dBTP, wanted {want} +0.2/-0.4"
        );
    }

    #[test]
    fn ebu_3341_case_15_fs_over_4_phase_0() {
        true_peak_case("case 15", 4.0, 0.5, 0.0, -6.0);
    }

    /// Case 16: fs/4 at 45 degrees. Every sample is +/-0.3536, so the sample
    /// peak reads -9.03 dBFS while the waveform between them peaks at -6.0.
    #[test]
    fn ebu_3341_case_16_samples_read_low_true_peak_does_not() {
        let r = stereo(&peak_signal(4.0, 0.5, 45.0));
        assert!(
            (r.sample_peak_dbfs - (-9.03)).abs() < 0.1,
            "the samples of case 16 are +/-0.3536 = -9.03 dBFS, got {}",
            r.sample_peak_dbfs
        );
        true_peak_case("case 16", 4.0, 0.5, 45.0, -6.0);
        assert!(r.true_peak_dbtp > r.sample_peak_dbfs + 2.5);
    }

    #[test]
    fn ebu_3341_case_17_fs_over_6_phase_60() {
        true_peak_case("case 17", 6.0, 0.5, 60.0, -6.0);
    }

    #[test]
    fn ebu_3341_case_18_fs_over_8_phase_67_5() {
        true_peak_case("case 18", 8.0, 0.5, 67.5, -6.0);
    }

    /// Case 19: fs/4 at 45 degrees, 1.41 of full scale: +3.0 dBTP. The samples
    /// are +/-0.997, a sample peak of -0.03 dBFS.
    #[test]
    fn ebu_3341_case_19_over_full_scale() {
        true_peak_case("case 19", 4.0, 1.41, 45.0, 3.0);
    }

    // ---- sample peak, RMS ------------------------------------------------

    /// A sine of peak 0.5 has an RMS of 0.5 / sqrt(2) = 0.35355: -9.03 dBFS,
    /// and a peak of -6.02 dBFS.
    #[test]
    fn rms_and_sample_peak_of_a_sine() {
        let r = stereo(&tone(-6.0206, 4.0));
        assert!((r.rms_dbfs - (-9.0309)).abs() < 0.01, "RMS {}", r.rms_dbfs);
        assert!(
            (r.sample_peak_dbfs - (-6.0206)).abs() < 0.01,
            "{}",
            r.sample_peak_dbfs
        );
    }

    /// The RMS is over every sample of the measured audio: half silence is
    /// 3.01 dB under the tone alone.
    #[test]
    fn rms_counts_the_silence() {
        let r = stereo(&concat(&[tone(-6.0206, 2.0), silence(2.0)]));
        assert!((r.rms_dbfs - (-12.0412)).abs() < 0.02, "RMS {}", r.rms_dbfs);
    }

    /// The peak is the largest sample of either channel.
    #[test]
    fn peak_takes_the_louder_channel() {
        let mut s = silence(1.0);
        s[1001] = -0.5; // right channel
        s[2000] = 0.25; // left channel
        let r = stereo(&s);
        assert!(
            (r.sample_peak_dbfs - (-6.0206)).abs() < 1e-3,
            "{}",
            r.sample_peak_dbfs
        );
    }

    /// Digital silence is the floor, never `-inf`.
    #[test]
    fn silence_reads_as_the_floor() {
        let r = stereo(&silence(2.0));
        assert_eq!(r.rms_dbfs, DB_FLOOR);
        assert_eq!(r.sample_peak_dbfs, DB_FLOOR);
        assert_eq!(r.noise_floor_dbfs, DB_FLOOR);
        assert!(
            r.integrated_lufs.is_none(),
            "all of it is under the absolute gate"
        );
        assert_eq!(r.clipping.events, 0);
    }

    // ---- clipping --------------------------------------------------------

    /// Mono 0.5 sine for 2 s with flat tops of `run` samples at full scale
    /// starting at each of `at` seconds.
    fn clipped_at(at: &[f64], run: usize) -> Vec<f32> {
        let n = SR as usize * 2;
        let mut s: Vec<f32> = (0..n)
            .map(|i| (0.5 * (std::f64::consts::TAU * 440.0 * i as f64 / SR as f64).sin()) as f32)
            .collect();
        for &t in at {
            let start = (t * SR as f64) as usize;
            for v in &mut s[start..start + run] {
                *v = 1.0;
            }
        }
        s
    }

    /// Three clipped stretches are three events, and the report says where each starts.
    #[test]
    fn clipping_is_counted_and_placed() {
        let s = clipped_at(&[0.25, 0.75, 1.5], 8);
        let r = measure(&s, SR, 1, QcOptions::default()).unwrap();
        assert_eq!(r.clipping.events, 3);
        assert_eq!(r.clipping.samples, 24);
        let want = [0.25, 0.75, 1.5];
        assert_eq!(r.clipping.first_times_s.len(), 3);
        for (got, want) in r.clipping.first_times_s.iter().zip(want) {
            assert!((got - want).abs() < 1e-4, "{got} vs {want}");
        }
    }

    /// Only the first N times are listed; the count is still all of them.
    #[test]
    fn only_the_first_n_clip_times_are_listed() {
        let s = clipped_at(&[0.1, 0.3, 0.5, 0.7, 0.9, 1.1], 5);
        let r = measure(&s, SR, 1, QcOptions { max_clip_times: 4 }).unwrap();
        assert_eq!(r.clipping.events, 6);
        assert_eq!(r.clipping.first_times_s.len(), 4);
        assert!((r.clipping.first_times_s[3] - 0.7).abs() < 1e-4);
    }

    /// One or two full-scale samples are a peak, not a clip.
    #[test]
    fn a_peak_that_touches_full_scale_is_not_a_clip() {
        let s = clipped_at(&[0.5], CLIP_MIN_RUN - 1);
        let r = measure(&s, SR, 1, QcOptions::default()).unwrap();
        assert_eq!(r.clipping.events, 0);
        assert_eq!(r.sample_peak_dbfs, 0.0);
    }

    /// Both channels clipped at once is one event; the samples of both count.
    #[test]
    fn clipping_on_both_channels_at_once_is_one_event() {
        let mono = clipped_at(&[0.5], 6);
        let both: Vec<f32> = mono.iter().flat_map(|&v| [v, v]).collect();
        let r = measure(&both, SR, 2, QcOptions::default()).unwrap();
        assert_eq!(r.clipping.events, 1);
        assert_eq!(r.clipping.samples, 12);
    }

    /// A negative flat top clips too.
    #[test]
    fn negative_clipping_counts() {
        let mut s = clipped_at(&[], 0);
        for v in &mut s[1000..1010] {
            *v = -1.0;
        }
        let r = measure(&s, SR, 1, QcOptions::default()).unwrap();
        assert_eq!(r.clipping.events, 1);
    }

    // ---- noise floor and contrast ----------------------------------------

    /// White noise (a fixed LCG, so the test is the same every run) scaled to
    /// an RMS of `dbfs`. Uniform noise on [-1, 1] has an RMS of 1 / sqrt(3).
    fn noise(dbfs: f64, seconds: f64, seed: u64) -> Vec<f32> {
        let n = (seconds * SR as f64).round() as usize;
        let mut x = seed;
        let gain = db_to_lin(dbfs) * 3f64.sqrt();
        (0..n)
            .map(|_| {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let u = ((x >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
                (u * gain) as f32
            })
            .collect()
    }

    fn mono_tone(dbrms: f64, seconds: f64) -> Vec<f32> {
        let n = (seconds * SR as f64).round() as usize;
        let a = db_to_lin(dbrms) * 2f64.sqrt();
        (0..n)
            .map(|i| (a * (std::f64::consts::TAU * 300.0 * i as f64 / SR as f64).sin()) as f32)
            .collect()
    }

    /// 1.5 s of room tone at -62 dBFS, 3 s of "speech" at -20 dBFS RMS, 1 s of
    /// room tone: the noise floor is the room tone and it starts at 0.
    #[test]
    fn the_noise_floor_is_the_quietest_half_second() {
        let s = concat(&[
            noise(-62.0, 1.5, 7),
            mono_tone(-20.0, 3.0),
            noise(-62.0, 1.0, 9),
        ]);
        let r = measure(&s, SR, 1, QcOptions::default()).unwrap();
        assert!(
            (r.noise_floor_dbfs - (-62.0)).abs() < 0.3,
            "noise floor {}",
            r.noise_floor_dbfs
        );
    }

    /// The report says where the quietest stretch is.
    #[test]
    fn the_noise_floor_says_where_it_is() {
        let s = concat(&[
            mono_tone(-20.0, 2.0),
            noise(-55.0, 1.0, 3),
            mono_tone(-20.0, 2.0),
        ]);
        let r = measure(&s, SR, 1, QcOptions::default()).unwrap();
        assert!(
            (r.noise_floor_dbfs - (-55.0)).abs() < 0.3,
            "{}",
            r.noise_floor_dbfs
        );
        assert!(
            r.noise_floor_at_s >= 1.5 && r.noise_floor_at_s <= 2.5,
            "the room tone is 2.0 s to 3.0 s, reported at {}",
            r.noise_floor_at_s
        );
    }

    /// Audio shorter than the window is read whole.
    #[test]
    fn a_clip_shorter_than_the_window_is_one_window() {
        let r = measure(&noise(-40.0, 0.2, 5), SR, 1, QcOptions::default()).unwrap();
        assert!(
            (r.noise_floor_dbfs - (-40.0)).abs() < 0.5,
            "{}",
            r.noise_floor_dbfs
        );
    }

    /// Speech at -20 dBFS over room tone at -55: 35 dB of contrast, which
    /// passes WCAG's 20 dB; with room tone at -35 it is 15 dB, which fails.
    #[test]
    fn contrast_is_speech_over_background() {
        let loud = concat(&[
            noise(-55.0, 1.0, 1),
            mono_tone(-20.0, 6.0),
            noise(-55.0, 1.0, 2),
        ]);
        let r = measure(&loud, SR, 1, QcOptions::default()).unwrap();
        assert!(
            (r.contrast.difference_db - 35.0).abs() < 0.8,
            "contrast {:?}",
            r.contrast
        );
        assert!(r.contrast.wcag_pass);

        let muddy = concat(&[
            noise(-35.0, 1.0, 1),
            mono_tone(-20.0, 6.0),
            noise(-35.0, 1.0, 2),
        ]);
        let r = measure(&muddy, SR, 1, QcOptions::default()).unwrap();
        assert!(
            (r.contrast.difference_db - 15.0).abs() < 0.8,
            "contrast {:?}",
            r.contrast
        );
        assert!(!r.contrast.wcag_pass);
    }

    // ---- the stream ------------------------------------------------------

    /// Chunks of any size give the answer one push does.
    #[test]
    fn chunked_pushes_equal_one_push() {
        let s = concat(&[
            noise(-60.0, 1.0, 4),
            tone(-18.0, 3.5),
            silence(0.5),
            tone(-30.0, 2.0),
        ]);
        let whole = measure(&s, SR, 2, QcOptions::default()).unwrap();
        let mut meter = QcMeter::new(SR, 2, QcOptions::default()).unwrap();
        // 977 frames is not a divisor of a hop, a block or a second.
        for chunk in s.chunks(977 * 2) {
            meter.push(chunk).unwrap();
        }
        let chunked = meter.finish().unwrap();
        assert_eq!(whole, chunked);
    }

    #[test]
    fn a_zero_rate_or_zero_channels_is_refused() {
        assert!(QcMeter::new(0, 2, QcOptions::default()).is_err());
        assert!(QcMeter::new(SR, 0, QcOptions::default()).is_err());
    }

    /// A buffer that is not a whole number of frames is refused, not padded.
    #[test]
    fn a_partial_frame_is_refused() {
        let mut meter = QcMeter::new(SR, 2, QcOptions::default()).unwrap();
        assert!(meter.push(&[0.0, 0.0, 0.0]).is_err());
    }

    /// Nothing pushed is an error: there is nothing to report.
    #[test]
    fn no_audio_is_an_error() {
        let meter = QcMeter::new(SR, 2, QcOptions::default()).unwrap();
        assert!(meter.finish().is_err());
    }

    /// Under 400 ms there is no gating block and under 3 s no short-term one.
    #[test]
    fn short_audio_leaves_the_long_window_numbers_empty() {
        let r = stereo(&tone(-23.0, 0.3));
        assert!(r.integrated_lufs.is_none() && r.momentary_max_lufs.is_none());
        let r = stereo(&tone(-23.0, 2.0));
        assert!(r.integrated_lufs.is_some() && r.momentary_max_lufs.is_some());
        assert!(r.short_term_max_lufs.is_none() && r.loudness_range_lu.is_none());
    }

    /// 44.1 kHz and mono, to be sure nothing is written for 48 kHz stereo.
    #[test]
    fn mono_at_44_1_khz_reads_the_same_level() {
        let n = 44_100 * 10;
        let a = db_to_lin(-23.0);
        let s: Vec<f32> = (0..n)
            .map(|i| (a * (std::f64::consts::TAU * 1000.0 * i as f64 / 44_100.0).sin()) as f32)
            .collect();
        let r = measure(&s, 44_100, 1, QcOptions::default()).unwrap();
        // One channel at -23 dBFS peak is 3.01 LU under two of them.
        near("I", r.integrated_lufs, -26.01, 0.1);
        assert!((r.rms_dbfs - (-26.01)).abs() < 0.05, "{}", r.rms_dbfs);
    }
}
