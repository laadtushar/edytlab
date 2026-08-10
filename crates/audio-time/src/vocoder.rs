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
//! ## What it does not do
//!
//! No transient preservation and no phase locking across bins. Sustained
//! tones come through cleanly; sharp attacks smear, and dense material
//! acquires the "phasiness" the technique is known for. Stretch factors
//! far from 1.0 make both worse. Those are properties of a plain phase
//! vocoder, not bugs — fixing them means peak-locking or a transient
//! detector, which can be added here without changing the public API.

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
    if input.is_empty() || !ratio.is_finite() || ratio <= 0.0 {
        return Vec::new();
    }
    // A signal shorter than one frame has no overlap structure to work
    // with; resampling it would change pitch, and windowing it would
    // just fade it. Length-scaling by repetition is the least wrong
    // answer, and at these sizes (< 46 ms) nothing is audible either way.
    if input.len() < FRAME {
        let out_len = ((input.len() as f32) * ratio).round().max(1.0) as usize;
        return (0..out_len)
            .map(|i| {
                let src = ((i as f32) / ratio).floor() as usize;
                input[src.min(input.len() - 1)]
            })
            .collect();
    }

    // What the caller is owed. The tool contract is "output duration =
    // input duration / factor", so the length has to be exact rather
    // than whatever falls out of the frame arithmetic — that lands a few
    // percent short, because the last partial frame has no room to be
    // analysed and the overlap-add ends on a frame boundary.
    let target_len = ((input.len() as f32) * ratio).round().max(1.0) as usize;

    // Pad so the tail of the input gets analysed at all.
    let mut padded = Vec::with_capacity(input.len() + FRAME);
    padded.extend_from_slice(input);
    padded.resize(input.len() + FRAME, 0.0);
    let input = &padded[..];

    let hop_s = ((HOP_A as f32) * ratio).round().max(1.0) as usize;
    let window = hann(FRAME);
    let fft = Fft::new();
    let bins = FRAME / 2 + 1;

    // Phase state carried between frames.
    let mut last_phase = vec![0.0f32; bins];
    let mut sum_phase = vec![0.0f32; bins];

    let frames = (input.len() - FRAME) / HOP_A + 1;
    let out_len = (frames - 1) * hop_s + FRAME;
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

        for k in 0..bins {
            let (re, im) = (spectrum[k].re, spectrum[k].im);
            let mag = (re * re + im * im).sqrt();
            let phase = im.atan2(re);

            // How far this bin's partial actually moved, beyond what its
            // centre frequency predicts. Wrapped, on the assumption that
            // it is within half a bin of the centre.
            let delta = wrap_phase(phase - last_phase[k] - expected[k]);
            last_phase[k] = phase;

            // Advance the synthesis phase by the true frequency scaled to
            // the synthesis hop. This is the step that keeps pitch fixed
            // while the timeline changes.
            let true_freq = expected[k] + delta;
            sum_phase[k] += true_freq * (hop_s as f32) / (HOP_A as f32);

            spectrum[k].re = mag * sum_phase[k].cos();
            spectrum[k].im = mag * sum_phase[k].sin();
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

        let out_start = f * hop_s;
        for i in 0..FRAME {
            // realfft's inverse is unnormalised; dividing by FRAME here
            // keeps the scalar in one place.
            let v = scratch_out[i] / (FRAME as f32) * window[i];
            out[out_start + i] += v;
            norm[out_start + i] += window[i] * window[i];
        }
    }

    // Divide out the summed window energy. It is constant across the
    // interior but ramps at both ends, where fewer frames overlap; the
    // epsilon keeps the very edges from dividing by ~0.
    for (o, n) in out.iter_mut().zip(norm.iter()) {
        if *n > 1e-6 {
            *o /= *n;
        }
    }

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
}
