//! `cut_range` and `trim` operate on the track's timeline, so they have to
//! see every clip on it.
//!
//! Both used to read `clips.first()`, rewrite that one clip, and assign the
//! result over `track.clips` — which threw away every other clip on the
//! track. A track split by an earlier interior cut therefore lost its tail
//! the moment you cut or trimmed it again, with no error to say so.
//!
//! Both also measured the track as `max(clip.length)` rather than the
//! furthest clip *end*, so a range past the longest single clip was
//! rejected as out of bounds even though the timeline reached it.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 8_000;

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

/// A ramp, so every rendered frame names the source frame it came from.
fn write_ramp_wav(dir: &Path) -> PathBuf {
    let path = dir.join("in.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SOURCE_FRAMES {
        let s = 0.5 * n as f32 / SOURCE_FRAMES as f32;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

struct Fixture {
    _tmp: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    src: PathBuf,
    out: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = TempDir::new().expect("tempdir");
    let store = session::Store::open(tmp.path()).expect("open store");
    let src = write_ramp_wav(tmp.path());
    let out = tmp.path().join("out.wav");
    Fixture {
        store,
        engine: audio_engine::Engine::new(),
        dispatcher: ToolDispatcher::default_dispatcher(),
        src,
        out,
        _tmp: tmp,
    }
}

/// Which source frame the rendered frame at `i` came from, recovered from
/// the ramp's amplitude.
fn source_frame_of(sample: i16) -> f32 {
    (sample as f32 / 32_768.0) / 0.5 * SOURCE_FRAMES as f32
}

fn read_samples(path: &Path) -> Vec<i16> {
    let mut reader = WavReader::open(path).expect("open output wav");
    reader
        .samples::<i16>()
        .map(|r| r.expect("sample"))
        .collect()
}

/// Cut twice. The second cut must remove its own range and leave the rest
/// of the track — including everything after the first cut — intact.
#[test]
fn a_second_cut_keeps_the_rest_of_a_split_track() {
    let mut f = fixture();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut f.store,
        engine: &mut f.engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    ok(f.dispatcher
        .invoke("load", json!({ "path": f.src.to_string_lossy() }), &mut ctx)
        .unwrap());

    // First cut: [1000, 2000) out of the middle. Timeline is now 7000
    // frames, in two clips.
    ok(f.dispatcher
        .invoke(
            "cut_range",
            json!({ "track": 0, "start_sample": 1000, "end_sample": 2000 }),
            &mut ctx,
        )
        .unwrap());

    // Second cut: [5000, 6000) — which lands in the *second* clip, past
    // the end of the first. The old code measured the track as
    // max(clip.length) = 5000 and would reject or mis-place this.
    let cut = ok(f
        .dispatcher
        .invoke(
            "cut_range",
            json!({ "track": 0, "start_sample": 5000, "end_sample": 6000 }),
            &mut ctx,
        )
        .unwrap());
    let node_id = cut["node_id"].as_str().unwrap().to_string();

    ok(f.dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": node_id,
                "format": "wav",
                "out_path": f.out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let got = read_samples(&f.out);
    assert_eq!(
        got.len(),
        6000,
        "two 1000-frame cuts from an 8000-frame track leave 6000"
    );

    // Timeline frame 0 is source 0; frame 1000 is source 2000 (first cut);
    // frame 4000 is source 5000; frame 5000 is source 7000 (second cut).
    for (timeline, expected_source) in [(0usize, 0.0), (999, 999.0), (1000, 2000.0), (3999, 4999.0)]
    {
        let got_source = source_frame_of(got[timeline]);
        assert!(
            (got_source - expected_source).abs() < 20.0,
            "timeline frame {timeline} should be source ~{expected_source}, got {got_source}"
        );
    }
    // The frame right after the second cut is the assertion the old code
    // could not pass: it had deleted everything past the first clip.
    let after_second_cut = source_frame_of(got[5000]);
    assert!(
        (after_second_cut - 7000.0).abs() < 20.0,
        "after the second cut the track should resume at source ~7000, got {after_second_cut}"
    );
}

/// Trimming a split track keeps the requested window of the *timeline*,
/// not just whatever fell inside the first clip.
#[test]
fn trimming_a_split_track_keeps_the_whole_window() {
    let mut f = fixture();
    let mut clipboard: Option<Vec<f32>> = None;
    let mut ctx = ToolContext {
        store: &mut f.store,
        engine: &mut f.engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };

    ok(f.dispatcher
        .invoke("load", json!({ "path": f.src.to_string_lossy() }), &mut ctx)
        .unwrap());
    ok(f.dispatcher
        .invoke(
            "cut_range",
            json!({ "track": 0, "start_sample": 1000, "end_sample": 2000 }),
            &mut ctx,
        )
        .unwrap());

    // Keep timeline [500, 4500) — 4000 frames spanning the join at 1000.
    let trim = ok(f
        .dispatcher
        .invoke(
            "trim",
            json!({ "track": 0, "start_sample": 500, "end_sample": 4500 }),
            &mut ctx,
        )
        .unwrap());
    let node_id = trim["node_id"].as_str().unwrap().to_string();

    ok(f.dispatcher
        .invoke(
            "render_final",
            json!({
                "node_id": node_id,
                "format": "wav",
                "out_path": f.out.to_string_lossy(),
            }),
            &mut ctx,
        )
        .unwrap());

    let got = read_samples(&f.out);
    assert_eq!(got.len(), 4000, "trim keeps exactly the requested window");

    // Timeline 500 was source 500; timeline 1000 was source 2000. After
    // the trim those become output frames 0 and 500.
    let first = source_frame_of(got[0]);
    assert!(
        (first - 500.0).abs() < 20.0,
        "trim should start at source ~500, got {first}"
    );
    let across_join = source_frame_of(got[500]);
    assert!(
        (across_join - 2000.0).abs() < 20.0,
        "the trimmed window should carry the second clip across the join \
         (source ~2000), got {across_join}"
    );
    let last = source_frame_of(got[3999]);
    assert!(
        (last - 5499.0).abs() < 20.0,
        "trim should end at source ~5499, got {last}"
    );
}
