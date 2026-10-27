//! The stem separator on the real model (#385): the pinned htdemucs graph on
//! the pinned ONNX Runtime, over a fixture mixed from known stems.
//!
//! The fixture is `tests/fixtures/stems/{vocals,drums,bass,other}.flac`: 8.5 s
//! of four unrelated recordings (a singer, a drum loop, a bass loop, a guitar
//! loop), cut and levelled to sit like the parts of one mix. They are CC0
//! and CC BY 4.0, which allows them in this repository; the credit is in
//! `tests/fixtures/stems/README.md`, and `make_stems.py` there says how they
//! were made. The test adds them up (that is the mixture), separates it, and
//! scores each estimate against the stem it should be.
//!
//! ## The score
//!
//! SDR in dB, the plain definition over the whole clip and both channels:
//! `10 log10(sum(s^2) / sum((s - s_hat)^2))`, where `s` is the stem that went
//! into the mix and `s_hat` the separator's. It is not museval's
//! (windowed, median, with a distortion filter allowed), which is kinder to a
//! gain error or a small delay and would give higher numbers; this one is
//! what a listener comparing waveforms would compute, and is stricter. The
//! numbers are for *this* clip: four recordings that were never one piece, a
//! hip-hop drum loop under a classical-style voice. They check that the
//! separator separates, in the right order, with the right gain, and that a
//! change to the pipeline does not make it worse; they are not a benchmark
//! (the model's published SDR on MUSDB18-HQ is 9.0 dB, and this clip is not
//! that).
//!
//! ## Running it
//!
//! `#[ignore]`d with the reason, and failing (not skipping) when what it needs
//! is missing, so a green run in `ml.yml` cannot be one that ran nothing:
//!
//! ```text
//! export ORT_DYLIB_PATH=.../libonnxruntime.so          # the pinned 1.23.2
//! export DEMUCS_MODEL_PATH="$(scripts/fetch-ml-model.sh htdemucs ml-models/htdemucs)"
//! cargo test -p ml-demucs --release --test real_model -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Release, because the realtime factor it prints is the one worth reading.

use std::path::{Path, PathBuf};
use std::time::Instant;

use audio_decoder::DecodedAudio;
use ml_demucs::{Control, DemucsModel, STEM_NAMES};

/// Length of the fixture's clips, seconds.
const FIXTURE_SECONDS: f64 = 8.5;

/// What each stem must reach, dB. Set from the first measured run in `ml.yml`
/// (the numbers are in the PR) less a margin for the runner's floating point
/// and for ONNX Runtime's own version-to-version differences.
const MIN_SDR_DB: [f64; 4] = [3.0, 3.0, 1.0, 0.0];

/// How much better than the unseparated mix an estimate must be. The mix
/// handed back as every stem scores the stem's share of it; a separator that
/// does not separate gets that, and this is the margin over it.
const MIN_GAIN_OVER_MIX_DB: f64 = 3.0;

fn require_env(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "{name} is not set. This test runs the real model and fails, rather than skips, \
             without it; see the header of tests/real_model.rs"
            )
        })
}

fn stem_fixture(name: &str) -> Vec<f32> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stems")
        .join(format!("{name}.flac"));
    let decoded = audio_decoder::decode_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        (decoded.sample_rate, decoded.channels),
        (44_100, 2),
        "{name}: the fixture is stereo at 44.1 kHz"
    );
    decoded.samples
}

/// The four fixture stems in [`STEM_NAMES`] order, and their sum.
fn fixture() -> ([Vec<f32>; 4], Vec<f32>) {
    let stems = STEM_NAMES.map(stem_fixture);
    let len = stems[0].len();
    assert!(stems.iter().all(|s| s.len() == len));
    assert_eq!(len as f64, FIXTURE_SECONDS * 44_100.0 * 2.0);
    let mix: Vec<f32> = (0..len).map(|i| stems.iter().map(|s| s[i]).sum()).collect();
    assert!(mix.iter().all(|v| v.abs() < 1.0), "the mix must not clip");
    (stems, mix)
}

