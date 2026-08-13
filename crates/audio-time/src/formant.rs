//! Formant preservation for [`crate::pitch_shift`].
//!
//! ## The problem
//!
//! A pitch shift multiplies every frequency by the same ratio. That is
//! right for the harmonics — they *are* the pitch — and wrong for the
//! resonances of whatever produced the sound. A vocal tract is a tube of
//! a fixed length, so its resonances sit where they sit no matter what
//! note the person sings. Moving them with the harmonics is what makes a
//! shifted voice sound like a chipmunk or a giant rather than the same
//! person singing higher or lower.
//!
//! ## The approach
//!
//! Separate the spectral envelope — the slowly-varying shape, which is
//! the resonances — from the fine structure, which is the harmonics.
//! Shift the fine structure and put the *original* envelope back.
//!
//! This runs as a correction pass over the already-shifted signal rather
//! than inside the vocoder. `pitch_shift` is a composition of a stretch
//! and a resample, and the envelope has to be measured on the input and
//! reapplied to the output; reaching into the middle of that composition
//! would mean teaching both halves about formants. As a pass it needs to
//! know only that the two signals are the same length and the same
//! moments in time — which `pitch_shift` guarantees, since it preserves
//! duration.
//!
//! Per frame, of the original and the shifted signal both:
//!
//! 1. Log-magnitude spectrum.
//! 2. Inverse transform it into the cepstrum. Log-magnitude is real and
//!    even, so this is a real sequence.
//! 3. Keep the low-quefrency coefficients ([`LIFTER_COEFFS`]) and zero
//!    the rest. Slow variation in frequency = low quefrency = envelope.
//! 4. Forward transform back: a smooth log spectrum. Exponentiate.
//!
//! Then scale the shifted frame's magnitudes by
//! `envelope_original / envelope_shifted`, keeping its phases, and
//! overlap-add.
//!
//! Cepstral rather than LPC, as the ticket specifies: it reuses the FFT
//! machinery `vocoder.rs` already sets up, and LPC is a refinement to
//! reach for if voices still sound wrong.

use std::f32::consts::PI;
use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// FFT size for the envelope analysis. Matches the vocoder's, so the
/// two passes see the same time-frequency resolution.
const FRAME: usize = 2048;

/// Hop. 75% overlap, as in the vocoder, so the Hann window sums flat.
const HOP: usize = FRAME / 4;

/// Cepstral coefficients kept as "the envelope".
///
/// Quefrency is time-like: coefficient `q` describes structure repeating
/// every `FRAME / q` bins. The harmonics of a voice at 100 Hz on a
/// 44.1 kHz, 2048-point analysis repeat every ~4.6 bins, which is
/// quefrency ~440. 40 keeps structure no finer than ~50 bins — well
/// above the formant widths (a few hundred Hz, ~10–20 bins) and well
/// below the harmonic spacing of any voice this is for.
///
/// Too high and the "envelope" starts tracking individual harmonics, at
/// which point dividing by it flattens the sound instead of neutralising
/// the shift. Too low and it is a tilt rather than a set of resonances.
const LIFTER_COEFFS: usize = 40;

/// Floor on the envelope, relative to the frame's peak envelope value.
///
/// The correction divides by the shifted signal's envelope. A band with
/// no energy in it — above the source's bandwidth, say, or in a gap —
/// has an envelope near zero there, and dividing by it turns numerical
/// noise into a full-scale band. That is the failure family of the
/// Nyquist bug in #73 and of the overlap-add blowup in #134: a divisor
/// that is legitimately tiny, used without asking whether the numerator
/// means anything at that point.
const ENVELOPE_FLOOR: f32 = 1e-4;

/// Widest correction applied to any one bin.
///
/// The floor above bounds the divisor; this bounds the *ratio*, which is
/// what actually reaches the audio. 4x is far more than any real formant
/// correction needs — a 12-semitone shift moves an envelope by an octave,
/// and the ratio between the two envelopes at a given bin stays well
/// inside this on real material. It exists so that a pathological frame
/// cannot produce a click.
const MAX_CORRECTION: f32 = 4.0;

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
        .collect()
}

struct Fft {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
}

impl Fft {
    fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        Self {
            fwd: planner.plan_fft_forward(FRAME),
            inv: planner.plan_fft_inverse(FRAME),
        }
    }
}

