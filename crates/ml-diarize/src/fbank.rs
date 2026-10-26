//! The 80-band Kaldi filterbank the speaker embedding network takes as input.
//!
//! WeSpeaker's ResNet34 does not take audio: it takes the log filterbank that
//! Kaldi defines, and the ONNX file does not contain the code that makes it.
//! pyannote.audio computes it with `torchaudio.compliance.kaldi.fbank` in
//! `ONNXWeSpeakerPretrainedSpeakerEmbedding.compute_fbank`, from the WeSpeaker
//! recipe (`wespeaker/bin/infer_onnx.py`), with these settings, which this
//! module follows number for number:
//!
//! | | |
//! |---|---|
//! | input | 16 kHz mono, scaled by 32768 (`1 << 15`) as if it were 16-bit integers |
//! | frames | 25 ms = 400 samples, every 10 ms = 160, `snip_edges`: only whole frames, `1 + (n - 400) / 160` of them |
//! | per frame | the mean taken out (`remove_dc_offset`), pre-emphasis 0.97, a Hamming window (`0.54 - 0.46 cos(2 pi i / 399)`), zero-padded to 512 |
//! | spectrum | power, `|X|^2`, bins 0..256 (Nyquist is left out, as Kaldi leaves it) |
//! | filterbank | 80 triangles, equal in the mel scale `1127 ln(1 + f / 700)`, from 20 Hz to the Nyquist frequency |
//! | log | `ln(max(energy, f32::EPSILON))` |
//! | dither | none |
//!
//! [`Fbank::compute`] makes that. The model is fed the frames **after the mean
//! of each band over the whole window is taken out** ([`subtract_mean`]),
//! which `compute_fbank` does on the 10 s window before it picks the frames of
//! one speaker; so the caller does it on the window, not on the pick.
//!
//! `tests/fixtures/make_fixtures.py` writes the reference, from
//! kaldi-native-fbank (the C++ Kaldi's feature code was ported to): three
//! seconds of real speech, every value compared within 2e-3.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

/// The sample rate the filterbank is defined for.
pub const SAMPLE_RATE: usize = 16_000;
/// 25 ms.
pub const FRAME_LENGTH: usize = 400;
/// 10 ms.
pub const FRAME_SHIFT: usize = 160;
/// Mel bands: the network's input width.
pub const BANDS: usize = 80;
/// The FFT size: the next power of two over [`FRAME_LENGTH`].
const FFT_SIZE: usize = 512;
/// FFT bins the filterbank reads: all but the Nyquist bin.
const FFT_BINS: usize = FFT_SIZE / 2;
const LOW_FREQ: f32 = 20.0;
const PREEMPHASIS: f32 = 0.97;
/// The audio is scaled to the range of 16-bit integers first.
const SCALE: f32 = 32_768.0;

/// Frames of a filterbank: `frames` rows of [`BANDS`] values, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Frames {
    pub values: Vec<f32>,
    pub frames: usize,
}

impl Frames {
    /// Row `i`.
    pub fn row(&self, i: usize) -> &[f32] {
        &self.values[i * BANDS..(i + 1) * BANDS]
    }
}

/// How many frames `samples` samples make: Kaldi's `snip_edges` count.
pub fn frame_count(samples: usize) -> usize {
    if samples < FRAME_LENGTH {
        0
    } else {
        1 + (samples - FRAME_LENGTH) / FRAME_SHIFT
    }
}

fn mel(freq: f32) -> f32 {
    1127.0 * (1.0 + freq / 700.0).ln()
}

/// The filterbank. Building one makes the window, the 80 triangles and the FFT
/// plan, so keep it for as many windows as there are.
pub struct Fbank {
    window: Vec<f32>,
    /// Per band: the first FFT bin it reads, and its weights from there.
    bands: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn RealToComplex<f32>>,
}

impl Default for Fbank {
    fn default() -> Self {
        Self::new()
    }
}

