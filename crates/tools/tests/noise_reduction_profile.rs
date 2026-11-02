//! `noise_reduction` can learn its noise profile from any range, on the same
//! track or another one (#769). Without the new arguments the output is the
//! same bytes as before.

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 44_100;

struct Noise(u64);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// `secs` seconds of white noise at about -34 dBFS RMS, with a 440 Hz tone at
/// about -17 dBFS over `tone` (seconds) when given.
fn signal(secs: f32, seed: u64, tone: Option<(f32, f32)>) -> Vec<f32> {
    let mut rng = Noise(seed);
    let amp = 0.02 * 3.0f32.sqrt();
    (0..(secs * SR as f32) as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let mut s = amp * rng.next();
            if let Some((a, b)) = tone {
                if t >= a && t < b {
                    s += 0.2 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
                }
            }
            s
        })
        .collect()
}

fn write_wav(path: &Path, samples: &[f32], rate: u32) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).unwrap();
    for &s in samples {
        w.write_sample((s * 32_767.0).round() as i16).unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32_768.0)
        .collect()
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
}

fn db(a: f32, b: f32) -> f32 {
    20.0 * (a / b).log10()
}

struct Rig {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Rig {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        Rig {
            store: session::Store::open(&project).unwrap(),
            dir,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn ok(&mut self, tool: &str, args: Value) {
        match self.call(tool, args) {
            ToolResult::Ok(_) => {}
            ToolResult::Error(m) => panic!("{tool}: {m}"),
        }
    }

    fn err(&mut self, tool: &str, args: Value) -> String {
        match self.call(tool, args) {
            ToolResult::Error(m) => m,
            ToolResult::Ok(v) => panic!("{tool} should have refused, got {v}"),
        }
    }

    fn load(&mut self, name: &str, samples: &[f32], rate: u32) {
        let p = self.dir.path().join(name);
        write_wav(&p, samples, rate);
        self.ok("load", json!({ "path": p.to_string_lossy() }));
    }

    /// Render the head and return its WAV bytes.
    fn render(&mut self) -> Vec<u8> {
        let out = self.dir.path().join("out.wav");
        let head = self.store.head().unwrap().to_hex();
        self.ok(
            "render_final",
            json!({ "node_id": head, "format": "wav", "out_path": out.to_string_lossy() }),
        );
        std::fs::read(&out).unwrap()
    }

    fn render_samples(&mut self) -> Vec<f32> {
        self.render();
        read_wav(&self.dir.path().join("out.wav"))
    }
}

/// Noise-only stretches at both ends, a tone in the middle: the first second
/// is noise, so the old default profile works too; used for the pin.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn pin_input() -> Vec<f32> {
    signal(3.0, 7, Some((1.0, 2.0)))
}

/// FNV-1a over the rendered file, so the test needs no golden file.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

// The hash is of a float FFT's output, which differs in the last bit between
// CPU architectures (it failed on macOS arm64), so it is held where it was
// recorded: Linux on x86_64, as CI's Linux leg runs.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn omitting_the_profile_arguments_gives_the_bytes_of_the_unchanged_tool() {
    let mut rig = Rig::new();
    rig.load("a.wav", &pin_input(), SR);
    rig.ok(
        "noise_reduction",
        json!({ "track": 0, "noise_duration_sec": 0.9 }),
    );
    let bytes = rig.render();
    // Recorded from the tool as it was on origin/main before this change.
    assert_eq!(fnv(&bytes), PINNED_FNV, "default-profile output changed");
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const PINNED_FNV: u64 = 12988580903378295446;

#[test]
fn a_profile_from_the_middle_of_the_track_lowers_the_noise_floor() {
    // Tone 0 s to 2 s over the noise, then 1 s of noise alone: the leading
    // profile is mostly tone, the range 2.1 s to 2.9 s is noise only.
    let input = signal(3.0, 11, Some((0.0, 2.0)));
    let tail = (2.2 * SR as f32) as usize..(2.9 * SR as f32) as usize;

    let mut rig = Rig::new();
    rig.load("a.wav", &input, SR);
    rig.ok(
        "noise_reduction",
        json!({ "track": 0, "noise_from": 2.1, "noise_to": 2.9 }),
    );
    let out = rig.render_samples();
    let drop = db(rms(&input[tail.clone()]), rms(&out[tail.clone()]));
    eprintln!("mid-track profile: noise floor dropped {drop:.1} dB");
    assert!(drop >= 6.0, "noise floor dropped only {drop:.1} dB");
}

#[test]
fn a_profile_from_another_track_lowers_the_noise_floor() {
    // Track 0 is noise under a tone throughout; track 1 is room tone alone.
    let input = signal(3.0, 21, Some((0.0, 3.0)));
    let room = signal(2.0, 22, None);

    let mut rig = Rig::new();
    rig.load("a.wav", &input, SR);
    rig.load("room.wav", &room, SR);
    rig.ok(
        "noise_reduction",
        json!({ "track": 0, "noise_from": 0.2, "noise_to": 1.8, "noise_track": 1 }),
    );
    // The room tone track is only the source of the profile.
    rig.ok("mute_track", json!({ "track": 1, "muted": true }));
    let out = rig.render_samples();
    // The tone is kept (440 Hz survives), the broadband floor goes: compare
    // the energy outside the tone with a DFT-free proxy, the difference of
    // the signal from a pure tone.
    let tone = |i: usize| 0.2 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SR as f32).sin();
    let resid = |x: &[f32]| {
        rms(&x
            .iter()
            .enumerate()
            .map(|(i, v)| v - tone(i))
            .collect::<Vec<_>>()[SR as usize..2 * SR as usize])
    };
    let drop = db(resid(&input), resid(&out));
    eprintln!("other-track profile: residual dropped {drop:.1} dB");
    assert!(drop >= 6.0, "residual dropped only {drop:.1} dB");
}

