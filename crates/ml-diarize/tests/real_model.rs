//! The two networks on real audio (#168 section 2).
//!
//! Everything here is `#[ignore]`d because it needs what CI does not have: the
//! pinned speaker model, 32.5 MB, and the ONNX Runtime library. Run by hand,
//! or by `.github/workflows/ml.yml`, with `DIARIZE_MODEL_PATH` naming the
//! model **folder** (the one that holds `segmentation/model.onnx` and
//! `embedding/model.onnx`, laid out as `crates/ml-pipeline/models.json` pins
//! them; `scripts/fetch-ml-model.sh speaker-diarization <dir>` makes it) and
//! `ORT_DYLIB_PATH` the ONNX Runtime library:
//!
//! ```text
//! export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
//! export DIARIZE_MODEL_PATH=/path/to/speaker-diarization
//! cargo test -p ml-diarize --release --test real_model -- --ignored --nocapture --test-threads=1
//! ```
//!
//! These tests **fail** when either is missing, with what is missing, rather
//! than returning early: an `#[ignore]`d test that was asked for with
//! `--ignored` and passes without running is the vacuous pass #263 is about.
//!
//! ## The recording
//!
//! `tests/fixtures/ami_es2004a_640_680s.wav` is 40 s of the AMI Meeting Corpus
//! (meeting ES2004a, headset mix, seconds 640 to 680; CC BY 4.0, see
//! `tests/fixtures/AMI-LICENSE.md`) in which two people talk and interrupt each
//! other, with the corpus' hand annotation (`.rttm`). `fixtures.json` holds what
//! pyannote.audio's algorithm (3.1's segmentation and clustering with 3.0's ONNX
//! frame-selection embedding) made from the same files
//! (`tests/fixtures/make_fixtures.py`).
//!
//! ## What is asserted
//!
//! * Segmentation: for 99.9 % of frames the Rust run picks the class pyannote's
//!   run picked.
//! * Voiceprints: the same number of them, each within cosine 0.999 of the
//!   reference's. That proves the filterbank, the mask and the network.
//! * Turns: against the reference's turns (same algorithm, different code) the
//!   disagreement is under 1 % of the time; against the human annotation the
//!   diarization error rate is under 25 % (the reference's own is 14.8 %, and
//!   13.1 % given two speakers) with the number of people estimated, and under
//!   20 % given `num_speakers = 2`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use ml_diarize::pipeline::{self, Embedder};
use ml_diarize::{DiarizeModel, Options, SpeakerBounds, SpeakerTurn, EMBEDDING_DIM};

const WAV: &[u8] = include_bytes!("fixtures/ami_es2004a_640_680s.wav");
const RTTM: &str = include_str!("fixtures/ami_es2004a_640_680s.rttm");
const FIXTURES: &str = include_str!("fixtures/fixtures.json");
const EMBEDDINGS: &[u8] = include_bytes!("fixtures/embeddings.f32");
const SEGMENTATION: &[u8] = include_bytes!("fixtures/segmentation.u8");

fn model() -> DiarizeModel {
    let runtime = std::env::var_os("ORT_DYLIB_PATH");
    let folder = std::env::var_os("DIARIZE_MODEL_PATH");
    assert!(
        runtime.is_some() && folder.is_some(),
        "this test needs ORT_DYLIB_PATH (an ONNX Runtime library) and DIARIZE_MODEL_PATH (the \
         speaker model folder, scripts/fetch-ml-model.sh speaker-diarization <dir>); missing: {}",
        [
            ("ORT_DYLIB_PATH", runtime.is_none()),
            ("DIARIZE_MODEL_PATH", folder.is_none())
        ]
        .iter()
        .filter(|(_, missing)| *missing)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
    );
    DiarizeModel::load(&PathBuf::from(folder.unwrap())).expect("the speaker model loads")
}

fn audio() -> Vec<f32> {
    hound::WavReader::new(std::io::Cursor::new(WAV))
        .unwrap()
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32768.0)
        .collect()
}

fn reference_embeddings() -> Vec<Vec<f32>> {
    EMBEDDINGS
        .chunks_exact(4 * EMBEDDING_DIM)
        .map(|row| {
            row.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect::<Vec<f32>>()
        })
        .filter(|row| !row[0].is_nan())
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let norm = |v: &[f32]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    dot / (norm(a) * norm(b))
}

/// Hands every call to the real network and keeps what it said, in the order
/// of the calls. It asks for one voiceprint at a time, so that order is the
/// order of the reference's rows; the threaded run is held to the same
/// reference by `the_excerpt_is_diarized_like_the_reference...`, which uses the
/// model as it is.
struct Recording<'a> {
    inner: &'a DiarizeModel,
    seen: Mutex<Vec<Vec<f32>>>,
}

