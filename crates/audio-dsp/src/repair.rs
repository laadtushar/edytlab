//! Short offline repairs. Each channel is interpolated independently.

/// Number of same-sign clipped runs, including runs with no usable edges.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeclipReport {
    pub found: usize,
    pub repaired: usize,
    /// Gain applied to leave room for reconstructed peaks in a PCM output.
    pub gain_db: f32,
}

fn cubic(x: &[f32], start: usize, end: usize, bounded: bool) -> Vec<f32> {
    let a = f64::from(x[start - 1]);
    let b = f64::from(x[end]);
    let length = (end - start + 1) as f64;
    let mut da = a - f64::from(x[start - 2]);
    let mut db = f64::from(x[end + 1]) - b;
    if bounded {
        let slope = (b - a) / length;
        let limit = 3.0 * slope.abs();
        da = if da * slope > 0.0 {
            da.clamp(-limit, limit)
        } else {
            0.0
        };
        db = if db * slope > 0.0 {
            db.clamp(-limit, limit)
        } else {
            0.0
        };
    }
    (1..end - start + 1)
        .map(|i| {
            let t = i as f64 / length;
            ((2.0 * t.powi(3) - 3.0 * t * t + 1.0) * a
                + (t.powi(3) - 2.0 * t * t + t) * length * da
                + (-2.0 * t.powi(3) + 3.0 * t * t) * b
                + (t.powi(3) - t * t) * length * db) as f32
        })
        .collect()
}

/// Restore runs above a specified clipping level with cubic Hermite
/// interpolation. Runs at the buffer edges or with fewer than two clean
/// surrounding samples are counted but left alone. If restored peaks exceed full
/// scale, one shared gain brings the buffer back to full scale for PCM storage.
pub fn declip(samples: &mut [f32], channels: usize, threshold: f32) -> DeclipReport {
    let mut report = DeclipReport::default();
    if channels == 0
        || !threshold.is_finite()
        || threshold <= 0.0
        || samples.iter().any(|s| !s.is_finite())
    {
        return report;
    }
    for ch in 0..channels {
        let input: Vec<_> = samples.iter().skip(ch).step_by(channels).copied().collect();
        let mut i = 0;
        while i < input.len() {
            if !input[i].is_finite() || input[i].abs() < threshold {
                i += 1;
                continue;
            }
            let start = i;
            let positive = input[i].is_sign_positive();
            while i < input.len()
                && input[i].abs() >= threshold
                && input[i].is_sign_positive() == positive
            {
                i += 1;
            }
            // The threshold can include a shoulder just below the flat top.
            // Narrow to the plateau itself before fitting its clean edges.
            let peak = input[start..i]
                .iter()
                .fold(0.0_f32, |peak, s| peak.max(s.abs()));
            let flat = |s: &f32| s.abs() >= peak - 1e-6;
            let first = input[start..i].iter().position(flat).unwrap();
            let last = input[start..i].iter().rposition(flat).unwrap();
            let end = start + last + 1;
            let start = start + first;
            // Three equal samples distinguish a plateau from the two equal
            // samples a smooth sampled peak can have. Single-sample peaks stay.
            if end - start < 3 || input[start..end].iter().any(|s| !flat(s)) {
                continue;
            }
            report.found += 1;
            if start < 2
                || end + 1 >= input.len()
                || flat(&input[start - 2])
                || flat(&input[end + 1])
            {
                continue;
            }
            let repaired = cubic(&input, start, end, false);
            if repaired
                .iter()
                .any(|s| !s.is_finite() || s.abs() > threshold * 4.0)
            {
                continue;
            }
            for (j, value) in repaired.into_iter().enumerate() {
                samples[(start + j) * channels + ch] = value;
            }
            report.repaired += 1;
        }
    }
    if report.repaired > 0 {
        let peak = samples.iter().fold(1.0_f32, |peak, s| peak.max(s.abs()));
        if peak > 1.0 {
            let gain = 1.0 / peak;
            for s in samples {
                *s *= gain;
            }
            report.gain_db = 20.0 * gain.log10();
        }
    }
    report
}

