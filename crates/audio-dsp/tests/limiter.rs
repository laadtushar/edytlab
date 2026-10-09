//! The limiter, as a limiter (#441).
//!
//! It used to be a hard clip: every sample past the ceiling was replaced
//! with the ceiling. That cannot be told apart from a limiter by looking
//! at the peak, which is the one thing the old tests checked, so most of
//! what is here is about the other things a limiter owes:
//!
//! * the ceiling is a *guarantee*, so it is hammered with random and
//!   adversarial signals rather than spot-checked;
//! * it must not be a clip, so the distortion of a sine pushed 6 dB over
//!   the ceiling is measured against the clip it replaced;
//! * it must recover, and recover at the rate it was asked to;
//! * it must be one limiter, whatever the block size and however many
//!   channels, and must not shift the stereo image.
//!
//! `audio-dsp` has no dependencies, deliberately, so the random signals
//! come from a small seeded generator below. Every failure message
//! carries its seed.

use audio_dsp::dynamics::{DEFAULT_RELEASE_MS, MAX_RELEASE_MS, MIN_RELEASE_MS};
use audio_dsp::{Limiter, Processor};

// ---- helpers ---------------------------------------------------------

/// xorshift64*: small, seeded, and good enough to throw varied signals
/// at a limiter.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[-1, 1)`.
    fn unit(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }

    /// Uniform in `0..n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())]
    }
}

fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// The behaviour the limiter replaced, kept here as the yardstick.
fn hard_clip(samples: &mut [f32], ceiling: f32) {
    for s in samples.iter_mut() {
        if s.abs() > ceiling {
            *s = s.signum() * ceiling;
        }
    }
}

/// Run a whole-frame block sequence through one limiter.
fn process_in_blocks(
    lim: &mut Limiter,
    signal: &mut [f32],
    channels: usize,
    rng: &mut Rng,
    max_frames: usize,
) {
    let mut at = 0;
    while at < signal.len() {
        let frames = 1 + rng.below(max_frames);
        let end = (at + frames * channels).min(signal.len());
        lim.process(&mut signal[at..end], channels);
        at = end;
    }
}

/// Amplitude of the component at `freq` in `x`, by direct correlation.
///
/// Exact (no leakage) when `x` holds a whole number of cycles of
/// `freq`, which every caller arranges.
fn tone_amplitude(x: &[f32], sample_rate: f32, freq: f32) -> f64 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    let w = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sample_rate);
    for (n, &v) in x.iter().enumerate() {
        let phase = w * n as f64;
        re += f64::from(v) * phase.cos();
        im += f64::from(v) * phase.sin();
    }
    2.0 * re.hypot(im) / x.len() as f64
}

/// Total harmonic distortion of a signal at `fundamental`: the RMS of
/// harmonics 2 to 10 over the fundamental. `x` must hold whole cycles.
fn thd(x: &[f32], sample_rate: f32, fundamental: f32) -> f64 {
    let f1 = tone_amplitude(x, sample_rate, fundamental);
    let harmonics: f64 = (2..=10)
        .map(|k| tone_amplitude(x, sample_rate, fundamental * k as f32).powi(2))
        .sum();
    harmonics.sqrt() / f1
}

fn sine(freq: f32, amp: f32, sample_rate: u32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| amp * (2.0 * std::f32::consts::PI * freq * n as f32 / sample_rate as f32).sin())
        .collect()
}

// ---- the ceiling is a guarantee --------------------------------------

