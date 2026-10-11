//! The STFT and iSTFT that HT-Demucs runs *outside* the network.
//!
//! The ONNX export of htdemucs has its STFT and iSTFT stripped out (see
//! the plan on #385): the graph takes the spectrogram `x` and the
//! waveform `xt`, and returns `x_out` and `xt_out`. So the spectrogram
//! going in, and the conversion of `x_out` back to a waveform, are ours
//! to do, and they have to match PyTorch's to the sample or the stems come
//! out wrong without any error.
//!
//! This is a port of `HTDemucs._spec` and `HTDemucs._ispec`, with the
//! `_magnitude` / `_mask` reshapes that turn a complex spectrogram into
//! channels and back (htdemucs is trained with `cac=True`):
//!
//! - `demucs/htdemucs.py`: `_spec`, `_ispec`, `_magnitude`, `_mask`
//! - `demucs/spec.py`: `spectro`, `ispectro` (`torch.stft` / `torch.istft`)
//! - `demucs/hdemucs.py`: `pad1d`
//!
//! all from <https://github.com/facebookresearch/demucs> (MIT) at commit
//! `e976d93ecc3865e5757426930257e200846a520a`. What the numbers are:
//!
//! | | |
//! |---|---|
//! | `n_fft` | 4096 ([`N_FFT`]; the pretrained htdemucs's `nfft`) |
//! | hop | 1024 = `n_fft / 4` ([`HOP_LENGTH`]) |
//! | window | Hann, periodic (`torch.hann_window(4096)`), length `n_fft` |
//! | normalisation | `normalized=True`: the forward transform is scaled by `1/sqrt(n_fft)`, the inverse by `1/sqrt(n_fft)` too |
//! | centring | `center=True`, `pad_mode="reflect"`: `n_fft / 2` reflected samples each side |
//! | extra padding | `hop / 2 * 3` = 1536 reflected samples each side, plus enough on the right to make the length a whole number of hops |
//! | bins | the Nyquist bin is dropped, leaving [`FREQS`] = 2048 |
//! | frames | `ceil(length / hop)`: the first two and last two of the `ceil + 4` frames are cropped |
//!
//! The extra padding and the cropping are not in `torch.stft`. They exist
//! so that the spectrogram has exactly `length / hop` frames when `length`
//! divides by `hop`, which lets the network line the time and frequency
//! branches up. For the 7.8 s segment of 343 980 samples that is 336
//! frames, the `336` in the model's `[1, 4, 2048, 336]` input.
//!
//! ## What the round trip is, and is not
//!
//! `ispec(spec(x), x.len())` is `x` in the middle of the signal, **not at
//! its ends and not for every signal**, because Demucs' own convention
//! loses two things:
//!
//! - **The Nyquist bin.** `_spec` drops it and `_ispec` puts back zeros. A
//!   signal with energy at exactly half the sample rate (white noise has
//!   plenty) comes back without it: about 1% error on full-band noise.
//!   Anything band-limited below Nyquist is unaffected.
//! - **The first and last 1536 samples.** `_spec` crops the two frames at
//!   each end, and `_ispec` divides by the window envelope of all
//!   `ceil + 4` frames, cropped ones included. A sample at the very start
//!   comes back at half its level (a constant signal of 0.5 returns 0.25),
//!   recovering fully by sample 1536. The exact region is
//!   `[1536, ceil(length / hop) * hop - 1536)`.
//!
//! Both are what the upstream code does, and what the model was trained
//! with, so they are kept: the network's output near the ends of a segment
//! already allows for them, and a "fix" here would put it out of step with
//! its own weights.

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// `nfft` of the pretrained htdemucs.
pub const N_FFT: usize = 4096;
/// Hop between frames: `n_fft / 4`. `_spec` asserts this.
pub const HOP_LENGTH: usize = N_FFT / 4;
/// Frequency bins kept: `n_fft / 2 + 1` from the transform, less the
/// Nyquist bin.
pub const FREQS: usize = N_FFT / 2;

