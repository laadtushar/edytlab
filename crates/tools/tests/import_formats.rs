//! Ogg Vorbis, AIFF and (where the operating system decodes it) M4A import
//! through the tools (#764).
//!
//! `audio-decoder` has the per-format decode tests. These check the path the
//! rest of the app takes: `load` decodes the file and, since the renderer
//! reads WAV only, writes the audio under a name made by
//! `provenance::audio_hash` into `derived/`, exactly as it does for an MP3
//! or a FLAC; `batch_apply` takes the new extensions; a format that is not
//! supported is refused by name and writes no node. M4A/AAC is decoded by the
//! system on macOS and Windows and refused on Linux, so those tests ask
//! `audio_decoder::OS_DECODER_AVAILABLE` what to expect.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../audio-decoder/tests/fixtures")
        .join(name)
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = session::Store::open(dir.path()).expect("open store");
        Self {
            dir,
            store,
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

    /// Where the head's first clip reads its audio from.
    fn first_clip_source(&self) -> PathBuf {
        let head = self.store.head().expect("a head");
        self.store.get(head).unwrap().state.tracks[0].clips[0]
            .source_path
            .clone()
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

/// (fixture, rate, channels): every format this platform opens beyond WAV/MP3/FLAC.
fn imports() -> Vec<(&'static str, u32, u16)> {
    let mut all = vec![
        ("tone_440_mono_44100.ogg", 44_100, 1),
        ("tones_440_880_stereo_48000.ogg", 48_000, 2),
        ("tone_440_mono_44100_s16.aiff", 44_100, 1),
        ("tones_440_880_stereo_48000_s24.aiff", 48_000, 2),
    ];
    if audio_decoder::OS_DECODER_AVAILABLE {
        all.push(("tone_440_mono_44100.m4a", 44_100, 1));
        all.push(("tones_440_880_stereo_48000.m4a", 48_000, 2));
    }
    all
}

#[test]
fn load_takes_each_new_format_the_way_it_takes_an_mp3() {
    for (name, rate, channels) in imports() {
        let mut s = Session::new();
        let src = s.dir.path().join(name);
        std::fs::copy(fixture(name), &src).unwrap();

        let v = ok(s.call("load", json!({ "path": src.to_string_lossy() })));
        assert_eq!(v["sample_rate"], json!(rate), "{name}: {v}");
        assert_eq!(v["channels"], json!(channels), "{name}: {v}");

        // The clip reads a WAV in `derived/` named by the audio's hash.
        let decoded = audio_decoder::decode_file(&src).unwrap();
        let expected = s
            .dir
            .path()
            .join(".audiograph")
            .join("derived")
            .join(format!(
                "{}.wav",
                tools::provenance::audio_hash(
                    &decoded.samples,
                    decoded.sample_rate,
                    decoded.channels
                )
            ));
        let source = s.first_clip_source();
        assert_eq!(
            source.canonicalize().unwrap(),
            expected.canonicalize().unwrap(),
            "{name}: the transcode is not named by audio_hash"
        );
        assert!(
            audio_decoder::WavStreamReader::open(&source).is_ok(),
            "{name}: the renderer cannot read the transcode"
        );

        // And it renders.
        let node = v["node_id"].as_str().unwrap().to_string();
        let out = s.dir.path().join(format!("{name}.rendered.wav"));
        ok(s.call(
            "render_final",
            json!({ "node_id": node, "format": "wav", "out_path": out.to_string_lossy() }),
        ));
        let back = audio_decoder::decode_file(&out).unwrap();
        assert_eq!(back.channels, channels, "{name}");
        assert_eq!(back.sample_rate, rate, "{name}");
        let (a, b) = (decoded.samples.len(), back.samples.len());
        assert!(
            a.abs_diff(b) <= 2 * channels as usize * 64,
            "{name}: loaded {a} samples, rendered {b}"
        );
    }
}

#[test]
fn an_aiff_is_transcoded_once_and_a_wav_of_the_same_audio_is_not_copied() {
    // AIFF 16-bit mono, and a WAV written from exactly what it decodes to.
    let mut s = Session::new();
    let aiff = s.dir.path().join("tone.aiff");
    std::fs::copy(fixture("tone_440_mono_44100_s16.aiff"), &aiff).unwrap();
    let d = audio_decoder::decode_file(&aiff).unwrap();
    let wav = s.dir.path().join("tone.wav");
    let mut w = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for x in &d.samples {
        w.write_sample((x * 32_768.0).round() as i16).unwrap();
    }
    w.finalize().unwrap();

    ok(s.call("load", json!({ "path": aiff.to_string_lossy() })));
    let first = s.first_clip_source();
    ok(s.call("load", json!({ "path": wav.to_string_lossy() })));
    // The WAV is streamable, so it is used where it is; the AIFF's transcode
    // is the only file in derived/.
    let derived = std::fs::read_dir(s.dir.path().join(".audiograph/derived"))
        .unwrap()
        .flatten()
        .count();
    assert_eq!(derived, 1);
    assert!(first.starts_with(s.dir.path().join(".audiograph/derived")));
}

#[test]
fn batch_apply_picks_up_ogg_and_aiff_and_skips_m4a_where_the_system_cannot_decode_it() {
    let mut s = Session::new();
    let template = s.dir.path().join("template.aiff");
    std::fs::copy(fixture("tone_440_mono_44100_s16.aiff"), &template).unwrap();
    ok(s.call("load", json!({ "path": template.to_string_lossy() })));
    ok(s.call("gain", json!({ "track": 0, "db": -6.0 })));
    let recipe = s.dir.path().join("chain.json");
    ok(s.call(
        "export_recipe",
        json!({ "out_path": recipe.to_string_lossy(), "name": "quieten" }),
    ));

    let takes = s.dir.path().join("takes");
    std::fs::create_dir_all(&takes).unwrap();
    std::fs::copy(fixture("tone_440_mono_44100.ogg"), takes.join("a.ogg")).unwrap();
    std::fs::copy(
        fixture("tone_440_mono_44100_s16.aiff"),
        takes.join("b.AIFF"),
    )
    .unwrap();
    std::fs::copy(fixture("tone_440_mono_44100_s16.aiff"), takes.join("c.aif")).unwrap();
    // Not importable here: skipped with the other non-audio files, not a
    // failure (on macOS and Windows the system decodes M4A, so it is taken).
    std::fs::copy(fixture("tone_440_mono_44100.m4a"), takes.join("d.m4a")).unwrap();
    std::fs::copy(fixture("tone_440_mono_48000.opus"), takes.join("e.opus")).unwrap();

    let v = ok(s.call(
        "batch_apply",
        json!({
            "recipe_path": recipe.to_string_lossy(),
            "input_dir": takes.to_string_lossy(),
            "output_dir": s.dir.path().join("out").to_string_lossy(),
        }),
    ));
    let expected = 3 + usize::from(audio_decoder::OS_DECODER_AVAILABLE);
    assert_eq!(v["files"], json!(expected), "{v}");
    assert_eq!(v["succeeded"], json!(expected), "{v}");
}

#[test]
fn load_refuses_opus_by_name_and_writes_nothing() {
    let mut s = Session::new();
    let name = "tone_440_mono_48000.opus";
    let src = s.dir.path().join(name);
    std::fs::copy(fixture(name), &src).unwrap();
    let msg = err(s.call("load", json!({ "path": src.to_string_lossy() })));
    assert!(msg.contains("Opus"), "{name}: {msg}");
    assert!(msg.contains("not supported"), "{name}: {msg}");
    assert!(
        s.store.head().is_none(),
        "{name}: a refused load wrote a node"
    );
}

/// Linux has no system AAC decoder, so `load` names the format and writes no
/// node. (macOS and Windows decode the same file: `imports()`.)
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn load_refuses_m4a_by_name_and_writes_nothing_where_the_system_has_no_decoder() {
    let mut s = Session::new();
    let name = "tone_440_mono_44100.m4a";
    let src = s.dir.path().join(name);
    std::fs::copy(fixture(name), &src).unwrap();
    let msg = err(s.call("load", json!({ "path": src.to_string_lossy() })));
    assert!(msg.contains("M4A"), "{name}: {msg}");
    assert!(msg.contains("not supported"), "{name}: {msg}");
    assert!(
        s.store.head().is_none(),
        "{name}: a refused load wrote a node"
    );
}

#[test]
fn the_load_description_names_what_it_opens() {
    let schemas = ToolDispatcher::default_dispatcher().tool_schemas();
    let load = schemas
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "load")
        .expect("load schema");
    let d = load["description"].as_str().unwrap();
    for word in ["WAV", "MP3", "FLAC", "Ogg Vorbis", "AIFF"] {
        assert!(
            d.contains(word),
            "load's description does not name {word}: {d}"
        );
    }
    // M4A is named with where it works, and what does not is said as not
    // supported, so the model does not offer a format that fails. The text is
    // the same on every platform: the tool list is one document.
    assert!(
        d.contains("M4A/AAC on macOS and Windows"),
        "load's description should say where M4A/AAC opens: {d}"
    );
    assert!(
        d.contains("M4A/AAC on Linux and Opus are not supported"),
        "load's description should say what is not supported: {d}"
    );
}
