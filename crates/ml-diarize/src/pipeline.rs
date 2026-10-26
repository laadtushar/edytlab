//! The whole pipeline, from audio to speaker turns, behind two small traits
//! so that everything but the two neural networks runs in CI.
//!
//! 1. **Windows.** The audio is cut into 10 s windows a second apart. The
//!    last is padded with silence to the full length, and so is a recording
//!    shorter than one window.
//! 2. **Segmentation.** The network says, for each of about 589 frames of a
//!    window, which of up to three local speakers are speaking, as one of
//!    seven classes (nobody, each alone, each pair): the *powerset* output,
//!    taken as the class with the highest score and turned back into a 0/1
//!    per speaker ([`powerset_to_multilabel`]). Local speakers are only
//!    labels inside a window; speaker 1 of one window is not speaker 1 of the
//!    next.
//! 3. **Counting.** How many speak in each frame, over all the windows that
//!    see it ([`crate::frames::Segmentation::speaker_count`]). If nobody ever
//!    does, the answer is no turns.
//! 4. **Embeddings.** For each local speaker of each window, the audio of the
//!    frames where only that speaker speaks (or all the frames the speaker
//!    speaks in, if there are too few of those) is turned into a filterbank
//!    ([`crate::fbank`]) and then into a 256-number voiceprint by the second
//!    network. Skipped altogether when at most one speaker is wanted.
//! 5. **Clustering** ([`crate::cluster`]): voiceprints that are close are the
//!    same person, which gives every local speaker of every window a global
//!    one.
//! 6. **Turns** ([`crate::frames`]): each global speaker's frames, cut into
//!    turns, then clipped to the audio and numbered by who speaks first.
//!
//! This is pyannote.audio 3.1's `SpeakerDiarization.apply`, and the tests
//! check it against that on a real recording.

use std::fmt;

use crate::cluster::{self, SpeakerBounds};
use crate::fbank::{self, Fbank};
use crate::frames::{self, Assignment, Segmentation, CHUNK_SAMPLES, FRAMES_PER_WINDOW};

/// The sample rate everything here is defined for.
pub const SAMPLE_RATE: u32 = 16_000;
/// Seconds between the starts of two windows: 10 % of a window, which is
/// pyannote's `segmentation_step`.
pub const WINDOW_STEP_S: f64 = 1.0;
/// Local speakers in a window.
pub const LOCAL_SPEAKERS: usize = 3;
/// Classes the segmentation network scores: nobody, 3 alone, 3 pairs.
pub const POWERSET_CLASSES: usize = 7;
/// Numbers in one voiceprint.
pub const EMBEDDING_DIM: usize = 256;
/// The fewest samples the embedding network gives a number for, found by
/// bisecting for the shortest input whose output is not `NaN` on the pinned
/// file (what pyannote's `min_num_samples` does), and re-checked by
/// `tests/real_model.rs`.
pub const MIN_EMBEDDING_SAMPLES: usize = 1_680;
/// The filterbank frames of [`MIN_EMBEDDING_SAMPLES`]: with fewer, the network
/// answers `NaN` (the pinned file's average pooling has nothing to average).
pub const MIN_EMBEDDING_FRAMES: usize = 9;

/// Why a diarization could not be made.
#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    /// The caller's `cancel` answered `true`.
    #[error("diarization cancelled")]
    Cancelled,
    /// Nothing to diarize.
    #[error("invalid audio: {0}")]
    InvalidAudio(String),
    /// A network failed, or answered with a shape this code does not read.
    #[error("inference failed: {0}")]
    Inference(String),
    /// The segmentation network answered a window with a number of frames the
    /// frame grid is not built for (every index in [`crate::frames`] assumes
    /// [`FRAMES_PER_WINDOW`]), so a developer's export that differs is refused
    /// rather than indexed out of bounds.
    #[error(
        "the segmentation network returned {got} frames for a window, not the {expected} this pipeline is built for"
    )]
    FrameCount { got: usize, expected: usize },
    #[error(transparent)]
    Bounds(#[from] cluster::BoundsError),
}

/// The segmentation network.
pub trait Segmenter {
    /// Scores for one window of [`CHUNK_SAMPLES`] samples at 16 kHz:
    /// `frames * `[`POWERSET_CLASSES`] numbers, frame first.
    fn segment(&self, window: &[f32]) -> Result<Vec<f32>, DiarizeError>;
}