// Fit a second-order least-squares predictor on clean context. Bidirectional
// prediction preserves a tonal waveform across many periods where a cubic
// through just the edges would flatten it. Ill-conditioned fits fall back to
// a bounded cubic, so silence and non-tonal material cannot explode.
fn predict(context: &[f32], count: usize) -> Option<Vec<f32>> {
    if context.len() < 8 {
        return None;
    }
    let (mut aa, mut ab, mut bb, mut ay, mut by) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for w in context.windows(3) {
        let (b, a, y) = (f64::from(w[0]), f64::from(w[1]), f64::from(w[2]));
        aa += a * a;
        ab += a * b;
        bb += b * b;
        ay += a * y;
        by += b * y;
    }
    let det = aa * bb - ab * ab;
    if det <= 1e-12 * aa * bb || aa < 1e-12 {
        return None;
    }
    let a = (ay * bb - by * ab) / det;
    let b = (by * aa - ay * ab) / det;
    // Stable AR(2) region, allowing rounding at the unit circle for pure tones.
    if b.abs() > 1.00001 || a.abs() > 1.0 - b + 0.00001 {
        return None;
    }
    let mut prev = f64::from(context[context.len() - 2]);
    let mut curr = f64::from(context[context.len() - 1]);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let next = a * curr + b * prev;
        out.push(next as f32);
        prev = curr;
        curr = next;
    }
    Some(out)
}

/// Fill `[start, end)` in frames using up to 10 ms of clean context on each
/// side. Requires two samples outside either edge. Samples outside the span
/// are never altered; time, layout and sample count are preserved.
pub fn repair_span(
    samples: &mut [f32],
    rate: u32,
    channels: usize,
    start: usize,
    end: usize,
) -> Result<(), &'static str> {
    if channels == 0
        || start < 2
        || start >= end
        || end >= (samples.len() / channels).saturating_sub(1)
    {
        return Err("repair_span needs two samples of context on each side");
    }
    let context_len = (rate as usize / 100).max(8);
    for ch in 0..channels {
        let x: Vec<_> = samples.iter().skip(ch).step_by(channels).copied().collect();
        let left = &x[start.saturating_sub(context_len)..start];
        let right: Vec<_> = x[end..(end + context_len).min(x.len())]
            .iter()
            .rev()
            .copied()
            .collect();
        let count = end - start;
        let prediction =
            predict(left, count)
                .zip(predict(&right, count))
                .map(|(forward, backward)| {
                    (0..count)
                        .map(|i| {
                            let t = (i + 1) as f32 / (count + 1) as f32;
                            let blend = t * t * (3.0 - 2.0 * t);
                            forward[i] * (1.0 - blend) + backward[count - 1 - i] * blend
                        })
                        .collect::<Vec<_>>()
                });
        let limit = left
            .iter()
            .chain(&right)
            .fold(0.0_f32, |peak, s| peak.max(s.abs()))
            .max(0.001)
            * 2.0;
        let fill = match prediction {
            Some(p) if p.iter().all(|s| s.is_finite() && s.abs() <= limit.min(1.0)) => p,
            _ => cubic(&x, start, end, true),
        };
        for (i, s) in fill.into_iter().enumerate() {
            samples[(start + i) * channels + ch] = s;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_and_edge_clipping_are_safe() {
        let mut silence = vec![0.0; 100];
        repair_span(&mut silence, 1000, 1, 20, 40).unwrap();
        assert_eq!(silence, vec![0.0; 100]);
        let mut clipped = vec![1.0; 100];
        let report = declip(&mut clipped, 1, 0.999);
        assert_eq!((report.found, report.repaired), (1, 0));
        assert_eq!(clipped, vec![1.0; 100]);
        let mut nonfinite = [f32::NAN, 0.0, f32::INFINITY];
        assert_eq!(declip(&mut nonfinite, 1, 0.999).repaired, 0);
    }
}
