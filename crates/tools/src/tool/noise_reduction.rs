//! Spectral noise-reduction tool — spectral subtraction with overlap-add.

use std::f32::consts::PI;

use realfft::RealFftPlanner;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    check_track_index, destructive_edit_then, flatten_track, load_head_state, parse_args,
    TrackAudio,
};
use crate::{Tool, ToolContext, ToolResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const FFT_SIZE: usize = 2048;
const HOP_SIZE: usize = 512; // 75 % overlap

// ---------------------------------------------------------------------------
// Core DSP — pub(crate) so unit tests can reach it without a ToolContext
// ---------------------------------------------------------------------------

/// Process a single mono channel through spectral subtraction (overlap-add).
///
/// * `mono`              – mono samples (one sample per frame)
/// * `noise_duration_sec` – seconds at the start of the clip that contain
///   only noise (used to build the noise profile)
/// * `strength` – how aggressively to subtract (0.0 … 1.0)
/// * `floor` – spectral floor as a fraction of the original
///   magnitude (prevents musical-noise artefacts)
/// * `sample_rate`       – samples per second (for converting seconds → frames)
pub(crate) fn process_channel(
    mono: &[f32],
    noise_duration_sec: f32,
    strength: f32,
    floor: f32,
    sample_rate: u32,
) -> Vec<f32> {
    process_channel_with_profile(mono, noise_duration_sec, None, strength, floor, sample_rate)
}

/// [`process_channel`] with the noise profile optionally learned from
/// `profile_range` (samples of `mono` or of another channel at the same
/// rate) instead of the first `noise_duration_sec` seconds of `mono`.
/// `None` is exactly [`process_channel`].
pub(crate) fn process_channel_with_profile(
    mono: &[f32],
    noise_duration_sec: f32,
    profile_range: Option<&[f32]>,
    strength: f32,
    floor: f32,
    sample_rate: u32,
) -> Vec<f32> {
    if mono.is_empty() {
        return Vec::new();
    }

    let n = mono.len();

    // --- Build Hann window --------------------------------------------------
    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / FFT_SIZE as f32).cos()))
        .collect();

    // --- FFT planner --------------------------------------------------------
    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT_SIZE);
    let c2r = planner.plan_fft_inverse(FFT_SIZE);
    let num_bins = FFT_SIZE / 2 + 1;

    // --- Build noise profile ------------------------------------------------
    let noise_profile = match profile_range {
        Some(range) => profile_of(range, &window, &r2c),
        None => leading_profile(mono, noise_duration_sec, sample_rate, &window, &r2c),
    };

    let mut frame_buf = vec![0.0f32; FFT_SIZE];
    let mut spectrum = r2c.make_output_vec();

    // --- Overlap-add output buffers -----------------------------------------
    //
    // The signal is read as if one frame of it were reflected beyond each end,
    // and the result trimmed back to `n`. Without that, the first and last
    // samples are covered by one frame at the tail of its window, where the
    // window squared is near zero, and the division below multiplies the
    // spectral-subtraction error (which the window does not scale) by up to
    // 1/w^2: a full-scale click in the first few milliseconds (#692). With a
    // frame of margin every real sample sits under the full overlap, whose
    // window-squared sum is a constant 1.5. Reflecting, not zero-filling, keeps
    // the frames that straddle an end from being half silence, which the
    // subtraction would take for a quieter signal. The frame grid moves by
    // FFT_SIZE, a multiple of HOP_SIZE, so interior frames are unchanged, and
    // the noise profile above reads `mono` itself, not the margin.
    let padded_len = n + 2 * FFT_SIZE;
    let mut output = vec![0.0f32; padded_len];
    let mut norm = vec![0.0f32; padded_len];

    // --- STFT subtraction loop ----------------------------------------------
    let mut ifft_buf = vec![0.0f32; FFT_SIZE];

    let mut pos = 0usize;
    // A frame starting at or before the last real sample covers it; later
    // frames hold only the margin.
    while pos < FFT_SIZE + n {
        for (i, fb) in frame_buf.iter_mut().enumerate() {
            *fb = reflected(mono, (pos + i) as isize - FFT_SIZE as isize) * window[i];
        }

        // Forward FFT.
        r2c.process(&mut frame_buf, &mut spectrum).unwrap();

        // Spectral subtraction.
        for (bin, s) in spectrum.iter_mut().enumerate() {
            let mag = s.norm();
            let phase = s.arg();
            let reduced = (mag - noise_profile[bin] * strength).max(mag * floor);
            *s = realfft::num_complex::Complex::from_polar(reduced, phase);
        }

        // realfft requires DC (bin 0) and Nyquist (last bin) to be purely real —
        // enforce this after the magnitude-preserving phase reconstruction above
        // since floating-point operations may leave tiny imaginary residues.
        spectrum[0].im = 0.0;
        let last = num_bins - 1;
        spectrum[last].im = 0.0;

        // Inverse FFT.
        c2r.process(&mut spectrum, &mut ifft_buf).unwrap();

        // Normalise by FFT_SIZE (realfft does not normalise inverse).
        let scale = 1.0 / FFT_SIZE as f32;
        for (i, v) in ifft_buf.iter().enumerate() {
            output[pos + i] += v * scale * window[i];
            norm[pos + i] += window[i] * window[i];
        }

        pos += HOP_SIZE;
    }

    // --- Divide by overlap-add normalisation factor, drop the margin --------
    output[FFT_SIZE..FFT_SIZE + n]
        .iter()
        .zip(&norm[FFT_SIZE..FFT_SIZE + n])
        .map(|(o, nm)| if *nm > 1e-10 { o / nm } else { *o })
        .collect()
}