/// `_spec`'s extra reflected padding each side: `hop // 2 * 3`.
const EDGE_PAD: usize = HOP_LENGTH / 2 * 3;
/// `center=True`'s padding each side: `n_fft // 2`.
const CENTER_PAD: usize = N_FFT / 2;
/// `_spec` crops this many frames off the start of the transform, and
/// `_ispec` puts the same number of zero frames back on each side.
const CROPPED_FRAMES: usize = 2;

/// The number of frames in the spectrogram of `length` samples:
/// `ceil(length / hop)`.
pub fn frames_for(length: usize) -> usize {
    length.div_ceil(HOP_LENGTH)
}

/// One channel's complex spectrogram: [`FREQS`] bins by [`Self::frames`]
/// frames, stored frequency-major like the `(Fr, T)` tensor it mirrors.
#[derive(Debug, Clone, PartialEq)]
pub struct Spectrogram {
    frames: usize,
    data: Vec<Complex32>,
}

impl Spectrogram {
    /// A spectrogram of `frames` frames, all zero.
    pub fn zeros(frames: usize) -> Self {
        Self {
            frames,
            data: vec![Complex32::new(0.0, 0.0); FREQS * frames],
        }
    }

    /// Build one from separate real and imaginary planes, each
    /// `FREQS * frames` long, frequency-major.
    ///
    /// # Panics
    ///
    /// If either plane is not `FREQS * frames` long.
    pub fn from_planes(re: &[f32], im: &[f32], frames: usize) -> Self {
        assert_eq!(re.len(), FREQS * frames, "real plane has the wrong size");
        assert_eq!(
            im.len(),
            FREQS * frames,
            "imaginary plane has the wrong size"
        );
        Self {
            frames,
            data: re
                .iter()
                .zip(im)
                .map(|(&re, &im)| Complex32::new(re, im))
                .collect(),
        }
    }

    /// Number of time frames.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The value at frequency bin `freq` of frame `frame`.
    pub fn get(&self, freq: usize, frame: usize) -> Complex32 {
        self.data[freq * self.frames + frame]
    }

    /// The whole thing, frequency-major: index `freq * frames() + frame`.
    pub fn as_slice(&self) -> &[Complex32] {
        &self.data
    }
}

/// Plans and the window, built once and shared. Cheap to call from many
/// threads: the plans are immutable and each call brings its own buffers.
pub struct Stft {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    /// `torch.hann_window(N_FFT)`: periodic, `0.5 - 0.5 cos(2 pi n / N)`.
    window: Vec<f32>,
}

impl Default for Stft {
    fn default() -> Self {
        Self::new()
    }
}

