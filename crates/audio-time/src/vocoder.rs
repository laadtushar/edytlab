//! Phase vocoder: the STFT machinery both public primitives sit on.
//!
//! ## Why this and not Rubber Band
//!
//! The original plan was FFI to the Rubber Band C++ library. That needs
//! `librubberband-dev` on Linux, vcpkg on Windows and Homebrew on macOS,
//! plus a C++ toolchain on each — for a project whose CI builds all
//! three targets, a native dependency is the kind of thing that breaks
//! every build at once, and it is why the DSP sat unimplemented long
//! enough for the tools to start lying about it.
//!
//! A phase vocoder is pure Rust on `realfft`, which is already the
//! workspace's blessed FFT path. It is not as good as Rubber Band —
//! see the honest limits below — but it does the thing, on every
//! platform, with no build story at all.
//!
//! ## How it works
//!
//! Analyse the signal in overlapping windowed frames, step the analysis
//! window by `Ha` and the synthesis window by `Hs`, and the output comes
//! out `Hs/Ha` times as long. Playing that back at the original rate is
//! a time change with no pitch change — provided each bin's phase is
//! advanced by *its own* true frequency rather than the bin centre.
//!
//! Recovering that true frequency is the whole trick. Between two
//! analysis frames a bin's phase advances by the amount its centre
//! frequency predicts, plus a deviation for how far the real partial
//! sits from the centre. The deviation is only known modulo 2π, so it is
//! wrapped into `(-π, π]` — the standard assumption that no partial
//! drifts more than half a bin per hop.
//!
//! ## Transient preservation
//!
//! Left alone, the phase advance above is exactly what smears an attack:
//! each bin's synthesis phase drifts from its analysis phase, so the
//! alignment that makes a snare hit *sound* like one edge is spread
//! across the window.
//!
//! On a frame that looks like an onset, every bin's synthesis phase is
//! reset to its analysis phase. That frame is then reconstructed with
//! the input's own phase relationships and the attack survives; the
//! frames after it carry on accumulating from the new origin.
//!
//! Onsets are found by spectral flux — the sum of per-bin magnitude
//! *increases* since the previous frame — which is the same measure
//! `audio-analysis::bpm` uses for beat detection. The algorithm is
//! borrowed; the code deliberately is not. `audio-time` has no workspace
//! dependencies at all, and taking one on `audio-analysis` to reach a
//! 40-line function would drag in `audio-decoder`, `ebur128` and
//! `serde`. It would also mean a second full STFT pass over data this
//! loop is already transforming, at a window size (1024) that doesn't
//! line up with this one (2048).
//!
//! Flux is normalised by the frame's total magnitude, so the detector
//! is level-independent and near-silence can't trip it. Thresholding is
//! causal — a rolling mean + 1.5σ over the last [`FLUX_HISTORY`] frames,
//! matching `bpm.rs`'s constant — because this loop sees frames in order
//! and cannot look ahead.
//!
//! A false positive is worse than a miss: resetting phase mid-tone puts
//! a discontinuity into a steady sound, which is audible as a click.
//! Hence the deliberately conservative pairing of a relative floor with
//! the adaptive test, and the refractory gap.
//!
//! ## What it still does not do
//!
//! No phase locking across bins. Dense material keeps some of the
//! "phasiness" the technique is known for, and stretch factors far from
//! 1.0 make it worse. Peak-locking is the next step and composes with
//! the above rather than replacing it.

use std::f32::consts::PI;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

/// FFT size. 2048 at 44.1 kHz is ~46 ms — long enough to resolve bass
/// partials, short enough that transient smearing stays tolerable.
const FRAME: usize = 2048;

/// Analysis hop. `FRAME / 4` gives 75% overlap, which is what makes the
/// Hann window sum to a constant and lets the overlap-add reconstruct
/// unity when nothing is changed.
const HOP_A: usize = FRAME / 4;

/// Periodic Hann window, used for both analysis and synthesis.
///
/// Periodic (`/ N`) rather than symmetric (`/ (N-1)`): with 75% overlap
/// the periodic form sums to a constant, so the overlap-add needs only a
/// single scalar normalisation instead of a per-sample envelope.
fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
        .collect()
}

/// Frames of spectral-flux history behind the adaptive threshold.
///
/// 16 frames is ~186 ms at 44.1 kHz with this hop — long enough to
/// establish what "normal" looks like for the current material, short
/// enough to track a change of texture.
const FLUX_HISTORY: usize = 16;

/// Standard deviations above the rolling mean that count as an onset.
/// 1.5 is the same constant `audio-analysis::bpm::pick_onsets` settled
/// on for the same measure.
const FLUX_THRESHOLD_K: f32 = 1.5;

