//! Sample-level helpers used by the offline render path.
//!
//! Phase 1 only needs gain; multi-track summation and crossfades arrive in
//! Phase 2. Helpers here operate in-place on interleaved `f32` so the caller
//! controls allocation.

/// Apply a per-track linear gain expressed in dB. A gain of `0.0` is a no-op
/// fast-path, which is also the unity-pass-through invariant relied on by
/// `render_unity_passthrough_byte_identical`.
pub fn apply_gain_db(samples: &mut [f32], gain_db: f32) {
    if gain_db == 0.0 {
        return;
    }
    let factor = 10f32.powf(gain_db / 20.0);
    for s in samples.iter_mut() {
        *s *= factor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unity_gain_is_noop() {
        let mut buf = vec![0.1, -0.2, 0.3];
        let original = buf.clone();
        apply_gain_db(&mut buf, 0.0);
        assert_eq!(buf, original);
    }

    #[test]
    fn six_db_doubles_samples() {
        let mut buf = vec![0.1, -0.2, 0.25];
        // 6.0205999... dB == ×2.0 exactly (in dB-domain), but float pow
        // gives an approximation. Within 1e-5 is plenty for the property.
        apply_gain_db(&mut buf, 20.0 * 2f32.log10());
        assert!((buf[0] - 0.2).abs() < 1e-5);
        assert!((buf[1] + 0.4).abs() < 1e-5);
        assert!((buf[2] - 0.5).abs() < 1e-5);
    }
}