/// The profile as it has always been built: frames from the start of `mono`
/// up to `noise_duration_sec`. Kept verbatim so a call without a profile
/// range gives the same bytes as before.
fn leading_profile(
    mono: &[f32],
    noise_duration_sec: f32,
    sample_rate: u32,
    window: &[f32],
    r2c: &std::sync::Arc<dyn realfft::RealToComplex<f32>>,
) -> Vec<f32> {
    let n = mono.len();
    let num_bins = FFT_SIZE / 2 + 1;
    let profile_samples = (noise_duration_sec * sample_rate as f32).round() as usize;
    let profile_samples = profile_samples.min(n);

    let mut noise_profile = vec![0.0f32; num_bins];
    let mut profile_frame_count = 0usize;

    let mut frame_buf = vec![0.0f32; FFT_SIZE];
    let mut spectrum = r2c.make_output_vec();

    let mut pos = 0usize;
    while pos + FFT_SIZE <= profile_samples.max(FFT_SIZE) && pos + FFT_SIZE <= n {
        for (i, fb) in frame_buf.iter_mut().enumerate() {
            *fb = if pos + i < n { mono[pos + i] } else { 0.0 } * window[i];
        }
        r2c.process(&mut frame_buf, &mut spectrum).unwrap();
        for (bin, s) in spectrum.iter().enumerate() {
            noise_profile[bin] += s.norm();
        }
        profile_frame_count += 1;
        pos += HOP_SIZE;
        if pos >= profile_samples {
            break;
        }
    }

    if profile_frame_count > 0 {
        let count = profile_frame_count as f32;
        for v in &mut noise_profile {
            *v /= count;
        }
    }
    noise_profile
}

/// Mean magnitude spectrum of every whole frame (hop `HOP_SIZE`) in `range`.
/// The caller guarantees `range.len() >= FFT_SIZE`.
fn profile_of(
    range: &[f32],
    window: &[f32],
    r2c: &std::sync::Arc<dyn realfft::RealToComplex<f32>>,
) -> Vec<f32> {
    let mut noise_profile = vec![0.0f32; FFT_SIZE / 2 + 1];
    let mut frame_buf = vec![0.0f32; FFT_SIZE];
    let mut spectrum = r2c.make_output_vec();
    let mut frames = 0usize;
    let mut pos = 0usize;
    while pos + FFT_SIZE <= range.len() {
        for (i, fb) in frame_buf.iter_mut().enumerate() {
            *fb = range[pos + i] * window[i];
        }
        r2c.process(&mut frame_buf, &mut spectrum).unwrap();
        for (bin, s) in spectrum.iter().enumerate() {
            noise_profile[bin] += s.norm();
        }
        frames += 1;
        pos += HOP_SIZE;
    }
    if frames > 0 {
        for v in &mut noise_profile {
            *v /= frames as f32;
        }
    }
    noise_profile
}

