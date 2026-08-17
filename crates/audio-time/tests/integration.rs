//! Contract tests for the time-stretch / pitch-shift primitives.
//!
//! These used to assert that valid arguments produced
//! `Err(NotImplemented)` — the stub's contract, and the reason the tools
//! above this crate spent so long reporting a change the audio never
//! received. Valid arguments now produce audio, so the argument
//! validation is checked alongside what the functions actually return.
//!
//! Signal-quality assertions (frequency accuracy, length exactness) live
//! next to the implementation in `src/`, where a failure points at the
//! line responsible.

use audio_time::{pitch_shift, shift, time_stretch, Error};

/// One second of a 440 Hz tone at 48 kHz.
fn tone() -> Vec<f32> {
    (0..48_000)
        .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 48_000.0).sin() * 0.5)
        .collect()
}

#[test]
fn time_stretch_validates_factor() {
    let samples = tone();

    // Valid arguments produce audio of the promised length.
    let out = time_stretch(&samples, 48_000, 1, 0.5, false).expect("valid factor");
    assert_eq!(out.len(), samples.len() * 2, "factor 0.5 is twice as long");

    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            matches!(
                time_stretch(&samples, 48_000, 1, bad, false),
                Err(Error::InvalidFactor(_))
            ),
            "factor {bad} should be rejected"
        );
    }
}

#[test]
fn pitch_shift_validates_semitones() {
    let samples = tone();

    let out = pitch_shift(&samples, 48_000, 1, 12.0, false).expect("valid semitones");
    assert_eq!(out.len(), samples.len(), "pitch shift preserves duration");

    for bad in [60.0, -60.0, f32::NAN] {
        assert!(
            matches!(
                pitch_shift(&samples, 48_000, 1, bad, false),
                Err(Error::InvalidSemitones(_))
            ),
            "{bad} semitones should be rejected"
        );
    }

    // Exactly at the boundary is accepted.
    assert!(pitch_shift(&samples, 48_000, 1, shift::MAX_SEMITONES, false).is_ok());
    assert!(pitch_shift(&samples, 48_000, 1, -shift::MAX_SEMITONES, false).is_ok());
}

#[test]
fn channel_mismatch_surfaces_distinct_error() {
    // 2 channels, 5 samples → not divisible.
    assert!(matches!(
        time_stretch(&[0.0; 5], 48_000, 2, 1.0, false),
        Err(Error::ChannelMismatch(_))
    ));
    // 0 channels.
    assert!(matches!(
        pitch_shift(&[0.0; 4], 48_000, 0, 0.0, false),
        Err(Error::ChannelMismatch(_))
    ));
}

/// `preserve_formants` does nothing for `time_stretch`, and that is the
/// answer rather than an omission.
///
/// A time stretch changes the timeline and leaves every frequency where
/// it was. Formants are frequencies, so nothing has moved and there is
/// nothing to put back. The flag stays in the signature because it is
/// part of the tool schema and because `pitch_shift` — which does move
/// them — shares it.
#[test]
fn preserve_formants_is_a_no_op_for_time_stretch() {
    let samples = tone();

    let with = time_stretch(&samples, 48_000, 1, 1.5, true).expect("flag accepted");
    let without = time_stretch(&samples, 48_000, 1, 1.5, false).expect("flag accepted");
    assert_eq!(
        with, without,
        "a stretch moves no frequency, so there is no envelope to correct"
    );
}

// ---------------------------------------------------------------------------
// Formant preservation (#104)
// ---------------------------------------------------------------------------

const SR: u32 = 44_100;

/// A synthetic vowel: a harmonic series at `f0`, shaped by two
/// resonances at fixed frequencies.
///
/// This is what makes the test meaningful. A bare sine has no envelope
/// to preserve — its only peak *is* the fundamental, so shifting it
/// moves the "formant" by definition and the flag could do nothing and
/// still look correct. The resonances here are properties of the
/// imaginary vocal tract, not of the note.
fn vowel(f0: f32, formants: [f32; 2], n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; n];
    let mut h = 1;
    while f0 * h as f32 * 2.0 < SR as f32 {
        let freq = f0 * h as f32;
        // Two resonances, each a simple bell in frequency.
        let mut amp = 0.02f32;
        for centre in formants {
            let bw = 180.0;
            amp += 1.0 / (1.0 + ((freq - centre) / bw).powi(2));
        }
        // Roll off the top so the series is band-limited rather than
        // ending on a cliff.
        amp *= (-freq / 6_000.0).exp();
        for (i, v) in out.iter_mut().enumerate() {
            *v += amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin();
        }
        h += 1;
    }
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        for v in out.iter_mut() {
            *v *= 0.5 / peak;
        }
    }
    out
}

