//! The destructive `limiter` tool is a limiter, not a hard clip (#441).
//!
//! It shares its implementation with the effect-chain limiter
//! (`audio_dsp::Limiter`); the DSP is tested in `audio-dsp`. What is
//! pinned here is the tool around it: that the ceiling holds through the
//! whole destructive-edit path, that the stereo image is kept (which
//! needs the tool to know the channel count of the buffer it limits),
//! that `release_ms` is validated and does something, and that a
//! `start_sec`/`end_sec` range leaves the rest of the track alone.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::tool::LimiterTool;
use tools::{Tool, ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

/// A WAV of `channels` interleaved channels, `frames` long, where
/// `sample(frame, channel)` gives each value.
fn write_wav(
    dir: &Path,
    name: &str,
    channels: u16,
    frames: usize,
    sample: impl Fn(usize, usize) -> f32,
) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for f in 0..frames {
        for c in 0..channels as usize {
            let v = (sample(f, c) * 32_767.0).clamp(-32_768.0, 32_767.0);
            w.write_sample(v as i16).unwrap();
        }
    }
    w.finalize().unwrap();
    path
}

fn tone(freq: f32, amp: f32, frame: usize) -> f32 {
    amp * (2.0 * std::f32::consts::PI * freq * frame as f32 / SAMPLE_RATE as f32).sin()
}

struct Session {
    _dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    /// A session whose only track plays `build(dir)`.
    fn with_track(build: impl FnOnce(&Path) -> PathBuf) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let wav = build(dir.path());
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            _dir: dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        ok(s.call("load", json!({ "path": wav.to_string_lossy() })));
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        self.try_call(tool, args).unwrap()
    }

    /// Through the dispatcher, which checks `args` against the tool's
    /// schema before the tool sees them.
    fn try_call(&mut self, tool: &str, args: Value) -> tools::Result<ToolResult> {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        self.dispatcher.invoke(tool, args, &mut ctx)
    }

    /// Straight to the tool, skipping the dispatcher's schema check, so
    /// the tool's own validation is what answers.
    fn call_tool_directly(&mut self, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        LimiterTool.invoke(args, &mut ctx).unwrap()
    }

    /// The samples now on track 0, as `(channels, interleaved)`.
    fn track_audio(&self) -> (usize, Vec<f32>) {
        let head = self.store.head().expect("a head");
        let state = self.store.get(head).expect("head node").state;
        let path = &state.tracks[0].clips[0].source_path;
        let mut reader = WavReader::open(path).expect("open track audio");
        let channels = reader.spec().channels as usize;
        let samples = reader
            .samples::<i16>()
            .map(|s| f32::from(s.expect("sample")) / 32_768.0)
            .collect();
        (channels, samples)
    }
}

fn ok(r: ToolResult) -> Value {
    match r {
        ToolResult::Ok(v) => v,
        ToolResult::Error(m) => panic!("expected Ok, got Error({m})"),
    }
}

fn err(r: ToolResult) -> String {
    match r {
        ToolResult::Error(m) => m,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
    }
}

