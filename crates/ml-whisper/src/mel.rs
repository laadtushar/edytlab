//! The log-mel spectrogram Whisper's encoder takes as input.
//!
//! This is a port of `log_mel_spectrogram` from `whisper/audio.py` in
//! <https://github.com/openai/whisper> (MIT), release 20250625, which is also
//! what Hugging Face's `WhisperFeatureExtractor` computes (the plan on #384
//! names that as the reference, because the pinned ONNX export is Hugging
//! Face's). The ONNX encoder takes `input_features` `[1, 80, 3000]`, so the
//! numbers have to match Whisper's to the digit that matters or the model
//! hears something other than the audio, without any error to say so.
//!
//! | | |
//! |---|---|
//! | sample rate | 16 kHz mono ([`SAMPLE_RATE`]) |
//! | `n_fft` | 400 = 25 ms ([`N_FFT`]) |
//! | hop | 160 = 10 ms ([`HOP_LENGTH`]) |
//! | window | Hann, periodic (`torch.hann_window(400)`), length `n_fft` |
//! | centring | `center=True`, `pad_mode="reflect"`: 200 reflected samples each side |
//! | spectrum | power, `|X|²`, bins `0..=200` ([`N_FREQS`] = 201) |
//! | frames | `len / 160`: the transform makes `1 + len / 160` and the last is dropped |
//! | filterbank | [`N_MELS`] = 80 Slaney-scale, Slaney-normalised triangles, 0 to 8 kHz (see [`mel_filters`]) |
//! | log | `log10(max(mel, 1e-10))` |
//! | range | clamp to `max - 8` (80 dB below the loudest value in *this* spectrogram) |
//! | scale | `(x + 4) / 4` |
//!
//! ## The 30 s window, and where this differs from `transcribe()`
//!
//! The encoder always takes 3000 frames. [`MelExtractor::log_mel_window`]
//! gets there the way Hugging Face's extractor does: it pads (or trims) the
//! **audio** to 30 s of samples, 480 000, **before** the transform. Frames
//! that see only padding are therefore the spectrogram of silence, the
//! constant `max - 8` floor, scaled like everything else.
//!
//! OpenAI's own `transcribe()` instead computes one spectrogram over the
//! whole recording plus 30 s of zero samples, slices windows out of it, and
//! pads a short last slice with `0.0` *after* scaling. That differs from
//! this in two ways: the first and last frame or two of a window, which see
//! real neighbouring audio instead of a reflection of their own edge, and the
//! value of the tail of a short last window. The model tolerates both. This
//! module follows the Hugging Face path because the plan pins that export;
//! the chunking step (plan step 5) must pick one convention for all windows
//! and say so. [`MelExtractor::log_mel`] is the building block for either.
//!
//! ## What the tests check against
//!
//! `tests/fixtures/make_mel_fixture.py` runs upstream's own function under
//! PyTorch on two short signals and writes what it produced; the Rust
//! output is compared with that within 1e-3 (the plan's figure). Beside that,
//! analytic checks that do not need a reference: silence gives the floor
//! value, a pure tone peaks in the right mel bin, and scaling the signal
//! shifts the whole spectrogram by a constant.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

/// The sample rate Whisper was trained at.
pub const SAMPLE_RATE: usize = 16_000;
/// STFT size: 25 ms.
pub const N_FFT: usize = 400;
/// STFT hop: 10 ms.
pub const HOP_LENGTH: usize = 160;
/// Mel bands of the pinned export (`large-v3` has 128; the plan pins `base`).
pub const N_MELS: usize = 80;
/// Frequency bins of a real transform of [`N_FFT`] samples.
pub const N_FREQS: usize = N_FFT / 2 + 1;
/// One encoder window of audio: 30 s.
pub const N_SAMPLES: usize = 30 * SAMPLE_RATE;
/// One encoder window of frames: 3000.
pub const N_FRAMES: usize = N_SAMPLES / HOP_LENGTH;

/// `torch.clamp(mel_spec, min=1e-10)`: the power below which a mel value is
/// treated as silence (-100 dB).
const MEL_FLOOR: f32 = 1e-10;
/// The spectrogram is clamped to this many decades (80 dB) below its loudest
/// value.
const DYNAMIC_RANGE_LOG10: f32 = 8.0;