impl Stft {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let window = (0..N_FFT)
            .map(|n| {
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos()) as f32
            })
            .collect();
        Self {
            forward: planner.plan_fft_forward(N_FFT),
            inverse: planner.plan_fft_inverse(N_FFT),
            window,
        }
    }

    /// `HTDemucs._spec` for one channel.
    ///
    /// Returns [`frames_for`]`(x.len())` frames of [`FREQS`] bins. An
    /// empty input gives an empty spectrogram.
    pub fn spec(&self, x: &[f32]) -> Spectrogram {
        let frames = frames_for(x.len());
        if frames == 0 {
            return Spectrogram::zeros(0);
        }
        // `pad1d(x, (pad, pad + le * hl - len), mode="reflect")`, then the
        // reflect padding `torch.stft(center=True)` adds itself.
        let padded = pad1d_reflect(x, EDGE_PAD, EDGE_PAD + frames * HOP_LENGTH - x.len());
        let centred = reflect_pad(&padded, CENTER_PAD, CENTER_PAD);

        let scale = 1.0 / (N_FFT as f32).sqrt();
        let mut data = vec![Complex32::new(0.0, 0.0); FREQS * frames];
        let mut frame = self.forward.make_input_vec();
        let mut bins = self.forward.make_output_vec();
        let mut scratch = self.forward.make_scratch_vec();
        for j in 0..frames {
            // `torch.stft`'s frame `t` starts at `t * hop` of the centred
            // signal; `_spec` keeps `t = 2 .. 2 + le`.
            let start = (j + CROPPED_FRAMES) * HOP_LENGTH;
            for (out, (&s, &w)) in frame
                .iter_mut()
                .zip(centred[start..start + N_FFT].iter().zip(&self.window))
            {
                *out = s * w;
            }
            self.forward
                .process_with_scratch(&mut frame, &mut bins, &mut scratch)
                .expect("buffers come from the plan's own make_*_vec");
            // `[..., :-1, :]`: the Nyquist bin (the last) is dropped.
            for (f, bin) in bins.iter().take(FREQS).enumerate() {
                data[f * frames + j] = bin * scale;
            }
        }
        Spectrogram { frames, data }
    }

    /// `HTDemucs._ispec` for one channel: a spectrogram back to `length`
    /// samples. `z` need not be the output of [`Stft::spec`]; the network's
    /// output is not a consistent STFT, and this is right for any input.
    ///
    /// # Panics
    ///
    /// If `z` does not have [`frames_for`]`(length)` frames.
    pub fn ispec(&self, z: &Spectrogram, length: usize) -> Vec<f32> {
        let frames = frames_for(length);
        assert_eq!(
            z.frames, frames,
            "a spectrogram of {} frames cannot make {length} samples ({frames} frames)",
            z.frames
        );
        if length == 0 {
            return Vec::new();
        }
        // `F.pad(z, (0, 0, 0, 1))` (a zero Nyquist bin) and `F.pad(z, (2, 2))`
        // (two zero frames each side): `frames + 4` frames in all.
        let all_frames = frames + 2 * CROPPED_FRAMES;
        let ola_len = (all_frames - 1) * HOP_LENGTH + N_FFT;

        // `torch.istft`: overlap-add the windowed frames, then divide by the
        // overlap-added squared window. The envelope counts the zero frames
        // too, which is why the ends of the signal do not come back whole.
        let mut y = vec![0.0f32; ola_len];
        let mut envelope = vec![0.0f32; ola_len];
        for k in 0..all_frames {
            for (e, &w) in envelope[k * HOP_LENGTH..].iter_mut().zip(&self.window) {
                *e += w * w;
            }
        }

        let scale = 1.0 / (N_FFT as f32).sqrt();
        let mut bins = self.inverse.make_input_vec();
        let mut frame = self.inverse.make_output_vec();
        let mut scratch = self.inverse.make_scratch_vec();
        for j in 0..frames {
            for (f, bin) in bins.iter_mut().take(FREQS).enumerate() {
                *bin = z.get(f, j);
            }
            // The Nyquist bin is the zero `_ispec` pads in. The imaginary
            // part of the DC bin has no meaning in a real signal; the
            // inverse transform ignores it (and `realfft` reports it).
            bins[FREQS] = Complex32::new(0.0, 0.0);
            bins[0].im = 0.0;
            self.inverse
                .process_with_scratch(&mut bins, &mut frame, &mut scratch)
                .expect("buffers come from the plan's own make_*_vec");
            let start = (j + CROPPED_FRAMES) * HOP_LENGTH;
            for ((out, &s), &w) in y[start..start + N_FFT]
                .iter_mut()
                .zip(&frame)
                .zip(&self.window)
            {
                *out += s * scale * w;
            }
        }

        // `center=True` drops `n_fft / 2` from the start; `_ispec` then
        // drops its own `pad`. The tail is cut by `length`.
        let first = CENTER_PAD + EDGE_PAD;
        y[first..first + length]
            .iter()
            .zip(&envelope[first..first + length])
            .map(|(&s, &e)| s / e)
            .collect()
    }

    /// `_spec` over every channel, laid out as `_magnitude` does with
    /// `cac=True`: the model's input `x` without its batch dimension.
    ///
    /// The result is `[2 * channels, FREQS, frames]`, row-major. Plane
    /// `2c` is the real part of channel `c` and plane `2c + 1` its
    /// imaginary part, so a stereo mix gives the `[4, 2048, 336]` of the
    /// ONNX export's `x` input (with a batch dimension of 1 in front).
    ///
    /// # Panics
    ///
    /// If the channels are not all the same length.
    pub fn spec_cac(&self, channels: &[&[f32]]) -> Vec<f32> {
        let length = channels.first().map_or(0, |c| c.len());
        assert!(
            channels.iter().all(|c| c.len() == length),
            "channels differ in length"
        );
        let plane = FREQS * frames_for(length);
        let mut out = vec![0.0f32; 2 * channels.len() * plane];
        for (c, samples) in channels.iter().enumerate() {
            let z = self.spec(samples);
            let (re, im) = out[2 * c * plane..(2 * c + 2) * plane].split_at_mut(plane);
            for ((re, im), bin) in re.iter_mut().zip(im.iter_mut()).zip(z.as_slice()) {
                *re = bin.re;
                *im = bin.im;
            }
        }
        out
    }

    /// The inverse of [`Stft::spec_cac`], for what `_mask` does with
    /// `cac=True`: the model's `x_out` (without its batch dimension) back
    /// to waveforms.
    ///
    /// `planes` is `[2 * n, FREQS, frames_for(length)]`: audio channel `i`
    /// of the output has real part plane `2i` and imaginary part plane
    /// `2i + 1`. For the model's output `[S * C * 2, ...]` that makes
    /// `i = s * C + c`, source-major, and the result `[S * C, length]` is
    /// `[S, C, length]` read row-major.
    ///
    /// # Panics
    ///
    /// If `planes` is not a whole number of (real, imaginary) plane pairs
    /// of `frames_for(length)` frames.
    pub fn ispec_cac(&self, planes: &[f32], length: usize) -> Vec<f32> {
        let frames = frames_for(length);
        let plane = FREQS * frames;
        assert!(
            plane > 0 && planes.len() % (2 * plane) == 0,
            "{} values are not whole (re, im) plane pairs of {FREQS} x {frames}",
            planes.len()
        );
        let mut out = Vec::with_capacity(planes.len() / (2 * plane) * length);
        for pair in planes.chunks_exact(2 * plane) {
            let (re, im) = pair.split_at(plane);
            out.extend(self.ispec(&Spectrogram::from_planes(re, im, frames), length));
        }
        out
    }
}

