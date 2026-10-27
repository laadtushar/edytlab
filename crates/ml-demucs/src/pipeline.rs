//! A decoded track in, four stem files out: everything around the network
//! that is about the *file* rather than the model.
//!
//! [`separate_to_dir`] takes audio as the decoder gives it (any sample rate,
//! any number of channels, interleaved), and writes `vocals.wav`, `drums.wav`,
//! `bass.wav` and `other.wav` into a directory that appears only when all four
//! are whole ([`crate::cache::Pending`]). The network is a
//! [`SegmentModel`], so everything here runs in CI against a fake.
//!
//! ## What happens to the audio
//!
//! 1. **To what the network takes.** Stereo at 44.1 kHz
//!    ([`crate::separate::SAMPLE_RATE`]). A mono file is the same signal on
//!    both channels; a file of more than two channels is its first two
//!    (`demucs/audio.py`'s `convert_audio` does `wav[:channels]`). The rate is
//!    converted with [`RateConverter`], which keeps the time and the length.
//! 2. **Separated in segments** by [`separate_streaming`], which hands each
//!    finished stretch of the four stems over as it completes.
//! 3. **Back to the file's own shape**: converted to the file's rate, mixed to
//!    mono if the file was mono (the stereo stem's two channels averaged),
//!    cut or padded with at most a few frames of silence to exactly the
//!    file's length, and written as 16-bit PCM, the format of everything else
//!    the project writes. A value past full scale is clipped, and counted
//!    ([`Separated::clipped`]).
//!
//! ## Memory
//!
//! Nothing here is the size of the track more than twice, and the stems are
//! not held at all: a 5-minute stereo 44.1 kHz file is 106 MB of decoded
//! samples, and 106 MB again as the network's planar input (the interleaved
//! buffer is dropped before the first segment), plus one segment of the four
//! sources (11 MB), the network's own working memory, and a few buffers of
//! one stretch each. The stems go to disk as they are made, through converters
//! that hold less than a block. The network's input is made whole, and not
//! windowed straight from the decoded buffer, because the track's mean and
//! deviation (over all of it) are needed before the first segment.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use audio_decoder::DecodedAudio;
use hound::{SampleFormat, WavSpec, WavWriter};

use crate::cache::{StemPaths, STEM_FILENAMES};
use crate::resample::{output_frames, RateConverter};
use crate::separate::{separate_streaming, Control, SegmentModel, Stereo, SAMPLE_RATE};
use crate::{DemucsError, Result};

/// Frames converted at a time on the way in.
const READ_BLOCK_FRAMES: usize = 1 << 16;

/// What a finished separation reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Separated {
    /// The four files, in a directory that is now an entry.
    pub paths: StemPaths,
    /// Frames in each stem: the file's own length.
    pub frames: u64,
    /// The stems' sample rate: the file's.
    pub sample_rate: u32,
    /// The stems' channel count: 1 for a mono file, else 2.
    pub channels: u16,
    /// Runs of the network.
    pub segments: usize,
    /// Samples that went past full scale and were clipped, per stem, in
    /// [`crate::STEM_NAMES`] order.
    pub clipped: [u64; 4],
}

/// Refuse audio that cannot be separated, with a reason that names it.
fn check(audio: &DecodedAudio) -> Result<()> {
    if audio.sample_rate == 0 || audio.channels == 0 {
        return Err(DemucsError::InvalidAudio(format!(
            "invalid header: sample_rate={} channels={}",
            audio.sample_rate, audio.channels
        )));
    }
    if !audio
        .samples
        .len()
        .is_multiple_of(usize::from(audio.channels))
    {
        return Err(DemucsError::InvalidAudio(format!(
            "{} samples are not a whole number of {}-channel frames",
            audio.samples.len(),
            audio.channels
        )));
    }
    if audio.samples.is_empty() {
        return Err(DemucsError::InvalidAudio("there is no audio".into()));
    }
    if !audio.samples.iter().all(|s| s.is_finite()) {
        return Err(DemucsError::InvalidAudio(
            "buffer contains non-finite samples".into(),
        ));
    }
    Ok(())
}

