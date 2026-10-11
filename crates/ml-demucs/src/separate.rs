//! The segment pipeline around the htdemucs network: cut a track into
//! overlapping segments, run the network on each, and blend the results
//! back into one track per stem.
//!
//! The network is fixed-size. It takes exactly [`SEGMENT_SAMPLES`] samples
//! (7.8 s at 44.1 kHz) of stereo audio and returns four stereo sources of
//! the same length, so a real track is processed in pieces. How the pieces
//! are cut, padded, weighted and added back is part of what the model was
//! evaluated with, and getting it slightly wrong gives audible seams or a
//! level jump at every segment boundary rather than an error. This module
//! is that part: a port of `apply_model(split=True)` and the normalisation
//! around it, behind the [`SegmentModel`] trait so it can be
//! tested without ONNX Runtime or a model file.
//!
//! Ported from <https://github.com/facebookresearch/demucs> (MIT) at
//! commit `e976d93ecc3865e5757426930257e200846a520a`:
//!
//! - `demucs/apply.py`: `apply_model` (the `split` branch) and
//!   `TensorChunk.padded`
//! - `demucs/utils.py`: `center_trim`
//! - `demucs/api.py`: `Separator.separate_tensor` (the normalisation)
//! - `demucs/pretrained.py`: `SOURCES`
//!
//! ## What it does
//!
//! 1. **Normalise** ([`separate_stereo`]): subtract the mean and divide by
//!    the standard deviation of the *mono* mix (`wav.mean(0)`; the
//!    standard deviation is torch's, with Bessel's correction, plus
//!    `1e-8`), the same two scalars for both channels. They are undone on
//!    the way out, on every stem.
//! 2. **Cut** ([`apply_model`]): segments of [`SegmentModel::segment_samples`]
//!    start every `int((1 - 0.25) * segment)` samples, so consecutive
//!    segments overlap by a quarter. Offsets are `range(0, length, stride)`,
//!    which includes a last, short one.
//! 3. **Pad.** The model always gets a full segment. A short chunk (the
//!    last one, or a whole track shorter than a segment) is *centred* in
//!    the window: it takes `(segment - chunk) / 2` samples of the audio
//!    before it as context when there is any, and zeros where there is no
//!    audio, and the output is trimmed back by the same amount on each side
//!    (`center_trim`; the odd sample goes on the right). It is not padded at
//!    the end only.
//! 4. **Weight and add.** Each segment's output is multiplied by a triangle
//!    (`1/h, 2/h, ... 1, ... 2/h, 1/h`, peak in the middle) and added into
//!    the output, the weights are added into a running total, and the
//!    output is divided by the total. Where only one segment reaches (the
//!    very start and end of the track) the weights cancel and the network's
//!    output passes through. A short last chunk uses the *first* `chunk`
//!    weights, the rising side, not the middle of the triangle.
//! 5. **Name the sources**: the model emits `drums, bass, other, vocals`
//!    ([`MODEL_SOURCES`]); [`stems_in_stem_order`] puts them into the
//!    crate's `vocals, drums, bass, other` ([`crate::STEM_NAMES`]) by name,
//!    from the order the model reports itself.
//!
//! ## Where this deliberately differs from upstream
//!
//! - **No shift augmentation.** Upstream's `apply_model` defaults to
//!   `shifts=1`: a random shift of up to half a second, applied and undone,
//!   averaged over `shifts` runs (its own docstring: "improves SDR by up to
//!   0.2 points"). Here the output is a pure function of the input, because
//!   the stem cache is keyed on the input and the model: two runs that differ
//!   would make the cache lie about what it holds. This is `shifts=0`.
//! - **Every stem gets the mean added back**, as upstream does
//!   (`out += ref.mean()` on all four sources). Stems of a signal with a DC
//!   offset therefore sum to the mix plus three times the offset. For audio
//!   that offset is negligible; it is what the reference does.
//!
//! ## What is not here
//!
//! The ONNX-backed [`SegmentModel`] (the spectrogram from the STFT port in
//! `spec.rs`, #385 step 2, and the waveform in, `xt_out + istft(x_out)` out), resampling to 44.1 kHz and
//! back, mono and surround handling, progress and cancellation, and the stem
//! cache. [`DemucsModel::separate`](crate::DemucsModel::separate) does not
//! call any of this and still returns [`DemucsError::NotImplemented`].

use crate::{DemucsError, Result, STEM_NAMES};

/// The sample rate the network was trained at, and the only one it takes.
pub const SAMPLE_RATE: u32 = 44_100;

/// Samples in one segment of the pinned model: 7.8 s at [`SAMPLE_RATE`]
/// (`int(7.8 * 44100)`), the `343980` in the ONNX export's `xt` input.
pub const SEGMENT_SAMPLES: usize = 343_980;