/// `mono[i]`, with `i` outside the clip read as the clip reflected about its
/// first and last samples (period `2 * (n - 1)`); a one-sample clip repeats.
fn reflected(mono: &[f32], i: isize) -> f32 {
    let n = mono.len() as isize;
    if n == 1 {
        return mono[0];
    }
    let period = 2 * (n - 1);
    let m = i.rem_euclid(period);
    mono[if m < n { m } else { period - m } as usize]
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

fn default_strength() -> f32 {
    0.85
}

fn default_floor() -> f32 {
    0.05
}

#[derive(Debug, Deserialize)]
struct Args {
    track: usize,
    #[serde(default)]
    noise_duration_sec: Option<f32>,
    #[serde(default)]
    noise_from: Option<f32>,
    #[serde(default)]
    noise_to: Option<f32>,
    #[serde(default)]
    noise_track: Option<usize>,
    #[serde(default = "default_strength")]
    strength: f32,
    #[serde(default = "default_floor")]
    floor: f32,
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

pub struct NoiseReductionTool;

impl Tool for NoiseReductionTool {
    fn name(&self) -> &'static str {
        "noise_reduction"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "noise_reduction",
            "Denoise a track. \
             It uses spectral subtraction with overlap-add; the first `noise_duration_sec` \
             seconds of the clip are used as the noise profile. \
             `strength` controls how aggressively noise is subtracted (default 0.85); \
             `floor` sets the minimum fraction of the original magnitude retained to avoid \
             musical-noise artefacts (default 0.05). To learn the noise from somewhere else, pass \
             `noise_from` and `noise_to` (seconds on the track's timeline, both together; \
             `noise_duration_sec` is then not needed) and optionally `noise_track` (another \
             track at the same sample rate, e.g. separately recorded room tone; default: the \
             track itself). The range must be at least 2048 frames long. Appends a new \
             session node.",
            json!({
                "type": "object",
                "required": ["track"],
                "additionalProperties": false,
                "properties": {
                    "track": { "type": "integer" },
                    "noise_duration_sec": { "type": "number" },
                    "noise_from": { "type": "number" },
                    "noise_to": { "type": "number" },
                    "noise_track": { "type": "integer" },
                    "strength": { "type": "number" },
                    "floor": { "type": "number" }
                }
            }),
        )
    }

    // A pure edit of the head's audio and one node on top of it: safe on a
    // staged store, and safe to run again on a newer head (#421).
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let parsed: Args = match parse_args(args) {
            Ok(a) => a,
            Err(r) => return Ok(r),
        };

        let range = match (parsed.noise_from, parsed.noise_to) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => {
                return Ok(ToolResult::Error(
                    "noise_from and noise_to must be given together".to_string(),
                ))
            }
        };
        if range.is_none() && parsed.noise_track.is_some() {
            return Ok(ToolResult::Error(
                "noise_track needs noise_from and noise_to".to_string(),
            ));
        }
        if range.is_some() && parsed.noise_duration_sec.is_some() {
            return Ok(ToolResult::Error(
                "noise_duration_sec cannot be combined with noise_from and noise_to".to_string(),
            ));
        }
        let noise_duration_sec = match (parsed.noise_duration_sec, range) {
            (Some(d), _) => d,
            // Unused when a range names the profile.
            (None, Some(_)) => 0.0,
            (None, None) => {
                return Ok(ToolResult::Error(
                    "noise_duration_sec is required unless noise_from and noise_to are given"
                        .to_string(),
                ))
            }
        };
        if range.is_none() && (!noise_duration_sec.is_finite() || noise_duration_sec <= 0.0) {
            return Ok(ToolResult::Error(
                "noise_duration_sec must be > 0 and finite".to_string(),
            ));
        }
        if let Some((a, b)) = range {
            if !a.is_finite() || !b.is_finite() || a < 0.0 || b <= a {
                return Ok(ToolResult::Error(format!(
                    "profile range must be finite with 0 <= noise_from < noise_to (got {a} to {b})"
                )));
            }
        }
        if !(0.0..=1.0).contains(&parsed.strength) {
            return Ok(ToolResult::Error(format!(
                "strength must be in [0.0, 1.0] (got {})",
                parsed.strength
            )));
        }
        if !(0.0..=1.0).contains(&parsed.floor) {
            return Ok(ToolResult::Error(format!(
                "floor must be in [0.0, 1.0] (got {})",
                parsed.floor
            )));
        }

        Ok(invoke_noise_reduction(
            ctx,
            parsed.track,
            noise_duration_sec,
            range.map(|(a, b)| ProfileSpec {
                start: a,
                end: b,
                track: parsed.noise_track,
            }),
            parsed.strength,
            parsed.floor,
        ))
    }
}

