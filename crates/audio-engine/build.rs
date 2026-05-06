// Generates fixtures used by integration tests:
//
// * `sine_440hz_1s.wav`  — the 1 s, 44.1 kHz, mono, 16-bit, -3 dBFS sine that
//   the unity pass-through test feeds through the engine. We duplicate the
//   audio-decoder build script's generator here on purpose: depending on
//   another crate's build artifact across `OUT_DIR`s is fragile, and the
//   spec calls for byte-identity against a freshly-generated source.
// * `unity_passthrough_golden.wav` — the expected unity-render output. With
//   gain == 0 dB and no effects, a unity render must be byte-identical to
//   the source it ingested, so this file is literally the same bytes as
//   `sine_440hz_1s.wav`. The test asserts engine output == this file.
// * `sine_440hz_10min.wav` — large fixture for the (#[ignore]'d) 20x
//   real-time perf gate. Generated here so it never lands in the repo
//   (~52 MB at 16-bit mono 44.1 kHz).

use std::env;
use std::path::{Path, PathBuf};

const SAMPLE_RATE: u32 = 44_100;
const FREQ_HZ: f32 = 440.0;
// -3 dBFS amplitude => 10^(-3/20) ≈ 0.7079.
const AMPLITUDE: f32 = 0.707_945_8;

fn generate_sine_pcm_i16(duration_secs: u32) -> Vec<i16> {
    let n = (SAMPLE_RATE * duration_secs) as usize;
    let sr = SAMPLE_RATE as f32;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sr;
        let s = (2.0 * std::f32::consts::PI * FREQ_HZ * t).sin() * AMPLITUDE;
        // 32_768 (not i16::MAX) keeps the i16 -> f32 -> i16 round-trip exact;
        // symphonia decodes i16 as `s / 32_768.0`.
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        samples.push(q);
    }
    samples
}

fn write_mono_wav(path: &Path, pcm: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create WAV writer");
    for s in pcm {
        w.write_sample(*s).expect("write sample");
    }
    w.finalize().expect("finalize WAV");
}

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    let src_1s = out_dir.join("sine_440hz_1s.wav");
    let golden = out_dir.join("unity_passthrough_golden.wav");
    let src_10min = out_dir.join("sine_440hz_10min.wav");

    let pcm_1s = generate_sine_pcm_i16(1);
    write_mono_wav(&src_1s, &pcm_1s);

    // The unity-render expectation is byte-identical to the source file, so
    // copy raw bytes rather than re-encoding (which can differ in chunk
    // ordering between hound versions even at the same data).
    std::fs::copy(&src_1s, &golden).expect("copy golden wav");

    let pcm_10min = generate_sine_pcm_i16(10 * 60);
    write_mono_wav(&src_10min, &pcm_10min);

    println!("cargo:rerun-if-changed=build.rs");
}