/// A spectrogram: `n_mels` rows of `n_frames` values, row-major, so that
/// `as_slice()` is the layout of the encoder's `input_features` `[1, 80, T]`
/// without a transpose.
#[derive(Debug, Clone, PartialEq)]
pub struct LogMel {
    n_mels: usize,
    n_frames: usize,
    data: Vec<f32>,
}

impl LogMel {
    /// Mel bands (rows).
    pub fn n_mels(&self) -> usize {
        self.n_mels
    }

    /// Frames (columns).
    pub fn n_frames(&self) -> usize {
        self.n_frames
    }

    /// All values, row-major: mel band `m`, frame `t` is at `m * n_frames + t`.
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// Consume into the row-major values.
    pub fn into_vec(self) -> Vec<f32> {
        self.data
    }

    /// One mel band across time.
    pub fn row(&self, mel: usize) -> &[f32] {
        &self.data[mel * self.n_frames..(mel + 1) * self.n_frames]
    }
}

/// The Slaney mel filterbank Whisper uses: [`N_MELS`] rows of [`N_FREQS`]
/// weights, row-major, as `librosa.filters.mel(sr=16000, n_fft=400,
/// n_mels=80)` returns (and as `whisper/assets/mel_filters.npz` stores it).
///
/// The scale is Slaney's: linear below 1 kHz (200/3 Hz per mel), logarithmic
/// above. Each filter is a triangle on that scale and is divided by its width
/// in Hz, so it has unit area (`2 / (f_hi - f_lo)` at the peak).
pub fn mel_filters() -> Vec<f32> {
    // Everything here is `librosa.filters.mel` with `fmin=0`, `fmax=sr/2`,
    // `htk=False`, `norm="slaney"`, in f64 and rounded to f32 at the end.
    let hz_per_bin = SAMPLE_RATE as f64 / N_FFT as f64;
    let mel_max = hz_to_mel(SAMPLE_RATE as f64 / 2.0);
    // N_MELS + 2 points, equally spaced on the mel scale, are the edges and
    // centres of the triangles: band m rises from point m, peaks at m + 1
    // and falls to m + 2.
    let points_hz: Vec<f64> = (0..N_MELS + 2)
        .map(|i| mel_to_hz(mel_max * i as f64 / (N_MELS + 1) as f64))
        .collect();

    let mut bank = vec![0.0f32; N_MELS * N_FREQS];
    for m in 0..N_MELS {
        let (lo, mid, hi) = (points_hz[m], points_hz[m + 1], points_hz[m + 2]);
        let area_norm = 2.0 / (hi - lo);
        for k in 0..N_FREQS {
            let f = k as f64 * hz_per_bin;
            let rising = (f - lo) / (mid - lo);
            let falling = (hi - f) / (hi - mid);
            let weight = rising.min(falling).max(0.0) * area_norm;
            bank[m * N_FREQS + k] = weight as f32;
        }
    }
    bank
}

/// Hz to the Slaney mel scale: 200/3 Hz per mel up to 1 kHz (mel 15), then
/// logarithmic with 27 mel per factor 6.4 in frequency.
fn hz_to_mel(hz: f64) -> f64 {
    const HZ_PER_MEL: f64 = 200.0 / 3.0;
    const LOG_START_HZ: f64 = 1000.0;
    if hz >= LOG_START_HZ {
        LOG_START_HZ / HZ_PER_MEL + (hz / LOG_START_HZ).ln() / log_step()
    } else {
        hz / HZ_PER_MEL
    }
}

/// The inverse of [`hz_to_mel`].
fn mel_to_hz(mel: f64) -> f64 {
    const HZ_PER_MEL: f64 = 200.0 / 3.0;
    const LOG_START_HZ: f64 = 1000.0;
    const LOG_START_MEL: f64 = LOG_START_HZ / HZ_PER_MEL;
    if mel >= LOG_START_MEL {
        LOG_START_HZ * (log_step() * (mel - LOG_START_MEL)).exp()
    } else {
        mel * HZ_PER_MEL
    }
}

/// `ln(6.4) / 27`: the Slaney scale's logarithmic step per mel.
fn log_step() -> f64 {
    6.4f64.ln() / 27.0
}