/// Smoothed log-magnitude envelope of the steady middle, and the bin
/// width it is sampled at.
///
/// Smoothing has to be wider than the harmonic spacing or the result
/// tracks individual harmonics instead of the resonances — a 120 Hz
/// window on a 150 Hz fundamental returned two adjacent harmonics of the
/// *first* formant and reported them as both formants.
fn envelope(x: &[f32]) -> (Vec<f32>, f32) {
    const N: usize = 8192;
    let mid = x.len() / 2;
    let seg = &x[mid - N / 2..mid + N / 2];
    let mut input: Vec<f32> = seg
        .iter()
        .enumerate()
        .map(|(i, v)| v * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()))
        .collect();

    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(N);
    let mut spectrum = fwd.make_output_vec();
    fwd.process(&mut input, &mut spectrum).expect("fft");

    let hz_per_bin = SR as f32 / N as f32;
    let half = ((400.0 / hz_per_bin) as usize).max(1);
    let mags: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();
    let env: Vec<f32> = (0..mags.len())
        .map(|k| {
            let lo = k.saturating_sub(half);
            let hi = (k + half + 1).min(mags.len());
            let mean = mags[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
            (mean + 1e-9).ln()
        })
        .collect();
    (env, hz_per_bin)
}

/// How far along the frequency axis `after`'s envelope has been stretched
/// relative to `before`'s.
///
/// Rather than picking peaks and pairing them — which is brittle, and
/// was the first thing I tried — this searches for the scale `s` that
/// best aligns `after(f)` with `before(f / s)`. A shift that drags the
/// resonances with it gives `s` near the pitch ratio; a shift that holds
/// them gives `s` near 1. One number, and it uses the whole envelope
/// rather than two points of it.
fn envelope_scale(before: &[f32], after: &[f32], hz_per_bin: f32) -> f32 {
    let lo = (300.0 / hz_per_bin) as usize;
    let hi = ((6_000.0 / hz_per_bin) as usize).min(before.len().min(after.len()) - 1);

    let mut best = (f32::NEG_INFINITY, 1.0f32);
    let mut s = 0.5f32;
    while s <= 3.0 {
        // Correlate the two over the band, sampling `before` at f / s.
        let mut sum = 0.0f64;
        let mut sa = 0.0f64;
        let mut sb = 0.0f64;
        let mut n = 0usize;
        for (k, &av) in after.iter().enumerate().take(hi).skip(lo) {
            let src = (k as f32 / s) as usize;
            if src >= before.len() {
                continue;
            }
            let (a, b) = (av as f64, before[src] as f64);
            sum += a * b;
            sa += a * a;
            sb += b * b;
            n += 1;
        }
        if n > 32 && sa > 0.0 && sb > 0.0 {
            let c = (sum / (sa.sqrt() * sb.sqrt())) as f32;
            if c > best.0 {
                best = (c, s);
            }
        }
        s += 0.01;
    }
    best.1
}

/// The acceptance criterion, measured rather than described.
///
/// Shift a vowel up a fifth. Without the flag the resonances travel with
/// the harmonics — that is the chipmunk. With it they stay put.
///
/// Seven semitones rather than twelve, and that is worth saying out
/// loud. At a ratio of 2.0 the vocoder's own phasiness was severe enough
/// that the envelope did not survive the shift *at all*: measured on
/// this fixture before #96, the uncorrected path reported a scale of
/// 0.71 where the shift itself is 2.0 — the envelope moving *down* while
/// the pitch went up. So +12 was a vocoder problem, not a formant
/// problem.
///
/// With phase locking the uncorrected path reads 1.42 against a true
/// 1.50, so the shift is finally coherent enough to measure through.
/// That also changes what the numbers below mean: the old absolute band
/// was calibrated in the broken regime, where the envelope was noise.
/// The claim worth pinning is the one that does not depend on the
/// vocoder's accuracy — how much of the travel the correction removes.
#[test]
fn preserve_formants_holds_the_resonances_while_the_pitch_moves() {
    let input = vowel(150.0, [700.0, 1_800.0], SR as usize);
    let plain = pitch_shift(&input, SR, 1, 7.0, false).expect("shift");
    let kept = pitch_shift(&input, SR, 1, 7.0, true).expect("shift");

    let (env_in, hz) = envelope(&input);
    let (env_plain, _) = envelope(&plain);
    let (env_kept, _) = envelope(&kept);

    let moved = envelope_scale(&env_in, &env_plain, hz);
    let held = envelope_scale(&env_in, &env_kept, hz);

    // A fifth is a ratio of 1.498.
    assert!(
        moved > 1.2,
        "without preservation the envelope should travel with the pitch; \
         it scaled by {moved:.2}x for a 1.50x shift"
    );
    assert!(
        held < moved * 0.85,
        "preservation must hold the envelope back: {held:.2}x with it \
         against {moved:.2}x without"
    );
    // Most of the way back, not merely "less than without". Measured on
    // this fixture: 1.42 uncorrected against 1.17 corrected, so 60% of
    // the travel removed. Scale-free, so it stays meaningful if the
    // vocoder's own accuracy moves again.
    assert!(
        (held - 1.0) < (moved - 1.0) * 0.6,
        "preservation should remove most of the envelope's travel, not \
         a sliver: {held:.2}x with it against {moved:.2}x without"
    );
    assert!(
        (0.8..=1.25).contains(&held),
        "with preservation the envelope should stay near where it was; \
         it scaled by {held:.2}x"
    );
}