/// The speaker embedding network.
///
/// `Sync`, because voiceprints are made on several threads at once when
/// [`Embedder::parallelism`] allows it.
pub trait Embedder: Sync {
    /// How many voiceprints can be made at the same time. 1 unless the
    /// implementation holds that many networks.
    fn parallelism(&self) -> usize {
        1
    }

    /// The voiceprint of `frames` filterbank frames (`frames * `[`fbank::BANDS`]
    /// numbers, row-major), [`EMBEDDING_DIM`] numbers. Called with at least
    /// [`MIN_EMBEDDING_FRAMES`] frames.
    fn embed(&self, features: &[f32], frames: usize) -> Result<Vec<f32>, DiarizeError>;
}

/// What the pipeline is doing, for a progress strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Segmentation,
    Embedding,
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Stage::Segmentation => "segmentation",
            Stage::Embedding => "embedding",
        })
    }
}

/// What a caller can ask for and watch.
pub struct Options<'a> {
    pub bounds: SpeakerBounds,
    /// Called as work is done: the stage, how many units are done, how many
    /// there are. Segmentation counts windows, embedding counts voiceprints.
    pub on_progress: &'a dyn Fn(Stage, usize, usize),
    /// Answers `true` once the user has asked to stop; read before every
    /// window and every voiceprint.
    pub cancel: &'a (dyn Fn() -> bool + Sync),
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            bounds: SpeakerBounds::default(),
            on_progress: &|_, _, _| {},
            cancel: &|| false,
        }
    }
}

/// One stretch of one speaker talking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerTurn {
    pub start_s: f64,
    pub end_s: f64,
    /// From 0, in the order the speakers first speak.
    pub speaker: usize,
}

/// Who spoke when. Turns of different speakers can overlap (people talk over
/// each other), and a speaker's own turns never do.
#[derive(Debug, Clone, PartialEq)]
pub struct Diarization {
    /// By start time, then speaker.
    pub turns: Vec<SpeakerTurn>,
    /// How many different speakers there are.
    pub speakers: usize,
    /// How long the audio is.
    pub duration_s: f64,
}

/// The classes of the segmentation network as speaker sets: nobody, speaker 0,
/// 1, 2, then the pairs (0,1), (0,2), (1,2): `Powerset(3, 2).mapping`.
const POWERSET: [[u8; LOCAL_SPEAKERS]; POWERSET_CLASSES] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 1, 0],
    [1, 0, 1],
    [0, 1, 1],
];

/// `Powerset.to_multilabel(soft=False)`: the class with the highest score,
/// as who is speaking. The first of equal scores wins, as `argmax` does.
pub fn powerset_to_multilabel(scores: &[f32]) -> [u8; LOCAL_SPEAKERS] {
    let mut best = 0;
    for (i, s) in scores.iter().enumerate() {
        if *s > scores[best] {
            best = i;
        }
    }
    POWERSET[best]
}

/// The windows of `audio`: starts in samples, each padded to a full window.
fn window_starts(samples: usize) -> Vec<usize> {
    let step = (WINDOW_STEP_S * f64::from(SAMPLE_RATE)).round() as usize;
    let complete = if samples >= CHUNK_SAMPLES {
        (samples - CHUNK_SAMPLES) / step + 1
    } else {
        0
    };
    let mut starts: Vec<usize> = (0..complete).map(|c| c * step).collect();
    if samples < CHUNK_SAMPLES || (samples - CHUNK_SAMPLES) % step > 0 {
        starts.push(complete * step);
    }
    starts
}

fn window(audio: &[f32], start: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; CHUNK_SAMPLES];
    let end = (start + CHUNK_SAMPLES).min(audio.len());
    out[..end - start].copy_from_slice(&audio[start..end]);
    out
}

/// The filterbank frames that belong to one local speaker.
///
/// `features` is the whole window's filterbank (its mean already taken out),
/// `mask` the speaker's 0/1 per segmentation frame, `clean` the same with the
/// frames where somebody else also speaks left out. The segmentation grid is
/// laid over the filterbank by nearest frame (`F.interpolate(mode="nearest")`),
/// and the speaker's own frames are used unless fewer than
/// [`min_clean_frames`] are left once the shared ones are out.
pub fn pick_frames(features: &fbank::Frames, mask: &[u8], clean: &[u8]) -> Vec<f32> {
    let used =
        if clean.iter().map(|v| usize::from(*v)).sum::<usize>() > min_clean_frames(mask.len()) {
            clean
        } else {
            mask
        };
    let mut out = Vec::new();
    for j in 0..features.frames {
        if used[j * used.len() / features.frames] != 0 {
            out.extend_from_slice(features.row(j));
        }
    }
    out
}