/// Computes log-mel spectrograms. Build one and reuse it: the FFT plan,
/// window and filterbank are made once.
pub struct MelExtractor {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// Per mel band, the first bin with a nonzero weight and the weights from
    /// there. The triangles are narrow, so this skips most of the 80 x 201
    /// product.
    bands: Vec<(usize, Vec<f32>)>,
}

impl Default for MelExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl MelExtractor {
    /// Plan the transform and build the window and filterbank.
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        // `torch.hann_window(400)` is the periodic window: the cosine goes
        // round once over `n_fft` samples, not `n_fft - 1`.
        let window = (0..N_FFT)
            .map(|n| {
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / N_FFT as f64).cos()) as f32
            })
            .collect();
        let bank = mel_filters();
        let bands = bank
            .chunks_exact(N_FREQS)
            .map(|row| {
                let first = row.iter().position(|w| *w > 0.0).unwrap_or(0);
                let last = row.iter().rposition(|w| *w > 0.0).map_or(first, |l| l + 1);
                (first, row[first..last.max(first)].to_vec())
            })
            .collect();
        Self { fft, window, bands }
    }

    /// Whisper's `log_mel_spectrogram(audio)`: the spectrogram of `audio` as
    /// it is, `audio.len() / 160` frames. No padding to 30 s; see
    /// [`log_mel_window`](Self::log_mel_window) for what the encoder takes.
    ///
    /// `audio` is 16 kHz mono and must be finite (the `transcribe` path
    /// checks that). Unlike `torch.stft`, which refuses an input shorter than
    /// the 200-sample reflection, a short input is reflected repeatedly the
    /// way numpy's `pad(mode="reflect")` does, so any length works; fewer
    /// than 160 samples make zero frames.
    pub fn log_mel(&self, audio: &[f32]) -> LogMel {
        let n_frames = audio.len() / HOP_LENGTH;
        let mut data = vec![0.0f32; N_MELS * n_frames];
        if n_frames == 0 {
            return LogMel {
                n_mels: N_MELS,
                n_frames,
                data,
            };
        }

        let mut frame = self.fft.make_input_vec();
        let mut spectrum = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        let mut power = [0.0f32; N_FREQS];

        for t in 0..n_frames {
            // Frame `t` of the centred transform covers samples
            // `t * 160 - 200 .. t * 160 + 200` of the unpadded signal.
            let first = (t * HOP_LENGTH) as isize - (N_FFT / 2) as isize;
            for (k, (slot, w)) in frame.iter_mut().zip(&self.window).enumerate() {
                let i = first + k as isize;
                let sample = match usize::try_from(i) {
                    Ok(i) if i < audio.len() => audio[i],
                    _ => audio[reflect_index(i, audio.len())],
                };
                *slot = sample * w;
            }
            self.fft
                .process_with_scratch(&mut frame, &mut spectrum, &mut scratch)
                .expect("buffer lengths come from the plan");
            for (p, c) in power.iter_mut().zip(&spectrum) {
                *p = c.norm_sqr();
            }
            for (m, (first_bin, weights)) in self.bands.iter().enumerate() {
                let mel: f32 = weights
                    .iter()
                    .zip(&power[*first_bin..])
                    .map(|(w, p)| w * p)
                    .sum();
                data[m * n_frames + t] = mel.max(MEL_FLOOR).log10();
            }
        }

        // Clamp to 80 dB below the loudest value of this spectrogram, then
        // scale. `max` is over the whole thing, so it is the caller's chunk
        // that sets the floor: see the module docs on windows.
        let loudest = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let floor = loudest - DYNAMIC_RANGE_LOG10;
        for v in &mut data {
            *v = (v.max(floor) + 4.0) / 4.0;
        }
        LogMel {
            n_mels: N_MELS,
            n_frames,
            data,
        }
    }

    /// The input of one encoder pass: [`N_FRAMES`] frames of [`N_MELS`]
    /// bands. `audio` is zero-padded to [`N_SAMPLES`] first, or cut to it if
    /// longer (the caller windows long audio; this does not).
    pub fn log_mel_window(&self, audio: &[f32]) -> LogMel {
        let mut padded = Vec::with_capacity(N_SAMPLES);
        padded.extend_from_slice(&audio[..audio.len().min(N_SAMPLES)]);
        padded.resize(N_SAMPLES, 0.0);
        self.log_mel(&padded)
    }
}