/// And the pitch must still move — a "formant preservation" that worked
/// by not shifting at all would pass the test above.
#[test]
fn preserving_formants_still_shifts_the_pitch() {
    let input = vowel(150.0, [700.0, 1_800.0], SR as usize);
    let kept = pitch_shift(&input, SR, 1, 7.0, true).expect("shift");

    // Harmonic spacing, read off the spectrum: the distance between
    // adjacent partials *is* the fundamental, and it survives the
    // envelope correction (which only rescales magnitudes) in a way a
    // single-peak estimate does not.
    let spacing = |x: &[f32]| -> f32 {
        const N: usize = 8192;
        let mid = x.len() / 2;
        let mut input: Vec<f32> = x[mid - N / 2..mid + N / 2]
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos())
            })
            .collect();
        let mut planner = realfft::RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(N);
        let mut spectrum = fwd.make_output_vec();
        fwd.process(&mut input, &mut spectrum).expect("fft");
        let hz = SR as f32 / N as f32;
        let mags: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();
        let hi = (4_000.0 / hz) as usize;
        let peak = mags[..hi].iter().fold(0.0f32, |m, v| m.max(*v));
        let mut last: Option<usize> = None;
        let mut gaps: Vec<f32> = Vec::new();
        for k in 1..hi - 1 {
            if mags[k] > peak * 0.15 && mags[k] > mags[k - 1] && mags[k] > mags[k + 1] {
                if let Some(p) = last {
                    gaps.push((k - p) as f32 * hz);
                }
                last = Some(k);
            }
        }
        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        gaps[gaps.len() / 2]
    };

    let f_in = spacing(&input);
    let f_out = spacing(&kept);
    let ratio = f_out / f_in;
    assert!(
        (1.35..=1.65).contains(&ratio),
        "a fifth up should raise the harmonic spacing by ~1.50x; \
         {f_in:.0} Hz -> {f_out:.0} Hz is {ratio:.2}x"
    );
}

// ---------------------------------------------------------------------------
// warp_to_grid (#97)
// ---------------------------------------------------------------------------

/// A click every `period` samples, each a short decaying burst so it has
/// a findable onset rather than being one sample wide.
fn click_track(period: usize, count: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; period * count + period];
    for c in 0..count {
        let start = c * period;
        for i in 0..(SR as usize / 200) {
            let t = i as f32 / SR as f32;
            let env = (-t * 600.0).exp();
            let s = (2.0 * std::f32::consts::PI * 3_000.0 * t).sin() * env;
            if start + i < out.len() {
                out[start + i] = s;
            }
        }
    }
    out
}

/// Sample positions of onsets, found by a simple threshold with a
/// refractory gap.
fn onsets(x: &[f32], threshold_frac: f32) -> Vec<usize> {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let thr = peak * threshold_frac;
    let refractory = SR as usize / 20;
    let mut hits = Vec::new();
    let mut cooldown = 0usize;
    for (i, &v) in x.iter().enumerate() {
        if cooldown > 0 {
            cooldown -= 1;
            continue;
        }
        if v.abs() > thr {
            hits.push(i);
            cooldown = refractory;
        }
    }
    hits
}