/// Frames of the segmentation grid a speaker must keep once shared speech is
/// out to be embedded on those alone: `ceil(frames * min_samples / window)`.
pub fn min_clean_frames(segmentation_frames: usize) -> usize {
    (segmentation_frames * MIN_EMBEDDING_SAMPLES).div_ceil(CHUNK_SAMPLES)
}

/// Who spoke when in `audio` (16 kHz mono, -1 to 1).
pub fn diarize(
    segmenter: &dyn Segmenter,
    embedder: &dyn Embedder,
    audio: &[f32],
    options: &Options<'_>,
) -> Result<Diarization, DiarizeError> {
    if audio.is_empty() {
        return Err(DiarizeError::InvalidAudio("no audio".to_string()));
    }
    if audio.iter().any(|s| !s.is_finite()) {
        return Err(DiarizeError::InvalidAudio(
            "the audio has samples that are not numbers".to_string(),
        ));
    }
    let duration_s = audio.len() as f64 / f64::from(SAMPLE_RATE);
    let starts = window_starts(audio.len());

    // 2. Segmentation
    let mut data = Vec::new();
    for (c, &start) in starts.iter().enumerate() {
        if (options.cancel)() {
            return Err(DiarizeError::Cancelled);
        }
        (options.on_progress)(Stage::Segmentation, c, starts.len());
        let scores = segmenter.segment(&window(audio, start))?;
        if scores.is_empty() || scores.len() % POWERSET_CLASSES != 0 {
            return Err(DiarizeError::Inference(format!(
                "the segmentation network returned {} numbers, not a whole number of frames of {POWERSET_CLASSES}",
                scores.len()
            )));
        }
        let frames = scores.len() / POWERSET_CLASSES;
        if frames != FRAMES_PER_WINDOW {
            return Err(DiarizeError::FrameCount {
                got: frames,
                expected: FRAMES_PER_WINDOW,
            });
        }
        for row in scores.chunks_exact(POWERSET_CLASSES) {
            data.extend_from_slice(&powerset_to_multilabel(row));
        }
    }
    (options.on_progress)(Stage::Segmentation, starts.len(), starts.len());
    let seg = Segmentation::new(
        starts.len(),
        FRAMES_PER_WINDOW,
        LOCAL_SPEAKERS,
        WINDOW_STEP_S,
        data,
    );

    // 3. Counting
    let mut count = seg.speaker_count();
    if count.iter().all(|&n| n == 0) {
        return Ok(Diarization {
            turns: Vec::new(),
            speakers: 0,
            duration_s,
        });
    }

    // 4. and 5. Voiceprints and clustering
    let bounds = options.bounds;
    let rows = seg.chunks * seg.speakers;
    let active: Vec<bool> = (0..rows)
        .map(|r| seg.active(r / seg.speakers, r % seg.speakers))
        .collect();
    let clusters = if bounds.max.is_some_and(|m| m < 2) {
        // One speaker at most: nothing to tell apart.
        vec![0; rows]
    } else {
        let (embeddings, usable) = voiceprints(embedder, audio, &starts, &seg, &active, options)?;
        cluster::cluster_embeddings(&embeddings, EMBEDDING_DIM, &usable, bounds).clusters
    };
    let assignment = Assignment {
        speakers: seg.speakers,
        clusters: (0..rows)
            .map(|r| active[r].then_some(clusters[r]))
            .collect(),
    };

    // 6. Turns
    if let Some(cap) = bounds.cap() {
        let cap = u8::try_from(cap).unwrap_or(u8::MAX);
        for n in &mut count {
            *n = (*n).min(cap);
        }
    }
    let activity = frames::reconstruct(&seg, &assignment, &count);
    Ok(finish(frames::turns(&activity), duration_s))
}

/// Voiceprints made per batch. A batch is read, run on all the threads there
/// are and finished before the next is read, which bounds the memory held to
/// a batch's filterbanks whatever the length of the recording.
const BATCH: usize = 32;

/// One voiceprint to make: whose, and the filterbank frames to make it from.
struct Job {
    row: usize,
    features: Vec<f32>,
    frames: usize,
}