/// Core noise-reduction logic.
///
/// Formerly a hand-copy of `destructive_edit`, made because the spectral
/// subtraction runs per channel and the shared helper didn't hand over
/// the channel count. It does now, and the copy had drifted — it still
/// edited only `clips[0]`, so a track split by an interior cut kept its
/// noise floor on everything after the cut.
fn invoke_noise_reduction(
    ctx: &mut ToolContext,
    track_idx: usize,
    noise_duration_sec: f32,
    profile: Option<ProfileSpec>,
    strength: f32,
    floor: f32,
) -> ToolResult {
    let label = match &profile {
        None => format!(
            "noise_reduction (profile={noise_duration_sec:.2}s, strength={strength:.2}, floor={floor:.2}) on track {track_idx}"
        ),
        Some(p) => format!(
            "noise_reduction (profile={:.2}s-{:.2}s of track {}, strength={strength:.2}, floor={floor:.2}) on track {track_idx}",
            p.start,
            p.end,
            p.track.unwrap_or(track_idx)
        ),
    };

    // Audio of another track to learn the profile from, read before the edit
    // so a bad track refuses with the session untouched.
    let other: Option<TrackAudio> = match profile.as_ref().and_then(|p| p.track) {
        Some(t) if t != track_idx => {
            let state = match load_head_state(ctx) {
                Ok(s) => s,
                Err(m) => return ToolResult::Error(m),
            };
            if let Err(m) = check_track_index(&state.tracks, t) {
                return ToolResult::Error(m);
            }
            if state.tracks[t].clips.is_empty() {
                return ToolResult::Error(format!("profile track {t} has no clips"));
            }
            match flatten_track(&state.tracks[t].clips) {
                Ok(a) => Some(a),
                Err(m) => return ToolResult::Error(m),
            }
        }
        _ => None,
    };

    destructive_edit_then(
        ctx,
        track_idx,
        move |samples, sample_rate, channels| {
            let channels = channels.max(1) as usize;
            let num_frames = samples.len() / channels;

            // Per channel, the frames the profile is learned from.
            let (pr_stride, pr_rate, pr_frames): (usize, u32, usize) = match (&profile, &other) {
                (None, _) => (1, sample_rate, 0),
                (Some(_), Some(o)) => (
                    o.channels.max(1) as usize,
                    o.sample_rate,
                    o.window.len() / o.channels.max(1) as usize,
                ),
                (Some(_), None) => (channels, sample_rate, num_frames),
            };
            let mut bounds = None;
            if let Some(p) = &profile {
                if pr_rate != sample_rate {
                    return Err(format!(
                        "profile track is at {pr_rate} Hz but track {track_idx} is at {sample_rate} Hz; \
                         resample one of them first"
                    ));
                }
                let from = (p.start as f64 * sample_rate as f64).round() as usize;
                let to = (p.end as f64 * sample_rate as f64).round() as usize;
                if to > pr_frames {
                    return Err(format!(
                        "profile range {:.3}s-{:.3}s runs past the end of the track ({:.3}s)",
                        p.start,
                        p.end,
                        pr_frames as f64 / sample_rate as f64
                    ));
                }
                if to.saturating_sub(from) < FFT_SIZE {
                    return Err(format!(
                        "profile range is {} frames; it needs at least {FFT_SIZE} ({:.0} ms at {sample_rate} Hz)",
                        to.saturating_sub(from),
                        FFT_SIZE as f64 * 1000.0 / sample_rate as f64
                    ));
                }
                // Both buffers are laid out from track frame 0, so a range in
                // a lead-in or a gap between clips is digital silence: an
                // all-zero profile subtracts nothing and the edit would report
                // success while changing (almost) nothing.
                let src: &[f32] = match &other {
                    Some(o) => &o.window,
                    None => samples,
                };
                if src[from * pr_stride..to * pr_stride]
                    .iter()
                    .all(|&s| s == 0.0)
                {
                    return Err(format!(
                        "profile range {:.3}s-{:.3}s is digital silence (no clip covers it, or the audio there is all zeros); \
                         pick a range that holds the noise",
                        p.start, p.end
                    ));
                }
                bounds = Some((from, to));
            }

            let mut result = vec![0.0f32; samples.len()];
            // Spectral subtraction runs per channel, so the interleaved
            // buffer is split apart and put back together around it.
            for ch in 0..channels {
                let mono: Vec<f32> = (0..num_frames)
                    .map(|f| samples[f * channels + ch])
                    .collect();
                let processed = match bounds {
                    None => {
                        process_channel(&mono, noise_duration_sec, strength, floor, sample_rate)
                    }
                    Some((from, to)) => {
                        // A profile track with fewer channels lends its last.
                        let src: &[f32] = match &other {
                            Some(o) => &o.window,
                            None => samples,
                        };
                        let pch = ch.min(pr_stride - 1);
                        let prof: Vec<f32> = (from..to).map(|f| src[f * pr_stride + pch]).collect();
                        process_channel_with_profile(
                            &mono,
                            noise_duration_sec,
                            Some(&prof),
                            strength,
                            floor,
                            sample_rate,
                        )
                    }
                };
                for (f, &s) in processed.iter().enumerate() {
                    if f < num_frames {
                        result[f * channels + ch] = s;
                    }
                }
            }
            *samples = result;
            Ok((sample_rate, channels as u16))
        },
        |_, _| serde_json::Map::default(),
        label,
    )
}