/// Interleaved test signal of a random kind, scaled to `scale`.
///
/// The kinds are the ones that stress a limiter's attack: steady noise,
/// rare enormous spikes, full-scale square waves down to the Nyquist
/// alternation, DC steps, bursts out of silence and back, and a grab
/// bag of extreme finite values.
fn random_signal(
    rng: &mut Rng,
    channels: usize,
    frames: usize,
    scale: f32,
) -> (Vec<f32>, &'static str) {
    let kind = rng.below(8);
    let mut x = vec![0.0f32; frames * channels];
    let name = match kind {
        0 => {
            for s in x.iter_mut() {
                *s = rng.unit() * scale;
            }
            "noise"
        }
        1 => {
            for s in x.iter_mut() {
                *s = if rng.below(60) == 0 {
                    rng.unit() * scale * 8.0
                } else {
                    rng.unit() * 0.01
                };
            }
            "sparse spikes"
        }
        2 => {
            for c in 0..channels {
                let period = 2 + rng.below(200);
                let phase = rng.below(period);
                for f in 0..frames {
                    let high = ((f + phase) / (period / 2).max(1)) % 2 == 0;
                    x[f * channels + c] = if high { scale } else { -scale };
                }
            }
            "square wave"
        }
        3 => {
            for c in 0..channels {
                let freq = 20.0 + (rng.unit().abs() * 9_000.0);
                for f in 0..frames {
                    x[f * channels + c] =
                        scale * (2.0 * std::f32::consts::PI * freq * f as f32 / 48_000.0).sin();
                }
            }
            "sine"
        }
        4 => {
            let mut level = 0.0f32;
            for f in 0..frames {
                if rng.below(40) == 0 {
                    level = rng.unit() * scale;
                }
                for c in 0..channels {
                    x[f * channels + c] = level * if c % 2 == 0 { 1.0 } else { 0.5 };
                }
            }
            "dc steps"
        }
        5 => {
            for f in 0..frames {
                for c in 0..channels {
                    let sign = if (f + c) % 2 == 0 { 1.0 } else { -1.0 };
                    x[f * channels + c] = sign * scale;
                }
            }
            "nyquist alternation"
        }
        6 => {
            let (a, b) = (rng.below(frames), rng.below(frames));
            let (a, b) = (a.min(b), a.max(b));
            for f in a..b {
                for c in 0..channels {
                    x[f * channels + c] = rng.unit() * scale;
                }
            }
            "burst"
        }
        _ => {
            let extremes = [
                0.0,
                scale,
                -scale,
                1e-30,
                -1e-30,
                f32::MAX * 0.5,
                -f32::MAX,
                f32::MIN_POSITIVE,
                1.0,
                -1.0,
                0.25,
            ];
            for s in x.iter_mut() {
                *s = rng.pick(&extremes);
            }
            "extremes"
        }
    };
    (x, name)
}

#[test]
fn the_ceiling_holds_on_random_signals_at_every_setting() {
    for seed in 0..400u64 {
        let mut rng = Rng::new(seed);
        let channels = rng.pick(&[1usize, 2, 2, 3, 6]);
        let sample_rate = rng.pick(&[8_000u32, 22_050, 44_100, 48_000, 96_000, 192_000]);
        let ceiling_db = rng.pick(&[0.0f32, -0.1, -0.3, -1.0, -3.0, -6.0, -12.0, -20.0, -60.0]);
        let release_ms = rng.pick(&[1.0f32, 5.0, 10.0, 50.0, DEFAULT_RELEASE_MS, 500.0, 5_000.0]);
        let scale = rng.pick(&[0.001f32, 0.3, 1.0, 4.0, 100.0, 1.0e6]);
        let frames = 1 + rng.below(5_000);

        let (mut x, kind) = random_signal(&mut rng, channels, frames, scale);
        let mut lim = Limiter::new(ceiling_db, release_ms, sample_rate);
        let ceiling = lim.ceiling();
        process_in_blocks(&mut lim, &mut x, channels, &mut rng, 700);

        for (i, y) in x.iter().enumerate() {
            assert!(
                y.is_finite() && y.abs() <= ceiling,
                "seed {seed} ({kind}, {channels} ch, {sample_rate} Hz, {ceiling_db} dB, \
                 {release_ms} ms, scale {scale}): sample {i} is {y}, over the ceiling {ceiling}"
            );
        }
    }
}

