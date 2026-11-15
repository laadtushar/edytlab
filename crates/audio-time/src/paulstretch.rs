//! Paulstretch: an extreme time stretch for ambient textures.
//!
//! Paul Nasca's algorithm (Paulstretch, 2006). It is not a phase vocoder:
//! it keeps no phase from one frame to the next, so it has nothing to
//! smear and no limit on the factor but the patience of the listener.
//!
//! 1. Cut a window of `N` samples from the input, shaped by
//!    `(1 - x^2)^1.25` for `x` from -1 to 1 (the window Paulstretch uses;
//!    it is close to a Hann window with softer shoulders).
//! 2. FFT it and keep every bin's magnitude.
//! 3. Throw the phases away and draw new ones at random, uniform over a
//!    full turn.
//! 4. Inverse FFT, and shape the frame by the same window again.
//! 5. Overlap-add the frame into the output, half a window after the
//!    previous one. The *input* window moves only `1 / stretch` as far,
//!    so a long stretch re-reads each stretch of the source many times,
//!    each time with a different phase.
//!
//! The re-randomised phases are why a stretched note becomes a smooth,
//! shimmering pad instead of a slowed-down note: the pitch (the bin
//! magnitudes) is the source's, and the transients (which live in the
//! phases) are gone.
//!
//! Where this differs from the reference scripts, on purpose:
//!
//! * **Level.** The reference scales the output with a fixed correction
//!   for its window. Here each output sample is divided by the square
//!   root of the summed squared window weights that reached it, times
//!   the window's mean square. Frames with random phase add in power, not
//!   in amplitude, so that division is the exact gain that returns the
//!   input's RMS for a steady tone, at the start and end as well.
//! * **Alignment.** Frame `k`'s centre is at output `k * N/2` and reads
//!   the input around `k * N/2 / stretch`, so a sound is where its time
//!   scaling says it is. The reference reads from `k * N/2 / stretch`
//!   as the window's *start*, which shifts the result by half a window.
//! * **Window size** is rounded up to a power of two (the next one at or
//!   above `window_sec`), which keeps the FFT fast at any sample rate.
//! * **Randomness** is a SplitMix64 stream seeded per channel from `seed`,
//!   so the same call gives the same samples on every platform. Each
//!   channel draws its own phases, as in the reference, which widens a
//!   stereo source a little.
//!
//! Sources: <http://hypermammut.sourceforge.net/paulstretch/> (the
//! author's description) and <https://github.com/paulnasca/paulstretch_python>
//! (the reference implementation, `paulstretch_mono.py` / `paulstretch_stereo.py`).

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use crate::vocoder::{deinterleave, interleave};
use crate::{check_channels, Error, Result};

/// The smallest factor: Paulstretch only lengthens. (Anything under 1.0 is
/// a speed-up, which `time_stretch` does with the input's own phases.)
pub const MIN_PAULSTRETCH_FACTOR: f32 = 1.0;

/// The largest factor. At 50x a minute of audio is fifty minutes; past it
/// the texture no longer changes and only the memory grows.
pub const MAX_PAULSTRETCH_FACTOR: f32 = 50.0;

/// The shortest window, in seconds. Below 20 ms a window holds under two
/// periods of a 100 Hz note, so the magnitudes no longer say what pitch it
/// was and the result is noise.
pub const MIN_PAULSTRETCH_WINDOW_SEC: f32 = 0.02;

/// The longest window, in seconds. The reference's default is 0.25 s; a few
/// seconds gives the slowest swells, and longer ones only blur the music
/// into one chord.
pub const MAX_PAULSTRETCH_WINDOW_SEC: f32 = 5.0;

/// Stretch interleaved `samples` by `stretch` with Paulstretch.
///
/// The output has `round(frames * stretch)` frames, in the input's layout.
/// `window_sec` is the analysis window in seconds, rounded up to a power of
/// two samples. `seed` makes the random phases repeatable.
///
/// # Errors
///
/// * [`Error::InvalidStretch`] — `stretch` is not finite or is outside
///   [`MIN_PAULSTRETCH_FACTOR`]..=[`MAX_PAULSTRETCH_FACTOR`].
/// * [`Error::InvalidWindow`] — `window_sec` is not finite or is outside
///   [`MIN_PAULSTRETCH_WINDOW_SEC`]..=[`MAX_PAULSTRETCH_WINDOW_SEC`].
/// * [`Error::InvalidSampleRate`] — `sample_rate` is zero.
/// * [`Error::ChannelMismatch`] — `channels` is zero, or `samples.len()`
///   does not divide evenly by it.
pub fn paulstretch(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    stretch: f32,
    window_sec: f32,
    seed: u64,
) -> Result<Vec<f32>> {
    if !(MIN_PAULSTRETCH_FACTOR..=MAX_PAULSTRETCH_FACTOR).contains(&stretch) {
        return Err(Error::InvalidStretch(stretch));
    }
    if !(MIN_PAULSTRETCH_WINDOW_SEC..=MAX_PAULSTRETCH_WINDOW_SEC).contains(&window_sec) {
        return Err(Error::InvalidWindow(window_sec));
    }
    if sample_rate == 0 {
        return Err(Error::InvalidSampleRate(sample_rate));
    }
    check_channels(samples.len(), channels)?;
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let n = window_len(sample_rate, window_sec);
    let planes: Vec<Vec<f32>> = deinterleave(samples, channels as usize)
        .iter()
        .enumerate()
        .map(|(ch, plane)| stretch_plane(plane, stretch as f64, n, channel_seed(seed, ch)))
        .collect();
    Ok(interleave(&planes))
}