#[test]
fn bad_profiles_are_refused_and_add_no_node() {
    let mut rig = Rig::new();
    rig.load("a.wav", &signal(2.0, 5, None), SR);
    rig.load("b.wav", &signal(1.0, 6, None), 48_000);
    let head = rig.store.head();
    let cases = [
        (
            json!({ "track": 0, "noise_from": 1.0, "noise_to": 0.5 }),
            "profile range",
        ),
        (
            json!({ "track": 0, "noise_from": 0.5, "noise_to": 0.5 }),
            "profile range",
        ),
        (
            json!({ "track": 0, "noise_from": -1.0, "noise_to": 0.5 }),
            "profile range",
        ),
        (
            json!({ "track": 0, "noise_from": 1.0, "noise_to": 5.0 }),
            "past the end",
        ),
        (
            json!({ "track": 0, "noise_from": 0.0, "noise_to": 0.01 }),
            "needs at least",
        ),
        (json!({ "track": 0, "noise_from": 0.0 }), "together"),
        (
            json!({ "track": 0, "noise_track": 1, "noise_duration_sec": 0.5 }),
            "noise_from",
        ),
        (
            json!({ "track": 0, "noise_from": 0.0, "noise_to": 0.5, "noise_track": 9 }),
            "track",
        ),
        (
            json!({ "track": 0, "noise_from": 0.0, "noise_to": 0.5, "noise_track": 1 }),
            "Hz",
        ),
        (json!({ "track": 0 }), "noise_duration_sec"),
        (
            json!({ "track": 0, "noise_from": 0.2, "noise_to": 0.8, "noise_duration_sec": 0.5 }),
            "cannot be combined",
        ),
    ];
    for (args, want) in cases {
        let m = rig.err("noise_reduction", args.clone());
        assert!(m.contains(want), "{args}: {m}");
    }
    assert_eq!(rig.store.head(), head, "a refusal appends no node");
}

#[test]
fn a_profile_range_of_digital_silence_is_refused_and_adds_no_node() {
    // Both buffers are laid out from track frame 0, so shifting the clip
    // later leaves a lead-in of zeros, and a silence inserted mid-clip leaves
    // a gap of them. A profile learned there is all zeros and the edit would
    // succeed while changing next to nothing.
    let mut rig = Rig::new();
    rig.load("a.wav", &signal(3.0, 31, None), SR);
    rig.load("b.wav", &signal(3.0, 32, None), SR);
    rig.ok("time_shift", json!({ "track": 0, "offset_sec": 1.5 }));
    rig.ok(
        "insert_silence",
        json!({ "track": 1, "at": 1.0, "duration": 1.0 }),
    );
    let head = rig.store.head();

    // The lead-in before track 0's clip, on its own track and as another's.
    let lead_in = json!({ "track": 0, "noise_from": 0.2, "noise_to": 1.2 });
    let gap = json!({ "track": 1, "noise_from": 1.2, "noise_to": 1.8 });
    let from_lead_in = json!({ "track": 1, "noise_from": 0.2, "noise_to": 1.2, "noise_track": 0 });
    for args in [lead_in, gap, from_lead_in] {
        let m = rig.err("noise_reduction", args.clone());
        assert!(m.contains("digital silence"), "{args}: {m}");
    }
    assert_eq!(rig.store.head(), head, "a refusal appends no node");

    // A range that reaches sound is still accepted.
    rig.ok(
        "noise_reduction",
        json!({ "track": 0, "noise_from": 1.6, "noise_to": 2.6 }),
    );
}
