//! `noise_reduction` must not click at the ends of the track (#692).
//!
//! The overlap-add divided each output sample by the sum of the squared
//! Hann windows over it. At the first samples of a track only frame 0
//! covers them, with a window value near zero, so the error spectral
//! subtraction leaves in a frame's edge samples (which the window does not
//! scale down) was multiplied by up to 1/w: a burst at full scale in the
//! first 2 to 7 ms. These tests drive the tool the way the agent does
//! (`load`, `noise_reduction`, `render_final`) and look at the audio that
//! comes out.

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 44_100;
const SECONDS: usize = 8;
const EDGE: usize = 2048;

/// A small xorshift, so the signal is the same on every machine.
struct Noise(u64);

impl Noise {
    /// Uniform in [-1, 1).
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// White noise at about -34 dBFS RMS all through, and from 1.0 s a
/// harmonic, voice-like signal at about -20 dBFS RMS .
fn synthetic() -> Vec<f32> {
    let n = SAMPLE_RATE as usize * SECONDS;
    let mut rng = Noise(0x9E37_79B9_7F4A_7C15);
    // Uniform noise of amplitude a has an RMS of a / sqrt(3): 0.02 is -34 dBFS.
    let noise_amp = 0.02 * 3.0f32.sqrt();
    let voice_start = SAMPLE_RATE as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let mut s = noise_amp * rng.next();
            if i >= voice_start {
                let voice: f32 = (1..=8)
                    .map(|h| (2.0 * std::f32::consts::PI * 140.0 * h as f32 * t).sin() / h as f32)
                    .sum();
                // The eight harmonics at 1/h have an RMS of about 0.874.
                s += 0.1 * voice / 0.874;
            }
            s
        })
        .collect()
}

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for &s in samples {
        w.write_sample((s * 32_767.0).round() as i16).unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).expect("wav reader");
    r.samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32_768.0)
        .collect()
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

/// `load` the signal, reduce noise with the given arguments, and render the
/// head to WAV.
fn reduced(input: &[f32], args: Value) -> Vec<f32> {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut store = session::Store::open(&project).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard = None;

    let source = dir.path().join("take.wav");
    write_wav(&source, input);
    let out = dir.path().join("out.wav");

    let mut call = |store: &mut session::Store, tool: &str, args: Value| {
        let mut ctx = ToolContext {
            store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        dispatcher.invoke(tool, args, &mut ctx).unwrap()
    };

    ok(call(
        &mut store,
        "load",
        json!({ "path": source.to_string_lossy() }),
    ));
    ok(call(&mut store, "noise_reduction", args));
    let head = store.head().expect("head").to_hex();
    ok(call(
        &mut store,
        "render_final",
        json!({ "node_id": head, "format": "wav", "out_path": out.to_string_lossy() }),
    ));
    read_wav(&out)
}

#[test]
fn the_ends_of_the_track_stay_at_the_noise_level() {
    let input = synthetic();
    assert!(
        (0.2..=0.6).contains(&peak(&input)),
        "the signal should peak well under full scale but far above the noise, not at {}",
        peak(&input)
    );

    let output = reduced(&input, json!({ "track": 0, "noise_duration_sec": 0.9 }));
    assert_eq!(output.len(), input.len(), "length is kept");

    let n = input.len();
    for (name, range) in [("start", 0..EDGE), ("end", n - EDGE..n)] {
        let (i, o) = (&input[range.clone()], &output[range]);
        let (in_peak, out_peak) = (peak(i), peak(o));
        let (in_rms, out_rms) = (rms(i), rms(o));
        eprintln!("{name}: input peak {in_peak:.4} rms {in_rms:.4}, output peak {out_peak:.4} rms {out_rms:.4}");
        // Reduction only takes noise away: a few dB over the input at the
        // very most (the rounding to 16 bits is a tenth of a percent).
        assert!(
            out_peak <= in_peak * 1.5,
            "{name}: output peak {out_peak:.4} is more than 3.5 dB over the input's {in_peak:.4}"
        );
        assert!(
            out_rms <= in_rms * 1.5,
            "{name}: output RMS {out_rms:.4} is more than 3.5 dB over the input's {in_rms:.4}"
        );
        assert!(out_peak < 0.5, "{name}: output reaches {out_peak:.4}");
    }
    assert!(
        peak(&output) < 0.6,
        "nothing in the track gets near full scale, peak {}",
        peak(&output)
    );
}
