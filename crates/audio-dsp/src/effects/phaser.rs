struct AllPass {
    a1: f32,
    z: f32,
}

impl AllPass {
    fn new(frequency: f32, sr: f32) -> Self {
        let k = (std::f32::consts::PI * frequency / sr).tan();
        let a1 = (k - 1.0) / (k + 1.0);
        Self { a1, z: 0.0 }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.a1 * x + self.z;
        self.z = x - self.a1 * y;
        y
    }
}

pub fn apply_phaser(
    samples: &mut [f32],
    sr: u32,
    channels: usize,
    rate_hz: f32,
    depth: f32,
    stages: u32,
) {
    let channels = channels.max(1);
    let stages = (stages as usize).clamp(2, 12);
    let n_frames = samples.len() / channels;
    let min_freq = 200.0f32;
    let max_freq = 4000.0f32;
    let mut all_passes: Vec<Vec<AllPass>> = (0..channels)
        .map(|_| {
            (0..stages)
                .map(|_| AllPass::new(min_freq, sr as f32))
                .collect()
        })
        .collect();
    for frame in 0..n_frames {
        let lfo = (2.0 * std::f32::consts::PI * rate_hz * frame as f32 / sr as f32).sin();
        let freq = min_freq + (max_freq - min_freq) * (lfo * 0.5 + 0.5);
        for ch in 0..channels {
            for ap in &mut all_passes[ch] {
                let k = (std::f32::consts::PI * freq / sr as f32).tan();
                ap.a1 = (k - 1.0) / (k + 1.0);
            }
            let x = samples[frame * channels + ch];
            let mut y = x;
            for ap in &mut all_passes[ch] {
                y = ap.process(y);
            }
            samples[frame * channels + ch] = x + y * depth;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_phaser;

    #[test]
    fn does_not_clip() {
        let mut samples: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.001).sin() * 0.8).collect();
        apply_phaser(&mut samples, 44100, 1, 1.0, 0.7, 4);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max <= 1.5,
            "phaser output should not clip excessively, got {max}"
        );
    }
}