/// The acceptance criterion: 120 BPM warped to 100 BPM, every click
/// within 10 ms of where the target grid says it should be.
///
/// The track has a lead-in rather than starting on beat one at sample 0.
/// That is what real audio looks like, and it also exercises the
/// head-scaling half of the output-length calculation, which a grid
/// starting at zero would skip entirely. A transient at sample 0 is
/// separately known not to survive the vocoder — it has one effective
/// analysis window behind it — which is documented on #96 and is not
/// what this test is about.
#[test]
fn a_click_track_lands_on_the_target_grid() {
    let src_period = (SR as f32 * 60.0 / 120.0) as usize; // 120 BPM
    let tgt_period = (SR as f32 * 60.0 / 100.0) as usize; // 100 BPM
    let beats = 8;
    let lead = src_period / 2;
    let mut input = vec![0.0f32; lead];
    input.extend_from_slice(&click_track(src_period, beats));

    let source: Vec<u64> = (0..beats).map(|i| (lead + i * src_period) as u64).collect();
    // The lead-in is scaled by the first segment's ratio, so beat one
    // does not land at `lead` in the output.
    let ratio = tgt_period as f32 / src_period as f32;
    let out_lead = (lead as f32 * ratio).round() as usize;
    let target: Vec<u64> = (0..beats)
        .map(|i| (out_lead + i * tgt_period) as u64)
        .collect();

    let out = audio_time::warp_to_grid(&input, SR, 1, &source, &target).expect("warp");

    let found = onsets(&out, 0.3);
    assert_eq!(
        found.len(),
        beats,
        "expected {beats} clicks, found {} at {found:?}",
        found.len()
    );

    let tolerance = (SR as f32 * 0.010) as usize; // 10 ms
    for (i, &got) in found.iter().enumerate() {
        let want = target[i] as usize;
        let err = got.abs_diff(want);
        assert!(
            err <= tolerance,
            "click {i} landed at {got} ({:.1} ms), target {want} ({:.1} ms) — \
             off by {:.1} ms",
            got as f32 * 1000.0 / SR as f32,
            want as f32 * 1000.0 / SR as f32,
            err as f32 * 1000.0 / SR as f32,
        );
    }
}

/// The seam test. A per-segment stretch would put a discontinuity at
/// every beat; a single pass with a varying hop must not.
///
/// The threshold is set from the *input's* own largest step, so this
/// asks "does the warp introduce a jump the source did not have" rather
/// than picking an absolute number that happens to pass.
#[test]
fn no_discontinuity_at_segment_boundaries() {
    // A sustained tone rather than clicks: a click track's own attacks
    // are large sample-to-sample steps and would mask a seam.
    let n = SR as usize * 2;
    let input: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * 220.0 * i as f32 / SR as f32).sin() * 0.5)
        .collect();

    // Beats every half second, warped to a grid that speeds up: each
    // segment gets a different ratio, so there are seven boundaries.
    let source: Vec<u64> = (0..8).map(|i| (i * SR as usize / 4) as u64).collect();
    let target: Vec<u64> = (0..8)
        .map(|i| {
            // Progressively tighter intervals.
            let mut acc = 0usize;
            for j in 0..i {
                acc += SR as usize / 4 - j * 400;
            }
            acc as u64
        })
        .collect();

    let out = audio_time::warp_to_grid(&input, SR, 1, &source, &target).expect("warp");

    let max_step = |x: &[f32]| {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    };
    let in_step = max_step(&input);
    let out_step = max_step(&out);
    assert!(
        out_step < in_step * 3.0,
        "the warp introduced a jump of {out_step:.4} where the input's \
         largest step is {in_step:.4} — that is a seam"
    );
}

/// Pitch must survive the warp; it is a time operation.
#[test]
fn warping_does_not_move_the_pitch() {
    let n = SR as usize * 2;
    let input: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SR as f32).sin() * 0.5)
        .collect();
    let source: Vec<u64> = (0..5).map(|i| (i * SR as usize / 4) as u64).collect();
    let target: Vec<u64> = (0..5).map(|i| (i * SR as usize / 3) as u64).collect();

    let out = audio_time::warp_to_grid(&input, SR, 1, &source, &target).expect("warp");

    // Zero crossings over the steady middle.
    let est = |x: &[f32]| {
        let s = &x[x.len() / 4..x.len() * 3 / 4];
        let crossings = s
            .windows(2)
            .filter(|w| (w[0] <= 0.0 && w[1] > 0.0) || (w[0] >= 0.0 && w[1] < 0.0))
            .count();
        (crossings as f32 / 2.0) * SR as f32 / s.len() as f32
    };
    let f = est(&out);
    assert!(
        (f - 440.0).abs() < 20.0,
        "warping must not move the pitch; got {f} Hz"
    );
}

/// Stereo stays frame-aligned and both channels get the same schedule.
#[test]
fn warping_keeps_stereo_frame_aligned() {
    let n = SR as usize;
    let mono: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * 330.0 * i as f32 / SR as f32).sin() * 0.4)
        .collect();
    let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s * 0.5]).collect();
    let source: Vec<u64> = (0..4).map(|i| (i * SR as usize / 5) as u64).collect();
    let target: Vec<u64> = (0..4).map(|i| (i * SR as usize / 4) as u64).collect();

    let out = audio_time::warp_to_grid(&stereo, SR, 2, &source, &target).expect("warp");
    assert_eq!(out.len() % 2, 0, "stereo output must be frame-aligned");
    assert!(!out.is_empty());
}
