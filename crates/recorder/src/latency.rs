//! Round-trip latency: the sweep, finding it again, and the clock model
//! that turns a time on the monitor into a frame of the capture (#203).
//!
//! ## The model
//!
//! The monitor is one buffer played from frame 0, and the capture is one
//! buffer recorded from its own frame 0. Neither stream's timestamps can
//! be compared with the other's: cpal's `StreamInstant` counts from an
//! origin of that stream's own (on ALSA, its trigger or its creation).
//! What the two streams do share is the process clock, so each callback
//! reads `Instant::now()` and reports it with its frame count.
//!
//! - Monitor frame `k` is *handed to the device* at `P0 + k/sr_out`. `P0`
//!   is the lower envelope of `t_callback - k_callback/sr_out`:
//!   scheduling can only make a callback late, so the minimum is the
//!   least-late estimate. The same for the capture, with `C0`, over
//!   `t_callback - (j_callback + n)/sr_in` (a buffer is complete when the
//!   callback runs).
//! - Everything that is not scheduling (buffer depth in the device and the
//!   driver, converters, USB or Bluetooth, the air between speaker and
//!   microphone) is one constant `L` for a device pair. Calibration
//!   measures it; nothing here guesses it.
//! - Session time `s` on the monitor is therefore captured at input frame
//!   `(s + (P0 - C0) + L) * sr_in` ([`input_frame_for`]).
//!
//! The envelope's bias (the minimum is not the true origin, only the least
//! late one) is the same in a calibration and in a take, so it cancels in
//! `L`: both are measured through the same estimator.

use std::sync::atomic::{AtomicI64, Ordering};

use realfft::RealFftPlanner;

pub const SWEEP_START_HZ: f64 = 200.0;
pub const SWEEP_END_HZ: f64 = 8_000.0;
pub const SWEEP_SEC: f64 = 0.1;
/// Peak level of the sweep. Half scale leaves headroom for a device that
/// adds gain, and the user's own volume sets how loud it is.
const SWEEP_AMPLITUDE: f64 = 0.5;
/// Raised-cosine edges, so the sweep starts and stops without a click.
const EDGE_SEC: f64 = 0.005;

/// A correlation peak must stand this many times above the correlation
/// elsewhere to count as a sweep. Noise alone peaks at about 4 to 6 times
/// its own RMS over a capture of this length; a sweep 0 dB under the
/// noise is far above this, because 4,800 samples of it add up.
pub const MIN_CONFIDENCE: f32 = 12.0;

/// How far around a peak the confidence leaves out of its noise figure.
const PEAK_EXCLUDE_SEC: f64 = 0.005;
/// Two sweeps are never closer than this, so anything nearer a found peak
/// is that peak's own side lobe.
const PEAK_GUARD_SEC: f64 = SWEEP_SEC / 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// Where the reference starts in the capture, in frames, to a
    /// fraction of one.
    pub position: f64,
    /// `position` rounded to a frame.
    pub frame: usize,
    /// Peak height over the correlation's RMS away from the peak.
    pub confidence: f32,
}

fn taper(t: f64) -> f64 {
    if !(0.0..=SWEEP_SEC).contains(&t) {
        return 0.0;
    }
    let edge = |x: f64| 0.5 * (1.0 - (std::f64::consts::PI * x / EDGE_SEC).cos());
    if t < EDGE_SEC {
        edge(t)
    } else if t > SWEEP_SEC - EDGE_SEC {
        edge(SWEEP_SEC - t)
    } else {
        1.0
    }
}

/// The sweep at time `t` seconds after its start: exponential, from
/// [`SWEEP_START_HZ`] to [`SWEEP_END_HZ`] over [`SWEEP_SEC`].
fn sweep_at(t: f64) -> f64 {
    let k = (SWEEP_END_HZ / SWEEP_START_HZ).ln();
    let phase =
        std::f64::consts::TAU * SWEEP_START_HZ * SWEEP_SEC / k * ((t / SWEEP_SEC * k).exp() - 1.0);
    SWEEP_AMPLITUDE * taper(t) * phase.sin()
}