/// Embeddings of every (window, local speaker), and which of them exist.
fn voiceprints(
    embedder: &dyn Embedder,
    audio: &[f32],
    starts: &[usize],
    seg: &Segmentation,
    active: &[bool],
    options: &Options<'_>,
) -> Result<(Vec<f32>, Vec<bool>), DiarizeError> {
    let rows = active.len();
    let mut embeddings = vec![f32::NAN; rows * EMBEDDING_DIM];
    let mut usable = vec![false; rows];
    let total = active.iter().filter(|a| **a).count();
    let bank = Fbank::new();
    let workers = embedder.parallelism().max(1);

    let mut batch: Vec<Job> = Vec::with_capacity(BATCH);
    let mut counted = 0; // active speakers walked, including those too short to embed
    let mut windows = (0..starts.len()).peekable();
    while windows.peek().is_some() || !batch.is_empty() {
        // Fill a batch from the next windows.
        while batch.len() < BATCH {
            let Some(c) = windows.next() else { break };
            if !(0..seg.speakers).any(|s| active[c * seg.speakers + s]) {
                continue;
            }
            let mut features = bank.compute(&window(audio, starts[c]));
            fbank::subtract_mean(&mut features);
            // Frames where more than one local speaker speaks.
            let shared: Vec<bool> = (0..seg.frames)
                .map(|f| seg.row(c, f).iter().map(|v| usize::from(*v)).sum::<usize>() >= 2)
                .collect();
            for s in 0..seg.speakers {
                let row = c * seg.speakers + s;
                if !active[row] {
                    continue;
                }
                counted += 1;
                let mask: Vec<u8> = (0..seg.frames).map(|f| seg.at(c, f, s)).collect();
                let clean: Vec<u8> = (0..seg.frames)
                    .map(|f| if shared[f] { 0 } else { mask[f] })
                    .collect();
                let picked = pick_frames(&features, &mask, &clean);
                let frames = picked.len() / fbank::BANDS;
                if frames >= MIN_EMBEDDING_FRAMES {
                    batch.push(Job {
                        row,
                        features: picked,
                        frames,
                    });
                }
            }
        }
        if (options.cancel)() {
            return Err(DiarizeError::Cancelled);
        }
        for (row, v) in run_batch(embedder, &batch, workers, options.cancel)? {
            embeddings[row * EMBEDDING_DIM..(row + 1) * EMBEDDING_DIM].copy_from_slice(&v);
            usable[row] = true;
        }
        batch.clear();
        (options.on_progress)(Stage::Embedding, counted, total);
    }
    (options.on_progress)(Stage::Embedding, total, total);
    Ok((embeddings, usable))
}

/// What one voiceprint job came to: its row and voiceprint, nothing when the
/// network's answer was not a number, or the error.
type Outcome = Result<Option<(usize, Vec<f32>)>, DiarizeError>;

/// Make the voiceprints of `jobs`, on up to `workers` threads, in the order of
/// the jobs. A voiceprint that is not a number is left out.
fn run_batch(
    embedder: &dyn Embedder,
    jobs: &[Job],
    workers: usize,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<(usize, Vec<f32>)>, DiarizeError> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let one = |job: &Job| -> Outcome {
        let v = embedder.embed(&job.features, job.frames)?;
        if v.len() != EMBEDDING_DIM {
            return Err(DiarizeError::Inference(format!(
                "the embedding network returned {} numbers, not {EMBEDDING_DIM}",
                v.len()
            )));
        }
        Ok((!v.iter().any(|x| x.is_nan())).then_some((job.row, v)))
    };
    let mut results: Vec<Option<Outcome>> = (0..jobs.len()).map(|_| None).collect();
    if workers <= 1 || jobs.len() <= 1 {
        for (i, job) in jobs.iter().enumerate() {
            if cancel() {
                return Err(DiarizeError::Cancelled);
            }
            results[i] = Some(one(job));
        }
    } else {
        let slots: Vec<std::sync::Mutex<&mut Option<_>>> =
            results.iter_mut().map(std::sync::Mutex::new).collect();
        std::thread::scope(|scope| {
            for _ in 0..workers.min(jobs.len()) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= jobs.len() || cancel() {
                        break;
                    }
                    let r = one(&jobs[i]);
                    let failed = r.is_err();
                    **slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                    if failed {
                        next.store(jobs.len(), Ordering::Relaxed);
                    }
                });
            }
        });
    }
    let mut out = Vec::with_capacity(jobs.len());
    for r in results {
        match r {
            Some(Ok(Some(v))) => out.push(v),
            Some(Ok(None)) => {}
            Some(Err(e)) => return Err(e),
            // Not run: a cancel stopped the workers first.
            None => return Err(DiarizeError::Cancelled),
        }
    }
    Ok(out)
}