/// The network's input: `audio` as stereo at [`SAMPLE_RATE`]. Consumes the
/// decoded samples, so they are freed before the network allocates.
fn to_network_input(audio: DecodedAudio) -> Result<Stereo> {
    let channels = usize::from(audio.channels);
    let frames = audio.samples.len() / channels;
    let wanted = output_frames(frames as u64, audio.sample_rate, SAMPLE_RATE) as usize;
    let mut converter = RateConverter::new(audio.sample_rate, SAMPLE_RATE, 2)?;
    let mut out: Stereo = [
        Vec::with_capacity(wanted + READ_BLOCK_FRAMES),
        Vec::with_capacity(wanted + READ_BLOCK_FRAMES),
    ];
    let mut block: Stereo = [Vec::new(), Vec::new()];
    for frames in audio.samples.chunks(READ_BLOCK_FRAMES * channels) {
        for (c, channel) in block.iter_mut().enumerate() {
            channel.clear();
            // A mono file is its one channel twice.
            channel.extend(
                frames
                    .chunks_exact(channels)
                    .map(|f| f[c.min(channels - 1)]),
            );
        }
        converter.push(&[&block[0], &block[1]], &mut out)?;
    }
    drop(audio);
    converter.finish(&mut out)?;
    Ok(out)
}

/// One stem's file, and the way from the network's audio to what it holds.
struct StemFile {
    writer: WavWriter<BufWriter<File>>,
    /// 44.1 kHz stereo to the file's rate; `None` if they are the same.
    converter: RateConverter,
    mono: bool,
    /// Converted frames not yet written.
    converted: Vec<Vec<f32>>,
    written: u64,
    target: u64,
    clipped: u64,
}