fn read_wav(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap()) / 32_768.0)
        .collect()
}

/// `10 log10(sum(s^2) / sum((s - e)^2))` over both channels, in `f64`.
fn sdr(reference: &[f32], estimate: &[f32]) -> f64 {
    assert_eq!(reference.len(), estimate.len());
    let (mut signal, mut noise) = (0.0f64, 0.0f64);
    for (&s, &e) in reference.iter().zip(estimate) {
        signal += f64::from(s).powi(2);
        noise += (f64::from(s) - f64::from(e)).powi(2);
    }
    10.0 * (signal / noise.max(1e-30)).log10()
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(b) {
        ab += f64::from(x) * f64::from(y);
        aa += f64::from(x).powi(2);
        bb += f64::from(y).powi(2);
    }
    ab / (aa.sqrt() * bb.sqrt()).max(1e-30)
}

/// Resident set size and its peak, MB, from `/proc/self/status` (Linux).
fn memory_mb() -> Option<(f64, f64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |key: &str| -> Option<f64> {
        status
            .lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse::<f64>()
            .ok()
    };
    Some((kb("VmRSS:")? / 1024.0, kb("VmHWM:")? / 1024.0))
}

fn load_model() -> (DemucsModel, f64) {
    require_env("ORT_DYLIB_PATH");
    let dir = require_env("DEMUCS_MODEL_PATH");
    let started = Instant::now();
    let flag = |name: &str| std::env::var(name).ok().map(|v| v == "1");
    let options = ml_pipeline::onnx_session::SessionOptions {
        cpu_memory_arena: flag("DEMUCS_TEST_ARENA"),
        memory_pattern: flag("DEMUCS_TEST_PATTERN"),
        intra_op_threads: std::env::var("DEMUCS_TEST_THREADS").ok().and_then(|v| v.parse().ok()),
        ..Default::default()
    };
    eprintln!("session options: {options:?}");
    let model = DemucsModel::load_with("htdemucs", &dir, options).unwrap_or_else(|e| {
        panic!(
            "the model in {} would not load: {e}. DEMUCS_MODEL_PATH names the folder that \
             holds htdemucs_fwd.onnx and htdemucs_fwd.onnx.data",
            dir.display()
        )
    });
    (model, started.elapsed().as_secs_f64())
}

#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library) and DEMUCS_MODEL_PATH (the model folder); run by ml.yml"]
fn a_mix_of_known_stems_is_separated_to_a_stated_sdr() {
    let (stems, mix) = fixture();
    let (model, load_seconds) = load_model();

    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("entry");
    let started = Instant::now();
    let done = model
        .separate_to_dir(
            DecodedAudio {
                samples: mix.clone(),
                sample_rate: 44_100,
                channels: 2,
            },
            &dest,
            &Control::none(),
        )
        .expect("separate");
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "load {load_seconds:.1} s; separation {seconds:.1} s for {FIXTURE_SECONDS} s of audio: \
         {:.2}x realtime ({} segments, clipped {:?})",
        FIXTURE_SECONDS / seconds,
        done.segments,
        done.clipped
    );
    assert_eq!(done.segments, 2);
    assert_eq!(
        (done.frames, done.sample_rate, done.channels),
        (374_850, 44_100, 2)
    );

    let estimates: Vec<Vec<f32>> = done.paths.iter().map(read_wav).collect();
    eprintln!(
        "{:<8}{:>9}{:>16}{:>14}",
        "stem", "SDR dB", "mix as stem dB", "needs dB"
    );
    let mut scores = [0.0f64; 4];
    for i in 0..4 {
        assert_eq!(estimates[i].len(), mix.len());
        scores[i] = sdr(&stems[i], &estimates[i]);
        eprintln!(
            "{:<8}{:>9.2}{:>16.2}{:>14.1}",
            STEM_NAMES[i],
            scores[i],
            sdr(&stems[i], &mix),
            MIN_SDR_DB[i]
        );
    }
    let sum: Vec<f32> = (0..mix.len())
        .map(|i| estimates.iter().map(|e| e[i]).sum())
        .collect();
    eprintln!(
        "the four estimates added up, against the mix: {:.2} dB",
        sdr(&mix, &sum)
    );

    // Whatever the scores, the stems are the right ones: each estimate is
    // closest to its own reference. A swapped source order fails here even
    // if the thresholds were loose.
    for i in 0..4 {
        let own = correlation(&stems[i], &estimates[i]);
        for (j, other) in stems.iter().enumerate() {
            if j != i {
                assert!(
                    own > correlation(other, &estimates[i]),
                    "the {} estimate is closer to the {} than to the {}",
                    STEM_NAMES[i],
                    STEM_NAMES[j],
                    STEM_NAMES[i]
                );
            }
        }
    }
    for i in 0..4 {
        let baseline = sdr(&stems[i], &mix);
        assert!(
            scores[i] >= baseline + MIN_GAIN_OVER_MIX_DB,
            "{}: {:.2} dB is not {MIN_GAIN_OVER_MIX_DB} dB better than the mix itself ({baseline:.2} dB)",
            STEM_NAMES[i],
            scores[i]
        );
        assert!(
            scores[i] >= MIN_SDR_DB[i],
            "{}: SDR {:.2} dB is under the {} dB it must reach",
            STEM_NAMES[i],
            scores[i],
            MIN_SDR_DB[i]
        );
    }
}