/// Overlap between consecutive segments, as a fraction of a segment
/// (`apply_model`'s default).
pub const OVERLAP: f64 = 0.25;

/// The sources htdemucs emits, in the order it emits them
/// (`demucs/pretrained.py`: `SOURCES`).
pub const MODEL_SOURCES: [&str; 4] = ["drums", "bass", "other", "vocals"];

/// Two channels of audio, left then right, one `Vec` each, all the same
/// length. Planar rather than interleaved: that is how the network and the
/// reference both see it.
pub type Stereo = [Vec<f32>; 2];

/// The network, seen from the pipeline: one fixed-size segment in, the
/// sources out.
///
/// `&mut self` because an ONNX Runtime session needs it to run.
pub trait SegmentModel {
    /// Samples in a segment. The pipeline only ever passes this many.
    /// [`SEGMENT_SAMPLES`] for htdemucs; tests use small values.
    fn segment_samples(&self) -> usize;

    /// The names of the sources, in the order [`Self::separate_segment`]
    /// returns them. [`MODEL_SOURCES`] for htdemucs.
    fn sources(&self) -> &[&'static str];

    /// Separate one segment of already-normalised audio.
    ///
    /// `segment` is two channels of exactly [`Self::segment_samples`]. The
    /// result is one [`Stereo`] per entry of [`Self::sources`], each also
    /// exactly that long.
    fn separate_segment(&mut self, segment: &Stereo) -> Result<Vec<Stereo>>;
}

/// Samples between the starts of consecutive segments:
/// `int((1 - overlap) * segment)`, and at least 1.
pub fn segment_stride(segment_samples: usize) -> usize {
    (((1.0 - OVERLAP) * segment_samples as f64) as usize).max(1)
}

/// How many times the network runs for `length` samples: one per offset in
/// `range(0, length, stride)`. A 5-minute track at 44.1 kHz is 52.
pub fn segment_count(length: usize, segment_samples: usize) -> usize {
    length.div_ceil(segment_stride(segment_samples))
}

/// The blend weights for a segment: a triangle rising `1/h .. 1` over the
/// first half and falling `1 .. 1/h` over the rest, where `h` is the larger
/// half. (`transition_power` is 1, which is upstream's default.)
///
/// ```text
/// weight = cat([arange(1, segment // 2 + 1), arange(segment - segment // 2, 0, -1)])
/// weight = weight / weight.max()
/// ```
fn triangle_weights(segment: usize) -> Vec<f32> {
    let half = segment / 2;
    let peak = (segment - half) as f32;
    (1..=half)
        .chain((1..=segment - half).rev())
        .map(|w| w as f32 / peak)
        .collect()
}

/// `TensorChunk(mix, offset, segment).padded(segment)`: the window of
/// `segment` samples in which the chunk starting at `offset` is centred,
/// with zeros where the window runs past either end of the audio.
fn padded_chunk(mix: &Stereo, offset: usize, segment: usize) -> Stereo {
    let total = mix[0].len();
    let chunk = segment.min(total - offset);
    let delta = segment - chunk;
    let start = offset as i64 - (delta / 2) as i64;
    let end = start + segment as i64;
    let from = start.max(0) as usize;
    let to = end.min(total as i64) as usize;
    let pad_left = (from as i64 - start) as usize;
    let pad_right = (end - to as i64) as usize;
    let channel = |samples: &[f32]| {
        let mut out = Vec::with_capacity(segment);
        out.resize(pad_left, 0.0);
        out.extend_from_slice(&samples[from..to]);
        out.resize(segment, 0.0);
        debug_assert_eq!(out.len(), pad_left + (to - from) + pad_right);
        out
    };
    [channel(mix[0].as_slice()), channel(mix[1].as_slice())]
}

/// `apply_model(model, mix, split=True, shifts=0, overlap=0.25)`: run
/// `model` over `mix` in overlapping segments and blend.
///
/// `mix` is used as given (normalise first, see [`separate_stereo`]). The
/// result has one [`Stereo`] per source in `model.sources()` order, each as
/// long as `mix`. An empty `mix` gives empty sources without running the
/// model.
///
/// The model is asked for a segment at a time and the first error stops
/// the run; nothing is returned for the segments before it.
pub fn apply_model<M: SegmentModel + ?Sized>(model: &mut M, mix: &Stereo) -> Result<Vec<Stereo>> {
    let length = mix[0].len();
    if mix[1].len() != length {
        return Err(DemucsError::InvalidAudio(format!(
            "channels differ in length: {} and {}",
            length,
            mix[1].len()
        )));
    }
    let segment = model.segment_samples();
    let source_count = model.sources().len();
    if segment == 0 || source_count == 0 {
        return Err(DemucsError::Pipeline(format!(
            "the model reports {segment} samples per segment and {source_count} sources"
        )));
    }

    let stride = segment_stride(segment);
    let weight = triangle_weights(segment);
    let mut out: Vec<Stereo> = (0..source_count)
        .map(|_| [vec![0.0f32; length], vec![0.0f32; length]])
        .collect();
    let mut sum_weight = vec![0.0f32; length];

    for offset in (0..length).step_by(stride) {
        let chunk = segment.min(length - offset);
        let produced = model.separate_segment(&padded_chunk(mix, offset, segment))?;
        if produced.len() != source_count
            || produced
                .iter()
                .any(|s| s[0].len() != segment || s[1].len() != segment)
        {
            return Err(DemucsError::Pipeline(format!(
                "the model returned {} sources of {:?} samples, expected {source_count} of {segment}",
                produced.len(),
                produced.first().map(|s| (s[0].len(), s[1].len())),
            )));
        }

        // `center_trim`: drop the same amount either side of the chunk that
        // `padded` added, the odd sample on the right.
        let trim = (segment - chunk) / 2;
        for (dst, src) in out.iter_mut().zip(&produced) {
            for (dst, src) in dst.iter_mut().zip(src) {
                for ((d, &s), &w) in dst[offset..offset + chunk]
                    .iter_mut()
                    .zip(&src[trim..trim + chunk])
                    .zip(&weight)
                {
                    *d += w * s;
                }
            }
        }
        for (t, &w) in sum_weight[offset..offset + chunk].iter_mut().zip(&weight) {
            *t += w;
        }
    }

    // Every sample is reached by at least one segment and the smallest
    // weight is 1/h, so this never divides by zero.
    for source in &mut out {
        for channel in source {
            for (v, &t) in channel.iter_mut().zip(&sum_weight) {
                *v /= t;
            }
        }
    }
    Ok(out)
}

/// The mean, and the standard deviation plus `1e-8`, of the mono mix
/// (`ref = wav.mean(0)`). The deviation is torch's: it divides by `n - 1`.
fn mono_mean_and_std(mix: &Stereo) -> (f32, f32) {
    let n = mix[0].len();
    if n == 0 {
        return (0.0, 1e-8);
    }
    let mono = |i: usize| (f64::from(mix[0][i]) + f64::from(mix[1][i])) / 2.0;
    let mean = (0..n).map(mono).sum::<f64>() / n as f64;
    let variance = (0..n).map(|i| (mono(i) - mean).powi(2)).sum::<f64>() / (n.max(2) - 1) as f64;
    (mean as f32, (variance.sqrt() + 1e-8) as f32)
}

/// Put the model's sources into [`STEM_NAMES`] order (`vocals, drums, bass,
/// other`), matching by name.
///
/// `sources` and `produced` are in the model's order, as
/// [`SegmentModel::sources`] and [`apply_model`] give them. It is an error
/// for them to be anything but the four stem names in some order.
pub fn stems_in_stem_order(sources: &[&str], mut produced: Vec<Stereo>) -> Result<[Stereo; 4]> {
    if sources.len() != STEM_NAMES.len() || produced.len() != STEM_NAMES.len() {
        return Err(DemucsError::Pipeline(format!(
            "expected the four sources {STEM_NAMES:?}, got {} names and {} stems",
            sources.len(),
            produced.len()
        )));
    }
    let mut out: [Stereo; 4] = Default::default();
    for (slot, name) in out.iter_mut().zip(STEM_NAMES) {
        let index = sources.iter().position(|s| *s == name).ok_or_else(|| {
            DemucsError::Pipeline(format!(
                "the model has no source named {name:?}: {sources:?}"
            ))
        })?;
        *slot = std::mem::take(&mut produced[index]);
    }
    Ok(out)
}

/// Separate a track: normalise, [`apply_model`], undo the normalisation,
/// and name the stems. The stems are in [`STEM_NAMES`] order (`vocals`,
/// `drums`, `bass`, `other`), each as long as `mix`.
///
/// `mix` is stereo at [`SAMPLE_RATE`]; getting it that way is the caller's
/// job. It must be finite, which this does not check.
pub fn separate_stereo<M: SegmentModel + ?Sized>(
    model: &mut M,
    mix: &Stereo,
) -> Result<[Stereo; 4]> {
    let (mean, std) = mono_mean_and_std(mix);
    let normalised: Stereo = [
        mix[0].iter().map(|v| (v - mean) / std).collect(),
        mix[1].iter().map(|v| (v - mean) / std).collect(),
    ];
    let mut sources = apply_model(model, &normalised)?;
    for source in &mut sources {
        for channel in source {
            for v in channel {
                *v = *v * std + mean;
            }
        }
    }
    stems_in_stem_order(model.sources(), sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Segment size for the tests: small enough to read, large enough to
    /// have a middle. Stride is 75.
    const SEG: usize = 100;
    const STRIDE: usize = 75;

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

    fn stereo_noise(len: usize, seed: u64) -> Stereo {
        [noise(len, seed), noise(len, seed + 100)]
    }

    /// A mix whose mono mean is exactly zero (every sample is followed by
    /// its negation), so that mean handling cannot blur a check on the rest.
    fn zero_mean_stereo(half: usize, seed: u64) -> Stereo {
        let mut mix = stereo_noise(half, seed);
        for ch in &mut mix {
            let negated: Vec<f32> = ch.iter().map(|v| -v).collect();
            ch.extend(negated);
        }
        mix
    }

    fn scaled(s: &Stereo, factor: f32) -> Stereo {
        [
            s[0].iter().map(|v| v * factor).collect(),
            s[1].iter().map(|v| v * factor).collect(),
        ]
    }

    fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max)
    }