impl Embedder for Recording<'_> {
    fn embed(&self, features: &[f32], frames: usize) -> Result<Vec<f32>, pipeline::DiarizeError> {
        let v = self.inner.embed(features, frames)?;
        self.seen.lock().unwrap().push(v.clone());
        Ok(v)
    }
}

/// Seconds of disagreement between two sets of turns, over `duration`, on a
/// 10 ms grid, with the speakers matched in the way that agrees most.
struct Der {
    miss: f64,
    false_alarm: f64,
    confusion: f64,
    reference_speech: f64,
}

impl Der {
    fn rate(&self) -> f64 {
        (self.miss + self.false_alarm + self.confusion) / self.reference_speech
    }
}

fn der(reference: &[(f64, f64, usize)], hypothesis: &[SpeakerTurn], duration: f64) -> Der {
    const STEP: f64 = 0.01;
    let frames = (duration / STEP) as usize;
    let nref = reference.iter().map(|t| t.2 + 1).max().unwrap_or(0);
    let nhyp = hypothesis.iter().map(|t| t.speaker + 1).max().unwrap_or(0);
    let active = |f: usize, k: usize, ref_side: bool| -> bool {
        let t = (f as f64 + 0.5) * STEP;
        if ref_side {
            reference.iter().any(|r| r.2 == k && r.0 <= t && t < r.1)
        } else {
            hypothesis
                .iter()
                .any(|h| h.speaker == k && h.start_s <= t && t < h.end_s)
        }
    };
    // overlap[h][r]: frames where hypothesis speaker h and reference speaker r both speak.
    let mut overlap = vec![vec![0usize; nref]; nhyp];
    let mut rows = Vec::with_capacity(frames);
    for f in 0..frames {
        let r: Vec<usize> = (0..nref).filter(|&k| active(f, k, true)).collect();
        let h: Vec<usize> = (0..nhyp).filter(|&k| active(f, k, false)).collect();
        for &hh in &h {
            for &rr in &r {
                overlap[hh][rr] += 1;
            }
        }
        rows.push((r, h));
    }
    // The best one-to-one matching, by trying them all (a handful of speakers).
    fn best(
        h: usize,
        taken: &mut Vec<bool>,
        overlap: &[Vec<usize>],
    ) -> (usize, Vec<Option<usize>>) {
        if h == overlap.len() {
            return (0, Vec::new());
        }
        let mut top = {
            let (s, mut rest) = best(h + 1, taken, overlap);
            rest.insert(0, None);
            (s, rest)
        };
        for r in 0..taken.len() {
            if !taken[r] {
                taken[r] = true;
                let (s, mut rest) = best(h + 1, taken, overlap);
                taken[r] = false;
                if s + overlap[h][r] > top.0 {
                    rest.insert(0, Some(r));
                    top = (s + overlap[h][r], rest);
                }
            }
        }
        top
    }
    let (_, mapping) = best(0, &mut vec![false; nref], &overlap);
    let mut out = Der {
        miss: 0.0,
        false_alarm: 0.0,
        confusion: 0.0,
        reference_speech: 0.0,
    };
    for (r, h) in rows {
        let correct = h
            .iter()
            .filter(|&&hh| mapping[hh].is_some_and(|rr| r.contains(&rr)))
            .count();
        out.reference_speech += r.len() as f64 * STEP;
        out.miss += r.len().saturating_sub(h.len()) as f64 * STEP;
        out.false_alarm += h.len().saturating_sub(r.len()) as f64 * STEP;
        out.confusion += (r.len().min(h.len()) - correct) as f64 * STEP;
    }
    out
}

fn human_turns() -> Vec<(f64, f64, usize)> {
    let mut names: Vec<&str> = Vec::new();
    RTTM.lines()
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (start, len): (f64, f64) = (f[3].parse().unwrap(), f[4].parse().unwrap());
            let k = names.iter().position(|n| *n == f[7]).unwrap_or_else(|| {
                names.push(f[7]);
                names.len() - 1
            });
            (start, start + len, k)
        })
        .collect()
}

/// Disagreement between two diarizations of the same audio, as a share of the
/// audio's length, speakers matched.
fn disagreement(a: &[SpeakerTurn], b: &[SpeakerTurn], duration: f64) -> f64 {
    let as_reference: Vec<(f64, f64, usize)> =
        a.iter().map(|t| (t.start_s, t.end_s, t.speaker)).collect();
    let d = der(&as_reference, b, duration);
    (d.miss + d.false_alarm + d.confusion) / duration
}