impl Fbank {
    pub fn new() -> Self {
        let window = (0..FRAME_LENGTH)
            .map(|i| {
                0.54 - 0.46
                    * (2.0 * std::f64::consts::PI * i as f64 / (FRAME_LENGTH - 1) as f64).cos()
                        as f32
            })
            .collect();

        let nyquist = SAMPLE_RATE as f32 / 2.0;
        let (mel_low, mel_high) = (mel(LOW_FREQ), mel(nyquist));
        let delta = (mel_high - mel_low) / (BANDS + 1) as f32;
        let bin_width = SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let bands = (0..BANDS)
            .map(|b| {
                let left = mel_low + b as f32 * delta;
                let center = mel_low + (b + 1) as f32 * delta;
                let right = mel_low + (b + 2) as f32 * delta;
                let mut first = None;
                let mut weights = Vec::new();
                for i in 0..FFT_BINS {
                    let m = mel(bin_width * i as f32);
                    let w = if m > left && m < right {
                        if m <= center {
                            (m - left) / (center - left)
                        } else {
                            (right - m) / (right - center)
                        }
                    } else {
                        0.0
                    };
                    if w > 0.0 {
                        first.get_or_insert(i);
                        weights.push(w);
                    } else if first.is_some() {
                        break;
                    }
                }
                (first.unwrap_or(0), weights)
            })
            .collect();

        Self {
            window,
            bands,
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE),
        }
    }

    /// The filterbank of `samples` (16 kHz mono, -1 to 1): [`frame_count`] rows.
    pub fn compute(&self, samples: &[f32]) -> Frames {
        let frames = frame_count(samples.len());
        let mut values = Vec::with_capacity(frames * BANDS);
        let mut input = self.fft.make_input_vec();
        let mut spectrum = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        let mut frame = [0.0f32; FRAME_LENGTH];
        for f in 0..frames {
            let at = f * FRAME_SHIFT;
            for (dst, src) in frame.iter_mut().zip(&samples[at..at + FRAME_LENGTH]) {
                *dst = src * SCALE;
            }
            let mean = frame.iter().sum::<f32>() / FRAME_LENGTH as f32;
            for v in &mut frame {
                *v -= mean;
            }
            // Pre-emphasis, last sample first so each uses the one before it
            // as it was; the first uses itself.
            for i in (1..FRAME_LENGTH).rev() {
                frame[i] -= PREEMPHASIS * frame[i - 1];
            }
            frame[0] -= PREEMPHASIS * frame[0];

            input.fill(0.0);
            for ((dst, v), w) in input.iter_mut().zip(&frame).zip(&self.window) {
                *dst = v * w;
            }
            self.fft
                .process_with_scratch(&mut input, &mut spectrum, &mut scratch)
                .expect("the buffers are the plan's own");
            for (first, weights) in &self.bands {
                let energy: f32 = weights
                    .iter()
                    .zip(&spectrum[*first..])
                    .map(|(w, c)| w * (c.re * c.re + c.im * c.im))
                    .sum();
                values.push(energy.max(f32::EPSILON).ln());
            }
        }
        Frames { values, frames }
    }
}

/// Take each band's mean over all the frames out of every frame: what
/// `compute_fbank` does to a whole window before the frames of one speaker are
/// picked from it.
pub fn subtract_mean(frames: &mut Frames) {
    if frames.frames == 0 {
        return;
    }
    let mut mean = [0.0f64; BANDS];
    for row in frames.values.chunks_exact(BANDS) {
        for (m, v) in mean.iter_mut().zip(row) {
            *m += f64::from(*v);
        }
    }
    let n = frames.frames as f64;
    for row in frames.values.chunks_exact_mut(BANDS) {
        for (v, m) in row.iter_mut().zip(&mean) {
            *v -= (m / n) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    fn excerpt() -> Vec<f32> {
        let mut reader = hound::WavReader::new(std::io::Cursor::new(
            &include_bytes!("../tests/fixtures/ami_es2004a_640_680s.wav")[..],
        ))
        .unwrap();
        reader
            .samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32768.0)
            .collect()
    }

    /// Kaldi's fbank (kaldi-native-fbank, which is what torchaudio's
    /// `compliance.kaldi.fbank` is checked against) on three seconds of real
    /// speech: every one of 298 x 80 values.
    #[test]
    fn matches_the_reference_on_real_speech() {
        let reference = floats(include_bytes!("../tests/fixtures/fbank_ref.f32"));
        let audio = excerpt();
        let got = Fbank::new().compute(&audio[8000..56000]);
        assert_eq!(got.frames, 298);
        assert_eq!(got.values.len(), reference.len());
        let worst = got
            .values
            .iter()
            .zip(&reference)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 2e-3, "largest difference {worst}");
    }
}