/// `F.pad(x, (left, right), mode="reflect")`: the edge sample is not
/// repeated. Needs `left` and `right` below `x.len()`, as PyTorch does.
fn reflect_pad(x: &[f32], left: usize, right: usize) -> Vec<f32> {
    assert!(
        left < x.len() && right < x.len(),
        "reflect padding of ({left}, {right}) needs more than {} samples",
        x.len()
    );
    let n = x.len();
    let mut out = Vec::with_capacity(n + left + right);
    out.extend((1..=left).rev().map(|i| x[i]));
    out.extend_from_slice(x);
    out.extend((1..=right).map(|i| x[n - 1 - i]));
    out
}

/// `pad1d(x, (left, right), mode="reflect")` from `demucs/hdemucs.py`:
/// reflect padding that also works on input shorter than the padding, by
/// first extending the signal with zeros (on the right, up to `right`
/// of them, then on the left) until it is long enough to reflect.
fn pad1d_reflect(x: &[f32], left: usize, right: usize) -> Vec<f32> {
    let max_pad = left.max(right);
    if x.len() > max_pad {
        return reflect_pad(x, left, right);
    }
    let extra = max_pad - x.len() + 1;
    let extra_right = right.min(extra);
    let extra_left = extra - extra_right;
    let mut extended = vec![0.0f32; extra_left];
    extended.extend_from_slice(x);
    extended.resize(extended.len() + extra_right, 0.0);
    reflect_pad(&extended, left - extra_left, right - extra_right)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Little-endian `f32`s, as `tests/fixtures/make_spec_fixture.py`
    /// writes them.
    fn f32s(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max)
    }

    // The golden fixtures: what PyTorch's own `_spec` / `_ispec` returned
    // for a 3000-sample stereo chirp (see make_spec_fixture.py).
    const INPUT: &[u8] = include_bytes!("../tests/fixtures/spec_input.f32");
    const CAC: &[u8] = include_bytes!("../tests/fixtures/spec_cac.f32");
    const ROUNDTRIP: &[u8] = include_bytes!("../tests/fixtures/ispec_roundtrip.f32");
    const MASKED: &[u8] = include_bytes!("../tests/fixtures/ispec_masked.f32");
    const GOLDEN_LENGTH: usize = 3000;

    fn golden_input() -> (Vec<f32>, Vec<f32>) {
        let input = f32s(INPUT);
        assert_eq!(input.len(), 2 * GOLDEN_LENGTH);
        let (left, right) = input.split_at(GOLDEN_LENGTH);
        (left.to_vec(), right.to_vec())
    }

    /// Deterministic noise in `[-1, 1)`, no `rand` needed.
    fn noise(len: usize, seed: u64) -> Vec<f32> {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
            })
            .collect()
    }

    /// Noise with nothing at half the sample rate, so that dropping the
    /// Nyquist bin loses nothing: eight passes of a one-pole low-pass
    /// (gain 1/3 at Nyquist each, so about 1e-4 in all), scaled to peak 0.9.
    fn band_limited_noise(len: usize, seed: u64) -> Vec<f32> {
        let mut x: Vec<f64> = noise(len, seed).into_iter().map(f64::from).collect();
        for _ in 0..8 {
            let mut state = 0.0;
            for s in &mut x {
                state = 0.5 * state + 0.5 * *s;
                *s = state;
            }
        }
        let peak = x.iter().fold(0.0f64, |m, s| m.max(s.abs()));
        x.iter().map(|s| (s / peak * 0.9) as f32).collect()
    }

    #[test]
    fn the_window_is_torchs_periodic_hann() {
        let stft = Stft::new();
        let w = &stft.window;
        assert_eq!(w.len(), N_FFT);
        assert_eq!(w[0], 0.0);
        assert_eq!(w[N_FFT / 2], 1.0);
        assert!((w[N_FFT / 4] - 0.5).abs() < 1e-7);
        // Periodic, not symmetric: w[n] == w[N - n], and w[N - 1] is not 0.
        for n in 1..N_FFT {
            assert!((w[n] - w[N_FFT - n]).abs() < 1e-7, "n={n}");
        }
        assert!(w[N_FFT - 1] > 0.0);
        // A periodic Hann sums to N / 2, which sets the scale of everything
        // downstream (see the tone test).
        let sum: f32 = w.iter().sum();
        assert!((sum - (N_FFT / 2) as f32).abs() < 1e-2, "{sum}");
    }

    /// 343 980 samples is the 7.8 s segment of the pinned model. Its
    /// spectrogram is the model's `x` input, `[1, 4, 2048, 336]` for
    /// stereo, and `xt` is the waveform itself.
    #[test]
    fn a_model_segment_gives_the_models_input_shape() {
        const SEGMENT: usize = 343_980;
        assert_eq!(frames_for(SEGMENT), 336);
        let stft = Stft::new();
        let left = noise(SEGMENT, 1);
        let right = noise(SEGMENT, 2);
        let x = stft.spec_cac(&[&left, &right]);
        assert_eq!(x.len(), 4 * 2048 * 336);
        assert!(x.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn frames_are_length_over_hop_rounded_up() {
        assert_eq!(frames_for(0), 0);
        assert_eq!(frames_for(1), 1);
        assert_eq!(frames_for(1024), 1);
        assert_eq!(frames_for(1025), 2);
        assert_eq!(frames_for(3000), 3);
    }

    /// An analytic check of the scale, independent of PyTorch: a sine of
    /// amplitude `A` on a bin centre has `|X[k0]| = A/2 * sum(w) / sqrt(N)
    /// = A * 16`, its two neighbours half that (the Hann main lobe), and
    /// nothing elsewhere.
    #[test]
    fn a_tone_on_a_bin_lands_on_that_bin_at_the_normalised_level() {
        const K0: usize = 100;
        const A: f32 = 0.5;
        let x: Vec<f32> = (0..8192)
            .map(|n| A * (2.0 * std::f64::consts::PI * (K0 * n) as f64 / N_FFT as f64).sin() as f32)
            .collect();
        let z = Stft::new().spec(&x);
        assert_eq!(z.frames(), 8);
        // Frame `j` covers samples `j * 1024 - 1536 ..= j * 1024 + 2559`.
        // Frames 2 to 5 lie wholly inside the signal; the others contain
        // reflected padding, which is not a continuation of the tone.
        for frame in 2..=5 {
            let mag = |f: usize| z.get(f, frame).norm();
            assert!(
                (mag(K0) - 16.0 * A).abs() < 1e-3,
                "frame {frame}: {}",
                mag(K0)
            );
            assert!((mag(K0 - 1) - 8.0 * A).abs() < 1e-3);
            assert!((mag(K0 + 1) - 8.0 * A).abs() < 1e-3);
            for f in (0..FREQS).filter(|f| f.abs_diff(K0) > 1) {
                assert!(mag(f) < 1e-3, "frame {frame} bin {f}: {}", mag(f));
            }
        }
    }

    /// `spec` against PyTorch. The values are up to about 1 in size and
    /// the largest difference measured is 2.4e-7 (two different `f32` FFTs),
    /// so 1e-4 leaves room for another CPU's SIMD paths and still catches a
    /// wrong scale, window, offset, padding or channel order, each of which
    /// moves values by 1e-2 or more.
    #[test]
    fn spec_matches_pytorch_on_a_chirp() {
        let (left, right) = golden_input();
        let want = f32s(CAC);
        assert_eq!(want.len(), 4 * FREQS * 3);

        let got = Stft::new().spec_cac(&[&left, &right]);
        let diff = max_abs_diff(&got, &want);
        assert!(diff < 1e-4, "max |rust - torch| = {diff}");
        // The golden is not trivially small, so that a tolerance this loose
        // cannot hide an all-zero answer.
        assert!(want.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.5);
    }

    /// `ispec` against PyTorch, on a spectrogram `spec` produced (largest
    /// difference measured: 3.6e-7). The chirp is 3000 samples, so its exact
    /// region `[1536, 3072 - 1536)` is empty and every sample here is an edge
    /// sample: this is the check that the fade at the ends is PyTorch's.
    #[test]
    fn ispec_matches_pytorch_including_the_attenuated_ends() {
        let (left, right) = golden_input();
        let want = f32s(ROUNDTRIP);
        let stft = Stft::new();

        let got: Vec<f32> = [&left, &right]
            .iter()
            .flat_map(|ch| stft.ispec(&stft.spec(ch), GOLDEN_LENGTH))
            .collect();
        let diff = max_abs_diff(&got, &want);
        assert!(diff < 1e-4, "max |rust - torch| = {diff}");

        // And it is not the input: that is the convention being tested.
        let input = [left, right].concat();
        assert!(max_abs_diff(&got, &input) > 0.1);
    }

    /// `ispec` for a spectrogram that is not a consistent STFT, laid out
    /// source-major as the model's `x_out` is. The golden's second source
    /// has its channels swapped and every odd bin scaled by 0.25. (Largest
    /// difference measured: 3.0e-7.)
    #[test]
    fn ispec_cac_matches_pytorch_on_an_inconsistent_two_source_output() {
        let cac = f32s(CAC);
        let frames = 3;
        let plane = FREQS * frames;
        // Source 0 is the mixture as it is. Source 1 swaps the channels
        // (planes 0-1 with 2-3) and scales odd bins by 0.25.
        let mut x_out = cac.clone();
        for p in [2usize, 3, 0, 1] {
            let src = &cac[p * plane..(p + 1) * plane];
            x_out.extend(src.iter().enumerate().map(|(i, v)| {
                if (i / frames) % 2 == 1 {
                    v * 0.25
                } else {
                    *v
                }
            }));
        }
        assert_eq!(x_out.len(), 8 * plane);

        let got = Stft::new().ispec_cac(&x_out, GOLDEN_LENGTH);
        let want = f32s(MASKED);
        assert_eq!(got.len(), 2 * 2 * GOLDEN_LENGTH); // [S, C, length]
        let diff = max_abs_diff(&got, &want);
        assert!(diff < 1e-4, "max |rust - torch| = {diff}");
    }

    #[test]
    fn the_round_trip_is_exact_in_the_middle_of_a_band_limited_signal() {
        const LEN: usize = 20_000;
        let stft = Stft::new();
        for seed in [1, 2] {
            let x = band_limited_noise(LEN, seed);
            let y = stft.ispec(&stft.spec(&x), LEN);
            assert_eq!(y.len(), LEN);
            let exact = EDGE_PAD..frames_for(LEN) * HOP_LENGTH - EDGE_PAD;
            let diff = max_abs_diff(&y[exact.clone()], &x[exact]);
            assert!(diff < 1e-4, "seed {seed}: max error {diff}");
        }
    }

    /// The Nyquist bin is dropped, so full-band noise does not come back
    /// whole. About `1/sqrt(2049)` of its amplitude is lost.
    #[test]
    fn white_noise_loses_its_nyquist_bin() {
        const LEN: usize = 20_000;
        let stft = Stft::new();
        let x = noise(LEN, 3);
        let y = stft.ispec(&stft.spec(&x), LEN);
        let exact = EDGE_PAD..frames_for(LEN) * HOP_LENGTH - EDGE_PAD;
        let diff = max_abs_diff(&y[exact.clone()], &x[exact]);
        assert!(
            diff > 1e-3,
            "{diff}: white noise should lose its Nyquist bin"
        );
        assert!(diff < 0.05, "{diff}: ...and only that");
    }

    /// The ends fade: a constant 0.5 comes back as 0.25 at sample 0, where
    /// the window envelope counts two cropped frames (an exact 1/2: the
    /// squared windows at offsets 1536 and 512 are 0.729 and 0.021 of the
    /// envelope 1.5), and is whole again from 1536.
    #[test]
    fn the_first_samples_come_back_at_half_level() {
        const LEN: usize = 8192;
        let stft = Stft::new();
        let y = stft.ispec(&stft.spec(&vec![0.5; LEN]), LEN);
        assert!((y[0] - 0.25).abs() < 1e-3, "{}", y[0]);
        // Sample 512 has the envelope 1.5 of which 1.25 is real frames.
        assert!((y[512] - 0.5 * 1.25 / 1.5).abs() < 1e-3, "{}", y[512]);
        for (i, s) in y[EDGE_PAD..LEN - EDGE_PAD].iter().enumerate() {
            assert!((s - 0.5).abs() < 1e-4, "sample {}: {s}", EDGE_PAD + i);
        }
        // And the tail: the same fade, at the mirrored end of the last hop.
        assert!((y[LEN - 1] - 0.25).abs() < 1e-3, "{}", y[LEN - 1]);
    }

    #[test]
    fn a_length_that_is_not_a_whole_number_of_hops_round_trips_in_its_exact_region() {
        // 20 000 is 19.53 hops: the last frame is partial.
        const LEN: usize = 20_000;
        let stft = Stft::new();
        let x = band_limited_noise(LEN, 4);
        let z = stft.spec(&x);
        assert_eq!(z.frames(), 20);
        let y = stft.ispec(&z, LEN);
        // The exact region ends 1536 before the last whole hop, which is
        // before the end of the signal.
        let end = frames_for(LEN) * HOP_LENGTH - EDGE_PAD;
        assert!(end < LEN);
        assert!(max_abs_diff(&y[EDGE_PAD..end], &x[EDGE_PAD..end]) < 1e-4);
    }

    #[test]
    fn channel_planes_are_real_then_imaginary_per_channel() {
        let stft = Stft::new();
        let left = band_limited_noise(5000, 5);
        let right = band_limited_noise(5000, 6);
        let cac = stft.spec_cac(&[&left, &right]);
        let frames = frames_for(5000);
        let plane = FREQS * frames;
        assert_eq!(cac.len(), 4 * plane);
        for (c, ch) in [&left, &right].into_iter().enumerate() {
            let z = stft.spec(ch);
            for (i, bin) in z.as_slice().iter().enumerate() {
                assert_eq!(cac[2 * c * plane + i], bin.re);
                assert_eq!(cac[(2 * c + 1) * plane + i], bin.im);
            }
        }
    }

    #[test]
    fn ispec_cac_inverts_spec_cac_channel_by_channel() {
        let stft = Stft::new();
        let left = band_limited_noise(5000, 7);
        let right = band_limited_noise(5000, 8);
        let back = stft.ispec_cac(&stft.spec_cac(&[&left, &right]), 5000);
        assert_eq!(back.len(), 2 * 5000);
        assert_eq!(&back[..5000], stft.ispec(&stft.spec(&left), 5000));
        assert_eq!(&back[5000..], stft.ispec(&stft.spec(&right), 5000));
    }

    #[test]
    fn silence_stays_silent_and_the_imaginary_dc_part_is_ignored() {
        let stft = Stft::new();
        let z = Spectrogram::zeros(3);
        assert!(stft.ispec(&z, 3000).iter().all(|&s| s == 0.0));

        // `torch.istft` ignores the imaginary part of the DC bin; so must
        // we (`realfft` would report it as an error).
        let mut with_im = Spectrogram::zeros(3);
        with_im.data[0] = Complex32::new(0.0, 5.0);
        assert!(stft.ispec(&with_im, 3000).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn empty_input_is_an_empty_spectrogram() {
        let stft = Stft::new();
        let z = stft.spec(&[]);
        assert_eq!(z.frames(), 0);
        assert!(stft.ispec(&z, 0).is_empty());
    }

    /// `pad1d`'s odd branch, for input shorter than the padding. The
    /// expected values are what `demucs.hdemucs.pad1d` returned (see
    /// make_spec_fixture.py, which holds the verbatim function).
    #[test]
    fn pad1d_reflects_short_input_as_demucs_does() {
        let cases: [(&[f32], usize, usize, &[f32]); 4] = [
            (
                &[1.0, 2.0, 3.0],
                4,
                4,
                &[0.0, 0.0, 3.0, 2.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 3.0],
            ),
            (
                &[1.0, 2.0, 3.0],
                3,
                6,
                &[0.0, 3.0, 2.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
            // Long enough to reflect as is.
            (
                &[1.0, 2.0, 3.0, 4.0, 5.0],
                2,
                3,
                &[3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 5.0, 4.0, 3.0, 2.0],
            ),
            (&[7.0], 3, 2, &[0.0, 7.0, 0.0, 7.0, 0.0, 0.0]),
        ];
        for (x, left, right, want) in cases {
            assert_eq!(
                pad1d_reflect(x, left, right),
                want,
                "x={x:?} ({left}, {right})"
            );
        }
    }

    #[test]
    fn a_short_signal_still_makes_a_spectrogram_and_a_waveform() {
        // 100 samples: shorter than the padding, so `pad1d`'s zero-extension
        // applies. The model never sees this (it pads to a whole segment
        // first), but the port should not fall over on it.
        let stft = Stft::new();
        let x: Vec<f32> = (0..100).map(|n| (n as f32 * 0.3).sin()).collect();
        let z = stft.spec(&x);
        assert_eq!(z.frames(), 1);
        assert_eq!(stft.ispec(&z, 100).len(), 100);
    }

    #[test]
    #[should_panic(expected = "cannot make")]
    fn ispec_refuses_a_spectrogram_of_the_wrong_length() {
        Stft::new().ispec(&Spectrogram::zeros(3), 10_000);
    }
}