#[test]
#[ignore = "needs the speaker model and an ONNX Runtime library (see the file's docs)"]
fn the_pinned_files_load_and_the_smallest_input_is_the_one_the_code_assumes() {
    let m = model();
    let features = |frames: usize| -> Vec<f32> {
        (0..frames * ml_diarize::fbank::BANDS)
            .map(|i| ((i * 7919) % 1000) as f32 / 500.0 - 1.0)
            .collect()
    };
    let enough = pipeline::MIN_EMBEDDING_FRAMES;
    let v = m.embed(&features(enough), enough).unwrap();
    assert_eq!(v.len(), EMBEDDING_DIM);
    assert!(v.iter().all(|x| x.is_finite()), "{enough} frames is enough");
    let v = m.embed(&features(enough - 1), enough - 1).unwrap();
    assert!(v.iter().any(|x| x.is_nan()), "{} frames is not", enough - 1);
}

#[test]
#[ignore = "needs the speaker model and an ONNX Runtime library (see the file's docs)"]
fn segmentation_picks_the_class_the_reference_picks() {
    use pipeline::Segmenter;
    let m = model();
    let audio = audio();
    let started = Instant::now();
    let (mut same, mut total) = (0usize, 0usize);
    for c in 0..31 {
        let start = c * 16_000;
        let mut window = vec![0.0f32; 160_000];
        let end = (start + 160_000).min(audio.len());
        window[..end - start].copy_from_slice(&audio[start..end]);
        let scores = m.segment(&window).unwrap();
        assert_eq!(scores.len(), 589 * 7);
        for (f, row) in scores.chunks_exact(7).enumerate() {
            let got = pipeline::powerset_to_multilabel(row);
            let at = (c * 589 + f) * 3;
            total += 1;
            same +=
                usize::from(got == [SEGMENTATION[at], SEGMENTATION[at + 1], SEGMENTATION[at + 2]]);
        }
    }
    println!(
        "segmentation: {same} of {total} frames agree, {:.2} s for 31 windows",
        started.elapsed().as_secs_f64()
    );
    assert!(same as f64 / total as f64 >= 0.999, "{same} of {total}");
}

#[test]
#[ignore = "needs the speaker model and an ONNX Runtime library (see the file's docs)"]
fn voiceprints_are_the_references() {
    let m = model();
    let recording = Recording {
        inner: &m,
        seen: Mutex::new(Vec::new()),
    };
    pipeline::diarize(&m, &recording, &audio(), &Options::default()).unwrap();
    let got = recording.seen.into_inner().unwrap();
    let want = reference_embeddings();
    assert_eq!(got.len(), want.len(), "the same speakers are embedded");
    let worst = got
        .iter()
        .zip(&want)
        .map(|(a, b)| cosine(a, b))
        .fold(1.0f64, f64::min);
    println!(
        "{} voiceprints, the least like the reference has cosine {worst:.6}",
        got.len()
    );
    assert!(worst >= 0.999, "{worst}");
}

#[test]
#[ignore = "needs the speaker model and an ONNX Runtime library (see the file's docs)"]
fn the_excerpt_is_diarized_like_the_reference_and_near_the_human_annotation() {
    let m = model();
    let audio = audio();
    let fixtures: serde_json::Value = serde_json::from_str(FIXTURES).unwrap();
    let case = |name: &str| {
        fixtures["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()
            .clone()
    };
    let human = human_turns();

    for (name, bounds, max_der) in [
        ("free", SpeakerBounds::default(), 0.25),
        (
            "num2",
            SpeakerBounds::new(Some(2), None, None).unwrap(),
            0.20,
        ),
    ] {
        let started = Instant::now();
        let got = m
            .diarize(
                &audio,
                &Options {
                    bounds,
                    ..Options::default()
                },
            )
            .unwrap();
        let took = started.elapsed().as_secs_f64();
        let reference: Vec<SpeakerTurn> = case(name)["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| SpeakerTurn {
                start_s: t[0].as_f64().unwrap(),
                end_s: t[1].as_f64().unwrap().min(40.0),
                speaker: t[2].as_u64().unwrap() as usize,
            })
            .filter(|t| t.start_s < t.end_s)
            .collect();
        let vs_reference = disagreement(&reference, &got.turns, 40.0);
        let vs_human = der(&human, &got.turns, 40.0);
        println!(
            "{name}: {} speakers, {:.1} s for 40 s of audio (RTF {:.3}); disagrees with the reference on {:.2} % of the time; DER against the human annotation {:.1} % (miss {:.1} s, false alarm {:.1} s, confusion {:.1} s of {:.1} s)",
            got.speakers, took, took / 40.0, vs_reference * 100.0, vs_human.rate() * 100.0,
            vs_human.miss, vs_human.false_alarm, vs_human.confusion, vs_human.reference_speech
        );
        assert!(vs_reference < 0.01, "{name}: {vs_reference}");
        assert!(vs_human.rate() < max_der, "{name}: {}", vs_human.rate());
        assert_eq!(
            got.speakers as u64,
            case(name)["clusters"].as_u64().unwrap()
        );
    }
}