impl StemFile {
    fn create(path: &Path, rate: u32, mono: bool, target: u64) -> Result<Self> {
        let spec = WavSpec {
            channels: if mono { 1 } else { 2 },
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let writer = WavWriter::create(path, spec).map_err(|e| DemucsError::Wav(e.to_string()))?;
        Ok(Self {
            writer,
            converter: RateConverter::new(SAMPLE_RATE, rate, 2)?,
            mono,
            converted: vec![Vec::new(), Vec::new()],
            written: 0,
            target,
            clipped: 0,
        })
    }

    fn quantise(&mut self, v: f32) -> i16 {
        let scaled = (v * 32_768.0).round();
        if !(-32_768.0..=32_767.0).contains(&scaled) {
            self.clipped += 1;
        }
        scaled.clamp(-32_768.0, 32_767.0) as i16
    }

    fn write_converted(&mut self) -> Result<()> {
        let frames = self.converted[0].len();
        let room = (self.target - self.written) as usize;
        for i in 0..frames.min(room) {
            let (l, r) = (self.converted[0][i], self.converted[1][i]);
            let samples: [i16; 2] = if self.mono {
                [self.quantise((l + r) / 2.0), 0]
            } else {
                [self.quantise(l), self.quantise(r)]
            };
            for &s in &samples[..if self.mono { 1 } else { 2 }] {
                self.writer
                    .write_sample(s)
                    .map_err(|e| DemucsError::Wav(e.to_string()))?;
            }
            self.written += 1;
        }
        self.converted.iter_mut().for_each(Vec::clear);
        Ok(())
    }

    fn push(&mut self, stem: &Stereo) -> Result<()> {
        self.converter
            .push(&[&stem[0], &stem[1]], &mut self.converted)?;
        self.write_converted()
    }

    /// Flush the converter, pad to the file's length, and close the file.
    /// Returns the samples clipped.
    fn finish(mut self) -> Result<u64> {
        self.converter.finish(&mut self.converted)?;
        self.write_converted()?;
        // Two rounded conversions can differ from the file's own length by a
        // frame; the stem is exactly as long as the file.
        while self.written < self.target {
            for _ in 0..if self.mono { 1 } else { 2 } {
                self.writer
                    .write_sample(0i16)
                    .map_err(|e| DemucsError::Wav(e.to_string()))?;
            }
            self.written += 1;
        }
        self.writer
            .finalize()
            .map_err(|e| DemucsError::Wav(e.to_string()))?;
        Ok(self.clipped)
    }
}

/// Separate `audio` with `model` and write the four stems as the entry
/// `dest` (a directory that does not exist yet; its parent is created).
///
/// Takes the audio by value so that the decoded samples are freed as soon as
/// the network's input is made (see the module docs). Reports through
/// `control.progress` and reads `control.cancel` before every segment. On
/// cancel or any error, nothing is left at `dest` or beside it. If `dest`
/// became a whole entry meanwhile (another run finished it first), that entry
/// stays and its paths are returned.
pub fn separate_to_dir<M: SegmentModel + ?Sized>(
    model: &mut M,
    audio: DecodedAudio,
    dest: &Path,
    control: &Control<'_>,
) -> Result<Separated> {
    check(&audio)?;
    let (rate, mono) = (audio.sample_rate, audio.channels == 1);
    let frames = (audio.samples.len() / usize::from(audio.channels)) as u64;
    let mix = to_network_input(audio)?;

    let pending = crate::cache::Pending::create(dest)?;
    let paths = StemPaths::for_dir(pending.dir());
    let mut files = Vec::with_capacity(4);
    for name in STEM_FILENAMES {
        files.push(StemFile::create(
            &pending.dir().join(name),
            rate,
            mono,
            frames,
        )?);
    }

    let mut segments = 0usize;
    let counted = |done: usize, total: usize| {
        (control.progress)(done, total);
    };
    let counting = Control {
        cancel: control.cancel,
        progress: &counted,
    };
    separate_streaming(model, &mix, &counting, &mut |_, stems| {
        segments += 1;
        for (file, stem) in files.iter_mut().zip(stems) {
            file.push(stem)?;
        }
        Ok(())
    })?;
    drop(mix);
    drop(paths);

    let mut clipped = [0u64; 4];
    for (slot, file) in clipped.iter_mut().zip(files) {
        *slot = file.finish()?;
    }
    let published = pending.publish()?;
    Ok(Separated {
        paths: published,
        frames,
        sample_rate: rate,
        channels: if mono { 1 } else { 2 },
        segments,
        clipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::separate::MODEL_SOURCES;
    use std::cell::{Cell, RefCell};

    /// Segment size of the fakes: a tenth of a second, so a short test file
    /// is several segments.
    const SEG: usize = 4410;

    /// Splits its input into four fractions, in the model's source order
    /// (drums, bass, other, vocals): vocals 0.4, drums 0.1, bass 0.2, other
    /// 0.3. Counts its runs.
    struct Quarters {
        runs: Cell<usize>,
        gain: f32,
    }

    impl Quarters {
        fn new() -> Self {
            Self {
                runs: Cell::new(0),
                gain: 1.0,
            }
        }
    }

    impl SegmentModel for Quarters {
        fn segment_samples(&self) -> usize {
            SEG
        }
        fn sources(&self) -> &[&'static str] {
            &MODEL_SOURCES
        }
        fn separate_segment(&mut self, segment: &Stereo) -> Result<Vec<Stereo>> {
            self.runs.set(self.runs.get() + 1);
            Ok([0.1f32, 0.2, 0.3, 0.4]
                .iter()
                .map(|f| {
                    [
                        segment[0].iter().map(|v| v * f * self.gain).collect(),
                        segment[1].iter().map(|v| v * f * self.gain).collect(),
                    ]
                })
                .collect())
        }
    }

    /// Fractions by stem file: vocals, drums, bass, other.
    const FRACTIONS: [f32; 4] = [0.4, 0.1, 0.2, 0.3];

    fn tones(frames: usize, rate: u32, freqs: &[f32], amp: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                freqs
                    .iter()
                    .map(|f| (2.0 * std::f32::consts::PI * f * i as f32 / rate as f32).sin())
                    .sum::<f32>()
                    * amp
            })
            .collect()
    }

    /// Interleaved audio: channel `c` of a file with `channels` channels.
    fn audio(frames: usize, rate: u32, channels: u16) -> DecodedAudio {
        let per_channel: Vec<Vec<f32>> = (0..channels)
            .map(|c| tones(frames, rate, &[220.0 + 110.0 * f32::from(c), 1234.5], 0.2))
            .collect();
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for i in 0..frames {
            for ch in &per_channel {
                samples.push(ch[i]);
            }
        }
        DecodedAudio {
            samples,
            sample_rate: rate,
            channels,
        }
    }

    /// Read a stem back as `f32`, with its spec.
    fn read(path: &Path) -> (WavSpec, Vec<f32>) {
        let mut reader = hound::WavReader::open(path).unwrap();
        let spec = reader.spec();
        let samples = reader
            .samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32_768.0)
            .collect();
        (spec, samples)
    }

    fn run(audio: DecodedAudio, model: &mut Quarters) -> (tempfile::TempDir, Result<Separated>) {
        let root = tempfile::tempdir().unwrap();
        let result = separate_to_dir(model, audio, &root.path().join("abc123"), &Control::none());
        (root, result)
    }

    #[test]
    fn a_stereo_44100_file_gives_four_wavs_of_its_own_shape() {
        let input = audio(22_050, 44_100, 2);
        let reference = input.samples.clone();
        let (_root, result) = run(input, &mut Quarters::new());
        let done = result.unwrap();
        assert!(done.paths.all_exist());
        assert_eq!(
            (done.frames, done.sample_rate, done.channels),
            (22_050, 44_100, 2)
        );
        assert_eq!(done.segments, crate::separate::segment_count(22_050, SEG));
        assert_eq!(done.clipped, [0; 4]);
        for (path, fraction) in done.paths.iter().zip(FRACTIONS) {
            let (spec, samples) = read(path);
            assert_eq!(
                (spec.channels, spec.sample_rate, spec.bits_per_sample),
                (2, 44_100, 16)
            );
            assert_eq!(samples.len(), 22_050 * 2, "{}", path.display());
            // The mean the fake leaves in is the file's own, which is ~0.
            let worst = samples
                .iter()
                .zip(&reference)
                .map(|(s, r)| (s - r * fraction).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 2.0 / 32_768.0 + 1e-4, "{}: {worst}", path.display());
        }
    }