/// The signals a clipper-style test would think of, plus the ones that
/// make a limiter with a slow attack leak: a spike out of silence, a
/// spike on the very first sample, a spike one sample after another
/// has already pulled the gain down, and values near the float limits.
#[test]
fn the_ceiling_holds_on_adversarial_signals() {
    let sample_rate = 48_000;
    for ceiling_db in [0.0f32, -1.0, -6.0, -24.0] {
        for release_ms in [MIN_RELEASE_MS, DEFAULT_RELEASE_MS, MAX_RELEASE_MS] {
            let mk = || Limiter::new(ceiling_db, release_ms, sample_rate);
            let ceiling = mk().ceiling();

            let mut cases: Vec<(&str, usize, Vec<f32>)> = Vec::new();

            // Full-scale square waves at several periods, mono and stereo.
            for period in [2usize, 3, 4, 10, 48, 480] {
                let sq: Vec<f32> = (0..4_800)
                    .map(|n| {
                        if (n / (period / 2).max(1)) % 2 == 0 {
                            1.0
                        } else {
                            -1.0
                        }
                    })
                    .collect();
                cases.push(("square wave", 1, sq.clone()));
                cases.push((
                    "square wave stereo",
                    2,
                    sq.iter().flat_map(|&s| [s, -s * 0.5]).collect(),
                ));
            }

            // A single-sample spike in silence, at several heights and
            // positions, either polarity, in either channel.
            for height in [1.0f32, 1.0001, 2.0, 10.0, 1.0e6, 1.0e30, f32::MAX] {
                for pos in [0usize, 1, 2, 500] {
                    for sign in [1.0f32, -1.0] {
                        let mut mono = vec![0.0f32; 1_000];
                        mono[pos] = sign * height;
                        cases.push(("spike", 1, mono));

                        let mut stereo = vec![0.0f32; 2_000];
                        stereo[pos * 2 + 1] = sign * height;
                        cases.push(("spike in the right channel", 2, stereo));
                    }
                }
            }

            // Spike trains: each one arrives while the previous is still
            // being released, and each is bigger or smaller than the last.
            for spacing in [1usize, 2, 7, 100] {
                let mut train = vec![0.05f32; 3_000];
                for (i, s) in train.iter_mut().enumerate() {
                    if i % spacing == 0 {
                        *s = (1.0 + (i / spacing % 5) as f32 * 3.0)
                            * if i % 2 == 0 { 1.0 } else { -1.0 };
                    }
                }
                cases.push(("spike train", 1, train));
            }

            // A step to a high DC level and back.
            let mut dc = vec![0.0f32; 3_000];
            dc[1_000..2_000].fill(5.0);
            cases.push(("dc step", 1, dc));

            // Subnormals and exact zeros.
            cases.push(("subnormals", 1, vec![f32::MIN_POSITIVE / 4.0; 100]));

            for (name, channels, signal) in cases {
                let mut lim = mk();
                let mut out = signal.clone();
                lim.process(&mut out, channels);
                for (i, y) in out.iter().enumerate() {
                    assert!(
                        y.is_finite() && y.abs() <= ceiling,
                        "{name} ({channels} ch, {ceiling_db} dB, {release_ms} ms): \
                         sample {i} came out {y} from {}, over the ceiling {ceiling}",
                        signal[i]
                    );
                }
            }
        }
    }
}