/// The window length in samples that `window_sec` becomes at `sample_rate`:
/// the next power of two at or above it, and at least 64.
pub fn window_len(sample_rate: u32, window_sec: f32) -> usize {
    let want = (window_sec as f64 * sample_rate as f64).round().max(1.0) as usize;
    want.next_power_of_two().max(64)
}

/// A stream per channel, so two channels never share their phases.
fn channel_seed(seed: u64, channel: usize) -> u64 {
    seed ^ (channel as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// SplitMix64 (Steele, Lea and Flood, 2014): a few lines, no dependency,
/// and the same stream everywhere.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)` from the top 24 bits.
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// `(1 - x^2)^1.25` over `n` samples, symmetric and never exactly zero.
fn window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = 2.0 * (i as f64 + 0.5) / n as f64 - 1.0;
            (1.0 - x * x).powf(1.25) as f32
        })
        .collect()
}

/// The least summed squared window weight a sample is divided by. Every
/// output sample is under at least one frame's window, whose weight is never
/// zero (`window` keeps its ends above zero), so this only keeps a sample at
/// a frame's extreme edge, where the weight is tiny, from being amplified
/// into noise.
const MIN_WEIGHT: f32 = 1e-6;

fn stretch_plane(input: &[f32], stretch: f64, n: usize, seed: u64) -> Vec<f32> {
    // `paulstretch` sends only non-empty planes and a stretch of at least 1,
    // so the output has at least one frame.
    let out_len = (input.len() as f64 * stretch).round() as usize;
    debug_assert!(out_len > 0, "an empty plane never reaches stretch_plane");
    let hop = n / 2;
    let w = window(n);
    let mean_w2 = w.iter().map(|v| v * v).sum::<f32>() / n as f32;

    // Frame `k` is centred on output `k * hop`; the buffer is shifted by
    // half a window so the first frame (centred on 0) starts at index 0.
    let frames = out_len.div_ceil(hop) + 2;
    let mut acc = vec![0.0f32; frames * hop + n];
    let mut weight = vec![0.0f32; acc.len()];

    let mut planner = RealFftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);
    let mut frame = forward.make_input_vec();
    let mut spectrum = forward.make_output_vec();
    let mut rng = SplitMix64(seed);
    let norm = 1.0 / n as f32;

    for k in 0..frames {
        let centre = (k * hop) as f64 / stretch;
        let start = centre.round() as isize - hop as isize;
        let mut silent = true;
        for (i, slot) in frame.iter_mut().enumerate() {
            let src = start + i as isize;
            let v = if src >= 0 {
                input.get(src as usize).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            silent &= v == 0.0;
            *slot = v * w[i];
        }
        let base = k * hop;
        for (i, wi) in w.iter().enumerate() {
            weight[base + i] += wi * wi;
        }
        // Nothing to hear in this frame; the draws it would have used are
        // skipped too, which keeps the stream a function of the input.
        if silent {
            continue;
        }

        forward
            .process(&mut frame, &mut spectrum)
            .expect("buffers made by the plan have its lengths");
        let last = spectrum.len() - 1;
        for (b, bin) in spectrum.iter_mut().enumerate() {
            let mag = bin.norm();
            // DC and Nyquist have no phase to give: the inverse wants them real.
            *bin = if b == 0 || b == last {
                Complex::new(mag, 0.0)
            } else {
                Complex::from_polar(mag, rng.unit() * std::f32::consts::TAU)
            };
        }
        inverse
            .process(&mut spectrum, &mut frame)
            .expect("buffers made by the plan have its lengths");
        for (i, v) in frame.iter().enumerate() {
            acc[base + i] += v * norm * w[i];
        }
    }

    (0..out_len)
        .map(|t| {
            let idx = t + hop;
            acc[idx] / (mean_w2 * weight[idx].max(MIN_WEIGHT)).sqrt()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_symmetric_positive_and_peaks_in_the_middle() {
        let w = window(1024);
        assert!(w.iter().all(|v| *v > 0.0 && *v <= 1.0));
        assert!((w[0] - w[1023]).abs() < 1e-6);
        assert!(w[512] > 0.99);
    }

    #[test]
    fn window_length_is_a_power_of_two_at_or_above_the_request() {
        assert_eq!(window_len(48_000, 0.25), 16_384);
        assert_eq!(window_len(44_100, 0.25), 16_384);
        assert_eq!(window_len(8_000, 0.02), 256);
        assert_eq!(window_len(8_000, 0.0001), 64);
    }

    #[test]
    fn the_random_stream_is_fixed() {
        // Pinned so a platform or a refactor cannot change what a seed means.
        let mut r = SplitMix64(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        let mut u = SplitMix64(1);
        assert!((0..1000).map(|_| u.unit()).all(|v| (0.0..1.0).contains(&v)));
    }

    #[test]
    fn channels_do_not_share_a_stream() {
        assert_ne!(channel_seed(5, 0), channel_seed(5, 1));
    }
}