/// Minimum frames between two phase resets.
///
/// One attack spans several frames at 75% overlap, and locking on each
/// of them in turn would re-cut the same edge repeatedly. Four frames
/// is roughly one window (~46 ms at 44.1 kHz).
///
/// In frames rather than milliseconds because this function is never
/// told the sample rate — it works on a bare `&[f32]`.
const MIN_TRANSIENT_GAP_FRAMES: usize = 4;

/// Floor on flux as a fraction of the frame's total magnitude.
///
/// The adaptive test alone is not enough: on a perfectly steady tone
/// both the mean and the deviation collapse toward zero, and float
/// noise clears `mean + kσ` on its own. Normalising by total magnitude
/// makes the measure level-independent, and this floor demands that a
/// real share of the spectrum actually appeared before phase is reset.
const MIN_RELATIVE_FLUX: f32 = 0.10;

/// Decide whether this frame's flux is an onset.
///
/// `history` holds the previous frames' relative flux, most recent
/// last. Returns false until the history is full: with nothing to
/// compare against, every early frame looks exceptional.
fn is_onset(rel_flux: f32, history: &[f32], frames_since_last: usize) -> bool {
    if history.len() < FLUX_HISTORY || frames_since_last < MIN_TRANSIENT_GAP_FRAMES {
        return false;
    }
    if rel_flux < MIN_RELATIVE_FLUX {
        return false;
    }
    let n = history.len() as f32;
    let mean = history.iter().sum::<f32>() / n;
    let variance = history.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n;
    rel_flux > mean + FLUX_THRESHOLD_K * variance.sqrt()
}

/// Wrap a phase into `(-π, π]`.
fn wrap_phase(p: f32) -> f32 {
    let mut x = p;
    while x > PI {
        x -= 2.0 * PI;
    }
    while x <= -PI {
        x += 2.0 * PI;
    }
    x
}

struct Fft {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
}

impl Fft {
    fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        Self {
            fwd: planner.plan_fft_forward(FRAME),
            inv: planner.plan_fft_inverse(FRAME),
        }
    }
}

/// Time-scale one channel of mono samples by `ratio`.
///
/// `ratio` is the output length over the input length: 2.0 is twice as
/// long (slower), 0.5 is half (faster). Pitch is unchanged.
pub(crate) fn stretch_mono(input: &[f32], ratio: f32) -> Vec<f32> {
    stretch_mono_opts(input, ratio, true)
}

/// [`stretch_mono`] with transient preservation switchable.
///
/// Production always passes `true`. The switch exists so the tests can
/// measure the feature against its own absence on identical input —
/// "attacks are sharp" is only meaningful next to how blunt they were.
pub(crate) fn stretch_mono_opts(input: &[f32], ratio: f32, preserve_transients: bool) -> Vec<f32> {
    if !ratio.is_finite() || ratio <= 0.0 {
        return Vec::new();
    }
    let target_len = ((input.len() as f32) * ratio).round().max(1.0) as usize;
    stretch_varying(input, &|_| ratio, target_len, preserve_transients)
}

