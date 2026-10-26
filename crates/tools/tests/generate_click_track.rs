//! Public dispatcher coverage for the click-track generator (#770).

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

struct Project {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    clipboard: Option<tools::Clipboard>,
    dispatcher: ToolDispatcher,
}

impl Project {
    fn empty() -> Self {
        let dir = TempDir::new().unwrap();
        let store = session::Store::open(dir.path()).unwrap();
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            clipboard: None,
            dispatcher: ToolDispatcher::default_dispatcher(),
        }
    }

    fn call(&mut self, name: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(name, args, &mut ctx).unwrap()
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        match self.call(name, args) {
            ToolResult::Ok(value) => value,
            ToolResult::Error(message) => panic!("{name}: {message}"),
        }
    }

    fn state(&self) -> session::SessionState {
        self.store
            .get(self.store.head().expect("head"))
            .unwrap()
            .state
    }

    fn samples(&self, track: usize) -> Vec<f32> {
        let clip = &self.state().tracks[track].clips[0];
        hound::WavReader::open(&clip.source_path)
            .unwrap()
            .into_samples::<f32>()
            .map(Result::unwrap)
            .collect()
    }

    fn load_wav(&mut self, rate: u32, seconds: usize, bpm: Option<f64>) {
        let path = self.dir.path().join("source.wav");
        write_wav(&path, rate, seconds, bpm);
        self.ok("load", json!({ "path": path }));
    }
}