fn ceiling(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

fn rms(x: impl Iterator<Item = f32>) -> f32 {
    let (sum, n) = x.fold((0.0f32, 0usize), |(s, n), v| (s + v * v, n + 1));
    (sum / n.max(1) as f32).sqrt()
}

/// Through the whole tool, the ceiling holds, and what comes out is a
/// sine with its gain turned down rather than a sine with its top cut
/// off. A hard clip at twice the ceiling leaves two thirds of the
/// samples on the ceiling; a limited sine leaves a few percent, around
/// its crest.
#[test]
fn the_tool_holds_the_ceiling_without_flattening_the_peaks() {
    let mut s = Session::with_track(|dir| {
        write_wav(dir, "loud.wav", 1, SAMPLE_RATE as usize, |f, _| {
            tone(1_000.0, 1.0, f)
        })
    });
    ok(s.call("limiter", json!({ "track": 0, "ceiling_db": -6.0 })));

    let (channels, out) = s.track_audio();
    assert_eq!(channels, 1);
    let c = ceiling(-6.0);
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // 16-bit quantisation, once on the way in and once on the way out.
    assert!(
        peak <= c + 2.0 / 32_768.0,
        "peak {peak} is over the ceiling {c}"
    );
    assert!(
        peak >= c - 2.0 / 32_768.0,
        "peak {peak} is well under the ceiling {c}"
    );

    let flat = out.iter().filter(|v| v.abs() > c * 0.995).count() as f32 / out.len() as f32;
    assert!(
        flat < 0.15,
        "{:.0}% of samples sit at the ceiling: the peaks were flattened, not limited",
        flat * 100.0
    );
}

/// The stereo image survives. A loud left channel over a quiet right
/// one, as in a hard-panned lead over a pad: a per-channel limiter would
/// cut the left and leave the right alone, changing their balance for as
/// long as the limiter is working. One gain for both keeps the ratio.
///
/// This also pins that the tool limits with the channel count of the
/// buffer it was given. It used to decode the first clip's source to
/// find out, and fall back to mono on failure.
#[test]
fn stereo_balance_is_preserved() {
    let mut s = Session::with_track(|dir| {
        write_wav(dir, "pan.wav", 2, SAMPLE_RATE as usize, |f, c| {
            if c == 0 {
                tone(440.0, 1.0, f)
            } else {
                tone(440.0, 0.1, f)
            }
        })
    });
    ok(s.call("limiter", json!({ "track": 0, "ceiling_db": -6.0 })));

    let (channels, out) = s.track_audio();
    assert_eq!(channels, 2);
    let left = rms(out.iter().step_by(2).copied());
    let right = rms(out.iter().skip(1).step_by(2).copied());
    let ratio = right / left;
    assert!(
        (ratio - 0.1).abs() < 0.002,
        "right/left balance went from 0.100 to {ratio:.4}; the channels were limited separately"
    );
    assert!(left < ceiling(-6.0), "the loud channel was not limited");
}

/// A longer release holds the level down for longer after a loud
/// moment. A burst, then quiet audio: how much of the quiet audio is
/// left a fifth of a second later depends on the release.
#[test]
fn release_ms_sets_how_long_the_level_stays_down() {
    let level_after = |release: Option<f64>| -> f32 {
        let mut s = Session::with_track(|dir| {
            write_wav(dir, "burst.wav", 1, SAMPLE_RATE as usize, |f, _| {
                // 50 ms at full scale, then a quiet tone.
                if f < 2_400 {
                    tone(200.0, 1.0, f)
                } else {
                    tone(200.0, 0.1, f)
                }
            })
        });
        let mut args = json!({ "track": 0, "ceiling_db": -12.0 });
        if let Some(r) = release {
            args["release_ms"] = json!(r);
        }
        ok(s.call("limiter", args));
        let (_, out) = s.track_audio();
        // 250-300 ms in: 200 ms after the burst ended. The input there
        // has an RMS of 0.1 / sqrt(2).
        let window = &out[12_000..14_400];
        rms(window.iter().copied()) / (0.1 / 2.0f32.sqrt())
    };

    let quick = level_after(Some(5.0));
    let slow = level_after(Some(1_000.0));
    let default = level_after(None);

    assert!(
        quick > 0.99,
        "a 5 ms release should be long gone by 200 ms, level {quick:.3}"
    );
    assert!(
        slow < 0.6,
        "a 1 s release should still be holding the level down, level {slow:.3}"
    );
    assert!(
        default > slow && default < quick,
        "the default release should sit between: quick {quick:.3}, default {default:.3}, slow {slow:.3}"
    );
}

#[test]
fn release_ms_out_of_range_is_rejected_not_clamped() {
    let mut s =
        Session::with_track(|dir| write_wav(dir, "t.wav", 1, 4_800, |f, _| tone(440.0, 0.5, f)));
    let head = s.store.head();

    for bad in [0.0, -5.0, 0.5, 10_000.0] {
        let args = json!({ "track": 0, "ceiling_db": -1.0, "release_ms": bad });

        // The schema's minimum and maximum stop it at the dispatcher...
        let refused = s
            .try_call("limiter", args.clone())
            .expect_err("the schema should refuse it")
            .to_string();
        assert!(
            refused.contains("release_ms"),
            "release_ms {bad}: {refused}"
        );

        // ...and the tool says the same if it is ever reached without
        // one, rather than clamping a 0 (a request for a hard clip) to
        // something else and carrying on.
        let msg = err(s.call_tool_directly(args));
        assert!(msg.contains("release_ms"), "release_ms {bad}: {msg}");
    }
    let wrong_type = s
        .try_call(
            "limiter",
            json!({ "track": 0, "ceiling_db": -1.0, "release_ms": "fast" }),
        )
        .expect_err("a string is not a number")
        .to_string();
    assert!(wrong_type.contains("release_ms"), "{wrong_type}");
    assert_eq!(s.store.head(), head, "a refused call must not add a node");

    // The edge values are fine, and so is leaving it out, which is every
    // call written before the parameter existed.
    for good in [1.0, 80.0, 5_000.0] {
        ok(s.call(
            "limiter",
            json!({ "track": 0, "ceiling_db": -1.0, "release_ms": good }),
        ));
    }
    ok(s.call("limiter", json!({ "track": 0, "ceiling_db": -1.0 })));
}

#[test]
fn the_schema_advertises_release_ms_as_optional() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let schema = dispatcher
        .tool_schemas()
        .as_array()
        .expect("an array of schemas")
        .iter()
        .find(|s| s["name"] == "limiter")
        .expect("limiter is registered")
        .clone();
    let props = &schema["input_schema"]["properties"];
    assert!(
        props.get("release_ms").is_some(),
        "no release_ms in {schema}"
    );
    let required: Vec<&str> = schema["input_schema"]["required"]
        .as_array()
        .expect("required list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(required, ["track", "ceiling_db"]);
    let description = schema["description"].as_str().unwrap_or_default();
    assert!(
        !description.to_lowercase().contains("hard-clip") && !description.contains("Brick-wall"),
        "the description still calls it a clip: {description}"
    );
}

/// A range edit touches only its range. Audio before it is untouched
/// by construction; audio after it must be too, which means the gain
/// reduction at the end of the range is not carried across.
#[test]
fn a_range_leaves_the_rest_of_the_track_alone() {
    let frames = SAMPLE_RATE as usize; // one second
    let mut s =
        Session::with_track(|dir| write_wav(dir, "r.wav", 1, frames, |f, _| tone(300.0, 0.9, f)));
    let (_, before) = s.track_audio();
    ok(s.call(
        "limiter",
        json!({ "track": 0, "ceiling_db": -12.0, "start_sec": 0.25, "end_sec": 0.5 }),
    ));
    let (_, after) = s.track_audio();
    assert_eq!(before.len(), after.len());

    let (start, end) = (SAMPLE_RATE as usize / 4, SAMPLE_RATE as usize / 2);
    let c = ceiling(-12.0);
    for i in 0..before.len() {
        if (start..end).contains(&i) {
            assert!(
                after[i].abs() <= c + 2.0 / 32_768.0,
                "frame {i} is over the ceiling"
            );
        } else {
            assert!(
                (after[i] - before[i]).abs() <= 1.0 / 32_768.0,
                "frame {i} is outside the range but changed: {} -> {}",
                before[i],
                after[i]
            );
        }
    }
    assert!(
        before[start..end].iter().any(|v| v.abs() > c),
        "the range never needed limiting, so the test proves nothing"
    );
}
