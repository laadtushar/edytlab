pub fn apply_limiter(samples: &mut [f32], _sr: u32, _channels: usize, ceiling_db: f32) {
    let ceiling = 10.0f32.powf(ceiling_db / 20.0);
    for s in samples.iter_mut() {
        if s.abs() > ceiling {
            *s = s.signum() * ceiling;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_limiter;

    #[test]
    fn clips_above_ceiling() {
        let mut samples = vec![0.5f32, 0.8, 1.5, -1.2, 0.3];
        apply_limiter(&mut samples, 44100, 1, -6.0);
        let ceiling = 10.0f32.powf(-6.0 / 20.0);
        for s in &samples {
            assert!(
                s.abs() <= ceiling + 1e-5,
                "sample {s} exceeds ceiling {ceiling}"
            );
        }
    }
}