/// Clip to the audio, number the speakers by who speaks first.
fn finish(turns: Vec<frames::Turn>, duration_s: f64) -> Diarization {
    let mut numbering: Vec<usize> = Vec::new();
    let mut out = Vec::with_capacity(turns.len());
    for t in turns {
        // The last window is padded with silence, which can be called speech.
        let end_s = t.end_s.min(duration_s);
        if t.start_s >= end_s {
            continue;
        }
        let speaker = match numbering.iter().position(|c| *c == t.cluster) {
            Some(i) => i,
            None => {
                numbering.push(t.cluster);
                numbering.len() - 1
            }
        };
        out.push(SpeakerTurn {
            start_s: t.start_s,
            end_s,
            speaker,
        });
    }
    Diarization {
        turns: out,
        speakers: numbering.len(),
        duration_s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::tests::{case, fixture_segmentation};
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const EXCERPT: &[u8] = include_bytes!("../tests/fixtures/ami_es2004a_640_680s.wav");

    fn excerpt() -> Vec<f32> {
        hound::WavReader::new(std::io::Cursor::new(EXCERPT))
            .unwrap()
            .samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32768.0)
            .collect()
    }

    /// Answers each window with the fixture's segmentation, as scores whose
    /// highest is the class the reference chose.
    struct ReplaySegmenter {
        next: Cell<usize>,
    }

    impl Segmenter for ReplaySegmenter {
        fn segment(&self, _window: &[f32]) -> Result<Vec<f32>, DiarizeError> {
            let seg = fixture_segmentation();
            let c = self.next.get();
            self.next.set(c + 1);
            let mut out = Vec::new();
            for f in 0..seg.frames {
                let row = seg.row(c, f);
                let class = POWERSET.iter().position(|p| p == row).unwrap();
                out.extend((0..POWERSET_CLASSES).map(|k| if k == class { 5.0 } else { -5.0 }));
            }
            Ok(out)
        }
    }

    /// Answers each call with the next real voiceprint the reference made.
    struct ReplayEmbedder {
        next: AtomicUsize,
        frames: Mutex<Vec<usize>>,
    }

    impl ReplayEmbedder {
        fn new() -> Self {
            Self {
                next: AtomicUsize::new(0),
                frames: Mutex::new(Vec::new()),
            }
        }
    }

    impl Embedder for ReplayEmbedder {
        fn embed(&self, _features: &[f32], frames: usize) -> Result<Vec<f32>, DiarizeError> {
            self.frames.lock().unwrap().push(frames);
            let all: Vec<f32> = include_bytes!("../tests/fixtures/embeddings.f32")
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let rows: Vec<&[f32]> = all
                .chunks_exact(EMBEDDING_DIM)
                .filter(|r| !r[0].is_nan())
                .collect();
            let i = self.next.fetch_add(1, Ordering::SeqCst);
            Ok(rows[i].to_vec())
        }
    }

    fn run(bounds: SpeakerBounds) -> Diarization {
        diarize(
            &ReplaySegmenter { next: Cell::new(0) },
            &ReplayEmbedder::new(),
            &excerpt(),
            &Options {
                bounds,
                ..Options::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn the_powerset_is_the_networks_seven_classes() {
        assert_eq!(
            powerset_to_multilabel(&[9., 0., 0., 0., 0., 0., 0.]),
            [0, 0, 0]
        );
        assert_eq!(
            powerset_to_multilabel(&[0., 0., 9., 0., 0., 0., 0.]),
            [0, 1, 0]
        );
        assert_eq!(
            powerset_to_multilabel(&[0., 0., 0., 0., 0., 9., 0.]),
            [1, 0, 1]
        );
        assert_eq!(
            powerset_to_multilabel(&[0., 0., 0., 0., 0., 0., 9.]),
            [0, 1, 1]
        );
        assert_eq!(
            powerset_to_multilabel(&[1., 1., 1., 1., 1., 1., 1.]),
            [0, 0, 0],
            "the first of equals"
        );
    }

    #[test]
    fn windows_are_a_second_apart_and_the_last_is_padded() {
        let rate = SAMPLE_RATE as usize;
        assert_eq!(
            window_starts(rate),
            vec![0],
            "a short recording is one padded window"
        );
        assert_eq!(window_starts(10 * rate), vec![0], "exactly one window");
        assert_eq!(
            window_starts(10 * rate + 1),
            vec![0, rate],
            "a sample over needs another"
        );
        assert_eq!(window_starts(12 * rate), vec![0, rate, 2 * rate]);
        assert_eq!(
            window_starts(40 * rate).len(),
            31,
            "the excerpt has 31 windows"
        );
        let w = window(&[1.0; 20_000], 16_000);
        assert_eq!(w.len(), CHUNK_SAMPLES);
        assert_eq!((w[3_999], w[4_000]), (1.0, 0.0));
    }

    #[test]
    fn a_speaker_alone_keeps_their_frames_and_shared_speech_is_left_out() {
        let features = fbank::Frames {
            values: (0..20 * fbank::BANDS).map(|v| v as f32).collect(),
            frames: 20,
        };
        // 10 segmentation frames over 20 filterbank frames: each covers two.
        let mask = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
        let mut clean = mask;
        clean[0] = 0;
        // 9 clean frames is more than the 1 the 10-frame grid asks for.
        assert_eq!(min_clean_frames(10), 1);
        let picked = pick_frames(&features, &mask, &clean);
        assert_eq!(
            picked.len() / fbank::BANDS,
            18,
            "the first two frames are shared speech"
        );
        assert_eq!(picked[0], (2 * fbank::BANDS) as f32);
        // With only the shared frame left to the speaker, all of theirs are used.
        let mut lonely = [0; 10];
        lonely[4] = 1;
        let mut all = [0; 10];
        all[4] = 1;
        all[5] = 1;
        let picked = pick_frames(&features, &all, &lonely);
        assert_eq!(
            picked.len() / fbank::BANDS,
            4,
            "one clean frame is not more than one"
        );
    }

    #[test]
    fn minimum_frames_are_those_of_pyannotes_bisection() {
        assert_eq!(min_clean_frames(589), 7);
        assert_eq!(
            fbank::frame_count(MIN_EMBEDDING_SAMPLES),
            MIN_EMBEDDING_FRAMES
        );
        assert_eq!(
            fbank::frame_count(MIN_EMBEDDING_SAMPLES - 1),
            MIN_EMBEDDING_FRAMES - 1
        );
    }

    /// Turns by speaker, as sets of (start, end), to compare two numberings.
    fn partition(turns: impl Iterator<Item = (f64, f64, usize)>) -> Vec<Vec<(i64, i64)>> {
        let mut groups: std::collections::BTreeMap<usize, Vec<(i64, i64)>> = Default::default();
        for (s, e, k) in turns {
            groups
                .entry(k)
                .or_default()
                .push(((s * 1e6).round() as i64, (e * 1e6).round() as i64));
        }
        let mut out: Vec<Vec<(i64, i64)>> = groups.into_values().collect();
        out.sort();
        out
    }

    #[test]
    fn every_setting_gives_pyannotes_turns_with_the_speakers_numbered_by_who_speaks_first() {
        for name in ["free", "num2", "num3", "num1", "min3", "max2"] {
            let c = case(name);
            let get = |k: &str| c["args"][k].as_u64().map(|v| v as usize);
            let bounds = SpeakerBounds::new(
                get("num_speakers"),
                get("min_speakers"),
                get("max_speakers"),
            )
            .unwrap();
            let got = run(bounds);
            let duration = 40.0;
            let want: Vec<(f64, f64, usize)> = c["turns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    (
                        t[0].as_f64().unwrap(),
                        t[1].as_f64().unwrap().min(duration),
                        t[2].as_u64().unwrap() as usize,
                    )
                })
                .filter(|(s, e, _)| s < e)
                .collect();
            assert_eq!(
                partition(got.turns.iter().map(|t| (t.start_s, t.end_s, t.speaker))),
                partition(want.into_iter()),
                "{name}"
            );
            assert_eq!(
                got.speakers as u64,
                c["clusters"].as_u64().unwrap(),
                "{name}"
            );
            // Numbered by first appearance.
            let mut seen = 0;
            for t in &got.turns {
                assert!(
                    t.speaker <= seen,
                    "{name}: speaker {} before {seen}",
                    t.speaker
                );
                seen = seen.max(t.speaker + 1);
            }
            // A speaker's own turns never overlap.
            for k in 0..got.speakers {
                let mut mine: Vec<&SpeakerTurn> =
                    got.turns.iter().filter(|t| t.speaker == k).collect();
                mine.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
                assert!(
                    mine.windows(2).all(|w| w[0].end_s <= w[1].start_s),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn one_speaker_wanted_embeds_nothing() {
        let embedder = ReplayEmbedder::new();
        let d = diarize(
            &ReplaySegmenter { next: Cell::new(0) },
            &embedder,
            &excerpt(),
            &Options {
                bounds: SpeakerBounds::new(Some(1), None, None).unwrap(),
                ..Options::default()
            },
        )
        .unwrap();
        assert!(
            embedder.frames.lock().unwrap().is_empty(),
            "no voiceprint is needed to say everyone is one person"
        );
        assert_eq!(d.speakers, 1);
        assert!(!d.turns.is_empty());
    }

    #[test]
    fn turns_stop_at_the_end_of_the_audio() {
        let d = run(SpeakerBounds::default());
        assert!(d
            .turns
            .iter()
            .all(|t| t.end_s <= d.duration_s + 1e-9 && t.start_s < t.end_s));
        assert_eq!(d.duration_s, 40.0);
    }

    #[test]
    fn progress_is_reported_in_order_and_cancel_stops_the_run() {
        let events = RefCell::new(Vec::new());
        let progress = |stage, done, total| events.borrow_mut().push((stage, done, total));
        diarize(
            &ReplaySegmenter { next: Cell::new(0) },
            &ReplayEmbedder::new(),
            &excerpt(),
            &Options {
                on_progress: &progress,
                ..Options::default()
            },
        )
        .unwrap();
        let events = events.into_inner();
        assert_eq!(events.first(), Some(&(Stage::Segmentation, 0, 31)));
        assert!(events.contains(&(Stage::Segmentation, 31, 31)));
        let last = events.last().unwrap();
        assert_eq!(
            (last.0, last.1),
            (Stage::Embedding, last.2),
            "ends with the embeddings done"
        );
        let first_embedding = events
            .iter()
            .position(|e| e.0 == Stage::Embedding)
            .expect("the excerpt has speech, so voiceprints are made");
        let last_segmentation = events
            .iter()
            .rposition(|e| e.0 == Stage::Segmentation)
            .expect("every window is reported");
        assert!(
            first_embedding > last_segmentation,
            "embedding progress comes after the last segmentation event"
        );

        // Cancel after five windows.
        let asked = AtomicUsize::new(0);
        let cancel = || asked.fetch_add(1, Ordering::SeqCst) >= 5;
        let seg = ReplaySegmenter { next: Cell::new(0) };
        let result = diarize(
            &seg,
            &ReplayEmbedder::new(),
            &excerpt(),
            &Options {
                cancel: &cancel,
                ..Options::default()
            },
        );
        assert!(matches!(result, Err(DiarizeError::Cancelled)));
        assert_eq!(seg.next.get(), 5, "no window was run after the cancel");
    }

    /// A voiceprint that is a function of the audio it is made from, so that
    /// it does not matter which thread, or in which order, it is asked for.
    struct FeatureEmbedder {
        workers: usize,
        threads: Mutex<std::collections::HashSet<std::thread::ThreadId>>,
    }

    impl Embedder for FeatureEmbedder {
        fn parallelism(&self) -> usize {
            self.workers
        }

        fn embed(&self, features: &[f32], frames: usize) -> Result<Vec<f32>, DiarizeError> {
            self.threads
                .lock()
                .unwrap()
                .insert(std::thread::current().id());
            std::thread::sleep(std::time::Duration::from_millis(2));
            let mean = |band: usize| {
                (0..frames)
                    .map(|f| features[f * fbank::BANDS + band])
                    .sum::<f32>()
                    / frames as f32
            };
            Ok((0..EMBEDDING_DIM)
                .map(|i| mean(i % fbank::BANDS) + (i / fbank::BANDS) as f32)
                .collect())
        }
    }

    #[test]
    fn voiceprints_made_on_several_threads_are_the_ones_made_on_one() {
        let run = |workers| {
            let embedder = FeatureEmbedder {
                workers,
                threads: Mutex::new(Default::default()),
            };
            let d = diarize(
                &ReplaySegmenter { next: Cell::new(0) },
                &embedder,
                &excerpt(),
                &Options::default(),
            )
            .unwrap();
            (d, embedder.threads.into_inner().unwrap().len())
        };
        let (serial, serial_threads) = run(1);
        let (parallel, parallel_threads) = run(4);
        assert_eq!(serial_threads, 1);
        assert!(
            parallel_threads > 1,
            "the work was spread over {parallel_threads} thread(s)"
        );
        assert_eq!(serial, parallel);
        assert!(serial.speakers >= 1);
    }

    #[test]
    fn a_failing_voiceprint_ends_the_run_with_its_error() {
        struct Failing;
        impl Embedder for Failing {
            fn parallelism(&self) -> usize {
                3
            }
            fn embed(&self, _: &[f32], _: usize) -> Result<Vec<f32>, DiarizeError> {
                Err(DiarizeError::Inference("boom".into()))
            }
        }
        let r = diarize(
            &ReplaySegmenter { next: Cell::new(0) },
            &Failing,
            &excerpt(),
            &Options::default(),
        );
        assert!(matches!(r, Err(DiarizeError::Inference(m)) if m == "boom"));
    }

    #[test]
    fn no_audio_and_audio_that_is_not_numbers_are_refused() {
        let seg = ReplaySegmenter { next: Cell::new(0) };
        let emb = ReplayEmbedder::new();
        assert!(matches!(
            diarize(&seg, &emb, &[], &Options::default()),
            Err(DiarizeError::InvalidAudio(_))
        ));
        assert!(matches!(
            diarize(&seg, &emb, &[0.0, f32::NAN], &Options::default()),
            Err(DiarizeError::InvalidAudio(_))
        ));
    }

    struct Silent;
    impl Segmenter for Silent {
        fn segment(&self, _: &[f32]) -> Result<Vec<f32>, DiarizeError> {
            let mut one = vec![0.0; 589 * POWERSET_CLASSES];
            for f in 0..589 {
                one[f * POWERSET_CLASSES] = 3.0;
            }
            Ok(one)
        }
    }
    struct Unreached;
    impl Embedder for Unreached {
        fn embed(&self, _: &[f32], _: usize) -> Result<Vec<f32>, DiarizeError> {
            panic!("nobody speaks, so there is nothing to embed")
        }
    }

    #[test]
    fn audio_nobody_speaks_in_has_no_turns_and_no_speakers() {
        let d = diarize(
            &Silent,
            &Unreached,
            &vec![0.0; 16_000 * 3],
            &Options::default(),
        )
        .unwrap();
        assert_eq!((d.turns.len(), d.speakers), (0, 0));
        assert_eq!(d.duration_s, 3.0);
    }

    struct WrongShape;
    impl Segmenter for WrongShape {
        fn segment(&self, _: &[f32]) -> Result<Vec<f32>, DiarizeError> {
            Ok(vec![0.0; 10])
        }
    }

    /// A whole number of frames, but not the 589 the frame grid is built on:
    /// a developer's export under `DIARIZE_MODEL_PATH` could do this.
    struct MoreFrames(usize);
    impl Segmenter for MoreFrames {
        fn segment(&self, _: &[f32]) -> Result<Vec<f32>, DiarizeError> {
            let mut one = vec![0.0; self.0 * POWERSET_CLASSES];
            for f in 0..self.0 {
                one[f * POWERSET_CLASSES] = 3.0;
            }
            Ok(one)
        }
    }

    #[test]
    fn a_network_with_another_frame_count_than_the_grid_is_a_typed_error_not_an_overrun() {
        for frames in [1, 588, 590, 1_000] {
            let r = diarize(
                &MoreFrames(frames),
                &Unreached,
                &vec![0.0; 16_000 * 12],
                &Options::default(),
            );
            assert!(
                matches!(
                    r,
                    Err(DiarizeError::FrameCount { got, expected })
                        if got == frames && expected == FRAMES_PER_WINDOW
                ),
                "{frames}: {r:?}"
            );
        }
    }

    #[test]
    fn a_network_that_answers_with_the_wrong_shape_is_an_error_not_a_guess() {
        let r = diarize(
            &WrongShape,
            &Unreached,
            &vec![0.0; 16_000],
            &Options::default(),
        );
        assert!(
            matches!(r, Err(DiarizeError::Inference(m)) if m.contains("whole number of frames"))
        );
    }
}
