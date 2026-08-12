//! FFT magnitude spectrum of a track region.
//!
//! The result has two audiences with opposite needs, and they are served
//! by two different parts of the same document.
//!
//! The **chart** wants every bin — `points` is the full curve, and the
//! agent loop lifts it out into an `ai::ToolView` that reaches the UI
//! over IPC.
//!
//! The **model** cannot read a spectrum out of 2048 unlabelled float
//! pairs, and used to be handed all of them anyway: at 44.1 kHz that is
//! ~83 KB, roughly 24k tokens, on every call, re-sent on every
//! subsequent round trip of the conversation. So the analysis a model
//! would otherwise have to infer — where the peak is, how the energy
//! splits across bands, how bright it is, where the noise floor sits —
//! is computed here and emitted as a handful of scalars. The loop drops
//! `points` before the result reaches the model.

use rustfft::{num_complex::Complex, FftPlanner};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{check_track_index, flatten_track, load_head_state};
use crate::{Tool, ToolContext, ToolResult};

const FFT_SIZE: usize = 4096;

/// Anything quieter than this reads as digital silence and is reported
/// as the floor rather than as a very negative number.
const DB_FLOOR: f32 = -120.0;

/// Named frequency bands, in the divisions mix engineers actually talk
/// in. `hi` is exclusive; the last band is clamped to Nyquist.
const BANDS: &[(&str, f32, f32)] = &[
    ("sub", 20.0, 60.0),
    ("bass", 60.0, 250.0),
    ("low_mid", 250.0, 500.0),
    ("mid", 500.0, 2_000.0),
    ("high_mid", 2_000.0, 6_000.0),
    ("air", 6_000.0, 20_000.0),
];

/// Fraction of total energy that defines the rolloff point.
const ROLLOFF_FRACTION: f32 = 0.85;