/// Time-scale one channel with a stretch ratio that varies along the
/// signal.
///
/// `ratio_at(pos)` gives the local ratio for an analysis frame whose
/// window starts at input sample `pos`. A constant function is an
/// ordinary stretch; a piecewise-constant one built from two beat grids
/// is a warp onto those beats, which is what `warp_to_grid` does.
///
/// `target_len` is the exact output length the caller is owed. The frame
/// arithmetic lands a few percent short on its own — the last partial
/// frame has no room to be analysed and the overlap-add ends on a frame
/// boundary — so the length is a contract rather than a consequence.
///
/// ## Why one pass rather than a stretch per segment
///
/// The obvious way to warp is to cut the signal at each beat, stretch
/// each piece by its own ratio, and concatenate. That produces an
/// audible seam at *every beat*: each call has its own overlap-add ramp
/// at both ends, and its own phase state starting from zero, so the
/// partials restart out of step at each join.
///
/// Here the phase state runs continuously across the whole signal and
/// only the synthesis hop changes, so a segment boundary is not an event
/// the reconstruction can see.
pub(crate) fn stretch_varying(
    input: &[f32],
    ratio_at: &dyn Fn(usize) -> f32,
    target_len: usize,
    preserve_transients: bool,
) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let ratio = ratio_at(0);
    if !ratio.is_finite() || ratio <= 0.0 {
        return Vec::new();
    }
    // A signal shorter than one frame has no overlap structure to work
    // with; resampling it would change pitch, and windowing it would
    // just fade it. Length-scaling by repetition is the least wrong
    // answer, and at these sizes (< 46 ms) nothing is audible either way.
    if input.len() < FRAME {
        return (0..target_len)
            .map(|i| {
                let src = ((i as f32) / ratio).floor() as usize;
                input[src.min(input.len() - 1)]
            })
            .collect();
    }

    // Synthesis hop for the frame whose window starts at padded position
    // `start`. `lead` is subtracted because the schedule speaks in the
    // caller's coordinates, not the padded buffer's; frames inside the
    // lead-in take the ratio at input sample 0.
    let hop_for = |ratio: f32| ((HOP_A as f32) * ratio).round().max(1.0) as usize;
    let hop_s = hop_for(ratio);

    // Analysis frames to run before the real signal starts.
    //
    // The overlap-add's window energy ramps from zero over the first
    // `FRAME - hop_s` output samples, so anything landing in that ramp
    // is reconstructed from a fraction of a window and comes out faded.
    // Feeding the loop silence first moves the whole ramp into padding
    // that is trimmed off, and the input's own sample 0 arrives with the
    // full overlap behind it.
    //
    // Two conditions bind here, and the lead has to satisfy both.
    //
    // *Synthesis* — enough frames that the real signal starts at or past
    // the end of the ramp: `lead_frames * hop_s >= FRAME - hop_s`. At
    // ratio 2.0 that is 1 frame; at 0.5 the synthesis frames are packed
    // four times tighter and it takes 7. With a varying hop the ramp
    // condition binds hardest where the hop is smallest, so the lead is
    // sized on the minimum over the whole schedule rather than on the
    // first frame's.
    //
    // Without this the leading samples were simply lost — a click at
    // sample 0 fell inside the ramp and faded out of a 6-hit train.
    // (It *looked* preserved before the normalisation fix above, but
    // only because the raw divisor was amplifying that same region.)
    //
    // *Analysis* — input sample 0 must be seen by a full set of analysis
    // windows: `lead >= FRAME - HOP_A`, three frames at 75% overlap.
    // Silence in front is how a frame at a negative offset is expressed
    // in padded coordinates. Below three, sample 0 is covered by two
    // windows rather than four and one of those weights it `w(0) = 0` —
    // one effective window, which nothing can be reconstructed from. At
    // ratio 2.0 the synthesis condition alone asks for a single frame,
    // so the analysis condition is the binding one there.
    let mut min_hop = hop_s;
    {
        let mut pos = 0usize;
        while pos + FRAME <= input.len() {
            min_hop = min_hop.min(hop_for(ratio_at(pos)));
            pos += HOP_A;
        }
    }
    let lead_frames = FRAME
        .div_ceil(min_hop)
        .max(FRAME.div_ceil(HOP_A))
        .saturating_sub(1);
    let lead = lead_frames * HOP_A;

    // Where the real signal stops, in padded coordinates. Frames
    // reaching past this see the trailing zero padding, and the step
    // from signal to silence is a discontinuity that splatters energy
    // across the spectrum — which reads as a large flux rise and fires
    // the onset detector at the end of every single stretch. Detection
    // is suppressed there; the cost is missing a transient in the final
    // window, which is a better trade than a phase reset planted in
    // every output.
    let real_len = lead + input.len();

    // Silence in front (see `lead_frames`), and enough behind that the
    // tail of the input gets analysed at all.
    let mut padded = Vec::with_capacity(lead + input.len() + FRAME);
    padded.resize(lead, 0.0);
    padded.extend_from_slice(input);
    padded.resize(lead + input.len() + FRAME, 0.0);
    let input = &padded[..];
    let window = hann(FRAME);
    let fft = Fft::new();
    let bins = FRAME / 2 + 1;

    // Phase state carried between frames.
    let mut last_phase = vec![0.0f32; bins];
    let mut sum_phase = vec![0.0f32; bins];

    // Onset detection state. `mags`/`phases` exist because the flux is a
    // whole-frame sum: the decision has to be made before the loop that
    // acts on it, so the per-bin values are computed once and reused
    // rather than the spectrum being read twice.
    let mut mags = vec![0.0f32; bins];
    let mut phases = vec![0.0f32; bins];
    let mut prev_mag = vec![0.0f32; bins];
    let mut flux_history: Vec<f32> = Vec::with_capacity(FLUX_HISTORY);
    let mut frames_since_transient = MIN_TRANSIENT_GAP_FRAMES;

    let frames = (input.len() - FRAME) / HOP_A + 1;

    // Per-frame synthesis hop, and where each frame lands. Uniform
    // spacing (`f * hop_s`) only holds when the hop is constant; with a
    // schedule the output position is the running sum of the hops before
    // it.
    // The accumulator is a float and each *position* is rounded once,
    // rather than each hop being rounded and the errors summing. At
    // ratio 1.2 the hop is 614.4 samples and rounding it per frame loses
    // 0.4 every 512 input samples; measured on a 120-to-100 BPM warp
    // that put beat five 10.5 ms early, which is outside the tolerance
    // the whole feature is judged by. Rounding the position instead
    // keeps the error bounded at half a sample forever.
    //
    // `hops[f]` is then the *actual* distance to the next frame, so the
    // phase advance below matches where the audio really goes.
    let mut starts: Vec<usize> = Vec::with_capacity(frames);
    {
        let mut acc = 0.0f64;
        for f in 0..frames {
            starts.push(acc.round() as usize);
            let padded_start = f * HOP_A;
            let r = if padded_start < lead {
                ratio
            } else {
                ratio_at(padded_start - lead)
            };
            acc += (HOP_A as f64) * (r.clamp(1e-3, 1e3) as f64);
        }
    }
    let hops: Vec<usize> = (0..frames)
        .map(|f| {
            if f + 1 < frames {
                starts[f + 1] - starts[f]
            } else {
                hop_s
            }
        })
        .collect();
    let out_len = starts[frames - 1] + FRAME;
    let mut out = vec![0.0f32; out_len];
    let mut norm = vec![0.0f32; out_len];

    let mut scratch_in = fft.fwd.make_input_vec();
    let mut spectrum = fft.fwd.make_output_vec();
    let mut scratch_out = fft.inv.make_output_vec();

    // Expected phase advance per analysis hop, for each bin centre.
    let expected: Vec<f32> = (0..bins)
        .map(|k| 2.0 * PI * (k as f32) * (HOP_A as f32) / (FRAME as f32))
        .collect();

    for f in 0..frames {
        let start = f * HOP_A;
        for i in 0..FRAME {
            scratch_in[i] = input[start + i] * window[i];
        }
        if fft.fwd.process(&mut scratch_in, &mut spectrum).is_err() {
            return Vec::new();
        }

        // Pass 1 — magnitudes, phases, and how much of the spectrum
        // grew since the last frame. Only increases count: a partial
        // decaying is not an onset.
        let mut flux = 0.0f32;
        let mut total_mag = 0.0f32;
        for k in 0..bins {
            let (re, im) = (spectrum[k].re, spectrum[k].im);
            let mag = (re * re + im * im).sqrt();
            mags[k] = mag;
            phases[k] = im.atan2(re);
            let rise = mag - prev_mag[k];
            if rise > 0.0 {
                flux += rise;
            }
            prev_mag[k] = mag;
            total_mag += mag;
        }

        let rel_flux = if total_mag > 1e-12 {
            flux / total_mag
        } else {
            0.0
        };
        // `start + FRAME > real_len` means this window straddles the
        // zero padding; see `real_len` above.
        let inside_real_signal = start + FRAME <= real_len;
        let transient = preserve_transients
            && inside_real_signal
            && is_onset(rel_flux, &flux_history, frames_since_transient);
        if flux_history.len() == FLUX_HISTORY {
            flux_history.remove(0);
        }
        flux_history.push(rel_flux);
        frames_since_transient = if transient {
            0
        } else {
            frames_since_transient.saturating_add(1)
        };

        // Pass 2 — advance or reset the synthesis phase, then rebuild.
        for k in 0..bins {
            // How far this bin's partial actually moved, beyond what its
            // centre frequency predicts. Wrapped, on the assumption that
            // it is within half a bin of the centre.
            let delta = wrap_phase(phases[k] - last_phase[k] - expected[k]);
            last_phase[k] = phases[k];

            if transient {
                // Reproduce this frame with the input's own phase
                // relationships. That alignment is what an attack *is*;
                // accumulating through it is what smears it.
                sum_phase[k] = phases[k];
            } else {
                // Advance the synthesis phase by the true frequency
                // scaled to the synthesis hop. This is the step that
                // keeps pitch fixed while the timeline changes.
                let true_freq = expected[k] + delta;
                sum_phase[k] += true_freq * (hops[f] as f32) / (HOP_A as f32);
            }

            spectrum[k].re = mags[k] * sum_phase[k].cos();
            spectrum[k].im = mags[k] * sum_phase[k].sin();
        }

        // A real signal's DC and Nyquist bins have no imaginary part, and
        // `realfft` enforces it — reconstructing them from a rotated
        // phase leaves a residue there and the inverse transform refuses
        // the whole frame. Both bins keep their magnitude and their sign.
        spectrum[0].im = 0.0;
        let last = bins - 1;
        spectrum[last].im = 0.0;

        if fft.inv.process(&mut spectrum, &mut scratch_out).is_err() {
            return Vec::new();
        }

        let out_start = starts[f];
        for i in 0..FRAME {
            // realfft's inverse is unnormalised; dividing by FRAME here
            // keeps the scalar in one place.
            let v = scratch_out[i] / (FRAME as f32) * window[i];
            out[out_start + i] += v;
            norm[out_start + i] += window[i] * window[i];
        }
    }

    // Divide out the summed window energy. It is constant across the
    // interior but ramps to zero at both ends, where fewer frames
    // overlap — and that ramp is why the divisor is floored at the
    // interior value rather than used raw.
    //
    // Dividing raw is only correct when the synthesis frame still
    // carries the analysis taper, because then the `w²` in the numerator
    // and the `w²` in the divisor cancel. Every frame here has had its
    // phases rewritten, so the inverse transform comes back at roughly
    // full amplitude across the whole window, taper and all. Near the
    // edges that full-amplitude value was divided by `w²` of a sample
    // just inside the window — 2.7e-6 at sample 15 — and the output
    // exploded. Measured on a plain 2 kHz sine stretched 2x: a peak of
    // 740.9 against an input of 1.0, entirely in the first few hundred
    // samples. A steady sine, not some pathological input.
    //
    // Flooring costs a fade of one synthesis hop at each end, which is
    // the honest answer: with only a fraction of a window overlapping
    // there, there is no reconstruction to recover, and inventing one by
    // amplification is what produced the blowup.
    let norm_interior = norm.iter().fold(0.0f32, |m, n| m.max(*n));
    if norm_interior > 1e-6 {
        for (o, n) in out.iter_mut().zip(norm.iter()) {
            *o /= n.max(norm_interior);
        }
    }

    // Drop the lead-in.
    //
    // `starts[lead_frames]` is the wrong amount, and was the reason a
    // stretched signal landed early. A frame does not time-scale its own
    // contents: whatever sits at offset `d` inside analysis frame `f` is
    // written back at offset `d` inside a synthesis frame beginning at
    // `starts[f]`. Padded position `q` therefore lands at `starts[f] + q
    // - f * HOP_A`, which depends on *which* frame you ask — and the one
    // carrying the weight is the frame holding `q` at its centre, since
    // that is where the analysis window peaks. For `q = lead` that frame
    // is `lead_frames - FRAME / (2 * HOP_A)`, and sample 0 lands at its
    // start plus half a frame.
    //
    // The old expression named the frame that holds sample 0 at its
    // *left edge* instead, where the window weight is zero. That is
    // exact at ratio 1 (the two agree when the hops match) and wrong in
    // proportion to `ratio - 1` either side: measured on a lone impulse,
    // 1.4k samples early at ratio 2.0 and 2.5k early at 4.0, against a
    // wanted position of `p * ratio`.
    //
    // `FRAME / (2 * HOP_A)` is 2 with these constants and `lead_frames`
    // is at least 3, so the index cannot underflow.
    let centre_frame = lead_frames - FRAME / (2 * HOP_A);
    let skip = (starts[centre_frame.min(frames - 1)] + FRAME / 2).min(out.len());
    out.drain(..skip);

    // Trim (or zero-extend) to the promised length. The overshoot is the
    // zero padding coming back out; the shortfall only happens for inputs
    // barely over one frame.
    out.resize(target_len, 0.0);
    out
}

