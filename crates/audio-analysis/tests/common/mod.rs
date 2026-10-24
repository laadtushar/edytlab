//! Synthetic signals for the tempo and key regression tests (#696).
//!
//! Everything is deterministic: the noise is a fixed-seed LCG, so a failure
//! reproduces on any machine.

#![allow(dead_code)]

use audio_decoder::DecodedAudio;

pub const SR: u32 = 44_100;

pub fn decoded(samples: Vec<f32>) -> DecodedAudio {
    DecodedAudio {
        samples,
        sample_rate: SR,
        channels: 1,
    }
}

/// Deterministic white noise in `[-1, 1]`.
pub struct Noise(u64);

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }
}

/// A kick drum: a sine sweeping 140 Hz down to 50 Hz with a ~80 ms decay.
pub fn add_kick_at(out: &mut [f32], start: usize, amp: f32) {
    add_kick(out, SR, start, amp)
}

fn add_kick(out: &mut [f32], sample_rate: u32, start: usize, amp: f32) {
    let len = (sample_rate as f32 * 0.25) as usize;
    let mut phase = 0.0f32;
    for i in 0..len {
        let t = i as f32 / sample_rate as f32;
        let freq = 50.0 + 90.0 * (-t * 30.0).exp();
        phase += 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
        if let Some(s) = out.get_mut(start + i) {
            *s += amp * (-t * 14.0).exp() * phase.sin();
        }
    }
}

/// A closed hi-hat: a 40 ms burst of noise, high-passed by differencing.
fn add_hat(out: &mut [f32], sample_rate: u32, start: usize, amp: f32, noise: &mut Noise) {
    let len = (sample_rate as f32 * 0.04) as usize;
    let mut prev = 0.0f32;
    for i in 0..len {
        let t = i as f32 / sample_rate as f32;
        let n = noise.next();
        let hp = n - prev;
        prev = n;
        if let Some(s) = out.get_mut(start + i) {
            *s += amp * (-t * 90.0).exp() * hp * 0.5;
        }
    }
}

/// Kick on every beat, hat on every off-beat. The first kick is at
/// `first_beat_s`. Returns the signal and the kick times in seconds.
pub fn kick_and_hats(
    bpm: f32,
    seconds: f32,
    first_beat_s: f32,
    kick_amp: f32,
    hat_amp: f32,
) -> (Vec<f32>, Vec<f32>) {
    kick_and_hats_at(SR, bpm, seconds, first_beat_s, kick_amp, hat_amp)
}

/// [`kick_and_hats`] at any sample rate.
pub fn kick_and_hats_at(
    sample_rate: u32,
    bpm: f32,
    seconds: f32,
    first_beat_s: f32,
    kick_amp: f32,
    hat_amp: f32,
) -> (Vec<f32>, Vec<f32>) {
    let n = (seconds * sample_rate as f32) as usize;
    let mut out = vec![0.0f32; n];
    let mut noise = Noise::new(7);
    let beat = 60.0 / bpm;
    let mut kicks = Vec::new();
    let mut k = 0usize;
    loop {
        let t = first_beat_s + k as f32 * beat;
        if t + 0.3 >= seconds {
            break;
        }
        add_kick(
            &mut out,
            sample_rate,
            (t * sample_rate as f32) as usize,
            kick_amp,
        );
        add_hat(
            &mut out,
            sample_rate,
            ((t + beat / 2.0) * sample_rate as f32) as usize,
            hat_amp,
            &mut noise,
        );
        kicks.push(t);
        k += 1;
    }
    (out, kicks)
}

/// Equal-tempered frequency of a MIDI note.
pub fn midi_hz(note: i32) -> f32 {
    440.0 * 2f32.powf((note - 69) as f32 / 12.0)
}

/// A chord of `notes` (MIDI numbers) for `seconds` at `SR`; see
/// [`chord_at`].
pub fn chord(notes: &[i32], seconds: f32, harmonics: usize) -> Vec<f32> {
    chord_at(SR, notes, seconds, harmonics, 0.0)
}

/// A chord of `notes` (MIDI numbers) for `seconds` at `sample_rate`, each note
/// with `harmonics` partials falling off as 1/h, tuned `cents` sharp, and a
/// 10 ms fade at each end so consecutive chords do not click.
pub fn chord_at(
    sample_rate: u32,
    notes: &[i32],
    seconds: f32,
    harmonics: usize,
    cents: f32,
) -> Vec<f32> {
    let n = (seconds * sample_rate as f32) as usize;
    let mut out = vec![0.0f32; n];
    for &note in notes {
        let f = midi_hz(note) as f64 * 2f64.powf(cents as f64 / 1200.0);
        for h in 1..=harmonics {
            let hf = f * h as f64;
            if hf > 8_000.0 {
                break;
            }
            let a = (1.0 / h as f64 / notes.len() as f64) as f32;
            // sin(w n) by the recurrence s[n+1] = 2 cos(w) s[n] - s[n-1]:
            // a table-free sine that is cheap in an unoptimised test build.
            let w = std::f64::consts::TAU * hf / sample_rate as f64;
            let c2 = 2.0 * w.cos();
            let (mut prev, mut cur) = (0.0f64, w.sin());
            for s in out.iter_mut() {
                *s += a * prev as f32;
                let next = c2 * cur - prev;
                prev = cur;
                cur = next;
            }
        }
    }
    let fade = (sample_rate as f32 * 0.01) as usize;
    for i in 0..fade.min(n) {
        let g = i as f32 / fade as f32;
        out[i] *= g;
        out[n - 1 - i] *= g;
    }
    out
}