/// Where sample `i` of the reflect-padded signal comes from, for any integer
/// `i` and a signal of `len >= 1` samples: numpy's `pad(mode="reflect")`,
/// which mirrors about the first and last sample without repeating them and
/// keeps bouncing if the pad is longer than the signal.
fn reflect_index(i: isize, len: usize) -> usize {
    if len == 1 {
        return 0;
    }
    let period = 2 * (len as isize - 1);
    let j = i.rem_euclid(period);
    if j >= len as isize {
        (period - j) as usize
    } else {
        j as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How closely the Rust output must match PyTorch's, on the normalised
    /// scale (`(log10 + 4) / 4`, so a 1.0 error is a factor 10^4 in power).
    /// The plan on #384 asks for 1e-3. Measured: 7.7e-6 on the tone and 1.3e-4
    /// on the chirp window, the latter in bands within 0.01 of the clamp
    /// floor, where a mel value is a sum of tiny leakage terms and two FFTs
    /// round differently in float32.
    const GOLDEN_TOLERANCE: f32 = 1e-3;

    const TONE_GOLDEN: &[u8] = include_bytes!("../tests/fixtures/mel_1khz_1s.f32");
    const CHIRP_INPUT: &[u8] = include_bytes!("../tests/fixtures/mel_chirp_input.f32");
    const CHIRP_WINDOW_HEAD: &[u8] = include_bytes!("../tests/fixtures/mel_chirp_window_head.f32");
    /// Frames of the padded window stored in `mel_chirp_window_head.f32`.
    const HEAD_FRAMES: usize = 110;

    fn f32s(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    /// One second of the fixture's tone: `0.5 * sin(2 pi (n mod 16) / 16)`.
    fn tone_1khz(seconds: usize) -> Vec<f32> {
        (0..SAMPLE_RATE * seconds)
            .map(|n| (0.5 * (2.0 * std::f64::consts::PI * (n % 16) as f64 / 16.0).sin()) as f32)
            .collect()
    }

    fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len(), "length mismatch");
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    }

    // ---- against PyTorch ---------------------------------------------------

    #[test]
    fn tone_matches_whispers_log_mel_spectrogram() {
        let expected = f32s(TONE_GOLDEN);
        assert_eq!(expected.len(), N_MELS * 100);

        let mel = MelExtractor::new().log_mel(&tone_1khz(1));

        assert_eq!((mel.n_mels(), mel.n_frames()), (80, 100));
        let err = max_abs_diff(mel.as_slice(), &expected);
        assert!(err < GOLDEN_TOLERANCE, "max abs error {err}");
    }

    #[test]
    fn padded_chirp_window_matches_whispers_pad_or_trim_then_log_mel() {
        let chirp = f32s(CHIRP_INPUT);
        assert_eq!(chirp.len(), SAMPLE_RATE);
        let head = f32s(CHIRP_WINDOW_HEAD);
        assert_eq!(head.len(), N_MELS * HEAD_FRAMES);

        let window = MelExtractor::new().log_mel_window(&chirp);

        assert_eq!((window.n_mels(), window.n_frames()), (80, 3000));
        // The first 110 frames: the signal, the two frames where the zero
        // padding replaces the reflection a bare 1 s input would get, and
        // frames of pure padding.
        let mut worst = 0.0f32;
        for m in 0..N_MELS {
            let got = &window.row(m)[..HEAD_FRAMES];
            let want = &head[m * HEAD_FRAMES..(m + 1) * HEAD_FRAMES];
            worst = worst.max(max_abs_diff(got, want));
        }
        assert!(worst < GOLDEN_TOLERANCE, "max abs error {worst}");

        // The script asserted that the other 2890 frames are one constant, the
        // floor; the last stored frame is that constant.
        let floor = head[HEAD_FRAMES - 1];
        for m in 0..N_MELS {
            for (t, v) in window.row(m).iter().enumerate().skip(HEAD_FRAMES) {
                assert!(
                    (v - floor).abs() < GOLDEN_TOLERANCE,
                    "mel {m} frame {t}: {v} vs floor {floor}"
                );
            }
        }
    }

    // ---- analytic ----------------------------------------------------------

    #[test]
    fn silence_is_the_floor_value_everywhere() {
        // mel = 0, clamped to 1e-10, log10 = -10; the max is -10 too, so the
        // `max - 8` clamp does nothing; (-10 + 4) / 4 = -1.5.
        let mel = MelExtractor::new().log_mel(&vec![0.0; SAMPLE_RATE]);
        assert_eq!(mel.n_frames(), 100);
        for v in mel.as_slice() {
            assert!((v - -1.5).abs() < 1e-6, "got {v}");
        }

        let window = MelExtractor::new().log_mel_window(&[]);
        assert_eq!(window.n_frames(), N_FRAMES);
        assert!(window.as_slice().iter().all(|v| (v - -1.5).abs() < 1e-6));
    }

    #[test]
    fn a_pure_tone_lands_in_the_mel_band_that_contains_it() {
        let mel = MelExtractor::new().log_mel(&tone_1khz(1));
        // Band 26 is centred on 1005 Hz: Slaney mel 15 is exactly 1 kHz, and
        // 81 intervals of (45.24 / 81) mel put centres at 968 Hz (band 25),
        // 1005 Hz (band 26) and 1043 Hz (band 27).
        let interior = 5..95;
        let mean = |m: usize| -> f32 {
            let row = &mel.row(m)[interior.clone()];
            row.iter().sum::<f32>() / row.len() as f32
        };
        let loudest = (0..N_MELS)
            .max_by(|&a, &b| mean(a).total_cmp(&mean(b)))
            .unwrap();
        assert_eq!(loudest, 26);

        // Steady state: a stationary tone is the same in every interior frame.
        for m in [25, 26, 27] {
            for v in &mel.row(m)[interior.clone()] {
                assert!(
                    (v - mean(m)).abs() < 1e-3,
                    "band {m} not steady: {v} vs {}",
                    mean(m)
                );
            }
        }

        // 1 kHz is exactly FFT bin 25 (40 Hz per bin) and 400 samples are
        // exactly 25 periods, so under the periodic Hann window the tone
        // occupies bins 24, 25 and 26 and nothing else. Only the bands whose
        // triangles reach those bins (24 to 27) rise above the clamp; every
        // other band is exactly `max - 2` on the normalised scale, in every
        // frame that does not touch the reflected edge.
        let max = mel.as_slice().iter().cloned().fold(f32::MIN, f32::max);
        let floor = max - 2.0;
        for m in (0..24).chain(28..N_MELS) {
            for v in &mel.row(m)[interior.clone()] {
                assert!((v - floor).abs() < 1e-6, "band {m}: {v} vs floor {floor}");
            }
        }
        for m in 24..=27 {
            assert!(mean(m) > floor + 1.0, "band {m} should carry the tone");
        }
    }

    #[test]
    fn scaling_the_signal_shifts_every_value_by_the_same_amount() {
        // x10 is +20 dB: power x100, log10 + 2, and (x + 4) / 4 turns that
        // into + 0.5. The `max - 8` clamp is relative and the 1e-10 floor is
        // never reached, so the whole spectrogram moves, floor included.
        let chirp = f32s(CHIRP_INPUT);
        let loud: Vec<f32> = chirp.iter().map(|s| s * 10.0).collect();
        let extractor = MelExtractor::new();
        let quiet = extractor.log_mel(&chirp);
        let louder = extractor.log_mel(&loud);
        for (a, b) in quiet.as_slice().iter().zip(louder.as_slice()) {
            assert!((b - a - 0.5).abs() < 1e-4, "{a} -> {b}");
        }
    }

    #[test]
    fn frame_count_is_len_over_hop_and_the_window_is_always_3000() {
        let extractor = MelExtractor::new();
        for (len, frames) in [
            (0, 0),
            (159, 0),
            (160, 1),
            (161, 1),
            (16_000, 100),
            (16_159, 100),
            (16_160, 101),
        ] {
            let mel = extractor.log_mel(&vec![0.25; len]);
            assert_eq!(
                (mel.n_frames(), mel.as_slice().len()),
                (frames, N_MELS * frames),
                "len {len}"
            );
        }
        for len in [
            0,
            1,
            199,
            200,
            201,
            16_000,
            N_SAMPLES - 1,
            N_SAMPLES,
            N_SAMPLES + 1,
            N_SAMPLES * 2,
        ] {
            let window = extractor.log_mel_window(&vec![0.25; len]);
            assert_eq!(
                (window.n_mels(), window.n_frames()),
                (80, 3000),
                "len {len}"
            );
        }
    }

    #[test]
    fn audio_shorter_than_the_reflection_does_not_panic() {
        // 161 samples is 1 frame, and the 200-sample reflection each side is
        // longer than the input: torch.stft would error, numpy reflects
        // repeatedly. Every value must be finite.
        let extractor = MelExtractor::new();
        for len in [1, 2, 3, 100, 161, 200, 201, 320] {
            let audio: Vec<f32> = (0..len)
                .map(|n| ((n * 7) % 13) as f32 / 13.0 - 0.5)
                .collect();
            let mel = extractor.log_mel(&audio);
            assert_eq!(mel.n_frames(), len / HOP_LENGTH);
            assert!(mel.as_slice().iter().all(|v| v.is_finite()), "len {len}");
        }
    }

    #[test]
    fn audio_past_30_seconds_is_cut_not_streamed() {
        // log_mel_window takes the first 30 s. Samples after that cannot
        // change the first frames (the last frame or two reflect off the cut).
        let extractor = MelExtractor::new();
        let mut long = tone_1khz(31);
        let a = extractor.log_mel_window(&long);
        for s in &mut long[N_SAMPLES..] {
            *s = 0.9;
        }
        let b = extractor.log_mel_window(&long);
        assert_eq!(a, b);
    }

    // ---- the filterbank ----------------------------------------------------

    #[test]
    fn filterbank_has_the_shape_and_edges_of_librosas_slaney_bank() {
        let bank = mel_filters();
        assert_eq!(bank.len(), N_MELS * N_FREQS);
        assert!(bank.iter().all(|w| *w >= 0.0 && w.is_finite()));

        let row = |m: usize| &bank[m * N_FREQS..(m + 1) * N_FREQS];
        let peak = |m: usize| -> usize {
            row(m)
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0
        };
        // No filter is empty, and their peaks climb with frequency.
        for m in 0..N_MELS {
            assert!(row(m).iter().any(|w| *w > 0.0), "band {m} is empty");
        }
        for m in 1..N_MELS {
            assert!(
                peak(m) >= peak(m - 1),
                "band {m} peaks below band {}",
                m - 1
            );
        }
        // Spot values taken from `whisper/assets/mel_filters.npz` (the file
        // upstream loads), in full f32 precision: the lowest filter's only
        // nonzero bin, a peak in the linear region, one in the log region,
        // and the highest filter's peak. `mel_filters()` was also compared
        // with all 80 x 201 values of that file: largest difference 1.9e-9,
        // and the same 391 nonzero weights.
        let at = |m: usize, k: usize| bank[m * N_FREQS + k];
        for (m, k, upstream) in [
            (0, 1, 0.024_862_595),
            (1, 2, 0.022_871_772),
            (13, 13, 0.025_880_683),
            (26, 25, 0.022_114_217),
            (40, 43, 0.014_735_566),
            (79, 192, 0.003_164_711_9),
            (79, 193, 0.003_141_313_3),
        ] {
            assert!(
                (at(m, k) - upstream).abs() < 1e-8,
                "band {m} bin {k}: {} vs {upstream}",
                at(m, k)
            );
        }
        assert_eq!(bank.iter().filter(|w| **w > 0.0).count(), 391);

        // Slaney normalisation: each triangle has unit area in Hz. A bin is
        // 40 Hz wide, so sum * 40 is the area, to within the sampling error
        // of a narrow triangle on a 40 Hz grid (upstream's own bank spans
        // 0.93 to 1.08).
        for m in 0..N_MELS {
            let area = row(m).iter().sum::<f32>() * 40.0;
            assert!((area - 1.0).abs() < 0.1, "band {m}: area {area}");
        }
    }
}