/// Resample one channel by `rate` — reading `rate` input samples per
/// output sample, so the result is `1/rate` as long and every frequency
/// is multiplied by `rate`.
///
/// Linear interpolation. Combined with [`stretch_mono`] this is what
/// makes a pitch shift: stretch to `rate` times the length, then read it
/// back `rate` times as fast, and the duration returns to where it
/// started with the pitch moved.
pub(crate) fn resample_mono(input: &[f32], rate: f32) -> Vec<f32> {
    if input.is_empty() || !rate.is_finite() || rate <= 0.0 {
        return Vec::new();
    }
    let out_len = ((input.len() as f32) / rate).round().max(1.0) as usize;
    (0..out_len)
        .map(|i| {
            let pos = (i as f32) * rate;
            let idx = pos.floor() as usize;
            let frac = pos - idx as f32;
            let a = input.get(idx).copied().unwrap_or(0.0);
            let b = input.get(idx + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

/// Split interleaved samples into per-channel planes.
pub(crate) fn deinterleave(samples: &[f32], channels: usize) -> Vec<Vec<f32>> {
    let frames = samples.len() / channels;
    (0..channels)
        .map(|ch| (0..frames).map(|f| samples[f * channels + ch]).collect())
        .collect()
}

/// Rebuild an interleaved buffer from per-channel planes.
///
/// Planes are truncated to the shortest, so a channel that came back a
/// sample longer cannot desynchronise the pair — a stereo file whose two
/// channels differ in length by one frame is worse than one that is a
/// frame short.
pub(crate) fn interleave(planes: &[Vec<f32>]) -> Vec<f32> {
    let frames = planes.iter().map(|p| p.len()).min().unwrap_or(0);
    let channels = planes.len();
    let mut out = vec![0.0f32; frames * channels];
    for (ch, plane) in planes.iter().enumerate() {
        for f in 0..frames {
            out[f * channels + ch] = plane[f];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A train of short percussive bursts on silence: a fast attack
    /// followed by an exponential decay. Closer to real percussion than
    /// a bare impulse, and it gives the envelope something measurable.
    fn click_train(sr: u32, hits: usize, gap: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; hits * gap];
        for h in 0..hits {
            let start = h * gap;
            for i in 0..(sr as usize / 100) {
                let t = i as f32 / sr as f32;
                let env = (-t * 400.0).exp();
                let s = (2.0 * PI * 2_000.0 * t).sin() * env;
                if start + i < out.len() {
                    out[start + i] = s;
                }
            }
        }
        out
    }

    /// Peak over RMS. Smearing an attack spreads its energy in time,
    /// which lowers the peak and raises the RMS — so this number falls
    /// exactly when transients are being lost, and it needs no reference
    /// signal to interpret.
    fn crest_factor(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        if rms <= 1e-12 {
            0.0
        } else {
            peak / rms
        }
    }

    /// Silence, then a steady tone from `onset` to the end. The edge is
    /// something whose position can be *measured*, unlike a continuous
    /// tone, and unlike a click it is material the vocoder is designed
    /// for, so what is left over is the timing error and not the smear.
    fn late_tone(sr: u32, len: usize, onset: usize) -> Vec<f32> {
        (0..len)
            .map(|n| {
                if n < onset {
                    0.0
                } else {
                    (2.0 * PI * 440.0 * (n - onset) as f32 / sr as f32).sin() * 0.8
                }
            })
            .collect()
    }

    /// First short-window RMS to reach half the signal's plateau —
    /// where the edge landed, to within the window.
    fn onset_of(x: &[f32]) -> Option<usize> {
        const W: usize = 256;
        let env: Vec<f32> = x
            .chunks(W)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let peak = env.iter().cloned().fold(0.0f32, f32::max);
        env.iter().position(|e| *e > peak * 0.5).map(|i| i * W)
    }

    /// Input sample `p` must come out at `p * ratio`.
    ///
    /// This is the contract the whole feature rests on — "same audio,
    /// different clock" — and nothing pinned it. The lead-in trim was
    /// computed from the frame holding input sample 0 at its left edge,
    /// where the analysis window weight is zero, instead of the frame
    /// holding it at its centre. The two agree only at ratio 1, which is
    /// why every existing test missed it: the error is proportional to
    /// `ratio - 1`.
    ///
    /// Measured before the fix, this same tone: 1.1k samples late at
    /// ratio 0.25 and 1.8k early at 2.0 — 25 ms and 41 ms, far past
    /// anything that could be called sample-accurate, and enough to pull
    /// a stretched track off a grid it was meant to land on.
    ///
    /// Ratios stop at 2.0 deliberately. At 4.0 the synthesis hop equals
    /// the frame and the overlap-add has no overlap left; timing there is
    /// governed by a different problem than this one.
    #[test]
    fn a_feature_lands_where_the_ratio_says_it_should() {
        let sr = 44_100u32;
        // Half an analysis window. A frame-based method cannot place an
        // edge closer than that, and the pre-fix error clears it at every
        // ratio but 1.
        let tolerance = (FRAME / 2) as f32;

        for ratio in [0.25f32, 0.5, 0.8, 1.0, 1.5, 2.0] {
            for onset in [sr as usize / 4, sr as usize / 2] {
                let input = late_tone(sr, sr as usize, onset);
                let out = stretch_mono(&input, ratio);
                let got = onset_of(&out).expect("the tone is in there somewhere") as f32;
                let want = onset as f32 * ratio;
                assert!(
                    (got - want).abs() <= tolerance,
                    "at ratio {ratio} an onset at {onset} landed at {got}, \
                     wanted {want} (off by {:.0} samples)",
                    got - want
                );
            }
        }
    }

    fn sine(freq: f32, sr: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|n| (2.0 * PI * freq * n as f32 / sr as f32).sin() * 0.5)
            .collect()
    }

    /// Estimate frequency by counting zero crossings — robust, and
    /// independent of the FFT the implementation itself uses.
    fn est_freq(samples: &[f32], sr: u32) -> f32 {
        // Skip the ramp-in and ramp-out regions where fewer frames
        // overlap and the amplitude is still climbing.
        let a = samples.len() / 4;
        let b = samples.len() * 3 / 4;
        let s = &samples[a..b];
        let mut crossings = 0usize;
        for w in s.windows(2) {
            if (w[0] <= 0.0 && w[1] > 0.0) || (w[0] >= 0.0 && w[1] < 0.0) {
                crossings += 1;
            }
        }
        (crossings as f32 / 2.0) * sr as f32 / s.len() as f32
    }

    #[test]
    fn ratio_one_preserves_length_and_frequency() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = stretch_mono(&input, 1.0);
        assert!(
            (out.len() as i64 - input.len() as i64).abs() < FRAME as i64,
            "identity should keep the length, got {} vs {}",
            out.len(),
            input.len()
        );
        let f = est_freq(&out, sr);
        assert!((f - 440.0).abs() < 15.0, "expected ~440 Hz, got {f}");
    }

    #[test]
    fn stretching_lengthens_without_moving_the_pitch() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = stretch_mono(&input, 2.0);

        let ratio = out.len() as f32 / input.len() as f32;
        assert!(
            (ratio - 2.0).abs() < 0.05,
            "expected roughly twice as long, got {ratio}x"
        );
        let f = est_freq(&out, sr);
        assert!(
            (f - 440.0).abs() < 15.0,
            "stretching must not move the pitch; got {f} Hz"
        );
    }

    #[test]
    fn compressing_shortens_without_moving_the_pitch() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = stretch_mono(&input, 0.5);

        let ratio = out.len() as f32 / input.len() as f32;
        assert!(
            (ratio - 0.5).abs() < 0.05,
            "expected roughly half as long, got {ratio}x"
        );
        let f = est_freq(&out, sr);
        assert!(
            (f - 440.0).abs() < 15.0,
            "compressing must not move the pitch; got {f} Hz"
        );
    }

    /// Stretching redistributes energy in time; it never creates any.
    ///
    /// This is the invariant the overlap-add normalisation broke: with
    /// the divisor used raw, a steady 2 kHz sine stretched 2x came back
    /// with a peak of 740.9 against an input of 1.0 — not a rounding
    /// error, an output that would blow a speaker. The frequencies are
    /// chosen off the bin centres (2 kHz is 92.9 bins at this size)
    /// because a partial sitting exactly on a centre hides the phase
    /// rewriting that causes it.
    #[test]
    fn stretching_never_amplifies_beyond_the_input() {
        let sr = 44_100;
        for freq in [220.0f32, 440.0, 2_000.0, 7_350.0] {
            for ratio in [0.5f32, 0.75, 1.5, 2.0, 4.0] {
                let input = sine(freq, sr, sr as usize / 2);
                let out = stretch_mono(&input, ratio);
                let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(
                    peak < 1.5,
                    "{freq} Hz at {ratio}x came back at {peak:.3} \
                     against an input peak of 1.0"
                );
            }
        }
    }

    /// The same invariant on material the analysis cannot model as a few
    /// stationary partials — a decaying burst has energy everywhere and
    /// is where an amplitude bug is loudest.
    #[test]
    fn stretching_a_decaying_burst_never_amplifies() {
        let sr = 44_100;
        let input: Vec<f32> = (0..sr as usize)
            .map(|i| {
                let t = i as f32 / sr as f32;
                (2.0 * PI * 2_000.0 * t).sin() * (-t * 4.0).exp()
            })
            .collect();
        for ratio in [0.5f32, 2.0] {
            let out = stretch_mono(&input, ratio);
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 1.5, "{ratio}x came back at {peak:.3}");
        }
    }

    #[test]
    fn resampling_doubles_frequency_and_halves_length() {
        let sr = 44_100;
        let input = sine(440.0, sr, sr as usize / 2);
        let out = resample_mono(&input, 2.0);
        assert!((out.len() as f32 / input.len() as f32 - 0.5).abs() < 0.01);
        let f = est_freq(&out, sr);
        assert!((f - 880.0).abs() < 25.0, "expected ~880 Hz, got {f}");
    }

    #[test]
    fn interleave_truncates_to_the_shortest_plane() {
        let planes = vec![vec![1.0, 2.0, 3.0], vec![-1.0, -2.0]];
        assert_eq!(interleave(&planes), vec![1.0, -1.0, 2.0, -2.0]);
    }

    #[test]
    fn deinterleave_round_trips() {
        let interleaved = vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        let planes = deinterleave(&interleaved, 2);
        assert_eq!(planes[0], vec![1.0, 2.0, 3.0]);
        assert_eq!(planes[1], vec![-1.0, -2.0, -3.0]);
        assert_eq!(interleave(&planes), interleaved);
    }
    // ------------------------------------------------------------------
    // Transient preservation
    // ------------------------------------------------------------------

    /// The point of the feature, measured against its own absence on
    /// identical input. A plain phase vocoder spreads each attack across
    /// the analysis window; preserving transients keeps the edge, which
    /// shows up as a higher crest factor.
    #[test]
    fn transient_preservation_keeps_attacks_sharper_than_without() {
        let sr = 44_100u32;
        let input = click_train(sr, 8, sr as usize / 4);

        let with = stretch_mono_opts(&input, 2.0, true);
        let without = stretch_mono_opts(&input, 2.0, false);

        let (cf_with, cf_without) = (crest_factor(&with), crest_factor(&without));
        assert!(
            cf_with > cf_without,
            "preserving transients must not blunt them further: \
             {cf_with:.2} with vs {cf_without:.2} without"
        );
        // A margin, so this fails on a detector that never fires rather
        // than passing on float noise.
        assert!(
            cf_with > cf_without * 1.05,
            "the improvement is within noise — the detector probably \
             never fired: {cf_with:.2} vs {cf_without:.2}"
        );
    }

    /// A steady tone must not trip the detector. Resetting phase in the
    /// middle of a sustained sound puts a discontinuity into it, which
    /// is audible as a click — a false positive is worse than a miss.
    #[test]
    fn a_steady_tone_does_not_trigger_a_phase_reset() {
        let sr = 44_100u32;
        let input = sine(440.0, sr, sr as usize);

        let with = stretch_mono_opts(&input, 2.0, true);
        let without = stretch_mono_opts(&input, 2.0, false);

        assert_eq!(with.len(), without.len());
        // On material with no onsets the two paths should agree
        // sample-for-sample: the detector should never have fired.
        let diff = with
            .iter()
            .zip(without.iter())
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(
            diff < 1e-6,
            "the detector fired on a steady 440 Hz sine (max sample \
             difference {diff:.6}); phase resets mid-tone are clicks"
        );
    }

    /// Near-silence has a tiny total magnitude, so an unnormalised flux
    /// measure would see huge relative swings in the noise floor. The
    /// relative-flux floor exists to stop that.
    #[test]
    fn near_silence_does_not_trigger_a_phase_reset() {
        let sr = 44_100u32;
        let input: Vec<f32> = sine(440.0, sr, sr as usize)
            .iter()
            .map(|s| s * 1e-7)
            .collect();

        let with = stretch_mono_opts(&input, 2.0, true);
        let without = stretch_mono_opts(&input, 2.0, false);
        let diff = with
            .iter()
            .zip(without.iter())
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(diff < 1e-9, "the detector fired on near-silence");
    }

    /// The exact-length contract predates this feature and must survive
    /// it: phase resets change the samples, never the frame count.
    #[test]
    fn preserving_transients_does_not_change_the_output_length() {
        let sr = 44_100u32;
        let input = click_train(sr, 6, sr as usize / 4);
        for ratio in [0.5f32, 1.0, 1.5, 2.0] {
            let with = stretch_mono_opts(&input, ratio, true);
            let without = stretch_mono_opts(&input, ratio, false);
            let expected = ((input.len() as f32) * ratio).round() as usize;
            assert_eq!(with.len(), expected, "ratio {ratio}");
            assert_eq!(without.len(), expected, "ratio {ratio}");
        }
    }

    /// Stretching must not add or drop hits. Counting envelope peaks
    /// above a fraction of the maximum is crude but independent of the
    /// detector under test.
    #[test]
    fn stretching_preserves_the_number_of_hits() {
        let sr = 44_100u32;
        let hits = 6;
        let input = click_train(sr, hits, sr as usize / 4);
        let out = stretch_mono(&input, 2.0);

        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let threshold = peak * 0.3;
        // Count runs above threshold, with a refractory window so one
        // decaying hit isn't counted several times.
        let mut count = 0usize;
        let mut cooldown = 0usize;
        for &v in &out {
            if cooldown > 0 {
                cooldown -= 1;
                continue;
            }
            if v.abs() > threshold {
                count += 1;
                cooldown = sr as usize / 10;
            }
        }
        assert_eq!(
            count, hits,
            "a 2x stretch of {hits} hits produced {count} of them"
        );
    }

    /// The threshold itself, away from the FFT.
    #[test]
    fn onset_detection_needs_history_a_gap_and_a_real_rise() {
        let steady = vec![0.05f32; FLUX_HISTORY];

        // A clear rise over a settled history.
        assert!(is_onset(0.5, &steady, MIN_TRANSIENT_GAP_FRAMES));

        // Same rise, but too soon after the last reset.
        assert!(!is_onset(0.5, &steady, MIN_TRANSIENT_GAP_FRAMES - 1));

        // Same rise, but nothing to compare against yet.
        assert!(!is_onset(0.5, &steady[..FLUX_HISTORY - 1], 99));

        // Above the adaptive threshold but below the absolute floor:
        // a big relative jump in a spectrum where nothing much happened.
        let quiet = vec![0.0f32; FLUX_HISTORY];
        assert!(!is_onset(MIN_RELATIVE_FLUX * 0.5, &quiet, 99));

        // No rise at all.
        assert!(!is_onset(0.05, &steady, 99));
    }
}
