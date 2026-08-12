pub fn apply_distortion(samples: &mut [f32], sr: u32, channels: usize, drive: f32, tone: f32) {
    let channels = channels.max(1);
    let drive = drive.max(1.0);
    let tone = tone.clamp(0.0, 1.0);
    let tanh_drive = drive.tanh().max(1e-6);
    for s in samples.iter_mut() {
        *s = (*s * drive).tanh() / tanh_drive;
    }
    let cutoff = 200.0 + tone * 8000.0;
    let k = (-2.0 * std::f32::consts::PI * cutoff / sr as f32).exp();
    let n_frames = samples.len() / channels;
    for ch in 0..channels {
        let mut z = 0.0f32;
        for frame in 0..n_frames {
            let idx = frame * channels + ch;
            z = samples[idx] * (1.0 - k) + z * k;
            samples[idx] = z;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_distortion;

    #[test]
    fn high_drive_clips() {
        let mut samples = vec![0.5f32; 100];
        apply_distortion(&mut samples, 44100, 1, 10.0, 0.5);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max <= 1.0 + 1e-5,
            "hard-clipped output should be within [-1,1]"
        );
    }

    #[test]
    fn low_drive_doesnt_clip() {
        let mut samples = vec![0.1f32; 100];
        apply_distortion(&mut samples, 44100, 1, 1.0, 0.5);
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        // At low drive with low signal, output should remain well below 1
        assert!(max < 0.15, "low-drive processing of 0.1 amplitude signal");
    }
}