/// Smooth `log_mag` (length `FRAME/2 + 1`) into a spectral envelope,
/// in place.
///
/// `scratch_time` and `scratch_spec` are the transform buffers, passed
/// in so a per-frame loop allocates nothing.
///
/// The round trip is inverse-then-forward rather than the other way
/// round, because the input here is already a spectrum. `realfft`'s
/// inverse is unnormalised, hence the division by `FRAME`; with no
/// liftering at all the pair is the identity, which
/// `an_unliftered_round_trip_is_the_identity` pins.
fn liftered_envelope(
    fft: &Fft,
    log_mag: &mut [f32],
    scratch_spec: &mut [realfft::num_complex::Complex<f32>],
    scratch_time: &mut [f32],
) {
    for (c, &m) in scratch_spec.iter_mut().zip(log_mag.iter()) {
        c.re = m;
        c.im = 0.0;
    }
    // A real inverse transform requires these two to have no imaginary
    // part. They already do not — everything above is real — but the
    // planner checks, so be explicit.
    scratch_spec[0].im = 0.0;
    let last = scratch_spec.len() - 1;
    scratch_spec[last].im = 0.0;

    if fft.inv.process(scratch_spec, scratch_time).is_err() {
        return; // leave log_mag untouched: no smoothing is better than garbage
    }
    let norm = 1.0 / FRAME as f32;
    for v in scratch_time.iter_mut() {
        *v *= norm;
    }

    // Lifter. The cepstrum of a real even spectrum is itself even, so
    // the high quefrencies live in the middle of the buffer and the low
    // ones at both ends.
    for (q, v) in scratch_time.iter_mut().enumerate() {
        let from_end = FRAME - q;
        if q > LIFTER_COEFFS && from_end > LIFTER_COEFFS {
            *v = 0.0;
        }
    }

    if fft.fwd.process(scratch_time, scratch_spec).is_err() {
        return;
    }
    for (m, c) in log_mag.iter_mut().zip(scratch_spec.iter()) {
        *m = c.re;
    }
}