/// A 100 ms exponential sweep from 200 Hz to 8 kHz with tapered edges,
/// sampled at `sample_rate`. It is a function of time, so the copy played
/// at one rate and the reference correlated at another are the same
/// waveform.
pub fn sweep(sample_rate: u32) -> Vec<f32> {
    let n = (SWEEP_SEC * sample_rate as f64).round() as usize;
    (0..n)
        .map(|i| sweep_at(i as f64 / sample_rate as f64) as f32)
        .collect()
}

/// The sweep arriving `delay_frames` (a fraction is fine) after frame 0,
/// in a buffer of `len` frames.
#[cfg(test)]
pub(crate) fn sweep_delayed(sample_rate: u32, delay_frames: f64, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| sweep_at((i as f64 - delay_frames) / sample_rate as f64) as f32)
        .collect()
}

/// `capture` against `reference`: entry `k` is how much of the reference
/// sits at frame `k`, scaled so a unit-gain copy gives 1. Done as one FFT
/// product; empty when the capture is shorter than the reference.
fn correlate(reference: &[f32], capture: &[f32]) -> Vec<f64> {
    let (m, n) = (reference.len(), capture.len());
    if m == 0 || n < m {
        return Vec::new();
    }
    let energy: f64 = reference.iter().map(|v| (*v as f64).powi(2)).sum();
    if energy == 0.0 {
        return Vec::new();
    }
    // A DC offset leaks into every lag; take it out first.
    let mean = capture.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
    let size = (n + m).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);

    let mut x = forward.make_input_vec();
    for (d, s) in x.iter_mut().zip(capture) {
        *d = *s as f64 - mean;
    }
    let mut r = forward.make_input_vec();
    for (d, s) in r.iter_mut().zip(reference) {
        *d = *s as f64;
    }
    let mut xs = forward.make_output_vec();
    let mut rs = forward.make_output_vec();
    forward.process(&mut x, &mut xs).expect("fft sizes agree");
    forward.process(&mut r, &mut rs).expect("fft sizes agree");
    for (a, b) in xs.iter_mut().zip(&rs) {
        *a *= b.conj();
    }
    let mut out = inverse.make_output_vec();
    inverse.process(&mut xs, &mut out).expect("fft sizes agree");
    let scale = 1.0 / (size as f64 * energy);
    out.truncate(n - m + 1);
    for v in out.iter_mut() {
        *v *= scale;
    }
    out
}