    /// The file's rate and length are kept whatever they are, and the stems
    /// still follow the audio (through two conversions).
    #[test]
    fn other_rates_come_back_at_their_own_rate_and_length() {
        for (rate, frames) in [
            (48_000u32, 48_001usize),
            (22_050, 11_111),
            (96_000, 30_000),
            (8_000, 4_001),
        ] {
            let input = audio(frames, rate, 2);
            let reference = input.samples.clone();
            let (_root, result) = run(input, &mut Quarters::new());
            let done = result.unwrap_or_else(|e| panic!("{rate} Hz: {e}"));
            assert_eq!((done.frames, done.sample_rate), (frames as u64, rate));
            let (spec, samples) = read(&done.paths.vocals);
            assert_eq!(spec.sample_rate, rate);
            assert_eq!(
                samples.len(),
                frames * 2,
                "{rate} Hz: exactly the file's length"
            );
            // Interior only: the resamplers' edges and the half-level ends.
            let margin = (rate as usize / 20).max(200) * 2;
            let worst = (margin..samples.len() - margin)
                .map(|i| (samples[i] - reference[i] * 0.4).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 0.01, "{rate} Hz: {worst}");
        }
    }

    #[test]
    fn a_mono_file_gives_mono_stems() {
        let input = audio(15_000, 32_000, 1);
        let reference = input.samples.clone();
        let (_root, result) = run(input, &mut Quarters::new());
        let done = result.unwrap();
        assert_eq!(done.channels, 1);
        for path in done.paths.iter() {
            let (spec, samples) = read(path);
            assert_eq!((spec.channels, spec.sample_rate), (1, 32_000));
            assert_eq!(samples.len(), 15_000);
        }
        let (_, bass) = read(&done.paths.bass);
        let worst = (1_000..14_000)
            .map(|i| (bass[i] - reference[i] * 0.2).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.01, "{worst}");
    }

    /// A 5.1 file is separated as its first two channels, and the stems are
    /// stereo (`convert_audio`'s `wav[:channels]`).
    #[test]
    fn more_than_two_channels_are_the_first_two() {
        let input = audio(8_820, 44_100, 6);
        let reference: Vec<f32> = input.samples.chunks(6).flat_map(|f| [f[0], f[1]]).collect();
        let (_root, result) = run(input, &mut Quarters::new());
        let done = result.unwrap();
        assert_eq!(done.channels, 2);
        let (spec, samples) = read(&done.paths.other);
        assert_eq!(spec.channels, 2);
        let worst = samples
            .iter()
            .zip(&reference)
            .map(|(s, r)| (s - r * 0.3).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 2.0 / 32_768.0 + 1e-4, "{worst}");
    }

    /// The stems are named for what the model's sources are called, whatever
    /// order it emits them in: vocals is the model's fourth.
    #[test]
    fn the_files_are_named_by_the_models_source_names() {
        let input = audio(8_820, 44_100, 2);
        let reference = input.samples.clone();
        let (_root, result) = run(input, &mut Quarters::new());
        let done = result.unwrap();
        for (path, name, fraction) in [
            (&done.paths.vocals, "vocals", 0.4),
            (&done.paths.drums, "drums", 0.1),
            (&done.paths.bass, "bass", 0.2),
            (&done.paths.other, "other", 0.3),
        ] {
            assert!(path.ends_with(format!("{name}.wav")));
            let (_, samples) = read(path);
            let err = (samples[3000] - reference[3000] * fraction).abs();
            assert!(err < 1e-3, "{name}: {err}");
        }
    }

    /// A value past full scale is clipped, not wrapped, and counted.
    #[test]
    fn values_past_full_scale_are_clipped_and_counted() {
        let mut input = audio(8_820, 44_100, 2);
        for v in input.samples.iter_mut() {
            *v = v.signum() * 0.9;
        }
        let mut model = Quarters::new();
        model.gain = 3.0; // vocals: 0.4 * 3 * 0.9 = 1.08
        let (_root, result) = run(input, &mut model);
        let done = result.unwrap();
        assert!(
            done.clipped[0] > 0,
            "vocals went past full scale: {:?}",
            done.clipped
        );
        assert_eq!(done.clipped[1], 0, "drums did not: {:?}", done.clipped);
        let (_, vocals) = read(&done.paths.vocals);
        assert!(vocals.iter().all(|v| v.abs() <= 1.0));
        assert!(vocals.iter().any(|v| *v > 0.999), "saturated, not wrapped");
        assert!(vocals.iter().filter(|v| **v < -0.999).count() > 0);
    }