    /// The plan's fake: a "model" that splits its input into four parts and
    /// returns them as the four sources. Each source is a fixed fraction of
    /// its input, and the fractions sum to one, so whatever the pipeline
    /// does right, the sources add up to what went in. The sources come out
    /// in the model's own order, which is not the stem order.
    ///
    /// Records every segment it is given.
    struct Quarters {
        segment: usize,
        sources: &'static [&'static str],
        fractions: [f32; 4],
        seen: Vec<Stereo>,
        /// If set, each call `k` (from 0) scales its output by `k + 1`, so
        /// that overlapping segments disagree and the blend weights matter.
        scale_by_call: bool,
    }

    impl Quarters {
        fn new(segment: usize) -> Self {
            Self {
                segment,
                sources: &MODEL_SOURCES,
                // drums, bass, other, vocals
                fractions: [0.1, 0.2, 0.3, 0.4],
                seen: Vec::new(),
                scale_by_call: false,
            }
        }
    }

    impl SegmentModel for Quarters {
        fn segment_samples(&self) -> usize {
            self.segment
        }
        fn sources(&self) -> &[&'static str] {
            self.sources
        }
        fn separate_segment(&mut self, segment: &Stereo) -> Result<Vec<Stereo>> {
            let call_scale = if self.scale_by_call {
                (self.seen.len() + 1) as f32
            } else {
                1.0
            };
            self.seen.push(segment.clone());
            Ok(self
                .fractions
                .iter()
                .map(|f| scaled(segment, f * call_scale))
                .collect())
        }
    }

    // -- the pieces ---------------------------------------------------------

    #[test]
    fn constants_are_the_pinned_models() {
        assert_eq!(SEGMENT_SAMPLES, (7.8 * f64::from(SAMPLE_RATE)) as usize);
        assert_eq!(segment_stride(SEGMENT_SAMPLES), 257_985);
        assert_eq!(MODEL_SOURCES, ["drums", "bass", "other", "vocals"]);
    }

    #[test]
    fn a_five_minute_track_is_52_segments() {
        // The plan estimated "about 46"; 300 s at 5.85 s a stride is 51.3.
        assert_eq!(segment_count(300 * 44_100, SEGMENT_SAMPLES), 52);
        assert_eq!(segment_count(0, SEGMENT_SAMPLES), 0);
        assert_eq!(segment_count(1, SEGMENT_SAMPLES), 1);
        assert_eq!(segment_count(257_985, SEGMENT_SAMPLES), 1);
        assert_eq!(segment_count(257_986, SEGMENT_SAMPLES), 2);
        assert_eq!(segment_count(175, SEG), 3);
    }

    #[test]
    fn the_blend_weights_are_a_triangle() {
        let w = triangle_weights(SEG);
        assert_eq!(w.len(), SEG);
        assert_eq!(w[0], 1.0 / 50.0);
        assert_eq!(w[48], 49.0 / 50.0);
        // The peak is two samples wide for an even segment.
        assert_eq!((w[49], w[50]), (1.0, 1.0));
        assert_eq!(w[51], 49.0 / 50.0);
        assert_eq!(w[99], 1.0 / 50.0);

        // An odd segment peaks on its middle sample.
        let w = triangle_weights(99);
        assert_eq!(w.len(), 99);
        assert_eq!(w[48], 49.0 / 50.0);
        assert_eq!(w[49], 1.0);
        assert_eq!(w[50], 49.0 / 50.0);

        let w = triangle_weights(SEGMENT_SAMPLES);
        assert_eq!(w.len(), SEGMENT_SAMPLES);
        assert_eq!(w.iter().cloned().fold(0.0, f32::max), 1.0);
    }

    #[test]
    fn a_chunk_is_centred_in_its_window() {
        let mix = stereo_noise(175, 1);
        let ch0 = &mix[0];

        // A whole segment is taken as it is.
        assert_eq!(padded_chunk(&mix, 0, SEG)[0], ch0[0..100]);
        assert_eq!(padded_chunk(&mix, 75, SEG)[0], ch0[75..175]);

        // The last chunk (offset 150, 25 samples) takes 37 samples of
        // context before it and is padded with 38 zeros after.
        let last = padded_chunk(&mix, 150, SEG);
        assert_eq!(last[0].len(), SEG);
        assert_eq!(last[0][..62], ch0[113..175]);
        assert!(last[0][62..].iter().all(|&v| v == 0.0));
        assert_eq!(last[1][..62], mix[1][113..175]);
    }

    #[test]
    fn a_track_shorter_than_a_segment_is_centred_between_zeros() {
        let mix = stereo_noise(30, 2);
        let w = padded_chunk(&mix, 0, SEG);
        assert!(w[0][..35].iter().all(|&v| v == 0.0));
        assert_eq!(w[0][35..65], mix[0][..]);
        assert!(w[0][65..].iter().all(|&v| v == 0.0));

        // An odd amount of padding puts the extra zero on the right.
        let mix = stereo_noise(25, 3);
        let w = padded_chunk(&mix, 0, SEG);
        assert!(w[0][..37].iter().all(|&v| v == 0.0));
        assert_eq!(w[0][37..62], mix[0][..]);
        assert!(w[0][62..].iter().all(|&v| v == 0.0));
    }

    // -- the pipeline, with the plan's fake ---------------------------------

    #[test]
    fn the_model_is_called_once_per_offset_on_exact_windows() {
        let mix = stereo_noise(175, 4);
        let mut model = Quarters::new(SEG);
        apply_model(&mut model, &mix).unwrap();

        // Offsets 0, 75 and 150.
        assert_eq!(model.seen.len(), 3);
        assert!(model
            .seen
            .iter()
            .all(|s| s[0].len() == SEG && s[1].len() == SEG));
        assert_eq!(model.seen[0][0], mix[0][0..100]);
        assert_eq!(model.seen[1][0], mix[0][75..175]);
        assert_eq!(model.seen[2][0][..62], mix[0][113..175]);
        assert!(model.seen[2][0][62..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn the_number_of_calls_follows_the_stride_for_every_length() {
        for length in [
            1, 2, 25, 74, 75, 76, 99, 100, 101, 149, 150, 151, 175, 1000, 1003,
        ] {
            let mix = stereo_noise(length, 5);
            let mut model = Quarters::new(SEG);
            apply_model(&mut model, &mix).unwrap();
            assert_eq!(model.seen.len(), length.div_ceil(STRIDE), "length {length}");
            assert_eq!(model.seen.len(), segment_count(length, SEG));
        }
    }

    /// The headline property, at every boundary case: when each segment's
    /// four sources add up to its input, the four stems add up to the mix.
    /// That holds only if every chunk lands at the right offset, the padding
    /// and the trim undo each other (including with an odd amount), and the
    /// blend divides by exactly what it added.
    #[test]
    fn the_stems_add_back_up_to_the_mix_at_every_length() {
        for length in [
            1, 2, 3, 25, 26, 50, 74, 75, 76, 99, 100, 101, 149, 150, 151, 174, 175, 176, 199, 200,
            225, 250, 251, 1000, 1001, 1003,
        ] {
            let mix = stereo_noise(length, 6);
            let mut model = Quarters::new(SEG);
            let sources = apply_model(&mut model, &mix).unwrap();

            assert_eq!(sources.len(), 4);
            for c in 0..2 {
                let sum: Vec<f32> = (0..length)
                    .map(|i| sources.iter().map(|s| s[c][i]).sum())
                    .collect();
                let diff = max_abs_diff(&sum, &mix[c]);
                assert!(diff < 1e-6, "length {length} channel {c}: max error {diff}");
            }
            // And each source is its own fraction of the mix, not a blend of
            // the others.
            for (s, f) in sources.iter().zip(model.fractions) {
                let want: Vec<f32> = mix[0].iter().map(|v| v * f).collect();
                assert!(max_abs_diff(&s[0], &want) < 1e-6, "length {length}");
            }
        }
    }

    /// The overlap weighting, against numbers worked out by hand rather than
    /// by the code under test. With segments scaled by 1, 2 and 3 where they
    /// overlap, the result at each sample is the weighted average of the
    /// scales.
    ///
    /// Length 175 means segments at 0 and 75 (both 100 long) and 150 (25
    /// long). Weights are `(j + 1) / 50` up the rising half and
    /// `(100 - j) / 50` down the falling one, `j` being the position within
    /// the segment.
    #[test]
    fn overlapping_segments_are_blended_by_the_triangle() {
        let mix: Stereo = [vec![1.0; 175], vec![1.0; 175]];
        let mut model = Quarters::new(SEG);
        model.scale_by_call = true;
        let sources = apply_model(&mut model, &mix).unwrap();
        let total: Vec<f32> = (0..175)
            .map(|i| sources.iter().map(|s| s[0][i]).sum())
            .collect();

        // Where only one segment reaches, its scale passes through.
        for (i, t) in total.iter().enumerate().take(75) {
            assert!((t - 1.0).abs() < 1e-6, "sample {i}: {t}");
        }
        // 75: segment 0 at j=75 (weight 25/50), segment 1 at j=0 (1/50).
        let at = |i: usize, num: f32, den: f32| {
            assert!(
                (total[i] - num / den).abs() < 1e-5,
                "sample {i}: {} != {}",
                total[i],
                num / den
            );
        };
        at(75, 0.5 * 1.0 + 0.02 * 2.0, 0.52);
        // 80: segment 0 at j=80 (20/50), segment 1 at j=5 (6/50).
        at(80, 0.4 * 1.0 + 0.12 * 2.0, 0.52);
        // The weights of two overlapping triangles add to a constant where
        // both are on a slope: 0.52 here.
        // 99: segment 0 at j=99 (1/50), segment 1 at j=24 (25/50).
        at(99, 0.02 * 1.0 + 0.5 * 2.0, 0.52);
        // Past segment 0, segment 1 alone, until segment 2 starts at 150.
        for (i, t) in total.iter().enumerate().take(150).skip(100) {
            assert!((t - 2.0).abs() < 1e-6, "sample {i}: {t}");
        }
        // 160: segment 1 at j=85 (15/50), segment 2 at j=10 (11/50).
        at(160, 0.3 * 2.0 + 0.22 * 3.0, 0.52);
        // 174, the last sample: segment 1 at j=99 (1/50) and the short last
        // segment at j=24. It uses the first 25 weights of the triangle, so
        // its weight is 25/50, not the 1/50 the middle of the triangle would
        // end on.
        at(174, 0.02 * 2.0 + 0.5 * 3.0, 0.52);
    }

    /// The same, for every sample of several lengths, against a direct
    /// computation of the blend from the definition.
    #[test]
    fn the_blend_matches_its_definition_at_every_sample() {
        let tri = |j: usize| {
            if j < 50 {
                (j + 1) as f32 / 50.0
            } else {
                (100 - j) as f32 / 50.0
            }
        };
        for length in [60, 100, 130, 175, 250, 333] {
            let mix: Stereo = [vec![1.0; length], vec![1.0; length]];
            let mut model = Quarters::new(SEG);
            model.scale_by_call = true;
            let sources = apply_model(&mut model, &mix).unwrap();
            for i in 0..length {
                let (mut num, mut den) = (0.0f32, 0.0f32);
                for (k, offset) in (0..length).step_by(STRIDE).enumerate() {
                    let chunk = SEG.min(length - offset);
                    if (offset..offset + chunk).contains(&i) {
                        let w = tri(i - offset);
                        num += w * (k + 1) as f32;
                        den += w;
                    }
                }
                let got: f32 = sources.iter().map(|s| s[0][i]).sum();
                assert!(
                    (got - num / den).abs() < 1e-5,
                    "length {length}, sample {i}: {got} != {}",
                    num / den
                );
            }
        }
    }

    #[test]
    fn an_empty_mix_runs_nothing() {
        let mut model = Quarters::new(SEG);
        let sources = apply_model(&mut model, &[vec![], vec![]]).unwrap();
        assert_eq!(sources.len(), 4);
        assert!(sources.iter().all(|s| s[0].is_empty() && s[1].is_empty()));
        assert!(model.seen.is_empty());
    }

    #[test]
    fn at_the_real_segment_size_the_stems_add_up_and_the_last_segment_is_short() {
        // 600 000 samples: segments at 0, 257 985 and 515 970; the last has
        // 84 030 of its 343 980 samples.
        let length = 600_000;
        let mix = stereo_noise(length, 7);
        let mut model = Quarters::new(SEGMENT_SAMPLES);
        let sources = apply_model(&mut model, &mix).unwrap();
        assert_eq!(model.seen.len(), 3);
        let sum: Vec<f32> = (0..length)
            .map(|i| sources.iter().map(|s| s[1][i]).sum())
            .collect();
        assert!(max_abs_diff(&sum, &mix[1]) < 1e-6);

        // The last window is the 84 030 samples centred: 129 975 of context
        // before them, and the rest zeros.
        let last = &model.seen[2][0];
        assert_eq!(last.len(), SEGMENT_SAMPLES);
        let context = (SEGMENT_SAMPLES - 84_030) / 2;
        assert_eq!(context, 129_975);
        assert_eq!(last[..context + 84_030], mix[0][515_970 - context..]);
        assert!(last[context + 84_030..].iter().all(|&v| v == 0.0));
    }

    // -- what goes wrong ----------------------------------------------------

    #[test]
    fn a_model_error_stops_the_run() {
        struct Fails(usize);
        impl SegmentModel for Fails {
            fn segment_samples(&self) -> usize {
                SEG
            }
            fn sources(&self) -> &[&'static str] {
                &MODEL_SOURCES
            }
            fn separate_segment(&mut self, _: &Stereo) -> Result<Vec<Stereo>> {
                self.0 += 1;
                Err(DemucsError::Ort("out of memory".into()))
            }
        }
        let mut model = Fails(0);
        let err = apply_model(&mut model, &stereo_noise(1000, 8)).unwrap_err();
        assert!(matches!(err, DemucsError::Ort(_)), "{err:?}");
        assert!(
            crate::is_oom_error(&err),
            "the model's error is passed on as it was"
        );
        assert_eq!(model.0, 1, "no further segments after a failure");
    }

    #[test]
    fn a_model_that_returns_the_wrong_shape_is_an_error_not_a_panic() {
        struct Wrong {
            sources: usize,
            samples: usize,
        }
        impl SegmentModel for Wrong {
            fn segment_samples(&self) -> usize {
                SEG
            }
            fn sources(&self) -> &[&'static str] {
                &MODEL_SOURCES
            }
            fn separate_segment(&mut self, _: &Stereo) -> Result<Vec<Stereo>> {
                Ok((0..self.sources)
                    .map(|_| [vec![0.0; self.samples], vec![0.0; self.samples]])
                    .collect())
            }
        }
        for (sources, samples) in [(3, SEG), (5, SEG), (4, SEG - 1), (4, SEG + 1), (0, SEG)] {
            let err =
                apply_model(&mut Wrong { sources, samples }, &stereo_noise(150, 9)).unwrap_err();
            assert!(
                matches!(err, DemucsError::Pipeline(_)),
                "{sources} x {samples}: {err:?}"
            );
        }
    }

    #[test]
    fn unusable_input_is_refused() {
        let err = apply_model(&mut Quarters::new(SEG), &[vec![0.0; 10], vec![0.0; 9]]).unwrap_err();
        assert!(matches!(err, DemucsError::InvalidAudio(_)), "{err:?}");
        let err = apply_model(&mut Quarters::new(0), &stereo_noise(10, 1)).unwrap_err();
        assert!(matches!(err, DemucsError::Pipeline(_)), "{err:?}");
    }

    // -- normalisation and source order -------------------------------------

    #[test]
    fn the_model_sees_audio_with_zero_mean_and_unit_deviation() {
        // One full segment, so nothing is padding: what the model gets is
        // the whole normalised track.
        let mut mix = stereo_noise(SEG, 10);
        for ch in &mut mix {
            for v in ch.iter_mut() {
                *v = *v * 0.2 + 0.3;
            }
        }
        let mut model = Quarters::new(SEG);
        separate_stereo(&mut model, &mix).unwrap();
        assert_eq!(model.seen.len(), 2); // offsets 0 and 75
        let seen = &model.seen[0];

        let mono: Vec<f64> = (0..SEG)
            .map(|i| (f64::from(seen[0][i]) + f64::from(seen[1][i])) / 2.0)
            .collect();
        let mean = mono.iter().sum::<f64>() / SEG as f64;
        let std = (mono.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (SEG - 1) as f64).sqrt();
        assert!(mean.abs() < 1e-6, "mean {mean}");
        assert!((std - 1.0).abs() < 1e-5, "std {std}");
    }

    /// The stems come back denormalised, each with the mean added, as
    /// upstream does: stem = fraction * (mix - mean) + mean.
    #[test]
    fn the_stems_are_denormalised_and_named() {
        let mut mix = stereo_noise(400, 11);
        for ch in &mut mix {
            for v in ch.iter_mut() {
                *v = *v * 0.2 + 0.05;
            }
        }
        let mean = mix[0]
            .iter()
            .zip(&mix[1])
            .map(|(a, b)| (f64::from(*a) + f64::from(*b)) / 2.0)
            .sum::<f64>() as f32
            / 400.0;
        let mut model = Quarters::new(SEG);
        let stems = separate_stereo(&mut model, &mix).unwrap();

        // The model emits drums, bass, other, vocals with fractions
        // 0.1, 0.2, 0.3, 0.4; the stems are vocals, drums, bass, other.
        let want_fraction = [0.4, 0.1, 0.2, 0.3];
        for (i, f) in want_fraction.iter().enumerate() {
            for (c, (got, input)) in stems[i].iter().zip(&mix).enumerate() {
                assert_eq!(got.len(), 400);
                let want: Vec<f32> = input.iter().map(|v| f * (v - mean) + mean).collect();
                let diff = max_abs_diff(got, &want);
                assert!(diff < 1e-5, "stem {} channel {c}: {diff}", STEM_NAMES[i]);
            }
        }
    }

    #[test]
    fn stems_that_add_up_to_a_mix_with_no_offset_do_so_after_separation() {
        let mix = zero_mean_stereo(500, 12);
        let stems = separate_stereo(&mut Quarters::new(SEG), &mix).unwrap();
        for c in 0..2 {
            let sum: Vec<f32> = (0..1000)
                .map(|i| stems.iter().map(|s| s[c][i]).sum())
                .collect();
            assert!(max_abs_diff(&sum, &mix[c]) < 1e-5);
        }
    }

    #[test]
    fn silence_and_a_constant_do_not_produce_nan() {
        for level in [0.0f32, 0.5] {
            let mix: Stereo = [vec![level; 300], vec![level; 300]];
            let stems = separate_stereo(&mut Quarters::new(SEG), &mix).unwrap();
            for stem in &stems {
                for ch in stem {
                    // Zero deviation: the model is given zeros, and every
                    // stem is the mean.
                    assert!(
                        ch.iter()
                            .all(|v| v.is_finite() && (*v - level).abs() < 1e-6),
                        "level {level}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_single_sample_is_handled() {
        let mix: Stereo = [vec![0.25], vec![-0.25]];
        let stems = separate_stereo(&mut Quarters::new(SEG), &mix).unwrap();
        assert!(stems.iter().all(|s| s[0].len() == 1 && s[0][0].is_finite()));
    }

    #[test]
    fn source_order_is_taken_from_the_model_not_assumed() {
        // A model that lists its sources in the stem order already.
        let mut model = Quarters::new(SEG);
        model.sources = &["vocals", "drums", "bass", "other"];
        let mix = zero_mean_stereo(150, 13);
        let stems = separate_stereo(&mut model, &mix).unwrap();
        // Fractions 0.1, 0.2, 0.3, 0.4 now belong to vocals, drums, bass, other.
        for (i, f) in [0.1f32, 0.2, 0.3, 0.4].iter().enumerate() {
            let want: Vec<f32> = mix[0].iter().map(|v| v * f).collect();
            assert!(
                max_abs_diff(&stems[i][0], &want) < 1e-5,
                "stem {}",
                STEM_NAMES[i]
            );
        }
    }

    #[test]
    fn stems_in_stem_order_maps_by_name() {
        let produced: Vec<Stereo> = (0..4)
            .map(|i| [vec![i as f32], vec![-(i as f32)]])
            .collect();
        let stems = stems_in_stem_order(&MODEL_SOURCES, produced).unwrap();
        // drums = 0, bass = 1, other = 2, vocals = 3
        assert_eq!(stems[0][0], [3.0]); // vocals
        assert_eq!(stems[1][0], [0.0]); // drums
        assert_eq!(stems[2][0], [1.0]); // bass
        assert_eq!(stems[3][0], [2.0]); // other
        assert_eq!(stems[0][1], [-3.0]);
    }

    #[test]
    fn stems_in_stem_order_refuses_a_source_set_that_is_not_the_four() {
        let four = || -> Vec<Stereo> { (0..4).map(|_| [vec![0.0], vec![0.0]]).collect() };
        // Wrong names, repeated names, and a count that disagrees.
        for sources in [
            vec!["drums", "bass", "other", "guitar"],
            vec!["drums", "drums", "other", "vocals"],
            vec!["drums", "bass", "other"],
        ] {
            let err = stems_in_stem_order(&sources, four()).unwrap_err();
            assert!(
                matches!(err, DemucsError::Pipeline(_)),
                "{sources:?}: {err:?}"
            );
        }
        let err = stems_in_stem_order(&MODEL_SOURCES, four()[..3].to_vec()).unwrap_err();
        assert!(matches!(err, DemucsError::Pipeline(_)), "{err:?}");
    }
}