/// Five minutes of audio through the real model: how long, how much memory.
///
/// The fixture's mix repeated 36 times is 306 s, 53 segments, as long as a
/// long song. The memory bound that matters is on the *growth* from the loaded
/// model to the end of the run: the model's own working set is ONNX
/// Runtime's, and what this crate adds must not scale with the track. It
/// prints both, and the realtime factor.
#[test]
#[ignore = "needs ORT_DYLIB_PATH (an ONNX Runtime library) and DEMUCS_MODEL_PATH (the model folder); run by ml.yml"]
fn five_minutes_of_audio_takes_bounded_memory() {
    let (_, mix) = fixture();
    let repeats: usize = std::env::var("DEMUCS_TEST_REPEATS").ok().and_then(|v| v.parse().ok()).unwrap_or(36);
    let mut samples = Vec::with_capacity(mix.len() * repeats);
    for _ in 0..repeats {
        samples.extend_from_slice(&mix);
    }
    let samples_len = samples.len();
    let seconds_of_audio = samples_len as f64 / 2.0 / 44_100.0;
    let (model, load_seconds) = load_model();
    let after_load = memory_mb();

    let out = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let done = model
        .separate_to_dir(
            DecodedAudio {
                samples,
                sample_rate: 44_100,
                channels: 2,
            },
            &out.path().join("entry"),
            &Control::none(),
        )
        .expect("separate");
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "{seconds_of_audio:.1} s of audio in {seconds:.1} s: {:.2}x realtime, {} segments; \
         model load {load_seconds:.1} s",
        seconds_of_audio / seconds,
        done.segments
    );
    assert_eq!(
        done.segments,
        ml_demucs::separate::segment_count(samples_len / 2, ml_demucs::separate::SEGMENT_SAMPLES)
    );
    if let (Some((rss_load, _)), Some((rss_end, peak))) = (after_load, memory_mb()) {
        let input_mb = seconds_of_audio * 44_100.0 * 2.0 * 4.0 / 1e6;
        eprintln!(
            "memory: {rss_load:.0} MB after the model loaded, {rss_end:.0} MB at the end, peak \
             {peak:.0} MB; the track is {input_mb:.0} MB of f32 (and the test holds it too)"
        );
        // The track's mix is held by this test (106 MB) and by the separator
        // (106 MB), and the four stems (53 MB each as 16-bit) must not be
        // held at all. Four whole-track f32 stems alone would be 424 MB.
        assert!(
            peak - rss_load < 3.0 * input_mb + 40_000.0,
            "the peak grew by {:.0} MB over a {input_mb:.0} MB track",
            peak - rss_load
        );
    }
}