fn write_wav(path: &Path, rate: u32, seconds: usize, bpm: Option<f64>) {
    let mut writer = WavWriter::create(
        path,
        WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        },
    )
    .unwrap();
    let period = bpm.map(|b| 60.0 * rate as f64 / b);
    for frame in 0..rate as usize * seconds {
        let pulse = period.map(|p| (frame as f64 % p) < 400.0).unwrap_or(false);
        let sample = if pulse {
            ((frame as f32 * 0.5).sin() * 0.65 * i16::MAX as f32) as i16
        } else {
            0
        };
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

fn assert_onsets(samples: &[f32], rate: u32, bpm: f64, offset: f64, count: usize) {
    for beat in 0..count {
        let frame = ((offset + beat as f64 * 60.0 / bpm) * rate as f64).round() as usize;
        assert!(samples[frame].abs() > 0.1, "beat {beat} at frame {frame}");
        if frame > 0 {
            assert_eq!(samples[frame - 1], 0.0, "early beat {beat}");
        }
    }
    let detected = usize::from(samples.first().is_some_and(|s| *s != 0.0))
        + samples
            .windows(2)
            .filter(|pair| pair[0] == 0.0 && pair[1] != 0.0)
            .count();
    assert_eq!(detected, count, "unexpected extra or missing click pulses");
}

#[test]
fn dispatcher_generates_click_track_on_an_empty_project() {
    let mut p = Project::empty();
    assert!(p.store.head().is_none());
    let result = p.ok("generate_click_track", json!({ "bpm": 120, "bars": 2 }));
    assert_eq!(result["track_index"], 0);
    let node = p.store.get(p.store.head().unwrap()).unwrap();
    assert!(node.parent.is_none());
    let state = node.state;
    assert_eq!(state.tracks.len(), 1);
    assert_eq!(state.sample_rate, 44_100);
    assert_eq!(state.length_samples, 176_400);
    assert_eq!(state.tracks[0].clips[0].length, 176_400);
    assert!(state.tracks[0].clips[0].source_path.exists());
    let samples = p.samples(0);
    assert_onsets(&samples, 44_100, 120.0, 0.0, 8);
    assert!(samples[0].abs() > samples[22_050].abs());
    assert_eq!(
        samples[0], samples[88_200],
        "each bar starts with an accent"
    );
    assert!(samples.iter().all(|sample| sample.abs() <= 0.5));
}

#[test]
fn fractional_tempo_offset_and_custom_bar_size_have_independent_frame_rounding() {
    let mut p = Project::empty();
    let (bpm, offset): (f64, f64) = (137.3, 0.037);
    p.ok("generate_click_track", json!({ "bpm": bpm, "bars": 7, "beats_per_bar": 3, "offset_sec": offset, "amplitude": 0.8 }));
    let samples = p.samples(0);
    assert_eq!(
        samples.len(),
        ((offset + 21.0 * 60.0 / bpm) * 44_100.0).round() as usize
    );
    assert_onsets(&samples, 44_100, bpm, offset, 21);
    let first = (offset * 44_100.0).round() as usize;
    let second = ((offset + 60.0 / bpm) * 44_100.0).round() as usize;
    assert!(samples[first].abs() > samples[second].abs());
    assert!(samples[..first].iter().all(|s| *s == 0.0));
}

#[test]
fn duration_is_total_length_including_offset_at_48_khz() {
    let mut p = Project::empty();
    p.load_wav(48_000, 1, None);
    let result = p.ok(
        "generate_click_track",
        json!({ "bpm": 120, "duration_sec": 3.0, "offset_sec": 0.25 }),
    );
    assert_eq!(result["track_index"], 1);
    let state = p.state();
    assert_eq!(state.sample_rate, 48_000);
    assert_eq!(state.tracks[1].clips[0].length, 144_000);
    let samples = p.samples(1);
    assert_eq!(samples.len(), 144_000);
    assert_onsets(&samples, 48_000, 120.0, 0.25, 6);
    assert!(samples[..12_000].iter().all(|s| *s == 0.0));
}

#[test]
fn track_tempo_uses_the_same_analysis_as_analyze_track() {
    let mut p = Project::empty();
    p.load_wav(44_100, 8, Some(120.0));
    let report = p.ok("analyze_track", json!({ "track": 0 }));
    let bpm = report["bpm"].as_f64().expect("steady beat");
    let result = p.ok("generate_click_track", json!({ "track": 0, "bars": 2 }));
    assert_eq!(result["track_index"], 1);
    assert_onsets(&p.samples(1), 44_100, bpm, 0.0, 8);
    let before = p.store.head();
    p.ok(
        "generate_click_track",
        json!({ "bpm": 100, "track": 99, "bars": 1 }),
    );
    assert_ne!(p.store.head(), before, "explicit BPM takes precedence");
}

#[test]
fn invalid_arguments_never_advance_the_head() {
    let invalid = [
        json!({ "bars": 1 }),
        json!({ "bpm": 0, "bars": 1 }),
        json!({ "bpm": -1, "bars": 1 }),
        json!({ "bpm": 6001, "bars": 1 }),
        json!({ "bpm": 120, "bars": 0 }),
        json!({ "bpm": 120, "bars": 1, "duration_sec": 1 }),
        json!({ "bpm": 120 }),
        json!({ "bpm": 120, "bars": 1, "beats_per_bar": 0 }),
        json!({ "bpm": 120, "bars": 1, "beats_per_bar": 33 }),
        json!({ "bpm": 120, "bars": 1, "offset_sec": -0.01 }),
        json!({ "bpm": 120, "duration_sec": 1, "offset_sec": 1 }),
        json!({ "bpm": 120, "duration_sec": 601 }),
        json!({ "bpm": 120, "bars": 999999 }),
        json!({ "bpm": 120, "bars": 1, "amplitude": -0.1 }),
        json!({ "bpm": 120, "bars": 1, "amplitude": 1.1 }),
    ];
    let mut p = Project::empty();
    for args in invalid {
        let before = p.store.head();
        assert!(
            matches!(
                p.call("generate_click_track", args.clone()),
                ToolResult::Error(_)
            ),
            "accepted {args}"
        );
        assert_eq!(p.store.head(), before, "changed head for {args}");
    }
    p.load_wav(44_100, 2, None);
    let before = p.store.head();
    assert!(matches!(
        p.call("generate_click_track", json!({ "track": 0, "bars": 1 })),
        ToolResult::Error(_)
    ));
    assert_eq!(p.store.head(), before, "no steady beat must not mutate");
}