/// The hard clip let `NaN` through and clamped `inf`; the limiter keeps
/// both, and must not let either turn the gain into something that
/// mutes or poisons the audio after it.
#[test]
fn non_finite_samples_never_escape_and_never_poison_the_gain() {
    for seed in 0..100u64 {
        let mut rng = Rng::new(seed ^ 0xBAD);
        let channels = rng.pick(&[1usize, 2, 6]);
        let frames = 200 + rng.below(1_000);
        let mut lim = Limiter::new(-3.0, 20.0, 48_000);
        let ceiling = lim.ceiling();

        let mut x: Vec<f32> = (0..frames * channels).map(|_| rng.unit() * 2.0).collect();
        let mut bad = Vec::new();
        for _ in 0..(1 + rng.below(30)) {
            let i = rng.below(x.len());
            x[i] = rng.pick(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
            bad.push(i);
        }
        let input = x.clone();
        lim.process(&mut x, channels);

        for (i, y) in x.iter().enumerate() {
            if input[i].is_nan() {
                assert!(y.is_nan(), "seed {seed}: NaN at {i} should pass through");
            } else {
                assert!(
                    y.abs() <= ceiling,
                    "seed {seed}: sample {i} is {y}, over the ceiling {ceiling}"
                );
            }
        }
        assert!(
            lim.gain().is_finite() && lim.gain() > 0.0 && lim.gain() <= 1.0,
            "seed {seed}: gain state is {}",
            lim.gain()
        );

        // And it still limits and releases like new afterwards: quiet
        // audio, a second later, comes through bit-for-bit.
        let quiet: Vec<f32> = (0..96_000 * channels)
            .map(|i| 0.01 * (i as f32 * 0.1).sin())
            .collect();
        let mut after = quiet.clone();
        lim.process(&mut after, channels);
        assert_eq!(
            &after[after.len() / 2..],
            &quiet[quiet.len() / 2..],
            "seed {seed}: the limiter did not return to unity after non-finite input"
        );
    }
}

// ---- it is one limiter, however it is fed ----------------------------

/// The renderer's contract: the output may not depend on how the signal
/// is divided into chunks. For whole frames it holds exactly, in every
/// channel count; for mono every chunk length is a whole frame.
#[test]
fn block_size_does_not_change_the_output() {
    for seed in 0..120u64 {
        let mut rng = Rng::new(seed + 10_000);
        let channels = rng.pick(&[1usize, 2, 3, 6]);
        let sample_rate = rng.pick(&[8_000u32, 44_100, 48_000, 96_000]);
        let ceiling_db = rng.pick(&[-0.3f32, -1.0, -6.0, -20.0]);
        let release_ms = rng.pick(&[1.0f32, 20.0, DEFAULT_RELEASE_MS, 1_000.0]);
        let scale = rng.pick(&[0.5f32, 1.0, 3.0, 50.0]);
        let frames = 1 + rng.below(6_000);
        let (signal, kind) = random_signal(&mut rng, channels, frames, scale);

        let mut whole = signal.clone();
        Limiter::new(ceiling_db, release_ms, sample_rate).process(&mut whole, channels);

        for max_frames in [1usize, 2, 3, 17, 128, 1_000, 10_000] {
            let mut pieces = signal.clone();
            let mut lim = Limiter::new(ceiling_db, release_ms, sample_rate);
            process_in_blocks(&mut lim, &mut pieces, channels, &mut rng, max_frames);
            let same = whole
                .iter()
                .zip(&pieces)
                .all(|(a, b)| a.to_bits() == b.to_bits());
            assert!(
                same,
                "seed {seed} ({kind}, {channels} ch): blocks of up to {max_frames} frames \
                 gave different output from one call"
            );
        }
    }
}

/// Block-by-block is also identical sample-by-sample, the extreme the
/// renderer's last short chunk can approach.
#[test]
fn a_mono_signal_fed_one_sample_at_a_time_matches_one_call() {
    let mut rng = Rng::new(77);
    let signal: Vec<f32> = (0..5_000).map(|_| rng.unit() * 3.0).collect();

    let mut whole = signal.clone();
    Limiter::new(-1.0, 30.0, 44_100).process(&mut whole, 1);

    let mut lim = Limiter::new(-1.0, 30.0, 44_100);
    let mut single = signal;
    for s in single.chunks_mut(1) {
        lim.process(s, 1);
    }
    assert_eq!(whole, single);
}

/// The one thing a streaming channel-linked limiter cannot do: when a
/// chunk ends between the channels of a frame, the earlier channels are
/// written before the later ones exist. The output may then differ from
/// the unsplit run, but the ceiling is not negotiable.
#[test]
fn a_chunk_that_ends_mid_frame_still_holds_the_ceiling() {
    for seed in 0..200u64 {
        let mut rng = Rng::new(seed + 50_000);
        let channels = rng.pick(&[2usize, 3, 6]);
        let frames = 1 + rng.below(3_000);
        let scale = rng.pick(&[0.5f32, 2.0, 40.0]);
        let (mut x, kind) = random_signal(&mut rng, channels, frames, scale);

        let mut lim = Limiter::new(-6.0, 20.0, 48_000);
        let ceiling = lim.ceiling();
        let mut at = 0;
        while at < x.len() {
            // Lengths in samples, deliberately not multiples of the
            // channel count.
            let end = (at + 1 + rng.below(13)).min(x.len());
            lim.process(&mut x[at..end], channels);
            at = end;
        }
        for (i, y) in x.iter().enumerate() {
            assert!(
                y.abs() <= ceiling,
                "seed {seed} ({kind}, {channels} ch): sample {i} is {y}, over the ceiling {ceiling}"
            );
        }
    }
}

#[test]
fn a_split_frame_gives_the_late_channel_the_same_gain_when_it_needs_it() {
    // Left is quiet, right is the over; the chunk boundary falls between
    // them. The right channel must still come out at the ceiling, and
    // the frame after it must carry the gain the right channel forced.
    let mut lim = Limiter::new(-6.0, 80.0, 48_000);
    let ceiling = lim.ceiling();

    let mut first = [0.2f32];
    lim.process(&mut first, 2);
    assert_eq!(first[0], 0.2, "nothing has asked for gain reduction yet");

    let mut rest = [2.0f32, 0.2, 0.2];
    lim.process(&mut rest, 2);
    assert!((rest[0] - ceiling).abs() < 1e-6, "got {}", rest[0]);
    let applied = rest[1] / 0.2;
    assert!(
        (applied - ceiling / 2.0).abs() < 1e-3,
        "the next frame should carry the gain the late channel forced, got {applied}"
    );
}

// ---- it only acts when it has to -------------------------------------

#[test]
fn a_signal_under_the_ceiling_passes_bit_for_bit() {
    for seed in 0..100u64 {
        let mut rng = Rng::new(seed + 20_000);
        let channels = rng.pick(&[1usize, 2, 6]);
        let ceiling_db = rng.pick(&[-0.1f32, -1.0, -6.0, -20.0]);
        let ceiling = db_to_linear(ceiling_db);
        let frames = 1 + rng.below(4_000);

        // Peaks right up against the ceiling, but never over it.
        let mut x: Vec<f32> = (0..frames * channels)
            .map(|_| rng.unit() * ceiling)
            .collect();
        x[rng.below(frames * channels)] = ceiling;
        let before = x.clone();

        let mut lim = Limiter::new(ceiling_db, DEFAULT_RELEASE_MS, 48_000);
        process_in_blocks(&mut lim, &mut x, channels, &mut rng, 300);
        assert_eq!(
            x, before,
            "seed {seed}: a signal at or under the ceiling changed"
        );
        assert_eq!(lim.gain(), 1.0, "seed {seed}");
    }
}

#[test]
fn the_ceiling_is_the_documented_conversion() {
    for db in [0.0f32, -0.3, -1.0, -6.0, -20.0] {
        assert_eq!(
            Limiter::new(db, DEFAULT_RELEASE_MS, 48_000).ceiling(),
            db_to_linear(db)
        );
    }
}

/// A release of zero would make the gain recover instantly, which is a
/// hard clip with extra steps: the sample after an over would be
/// restored to unity gain. It is clamped to a real (1 ms) release
/// instead, which at 48 kHz gives back a few percent of the reduction
/// per sample, not all of it.
#[test]
fn a_zero_release_cannot_turn_it_back_into_a_clip() {
    for release_ms in [0.0f32, -10.0, f32::NEG_INFINITY] {
        let mut lim = Limiter::new(-6.0, release_ms, 48_000);
        let mut x = [2.0f32, 0.3, 0.3, 0.3];
        lim.process(&mut x, 1);
        let held = lim.ceiling() / 2.0;
        let after = x[1] / 0.3;
        assert!(
            after >= held && after < held + 0.05,
            "release {release_ms}: the sample after an over has gain {after}, \
             expected about {held}; a clip would have restored it to 1.0"
        );
    }
}

// ---- release ---------------------------------------------------------

/// The gain coming back: along one exponential, at the speed asked for,
/// and all the way to unity so the limiter goes quiet again.
#[test]
fn the_gain_recovers_to_unity_within_a_few_release_times() {
    let sample_rate = 48_000u32;
    for release_ms in [10.0f32, 50.0, 200.0] {
        let release_frames = (release_ms * 0.001 * sample_rate as f32) as usize;
        let mut lim = Limiter::new(-6.0, release_ms, sample_rate);
        let ceiling = lim.ceiling();

        // One over (+12 dB), then steady quiet audio.
        let spike = ceiling * 4.0;
        let mut x = vec![0.05f32; release_frames * 30];
        x[0] = spike;
        let mut gains = Vec::with_capacity(x.len());
        for s in x.chunks_mut(1) {
            lim.process(s, 1);
            gains.push(lim.gain());
        }

        let initial = 1.0 - ceiling / spike;
        assert!((gains[0] - ceiling / spike).abs() < 1e-6);
        assert!(
            (gains[0] - 0.25).abs() < 1e-6,
            "attack should be instant and exact: gain {} at the spike",
            gains[0]
        );

        // After k release times the reduction is initial * e^-k.
        for k in 1..=5usize {
            let measured = 1.0 - gains[k * release_frames];
            let expected = initial * (-(k as f32)).exp();
            assert!(
                (measured - expected).abs() < expected * 0.02 + 1e-6,
                "{release_ms} ms: after {k} release times the reduction is {measured}, \
                 expected {expected}"
            );
        }
        assert!(
            gains[5 * release_frames] > 0.99,
            "{release_ms} ms: only back to {} after five release times",
            gains[5 * release_frames]
        );

        // And it is exactly unity, not almost, so the audio is
        // bit-for-bit again.
        assert_eq!(*gains.last().unwrap(), 1.0, "{release_ms} ms");
        let tail = &x[x.len() - 100..];
        assert!(
            tail.iter().all(|&s| s == 0.05),
            "{release_ms} ms: tail changed"
        );
    }
}

#[test]
fn a_longer_release_holds_the_gain_down_for_longer() {
    let at_100ms = |release_ms: f32| {
        let mut lim = Limiter::new(-6.0, release_ms, 48_000);
        let mut x = vec![0.05f32; 4_801];
        x[0] = 2.0;
        lim.process(&mut x, 1);
        x[4_800] / 0.05
    };
    let (quick, slow) = (at_100ms(10.0), at_100ms(400.0));
    assert!(quick > 0.999, "10 ms release, 100 ms later: gain {quick}");
    assert!(slow < 0.8, "400 ms release, 100 ms later: gain {slow}");
}

// ---- stereo ----------------------------------------------------------

/// One gain per frame. A hard-left over has to turn the right channel
/// down by the same amount; a per-channel limiter would leave it alone
/// and the image would lurch right for the length of the release.
#[test]
fn every_channel_of_a_frame_gets_the_same_gain() {
    for channels in [2usize, 3, 6] {
        for seed in 0..60u64 {
            let mut rng = Rng::new(seed + 30_000);
            let frames = 2_000;
            let mut x = vec![0.0f32; frames * channels];
            for f in 0..frames {
                // Channel 0 carries overs; the others stay modest and
                // all differ, so any gain mismatch is visible.
                for c in 0..channels {
                    let scale = if c == 0 { 3.0 } else { 0.2 + 0.1 * c as f32 };
                    let v = rng.unit() * scale;
                    // Keep every sample away from zero so a ratio means something.
                    x[f * channels + c] = if v.abs() < 0.02 { 0.02 } else { v };
                }
            }
            let input = x.clone();
            let mut lim = Limiter::new(-3.0, 60.0, 48_000);
            lim.process(&mut x, channels);

            let mut reduced_frames = 0;
            for f in 0..frames {
                let gains: Vec<f32> = (0..channels)
                    .map(|c| x[f * channels + c] / input[f * channels + c])
                    .collect();
                if gains[0] < 0.999 {
                    reduced_frames += 1;
                }
                for g in &gains {
                    assert!(
                        (g - gains[0]).abs() < 1e-5 * gains[0].max(1e-3),
                        "{channels} ch seed {seed} frame {f}: channels got different gains {gains:?}"
                    );
                }
            }
            assert!(reduced_frames > 100, "the test never made the limiter work");
        }
    }
}

#[test]
fn a_hard_left_transient_turns_the_right_channel_down_with_it() {
    let mut lim = Limiter::new(-6.0, 80.0, 48_000);
    let ceiling = lim.ceiling();
    // L: 1.0 (twice the ceiling). R: 0.1, a quiet voice that must keep
    // its place relative to the left channel.
    let mut frame = [1.0f32, 0.1];
    lim.process(&mut frame, 2);
    let gain = ceiling / 1.0;
    assert!((frame[0] - ceiling).abs() < 1e-6);
    assert!(
        (frame[1] - 0.1 * gain).abs() < 1e-6,
        "right channel came out {}, expected {} — a per-channel limiter leaves it at 0.1",
        frame[1],
        0.1 * gain
    );
}

// ---- what the old limiter got wrong ----------------------------------

/// The point of the change, in numbers.
///
/// A 1 kHz sine 6 dB over the ceiling. Hard-clipped it turns into a
/// near-square wave with strong odd harmonics. Limited, the same sine
/// is a quieter sine: the gain settles and barely moves within a cycle.
///
/// Measured twice: over the steady state, once the limiter has settled
/// (the 100 ms after the first over are excluded; a limiter with no
/// lookahead does clamp the first quarter-cycle on its way in), and
/// over the whole second including that entry.
#[test]
fn a_sine_six_db_over_the_ceiling_is_far_cleaner_than_a_hard_clip() {
    let sample_rate = 48_000u32;
    let ceiling_db = -6.0f32;
    let ceiling = db_to_linear(ceiling_db);
    let amplitude = ceiling * db_to_linear(6.0);

    let input = sine(1_000.0, amplitude, sample_rate, 48_000);

    let mut clipped = input.clone();
    hard_clip(&mut clipped, ceiling);

    let mut limited = input.clone();
    Limiter::new(ceiling_db, DEFAULT_RELEASE_MS, sample_rate).process(&mut limited, 1);

    // 1 kHz at 48 kHz is exactly 48 samples a cycle, so these windows
    // are whole numbers of cycles.
    let settled = 4_800..48_000;
    let whole = 0..48_000;
    let sr = sample_rate as f32;

    let clip_steady = thd(&clipped[settled.clone()], sr, 1_000.0);
    let lim_steady = thd(&limited[settled], sr, 1_000.0);
    let clip_whole = thd(&clipped[whole.clone()], sr, 1_000.0);
    let lim_whole = thd(&limited[whole], sr, 1_000.0);

    eprintln!(
        "1 kHz, +6 dB over a {ceiling_db} dBFS ceiling, {DEFAULT_RELEASE_MS} ms release:\n  \
         steady state  THD: hard clip {:.3}% ({:.1} dB)   limiter {:.4}% ({:.1} dB)   \
         reduction {:.0}x ({:.1} dB)\n  \
         whole second  THD: hard clip {:.3}% ({:.1} dB)   limiter {:.4}% ({:.1} dB)   \
         reduction {:.0}x ({:.1} dB)",
        clip_steady * 100.0,
        20.0 * clip_steady.log10(),
        lim_steady * 100.0,
        20.0 * lim_steady.log10(),
        clip_steady / lim_steady,
        20.0 * (clip_steady / lim_steady).log10(),
        clip_whole * 100.0,
        20.0 * clip_whole.log10(),
        lim_whole * 100.0,
        20.0 * lim_whole.log10(),
        clip_whole / lim_whole,
        20.0 * (clip_whole / lim_whole).log10(),
    );

    // The ceiling held for both, so this is not a trade of one for the other.
    assert!(limited.iter().all(|s| s.abs() <= ceiling));

    // A hard clip of a sine at twice the ceiling is badly distorted.
    assert!(
        clip_steady > 0.15,
        "the yardstick is off: hard clip THD is only {clip_steady}"
    );
    assert!(
        lim_steady * 50.0 < clip_steady,
        "limiter THD {lim_steady} is not 50x below the clip's {clip_steady}"
    );
    assert!(
        lim_whole * 50.0 < clip_whole,
        "limiter THD {lim_whole} is not 50x below the clip's {clip_whole}, entry included"
    );
}

/// The case that is hardest on a limiter: low frequencies, where the
/// gain has less time to settle within a cycle. It is still far below
/// a clip, which is the comparison that matters.
#[test]
fn bass_is_cleaner_than_a_hard_clip_too() {
    let sample_rate = 48_000u32;
    let ceiling = db_to_linear(-1.0);
    let input = sine(100.0, ceiling * db_to_linear(6.0), sample_rate, 48_000);

    let mut clipped = input.clone();
    hard_clip(&mut clipped, ceiling);
    let mut limited = input;
    Limiter::new(-1.0, DEFAULT_RELEASE_MS, sample_rate).process(&mut limited, 1);

    // 100 Hz is 480 samples a cycle; skip the first 0.5 s, keep 50 cycles.
    let sr = sample_rate as f32;
    let clip = thd(&clipped[24_000..48_000], sr, 100.0);
    let lim = thd(&limited[24_000..48_000], sr, 100.0);
    eprintln!(
        "100 Hz, +6 dB over a -1 dBFS ceiling, {DEFAULT_RELEASE_MS} ms release: \
         steady THD hard clip {:.3}%, limiter {:.4}%, reduction {:.0}x",
        clip * 100.0,
        lim * 100.0,
        clip / lim
    );
    // Bass is the limiter's weak spot: 10 ms cycles against an 80 ms
    // release leave a visible ripple in the gain. Still an order of
    // magnitude under the clip.
    assert!(lim * 10.0 < clip, "limiter THD {lim} vs clip {clip}");
}

/// Distortion that does not stay in the harmonics: a loud bass note
/// with something quiet and high above it, as in any mix. Clipping the
/// bass chops the high tone every time the sum crosses the ceiling,
/// spraying sidebands around it; a limiter only turns the pair down.
#[test]
fn a_clipped_bass_note_smears_the_tone_above_it_but_a_limiter_does_not() {
    let sample_rate = 48_000u32;
    let ceiling = db_to_linear(-3.0);
    let bass = sine(100.0, ceiling * 1.6, sample_rate, 48_000);
    let tone = sine(5_000.0, 0.05, sample_rate, 48_000);
    let input: Vec<f32> = bass.iter().zip(&tone).map(|(a, b)| a + b).collect();

    let mut clipped = input.clone();
    hard_clip(&mut clipped, ceiling);
    let mut limited = input;
    Limiter::new(-3.0, DEFAULT_RELEASE_MS, sample_rate).process(&mut limited, 1);

    // Skip the entry; 24000..48000 holds 50 cycles of 100 Hz and of
    // 5 kHz. Sidebands at 5 kHz +/- 100 Hz and +/- 200 Hz, relative to
    // the tone itself.
    let sr = sample_rate as f32;
    let sidebands = |x: &[f32]| -> f64 {
        let w = &x[24_000..48_000];
        let side: f64 = [4_800.0f32, 4_900.0, 5_100.0, 5_200.0]
            .iter()
            .map(|&f| tone_amplitude(w, sr, f).powi(2))
            .sum();
        side.sqrt() / tone_amplitude(w, sr, 5_000.0)
    };
    let (clip, lim) = (sidebands(&clipped), sidebands(&limited));
    eprintln!(
        "5 kHz tone over a clipped 100 Hz bass: sideband level re the tone: \
         hard clip {:.2}% ({:.1} dB), limiter {:.3}% ({:.1} dB), reduction {:.0}x",
        clip * 100.0,
        20.0 * clip.log10(),
        lim * 100.0,
        20.0 * lim.log10(),
        clip / lim
    );
    assert!(lim * 10.0 < clip, "limiter sidebands {lim} vs clip {clip}");
}

// ---- parameters ------------------------------------------------------

#[test]
fn the_defaults_are_inside_the_allowed_range() {
    assert!((MIN_RELEASE_MS..=MAX_RELEASE_MS).contains(&DEFAULT_RELEASE_MS));
    // The range the issue asked the default to sit in.
    assert!((50.0..=100.0).contains(&DEFAULT_RELEASE_MS));
}