/// Peaks of `|corr|` at least [`MIN_CONFIDENCE`] over the noise, strongest
/// first, no two within [`PEAK_GUARD_SEC`] of each other.
fn peaks(corr: &[f64], sample_rate: u32, max: usize) -> Vec<Located> {
    if corr.is_empty() || max == 0 {
        return Vec::new();
    }
    let abs: Vec<f64> = corr.iter().map(|v| v.abs()).collect();
    let total: f64 = abs.iter().map(|v| v * v).sum();
    let exclude = (PEAK_EXCLUDE_SEC * sample_rate as f64) as usize;
    let guard = (PEAK_GUARD_SEC * sample_rate as f64) as usize;
    let mut taken = vec![false; abs.len()];
    let mut found = Vec::new();

    while found.len() < max {
        let Some((p, &height)) = abs
            .iter()
            .enumerate()
            .filter(|(i, _)| !taken[*i])
            .max_by(|a, b| a.1.total_cmp(b.1))
        else {
            break;
        };
        if height <= 1e-9 {
            break;
        }
        let lo = p.saturating_sub(exclude);
        let hi = (p + exclude + 1).min(abs.len());
        let inside: f64 = abs[lo..hi].iter().map(|v| v * v).sum();
        let outside_n = abs.len() - (hi - lo);
        let rms_out = if outside_n == 0 {
            0.0
        } else {
            ((total - inside).max(0.0) / outside_n as f64).sqrt()
        };
        let confidence = (height / rms_out.max(1e-12)) as f32;
        if confidence < MIN_CONFIDENCE {
            break;
        }

        // Parabola through the peak and its neighbours, for a position
        // finer than a frame. Without it a calibration cannot tell a
        // steady latency from one that rounds differently each run.
        // At either end of the capture there is no neighbour to fit.
        let shift = if p > 0 && p + 1 < abs.len() {
            let (a, b, c) = (abs[p - 1], height, abs[p + 1]);
            let denom = a - 2.0 * b + c;
            if denom.abs() > 1e-12 {
                (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let position = p as f64 + shift;
        found.push(Located {
            position,
            frame: position.round().max(0.0) as usize,
            confidence,
        });

        let from = p.saturating_sub(guard);
        let to = (p + guard + 1).min(abs.len());
        for t in &mut taken[from..to] {
            *t = true;
        }
    }
    found
}

/// The strongest sweep in `capture`, or `None` when nothing stands clear
/// of the noise by [`MIN_CONFIDENCE`].
pub fn locate(reference: &[f32], capture: &[f32], sample_rate: u32) -> Option<Located> {
    peaks(&correlate(reference, capture), sample_rate, 1)
        .into_iter()
        .next()
}

/// Every sweep in `capture`, up to `max`, in order of position.
pub fn locate_all(
    reference: &[f32],
    capture: &[f32],
    sample_rate: u32,
    max: usize,
) -> Vec<Located> {
    let mut all = peaks(&correlate(reference, capture), sample_rate, max);
    all.sort_by(|a, b| a.position.total_cmp(&b.position));
    all
}

/// The capture frame at which the monitor's `rel_sec` is heard.
///
/// `skew_ns` is `P0 - C0` (see the module docs) and `latency_sec` the
/// device pair's constant. Negative when that moment is before the capture
/// began: the caller fills those frames with silence, not with something
/// wrapped around.
pub fn input_frame_for(rel_sec: f64, skew_ns: i64, latency_sec: f64, in_sr: u32) -> i64 {
    ((rel_sec + skew_ns as f64 * 1e-9 + latency_sec) * in_sr as f64).round() as i64
}

/// The lower envelope of `t_callback - frame/sr` over a stream's
/// callbacks: the stream's origin on the process clock, as far as
/// scheduling lets us see it.
///
/// Written by one callback thread and read after the stream is dropped,
/// with atomics and no lock: a callback that blocks drops frames, and a
/// dropped frame shifts every frame after it.
pub struct Envelope {
    sr: u32,
    first_ns: AtomicI64,
    min_ns: AtomicI64,
}

impl Envelope {
    /// A device asks for its first buffers back to back to fill its own
    /// queue, and those callbacks run long before the steady schedule
    /// puts them. They would pull the minimum down for good.
    pub const WARMUP_NS: i64 = 250_000_000;

    pub fn new(sr: u32) -> Self {
        Self {
            sr,
            first_ns: AtomicI64::new(i64::MIN),
            min_ns: AtomicI64::new(i64::MAX),
        }
    }

    /// One callback: `t_cb_ns` on the process clock, and the stream
    /// frame the callback is about (the first frame of an output buffer,
    /// one past the last frame of an input buffer).
    pub fn observe(&self, t_cb_ns: i64, frame: u64) {
        let first = match self.first_ns.compare_exchange(
            i64::MIN,
            t_cb_ns,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => t_cb_ns,
            Err(first) => first,
        };
        if t_cb_ns - first < Self::WARMUP_NS {
            return;
        }
        let frame_ns = (frame as u128 * 1_000_000_000 / self.sr as u128) as i64;
        self.min_ns.fetch_min(t_cb_ns - frame_ns, Ordering::Relaxed);
    }

    /// The origin, or `None` before any callback past the warm-up.
    pub fn origin_ns(&self) -> Option<i64> {
        match self.min_ns.load(Ordering::Relaxed) {
            i64::MAX => None,
            v => Some(v),
        }
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    /// A small deterministic generator, so a failing seed can be rerun.
    pub struct Rng(pub u64);
    impl Rng {
        pub fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        /// Uniform in [0, 1).
        pub fn uniform(&mut self) -> f64 {
            (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
        }
        /// Roughly standard normal (sum of twelve uniforms).
        pub fn gauss(&mut self) -> f64 {
            (0..12).map(|_| self.uniform()).sum::<f64>() - 6.0
        }
    }

    pub fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    pub fn noise(len: usize, rms_target: f64, seed: u64) -> Vec<f32> {
        let mut r = Rng(seed);
        (0..len).map(|_| (r.gauss() * rms_target) as f32).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    const SR: u32 = 48_000;

    fn capture_with(sweep: &[f32], at: usize, total: usize) -> Vec<f32> {
        let mut c = vec![0.0f32; total];
        for (i, v) in sweep.iter().enumerate() {
            c[at + i] = *v;
        }
        c
    }

    #[test]
    fn the_sweep_is_a_tapered_tenth_of_a_second_inside_its_band() {
        let s = sweep(SR);
        assert_eq!(s.len(), 4_800);
        assert!(s.iter().all(|v| v.abs() <= 0.5 + 1e-6), "headroom");
        assert!(
            s[0].abs() < 1e-3 && s[s.len() - 1].abs() < 1e-3,
            "tapered edges"
        );
        assert!(rms(&s) > 0.1, "it carries energy");
    }

    #[test]
    fn finds_the_sweep_at_exactly_its_frame() {
        let s = sweep(SR);
        for at in [0usize, 1, 1_234, 48_000] {
            let c = capture_with(&s, at, 100_000);
            let hit = locate(&s, &c, SR).unwrap_or_else(|| panic!("not found at {at}"));
            assert_eq!(hit.frame, at, "frame");
            assert!(
                (hit.position - at as f64).abs() < 0.05,
                "position {}",
                hit.position
            );
            assert!(hit.confidence >= MIN_CONFIDENCE);
        }
    }

    #[test]
    fn finds_a_sweep_that_arrives_between_two_frames() {
        let s = sweep(SR);
        let late = sweep_delayed(SR, 1_000.4, 8_000);
        let hit = locate(&s, &capture_with(&late, 0, 20_000), SR).unwrap();
        assert!(
            (hit.position - 1_000.4).abs() < 0.1,
            "position {} should be near 1000.4",
            hit.position
        );
    }

    #[test]
    fn finds_it_quiet() {
        let s = sweep(SR);
        let quiet: Vec<f32> = s.iter().map(|v| v * 0.0316).collect(); // -30 dB
        let c = capture_with(&quiet, 5_000, 60_000);
        assert_eq!(locate(&s, &c, SR).unwrap().frame, 5_000);
    }

    #[test]
    fn finds_it_upside_down() {
        // Some interfaces invert polarity; the delay is the same.
        let s = sweep(SR);
        let inv: Vec<f32> = s.iter().map(|v| -v).collect();
        let c = capture_with(&inv, 7_777, 60_000);
        assert_eq!(locate(&s, &c, SR).unwrap().frame, 7_777);
    }

    #[test]
    fn finds_it_with_a_dc_offset() {
        let s = sweep(SR);
        let mut c = capture_with(&s, 3_000, 60_000);
        for v in c.iter_mut() {
            *v += 0.2;
        }
        assert_eq!(locate(&s, &c, SR).unwrap().frame, 3_000);
    }

    #[test]
    fn finds_it_in_white_noise_at_10_db() {
        let s = sweep(SR);
        let noise_rms = rms(&s) / 10f64.powf(10.0 / 20.0);
        for seed in 1..=20u64 {
            let mut c = capture_with(&s, 9_000, 120_000);
            for (v, n) in c.iter_mut().zip(noise(120_000, noise_rms, seed)) {
                *v += n;
            }
            let hit = locate(&s, &c, SR).unwrap_or_else(|| panic!("seed {seed}: not found"));
            assert_eq!(hit.frame, 9_000, "seed {seed}");
        }
    }

    #[test]
    fn noise_alone_is_not_a_sweep() {
        let s = sweep(SR);
        for seed in 1..=20u64 {
            for level in [0.001, 0.05, 0.3] {
                let c = noise(60_000, level, seed);
                assert!(
                    locate(&s, &c, SR).is_none(),
                    "seed {seed}, level {level}: noise was taken for a sweep"
                );
            }
        }
    }

    #[test]
    fn silence_and_short_captures_are_not_a_sweep() {
        let s = sweep(SR);
        assert!(locate(&s, &vec![0.0; 50_000], SR).is_none());
        assert!(locate(&s, &s[..100], SR).is_none());
        assert!(locate(&s, &[], SR).is_none());
    }

    #[test]
    fn locate_all_returns_every_sweep_in_order() {
        let s = sweep(SR);
        let mut c = vec![0.0f32; 200_000];
        let at = [3_000usize, 25_000, 60_000, 90_001, 140_000];
        for a in at {
            for (i, v) in s.iter().enumerate() {
                c[a + i] += *v;
            }
        }
        let hits = locate_all(&s, &c, SR, 8);
        let frames: Vec<usize> = hits.iter().map(|h| h.frame).collect();
        assert_eq!(frames, at.to_vec());
    }

    #[test]
    fn a_frame_is_placed_by_time_skew_and_latency() {
        // 1 s on the monitor, the output clock 3 ms ahead of the input
        // clock, 40 ms of round trip, 48 kHz in.
        let f = input_frame_for(1.0, 3_000_000, 0.040, SR);
        assert_eq!(f, ((1.0 + 0.003 + 0.040) * 48_000.0f64).round() as i64);
        // Before the capture began it is negative, which the caller
        // zero-fills rather than wraps.
        assert!(input_frame_for(0.0, -10_000_000, 0.0, SR) < 0);
    }

    /// A stream of callbacks whose timing is `origin + k/sr` plus a delay
    /// that can only be late.
    fn callbacks(origin_ns: i64, n: usize, buf: u64, jitter_ms: f64, seed: u64) -> Vec<(i64, u64)> {
        let mut r = Rng(seed);
        (0..n)
            .map(|k| {
                let frame = k as u64 * buf;
                let ideal = origin_ns + (frame as f64 / SR as f64 * 1e9) as i64;
                (ideal + (r.uniform() * jitter_ms * 1e6) as i64, frame)
            })
            .collect()
    }

    #[test]
    fn the_envelope_recovers_the_origin_through_late_callbacks() {
        let origin = 123_456_789i64;
        let env = Envelope::new(SR);
        for (t, f) in callbacks(origin, 600, 480, 3.0, 7) {
            env.observe(t, f);
        }
        let got = env.origin_ns().unwrap();
        let frame_ns = 1e9 / SR as f64;
        assert!(got >= origin, "lateness can only raise the estimate");
        assert!(
            ((got - origin) as f64) < frame_ns,
            "off by {} ns, a frame is {frame_ns}",
            got - origin
        );
    }

    #[test]
    fn the_envelope_ignores_the_burst_at_startup() {
        // A device asks for its first buffers back to back: those
        // callbacks run long before the steady schedule says they should.
        let origin = 1_000_000_000i64;
        let env = Envelope::new(SR);
        for k in 0..4u64 {
            env.observe(origin + k as i64 * 100_000, k * 480);
        }
        for (t, f) in callbacks(origin, 600, 480, 2.0, 11) {
            if f >= 4 * 480 {
                env.observe(t, f);
            }
        }
        let got = env.origin_ns().unwrap();
        assert!(
            (got - origin).abs() < 100_000,
            "the burst pulled the estimate {} ns off",
            got - origin
        );
    }

    #[test]
    fn an_envelope_with_nothing_past_the_warmup_has_no_origin() {
        let env = Envelope::new(SR);
        assert!(env.origin_ns().is_none());
        env.observe(0, 0);
        env.observe(10_000_000, 480);
        assert!(env.origin_ns().is_none(), "only warm-up callbacks so far");
    }
}