/// Linear magnitude per bin, normalised by the transform length.
///
/// Linear rather than dB because every statistic below — band energy,
/// centroid, rolloff — is a sum or a ratio, and those are meaningless
/// on a logarithmic scale. dB is applied at the edge, per point.
pub(crate) fn compute_fft_magnitude(samples: &[f32], fft_size: usize) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    let mut buf: Vec<Complex<f32>> = (0..fft_size)
        .map(|i| {
            let window =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
            let s = samples.get(i).copied().unwrap_or(0.0);
            Complex::new(s * window, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    (0..fft_size / 2)
        .map(|i| buf[i].norm() / fft_size as f32)
        .collect()
}

pub(crate) fn to_db(mag: f32) -> f32 {
    if mag > 1e-10 {
        (20.0 * mag.log10()).max(DB_FLOOR)
    } else {
        DB_FLOOR
    }
}

/// The scalars the model gets in place of the curve.
pub(crate) struct SpectrumStats {
    pub peak_hz: f32,
    pub peak_db: f32,
    pub centroid_hz: f32,
    pub rolloff_hz: f32,
    pub noise_floor_db: f32,
    /// `(name, dbfs)` in `BANDS` order. RMS across the band's bins.
    pub bands: Vec<(&'static str, f32)>,
}

/// Reduce a magnitude spectrum to the handful of numbers that answer the
/// questions a spectrum is usually consulted for.
pub(crate) fn summarise(mags: &[f32], bin_hz: f32) -> SpectrumStats {
    let hz_of = |i: usize| i as f32 * bin_hz;

    let (peak_bin, peak_mag) =
        mags.iter().enumerate().fold(
            (0usize, 0.0f32),
            |acc, (i, &m)| {
                if m > acc.1 {
                    (i, m)
                } else {
                    acc
                }
            },
        );

    // Centroid is the magnitude-weighted mean frequency — the standard
    // one-number stand-in for "brightness".
    let total: f32 = mags.iter().sum();
    let centroid_hz = if total > 0.0 {
        mags.iter()
            .enumerate()
            .map(|(i, &m)| hz_of(i) * m)
            .sum::<f32>()
            / total
    } else {
        0.0
    };

    // Rolloff is defined on energy, not amplitude, hence the squares.
    let total_energy: f32 = mags.iter().map(|m| m * m).sum();
    let mut rolloff_hz = 0.0;
    if total_energy > 0.0 {
        let target = total_energy * ROLLOFF_FRACTION;
        let mut acc = 0.0;
        for (i, &m) in mags.iter().enumerate() {
            acc += m * m;
            if acc >= target {
                rolloff_hz = hz_of(i);
                break;
            }
        }
    }

    // Median, not mean: a spectrum is mostly floor with a few loud bins,
    // and the mean would be dragged up by exactly the bins that are not
    // the floor.
    let noise_floor_db = {
        let mut sorted = mags.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        to_db(sorted.get(sorted.len() / 2).copied().unwrap_or(0.0))
    };

    let nyquist = hz_of(mags.len());
    let bands = BANDS
        .iter()
        .map(|&(name, lo, hi)| {
            let hi = hi.min(nyquist);
            let lo_bin = (lo / bin_hz).ceil() as usize;
            let hi_bin = ((hi / bin_hz).floor() as usize).min(mags.len());
            if lo_bin >= hi_bin {
                return (name, DB_FLOOR);
            }
            let slice = &mags[lo_bin..hi_bin];
            let rms = (slice.iter().map(|m| m * m).sum::<f32>() / slice.len() as f32).sqrt();
            (name, to_db(rms))
        })
        .collect();

    SpectrumStats {
        peak_hz: hz_of(peak_bin),
        peak_db: to_db(peak_mag),
        centroid_hz,
        rolloff_hz,
        noise_floor_db,
        bands,
    }
}

fn format_hz(hz: f32) -> String {
    if hz >= 1_000.0 {
        format!("{:.1} kHz", hz / 1_000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    start_sec: f64,
    end_sec: f64,
}

pub struct PlotSpectrumTool;

impl Tool for PlotSpectrumTool {
    fn name(&self) -> &'static str {
        "plot_spectrum"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "plot_spectrum",
            "Compute the frequency spectrum of a track region and show the user a chart of it. \
             Returns the analysis you need as numbers: peak frequency and level, energy per \
             band (sub/bass/low_mid/mid/high_mid/air) in dBFS, spectral centroid (brightness), \
             85% rolloff, and the noise floor. Use these to decide on EQ moves. Does not modify \
             audio.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer" },
                    "start_sec": { "type": "number", "description": "Region start in seconds" },
                    "end_sec": { "type": "number", "description": "Region end in seconds" }
                },
                "required": ["track", "start_sec", "end_sec"]
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if args.start_sec >= args.end_sec {
            return Ok(ToolResult::Error("start_sec must be < end_sec".into()));
        }
        let state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if let Err(e) = check_track_index(&state.tracks, args.track) {
            return Ok(ToolResult::Error(e));
        }
        let clips = &state.tracks[args.track].clips;
        if clips.is_empty() {
            return Ok(ToolResult::Error(format!(
                "track {} has no clips",
                args.track
            )));
        }
        // Seconds are positions on the track, so the buffer has to be the
        // track. Indexing `clips[0]`'s source file put the window at the
        // wrong place the moment anything had been cut, and never saw the
        // second clip of a split track at all.
        let audio = match flatten_track(clips) {
            Ok(a) => a,
            Err(msg) => return Ok(ToolResult::Error(msg)),
        };
        let sr = audio.sample_rate;
        let channels = (audio.channels as usize).max(1);
        let total_frames = audio.window.len() / channels;
        let start_frame = ((args.start_sec * sr as f64) as usize).min(total_frames);
        let end_frame = ((args.end_sec * sr as f64) as usize).min(total_frames);
        let mono: Vec<f32> = (start_frame..end_frame)
            .map(|f| {
                (0..channels)
                    .map(|ch| audio.window[f * channels + ch])
                    .sum::<f32>()
                    / channels as f32
            })
            .collect();
        let magnitudes = compute_fft_magnitude(&mono, FFT_SIZE);
        let bin_hz = sr as f32 / FFT_SIZE as f32;
        let stats = summarise(&magnitudes, bin_hz);

        let points: Vec<Value> = magnitudes
            .iter()
            .enumerate()
            .map(|(i, &m)| json!({ "hz": i as f32 * bin_hz, "db": to_db(m) }))
            .collect();

        let bands: serde_json::Map<String, Value> = stats
            .bands
            .iter()
            .map(|&(name, db)| {
                (
                    name.to_string(),
                    json!((db * 10.0).round() / 10.0), // one decimal is plenty
                )
            })
            .collect();

        let summary = format!(
            "Track {} {:.2}\u{2013}{:.2}s: peak {} at {:.1} dBFS, centroid {}, \
             85% rolloff {}, noise floor {:.1} dBFS.",
            args.track,
            args.start_sec,
            args.end_sec,
            format_hz(stats.peak_hz),
            stats.peak_db,
            format_hz(stats.centroid_hz),
            format_hz(stats.rolloff_hz),
            stats.noise_floor_db,
        );

        Ok(ToolResult::Ok(json!({
            "type": "spectrum",
            "track": args.track,
            "start_sec": args.start_sec,
            "end_sec": args.end_sec,
            "sample_rate": sr,
            "fft_size": FFT_SIZE,
            "peak_hz": (stats.peak_hz * 10.0).round() / 10.0,
            "peak_db": (stats.peak_db * 10.0).round() / 10.0,
            "centroid_hz": (stats.centroid_hz * 10.0).round() / 10.0,
            "rolloff_hz": (stats.rolloff_hz * 10.0).round() / 10.0,
            "noise_floor_db": (stats.noise_floor_db * 10.0).round() / 10.0,
            "bands_dbfs": bands,
            // The chart's copy. Lifted into an `ai::ToolView` and then
            // stripped before the result reaches the model — see the
            // module docs and `agent_loop::strip_view_only_fields`.
            "points": points,
            "summary": summary,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 440 Hz sine at 44.1 kHz, one second.
    fn sine(hz: f32, sr: u32) -> Vec<f32> {
        (0..sr)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / sr as f32).sin())
            .collect()
    }

    #[test]
    fn sine_440hz_peak_near_440() {
        let sr = 44100u32;
        let bins = compute_fft_magnitude(&sine(440.0, sr), 4096);
        let peak_bin = bins
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        let peak_freq = peak_bin as f32 * sr as f32 / 4096.0;
        assert!(
            (peak_freq - 440.0).abs() < 20.0,
            "peak at {peak_freq}Hz, expected ~440Hz"
        );
    }

    #[test]
    fn summary_reports_the_peak_frequency() {
        let sr = 44100u32;
        let mags = compute_fft_magnitude(&sine(440.0, sr), 4096);
        let stats = summarise(&mags, sr as f32 / 4096.0);
        assert!(
            (stats.peak_hz - 440.0).abs() < 20.0,
            "peak_hz {} should be ~440",
            stats.peak_hz
        );
        assert!(
            stats.peak_db > DB_FLOOR,
            "a full-scale sine should not report the floor"
        );
    }

    /// The whole point of the band split: a model asking "is it boomy or
    /// harsh" must be able to tell those apart from the numbers alone.
    #[test]
    fn band_energies_follow_the_tone() {
        let sr = 44100u32;
        let bin_hz = sr as f32 / 4096.0;
        let low = summarise(&compute_fft_magnitude(&sine(100.0, sr), 4096), bin_hz);
        let high = summarise(&compute_fft_magnitude(&sine(8_000.0, sr), 4096), bin_hz);

        let band = |s: &SpectrumStats, name: &str| {
            s.bands
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, db)| *db)
                .expect("band present")
        };

        assert!(
            band(&low, "bass") > band(&low, "air") + 20.0,
            "a 100 Hz tone must read as bass-dominant: bass {:.1} vs air {:.1}",
            band(&low, "bass"),
            band(&low, "air")
        );
        assert!(
            band(&high, "air") > band(&high, "bass") + 20.0,
            "an 8 kHz tone must read as air-dominant: air {:.1} vs bass {:.1}",
            band(&high, "air"),
            band(&high, "bass")
        );
    }

    /// Centroid is the brightness proxy the model will reason with, so
    /// it has to move in the obvious direction.
    #[test]
    fn centroid_rises_with_pitch() {
        let sr = 44100u32;
        let bin_hz = sr as f32 / 4096.0;
        let low = summarise(&compute_fft_magnitude(&sine(200.0, sr), 4096), bin_hz);
        let high = summarise(&compute_fft_magnitude(&sine(5_000.0, sr), 4096), bin_hz);
        assert!(
            high.centroid_hz > low.centroid_hz * 4.0,
            "centroid {:.0} Hz for a 5 kHz tone vs {:.0} Hz for 200 Hz",
            high.centroid_hz,
            low.centroid_hz
        );
    }

    #[test]
    fn silence_reports_the_floor_without_dividing_by_zero() {
        let mags = compute_fft_magnitude(&vec![0.0; 4096], 4096);
        let stats = summarise(&mags, 44100.0 / 4096.0);
        assert_eq!(stats.peak_db, DB_FLOOR);
        assert_eq!(stats.centroid_hz, 0.0);
        assert_eq!(stats.rolloff_hz, 0.0);
        assert!(stats.bands.iter().all(|(_, db)| *db == DB_FLOOR));
    }
}