/// Reapply `original`'s spectral envelope to `shifted`.
///
/// Both must be mono and the same length — `pitch_shift` preserves
/// duration, so its input and output are. Returns a buffer the same
/// length as `shifted`.
///
/// Signals shorter than one analysis frame come back untouched: there is
/// no envelope to measure, and at under 46 ms there is no formant to
/// hear either.
pub(crate) fn reapply_envelope(original: &[f32], shifted: &[f32]) -> Vec<f32> {
    let n = shifted.len();
    if n < FRAME || original.len() < FRAME {
        return shifted.to_vec();
    }

    let fft = Fft::new();
    let window = hann(FRAME);
    let bins = FRAME / 2 + 1;

    let mut out = vec![0.0f32; n];
    let mut norm = vec![0.0f32; n];

    let mut in_a = fft.fwd.make_input_vec();
    let mut in_b = fft.fwd.make_input_vec();
    let mut spec_a = fft.fwd.make_output_vec();
    let mut spec_b = fft.fwd.make_output_vec();
    let mut cep_spec = fft.fwd.make_output_vec();
    let mut cep_time = fft.inv.make_output_vec();
    let mut env_a = vec![0.0f32; bins];
    let mut env_b = vec![0.0f32; bins];
    let mut inv_out = fft.inv.make_output_vec();

    let frames = if n >= FRAME { (n - FRAME) / HOP + 1 } else { 0 };

    for f in 0..frames {
        let start = f * HOP;
        for i in 0..FRAME {
            in_a[i] = original.get(start + i).copied().unwrap_or(0.0) * window[i];
            in_b[i] = shifted[start + i] * window[i];
        }
        if fft.fwd.process(&mut in_a, &mut spec_a).is_err()
            || fft.fwd.process(&mut in_b, &mut spec_b).is_err()
        {
            return shifted.to_vec();
        }

        // Log magnitudes. The epsilon keeps `ln` finite in silent bands;
        // it is far below anything audible and the floor below is what
        // actually governs the correction there.
        for k in 0..bins {
            env_a[k] = (spec_a[k].norm() + 1e-12).ln();
            env_b[k] = (spec_b[k].norm() + 1e-12).ln();
        }
        liftered_envelope(&fft, &mut env_a, &mut cep_spec, &mut cep_time);
        liftered_envelope(&fft, &mut env_b, &mut cep_spec, &mut cep_time);

        // Back to linear, and find the peak so the floor can be relative
        // — an absolute floor would behave differently on quiet and loud
        // material, which is exactly the level dependence the vocoder's
        // onset detector avoids for the same reason.
        let mut peak = 0.0f32;
        for k in 0..bins {
            env_a[k] = env_a[k].exp();
            env_b[k] = env_b[k].exp();
            peak = peak.max(env_a[k]).max(env_b[k]);
        }
        let floor = peak * ENVELOPE_FLOOR;

        for k in 0..bins {
            let ratio = if env_b[k] > floor && floor > 0.0 {
                (env_a[k].max(floor) / env_b[k]).clamp(1.0 / MAX_CORRECTION, MAX_CORRECTION)
            } else {
                1.0
            };
            spec_b[k].re *= ratio;
            spec_b[k].im *= ratio;
        }
        spec_b[0].im = 0.0;
        spec_b[bins - 1].im = 0.0;

        if fft.inv.process(&mut spec_b, &mut inv_out).is_err() {
            return shifted.to_vec();
        }
        let scale = 1.0 / FRAME as f32;
        for i in 0..FRAME {
            out[start + i] += inv_out[i] * scale * window[i];
            norm[start + i] += window[i] * window[i];
        }
    }

    // Divide out the summed window energy where there is enough of it to
    // divide by, and fall back to the uncorrected signal where there is
    // not — the head and tail, which no frame fully covers.
    let norm_max = norm.iter().fold(0.0f32, |m, v| m.max(*v));
    let threshold = norm_max * 0.25;
    for i in 0..n {
        if norm[i] > threshold && threshold > 0.0 {
            out[i] /= norm[i];
        } else {
            out[i] = shifted[i];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With every coefficient kept, the cepstral round trip must return
    /// what it was given. If this drifts, the normalisation is wrong and
    /// every envelope is a power of the real one.
    #[test]
    fn an_unliftered_round_trip_is_the_identity() {
        let fft = Fft::new();
        let bins = FRAME / 2 + 1;
        let mut log_mag: Vec<f32> = (0..bins)
            .map(|k| ((k as f32 / 30.0).sin() * 2.0) - 3.0)
            .collect();
        let expected = log_mag.clone();

        let mut cep_spec = fft.fwd.make_output_vec();
        let mut cep_time = fft.inv.make_output_vec();

        // Inline the body with the lifter disabled by making the cutoff
        // cover everything.
        for (c, &m) in cep_spec.iter_mut().zip(log_mag.iter()) {
            c.re = m;
            c.im = 0.0;
        }
        fft.inv.process(&mut cep_spec, &mut cep_time).unwrap();
        for v in cep_time.iter_mut() {
            *v /= FRAME as f32;
        }
        fft.fwd.process(&mut cep_time, &mut cep_spec).unwrap();
        for (m, c) in log_mag.iter_mut().zip(cep_spec.iter()) {
            *m = c.re;
        }

        for (got, want) in log_mag.iter().zip(expected.iter()) {
            assert!(
                (got - want).abs() < 1e-3,
                "round trip drifted: {got} vs {want}"
            );
        }
    }

    /// The lifter must remove fine structure and keep the broad shape.
    /// A slow curve plus a fast ripple should come back as the curve.
    #[test]
    fn liftering_keeps_the_shape_and_drops_the_ripple() {
        let fft = Fft::new();
        let bins = FRAME / 2 + 1;
        let slow: Vec<f32> = (0..bins)
            .map(|k| -3.0 + 2.0 * (PI * k as f32 / bins as f32).sin())
            .collect();
        let mut mixed: Vec<f32> = slow
            .iter()
            .enumerate()
            .map(|(k, s)| s + 0.5 * (2.0 * PI * k as f32 / 5.0).sin())
            .collect();

        let mut cep_spec = fft.fwd.make_output_vec();
        let mut cep_time = fft.inv.make_output_vec();
        liftered_envelope(&fft, &mut mixed, &mut cep_spec, &mut cep_time);

        // Compare away from the edges, where the implicit periodic
        // extension of a non-periodic curve rings.
        let lo = bins / 8;
        let hi = bins - bins / 8;
        let err = mixed[lo..hi]
            .iter()
            .zip(slow[lo..hi].iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            err < 0.25,
            "the envelope should track the slow curve; worst error {err:.3}"
        );
    }

    /// A signal shorter than one analysis frame is returned untouched
    /// rather than half-processed.
    #[test]
    fn short_input_passes_through() {
        let short = vec![0.5f32; 100];
        assert_eq!(reapply_envelope(&short, &short), short);
    }

    /// Correcting a signal against itself must not change it — the two
    /// envelopes are identical, so every ratio is 1.
    #[test]
    fn correcting_against_itself_is_a_near_no_op() {
        let sr = 44_100.0;
        let sig: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * PI * 300.0 * i as f32 / sr).sin() * 0.4)
            .collect();
        let out = reapply_envelope(&sig, &sig);
        assert_eq!(out.len(), sig.len());
        let worst = out
            .iter()
            .zip(sig.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.02, "self-correction moved a sample by {worst:.4}");
    }

    /// Silence in, silence out — and specifically no NaN. A silent band
    /// is where a naive envelope divide explodes.
    #[test]
    fn silence_produces_no_nan() {
        let silence = vec![0.0f32; 44_100];
        let out = reapply_envelope(&silence, &silence);
        assert!(
            out.iter().all(|v| v.is_finite()),
            "produced a non-finite sample"
        );
        assert!(out.iter().all(|v| v.abs() < 1e-3), "silence gained energy");
    }

    /// A signal that is silent above some frequency is the case the
    /// floor exists for: the shifted envelope is ~0 up there, and
    /// dividing by it would turn numerical noise into a full-scale band.
    #[test]
    fn a_band_limited_signal_does_not_explode() {
        let sr = 44_100.0;
        let low: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * PI * 200.0 * i as f32 / sr).sin() * 0.5)
            .collect();
        let high: Vec<f32> = (0..sr as usize)
            .map(|i| (2.0 * PI * 8_000.0 * i as f32 / sr).sin() * 0.5)
            .collect();
        // Deliberately mismatched: all the original's energy is low, all
        // the shifted signal's is high. Every bin is a division by
        // something tiny on one side or the other.
        let out = reapply_envelope(&low, &high);
        assert!(out.iter().all(|v| v.is_finite()));
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(
            peak < 2.0,
            "envelope correction amplified to {peak:.2} against inputs of 0.5"
        );
    }
}