    // -- an entry exists whole or not at all --------------------------------

    #[test]
    fn a_cancel_leaves_nothing_at_the_destination_or_beside_it() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        let model_runs = Cell::new(0usize);
        struct Counting<'a>(Quarters, &'a Cell<usize>);
        impl SegmentModel for Counting<'_> {
            fn segment_samples(&self) -> usize {
                self.0.segment_samples()
            }
            fn sources(&self) -> &[&'static str] {
                self.0.sources()
            }
            fn separate_segment(&mut self, s: &Stereo) -> Result<Vec<Stereo>> {
                self.1.set(self.1.get() + 1);
                self.0.separate_segment(s)
            }
        }
        let mut model = Counting(Quarters::new(), &model_runs);
        let cancel = || model_runs.get() >= 2;
        let control = Control {
            cancel: &cancel,
            progress: &|_, _| {},
        };
        let err =
            separate_to_dir(&mut model, audio(22_050, 44_100, 2), &dest, &control).unwrap_err();
        assert!(matches!(err, DemucsError::Cancelled), "{err:?}");
        assert_eq!(model_runs.get(), 2);
        assert!(!dest.exists());
        assert!(
            crate::read_cached_stems(&dest).is_none(),
            "no partial cache hit"
        );
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            0,
            "no temporary directory either"
        );
    }

    #[test]
    fn a_model_that_fails_leaves_nothing() {
        struct Broken;
        impl SegmentModel for Broken {
            fn segment_samples(&self) -> usize {
                SEG
            }
            fn sources(&self) -> &[&'static str] {
                &MODEL_SOURCES
            }
            fn separate_segment(&mut self, _: &Stereo) -> Result<Vec<Stereo>> {
                Err(DemucsError::Ort("out of memory".into()))
            }
        }
        let root = tempfile::tempdir().unwrap();
        let err = separate_to_dir(
            &mut Broken,
            audio(8_820, 44_100, 2),
            &root.path().join("abc123"),
            &Control::none(),
        )
        .unwrap_err();
        assert!(crate::is_oom_error(&err));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn progress_is_reported_through_to_the_caller() {
        let root = tempfile::tempdir().unwrap();
        let seen = RefCell::new(Vec::new());
        let progress = |d: usize, t: usize| seen.borrow_mut().push((d, t));
        let control = Control {
            cancel: &|| false,
            progress: &progress,
        };
        let done = separate_to_dir(
            &mut Quarters::new(),
            audio(22_050, 44_100, 2),
            &root.path().join("abc123"),
            &control,
        )
        .unwrap();
        let total = done.segments;
        assert_eq!(
            *seen.borrow(),
            (0..=total).map(|d| (d, total)).collect::<Vec<_>>()
        );
    }

    // -- audio that cannot be separated -------------------------------------

    #[test]
    fn unusable_audio_is_refused_before_anything_is_made() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("abc123");
        let mut bad_header = audio(100, 44_100, 2);
        bad_header.sample_rate = 0;
        let mut nan = audio(100, 44_100, 2);
        nan.samples[5] = f32::NAN;
        let mut ragged = audio(100, 44_100, 2);
        ragged.samples.pop();
        let empty = DecodedAudio {
            samples: vec![],
            sample_rate: 44_100,
            channels: 2,
        };
        for (what, input) in [
            ("header", bad_header),
            ("nan", nan),
            ("ragged", ragged),
            ("empty", empty),
        ] {
            let model = &mut Quarters::new();
            let err = separate_to_dir(model, input, &dest, &Control::none()).unwrap_err();
            assert!(
                matches!(err, DemucsError::InvalidAudio(_)),
                "{what}: {err:?}"
            );
            assert_eq!(model.runs.get(), 0, "{what}");
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    /// One sample of audio is separated, and is one frame of each stem.
    #[test]
    fn a_single_frame_is_separated() {
        let input = DecodedAudio {
            samples: vec![0.25, -0.25],
            sample_rate: 44_100,
            channels: 2,
        };
        let (_root, result) = run(input, &mut Quarters::new());
        let done = result.unwrap();
        assert_eq!(done.frames, 1);
        assert_eq!(read(&done.paths.vocals).1.len(), 2);
    }
}