/// Where the noise is learned from when it is not the start of the track.
struct ProfileSpec {
    start: f32,
    end: f32,
    track: Option<usize>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        (sum / samples.len() as f32).sqrt()
    }

    #[test]
    fn output_same_length_as_input() {
        let sr = 44100u32;
        let n = sr as usize; // 1 second
        let input: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / sr as f32).sin())
            .collect();
        let output = process_channel(&input, 0.1, 0.85, 0.05, sr);
        assert_eq!(output.len(), input.len());
    }

    #[test]
    fn silent_input_stays_silent() {
        let sr = 44100u32;
        let n = sr as usize;
        let input = vec![0.0f32; n];
        let output = process_channel(&input, 0.1, 0.85, 0.05, sr);
        assert_eq!(output.len(), n);
        for (i, &s) in output.iter().enumerate() {
            assert!(
                s.abs() < 1e-5,
                "silent input produced non-zero output at sample {i}: {s}"
            );
        }
    }

    #[test]
    fn reduces_constant_noise_floor() {
        // Build: 0.5 s noise-only, then 0.5 s noise + loud tone.
        let sr = 44100u32;
        let half = sr as usize / 2;
        let noise_amp = 0.02f32;
        let tone_amp = 0.5f32;
        let tone_freq = 1000.0f32;

        let mut input = Vec::with_capacity(half * 2);
        // Noise-only region.
        for i in 0..half {
            // Simple pseudo-noise: sign-alternating scaled sine at high freq.
            let n = noise_amp * (2.0 * PI * 8000.0 * i as f32 / sr as f32).sin();
            input.push(n);
        }
        // Tone + noise region.
        for i in 0..half {
            let tone = tone_amp * (2.0 * PI * tone_freq * i as f32 / sr as f32).sin();
            let n = noise_amp * (2.0 * PI * 8000.0 * (i + half) as f32 / sr as f32).sin();
            input.push(tone + n);
        }

        let output = process_channel(&input, 0.5, 0.85, 0.05, sr);

        let noise_region_rms = rms(&output[..half]);
        let tone_region_rms = rms(&output[half..]);

        // The tone region should be substantially louder than the noise-only region.
        assert!(
            tone_region_rms > noise_region_rms * 5.0,
            "expected tone region RMS ({tone_region_rms:.4}) >> noise region RMS ({noise_region_rms:.4})"
        );
    }

    #[test]
    fn short_inputs_keep_their_length_and_stay_finite() {
        let sr = 44100u32;
        for n in [1usize, 2, 3, 100, FFT_SIZE - 1, FFT_SIZE, FFT_SIZE + 1] {
            let input: Vec<f32> = (0..n).map(|i| 0.1 * ((i * 7 % 13) as f32 - 6.0)).collect();
            let output = process_channel(&input, 0.01, 0.85, 0.05, sr);
            assert_eq!(output.len(), n);
            assert!(output.iter().all(|s| s.is_finite()), "n = {n}");
        }
    }

    #[test]
    fn reflection_mirrors_about_the_end_samples() {
        let x = [1.0f32, 2.0, 3.0, 4.0];
        let read: Vec<f32> = (-4..8).map(|i| reflected(&x, i)).collect();
        assert_eq!(
            read,
            [3.0, 4.0, 3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 2.0]
        );
        assert_eq!(reflected(&[5.0], -9), 5.0);
    }
}
