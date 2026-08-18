//! Tags survive onto the exported file (#170).
//!
//! A podcast episode used to ship with no title, artist or album,
//! because neither encoder wrote any. So the test that matters is not
//! that the tool accepts a `metadata` argument — it is that the bytes
//! come back out of the finished file.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

fn write_sine(path: &Path) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<Vec<f32>>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let src = write_sine(&dir.path().join("in.wav"));
        let store = session::Store::open(dir.path()).expect("open store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": src.to_string_lossy() }));
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> ToolResult {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
        };
        self.dispatcher.invoke(tool, args, &mut ctx).unwrap()
    }

    fn head(&self) -> String {
        self.store.head().expect("a head").to_hex()
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

/// **MP3.** An ID3v2 tag is a prefix, so the test reads the head of the
/// file and expects both the header and the text in it.
#[test]
fn an_mp3_export_carries_its_id3_tag() {
    let mut s = Session::new();
    let out = s.dir.path().join("ep.mp3");
    let node = s.head();

    let v = ok(s.call(
        "render_final",
        json!({
            "node_id": node,
            "format": "mp3",
            "out_path": out.to_string_lossy(),
            "metadata": {
                "title": "Episode 12",
                "artist": "A Podcast",
                "album": "Season 2",
                "year": "2026",
                "comment": "one take",
            },
        }),
    ));
    assert_eq!(v["tagged"], json!(true), "{v}");

    let bytes = std::fs::read(&out).expect("the exported mp3");
    assert_eq!(&bytes[0..3], b"ID3", "the file must start with the tag");
    let head = String::from_utf8_lossy(&bytes[..2048.min(bytes.len())]);
    for expected in ["Episode 12", "A Podcast", "Season 2", "2026", "one take"] {
        assert!(head.contains(expected), "missing {expected:?} in the tag");
    }

    // And the audio is still there behind the tag.
    assert!(
        bytes.len() > 4096,
        "the export is too small to contain audio: {} bytes",
        bytes.len()
    );
}

/// **FLAC.** Vorbis comments are blocks inside the stream, read back
/// here with the same decoder that wrote them.
#[test]
fn a_flac_export_carries_its_vorbis_comments() {
    let mut s = Session::new();
    let out = s.dir.path().join("ep.flac");
    let node = s.head();

    let v = ok(s.call(
        "render_final",
        json!({
            "node_id": node,
            "format": "flac",
            "out_path": out.to_string_lossy(),
            "metadata": { "title": "Episode 12", "artist": "A Podcast" },
        }),
    ));
    assert_eq!(v["tagged"], json!(true), "{v}");

    let tags = audio_engine::read_flac_tags(&out).expect("read the tags back");
    let find = |key: &str| {
        tags.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(find("TITLE"), Some("Episode 12"));
    assert_eq!(find("ARTIST"), Some("A Podcast"));

    // The audio still decodes — tagging must not disturb it.
    let decoded = audio_decoder::decode_file(&out).expect("decode the tagged flac");
    assert_eq!(decoded.sample_rate, SAMPLE_RATE);
    assert!(!decoded.samples.is_empty());
}

/// WAV has no tag container worth using, and saying so beats writing a
/// file the tags silently fell off.
#[test]
fn tagging_a_wav_is_refused_with_a_reason() {
    let mut s = Session::new();
    let out = s.dir.path().join("ep.wav");
    let node = s.head();

    let msg = err(s.call(
        "render_final",
        json!({
            "node_id": node,
            "format": "wav",
            "out_path": out.to_string_lossy(),
            "metadata": { "title": "Episode 12" },
        }),
    ));
    assert!(msg.contains("flac or mp3"), "should say what to do: {msg}");
    assert!(!out.exists(), "and refuse before writing anything");
}

/// Markers become chapters only when asked. A marker is a working
/// annotation and not every one is a chapter worth shipping.
#[test]
fn markers_become_chapters_only_on_request() {
    let mut s = Session::new();
    let node = s.head();
    ok(s.call("label", json!({ "time": 0.0, "name": "Intro" })));
    ok(s.call("label", json!({ "time": 0.5, "name": "The interview" })));
    // Labels append nodes, so export the head they produced rather than
    // the one captured before them.
    let _ = node;
    let node = s.head();

    let without = s.dir.path().join("plain.mp3");
    let v = ok(s.call(
        "render_final",
        json!({
            "node_id": node,
            "format": "mp3",
            "out_path": without.to_string_lossy(),
            "metadata": { "title": "Episode 12" },
        }),
    ));
    assert_eq!(v["chapters"], json!(0), "markers must not travel unasked");

    let with = s.dir.path().join("chaptered.mp3");
    let v = ok(s.call(
        "render_final",
        json!({
            "node_id": node,
            "format": "mp3",
            "out_path": with.to_string_lossy(),
            "metadata": { "title": "Episode 12" },
            "markers_as_chapters": true,
        }),
    ));
    assert_eq!(v["chapters"], json!(2), "{v}");

    let bytes = std::fs::read(&with).expect("the chaptered export");
    let head = String::from_utf8_lossy(&bytes[..4096.min(bytes.len())]);
    assert!(head.contains("Intro"), "the chapter names have to be in it");
    assert!(head.contains("The interview"));
}

/// No metadata argument means no tag, and the export is unchanged from
/// what it always was.
#[test]
fn an_untagged_export_is_untouched() {
    let mut s = Session::new();
    let out = s.dir.path().join("plain.mp3");
    let node = s.head();

    let v = ok(s.call(
        "render_final",
        json!({ "node_id": node, "format": "mp3", "out_path": out.to_string_lossy() }),
    ));
    assert!(v.get("tagged").is_none(), "nothing to report: {v}");

    let bytes = std::fs::read(&out).expect("the export");
    assert_ne!(&bytes[0..3], b"ID3", "an untagged mp3 has no tag on it");
}
