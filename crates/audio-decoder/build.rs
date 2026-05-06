// Generates the golden 1-second 440 Hz mono WAV and MP3 used by integration tests.
// Written into OUT_DIR; tests locate them via env!("OUT_DIR").

use std::env;
use std::path::PathBuf;

const SAMPLE_RATE: u32 = 44_100;
const FREQ_HZ: f32 = 440.0;
// -3 dBFS amplitude => 10^(-3/20) ≈ 0.7079.
const AMPLITUDE: f32 = 0.707_945_8;
const DURATION_SECS: u32 = 1;

fn generate_pcm_i16() -> Vec<i16> {
    let n = (SAMPLE_RATE * DURATION_SECS) as usize;
    let sr = SAMPLE_RATE as f32;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sr;
        let s = (2.0 * std::f32::consts::PI * FREQ_HZ * t).sin() * AMPLITUDE;
        // Use 32_768 (not i16::MAX) so the encode/decode round-trip is exact:
        // symphonia decodes i16->f32 as `s / 32_768.0`.
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        samples.push(q);
    }
    samples
}

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let wav_path = out_dir.join("sine_440hz_1s.wav");
    let mp3_path = out_dir.join("sine_440hz_1s.mp3");

    let pcm = generate_pcm_i16();

    // WAV
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(&wav_path, spec).expect("failed to create golden WAV writer");
    for s in &pcm {
        writer.write_sample(*s).expect("failed to write sample");
    }
    writer.finalize().expect("failed to finalize WAV");

    // MP3 — pure-Rust encoder via shine-rs. We regenerate at build time so the
    // golden MP3 is always exactly the synthesised 440 Hz signal, not a
    // checked-in artifact that may drift from the source.
    let cfg = shine_rs::Mp3EncoderConfig::new()
        .sample_rate(SAMPLE_RATE)
        .bitrate(128)
        .channels(1)
        .stereo_mode(shine_rs::StereoMode::Mono);
    let mp3_bytes = shine_rs::encode_pcm_to_mp3(cfg, &pcm).expect("MP3 encoding failed");
    std::fs::write(&mp3_path, &mp3_bytes).expect("failed to write golden MP3");

    println!("cargo:rerun-if-changed=build.rs");
}
