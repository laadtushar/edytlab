//! A paste replays from the clipboard it recorded (#377, row 2 of #356).
//!
//! Replay runs each recorded step in a scratch project, whose clipboard
//! starts empty, so every paste failed to replay and the history sweep
//! kept its audio for good. A paste records the clipboard it spliced as
//! a blob named by the hash of its samples; replay now loads that blob
//! before the step, and refuses one that does not hold exactly the audio
//! it is named for.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SR: u32 = 48_000;

/// A 4-second tone, as 16-bit PCM or as 32-bit float. The float one holds
/// samples no 16-bit file can — what a clipboard copied from it holds
/// too.
fn write_tone(path: &Path, float: bool) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: if float { 32 } else { 16 },
        sample_format: if float {
            SampleFormat::Float
        } else {
            SampleFormat::Int
        },
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..(SR as usize * 4) {
        let v = (2.0 * std::f32::consts::PI * 440.0 * n as f32 / SR as f32).sin() * 0.3;
        if float {
            w.write_sample(v).unwrap();
        } else {
            w.write_sample((v * 32_767.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

struct Session {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Session {
    /// A take, a copy of 0.5–1.0 s, a paste of it at 2 s, and two edits
    /// after, so the paste's audio is history.
    fn pasted(float: bool) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let take = write_tone(&dir.path().join("take.wav"), float);
        let store = session::Store::open(dir.path()).expect("store");
        let mut s = Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        s.call("load", json!({ "path": take }));
        s.call(
            "copy_region",
            json!({ "track": 0, "range": { "start_sec": 0.5, "end_sec": 1.0 } }),
        );
        s.call("paste_region", json!({ "track": 0, "at": 2.0 }));
        s
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        let mut ctx = ToolContext {
            store: &mut self.store,
            engine: &mut self.engine,
            user_message: "",
            clipboard: &mut self.clipboard,
            allowed_tools: None,
        };
        match self
            .dispatcher
            .invoke(tool, args, &mut ctx)
            .expect("invoke")
        {
            ToolResult::Ok(v) => v,
            ToolResult::Error(m) => panic!("{tool} failed: {m}"),
        }
    }

    /// The derived file the head names — right after the paste, the
    /// pasted audio.
    fn head_audio(&self) -> PathBuf {
        let head = self.store.head().expect("head");
        self.store.get(head).expect("node").state.tracks[0].clips[0]
            .source_path
            .clone()
    }

    fn move_on(&mut self) {
        for start in [0.1, 3.2] {
            self.call(
                "silence_region",
                json!({ "track": 0, "start_sec": start, "end_sec": start + 0.1 }),
            );
        }
    }
}

fn assert_pasted_audio_is_swept_and_comes_back(float: bool) {
    let mut s = Session::pasted(float);
    let pasted = s.head_audio();
    let paste_node = s.store.head().expect("head");
    let bytes = std::fs::read(&pasted).expect("pasted audio");
    s.move_on();

    let report = tools::reclaim::sweep(&s.store, 0).expect("sweep");

    assert_eq!(report.kept_unverified, 0, "{report:?}");
    assert!(!pasted.is_file(), "the paste's audio was swept: {report:?}");
    tools::rederive::materialize(&s.store, paste_node).expect("rebuilt");
    assert!(
        std::fs::read(&pasted).expect("back") == bytes,
        "and came back byte for byte"
    );
}

#[test]
fn a_paste_from_16_bit_audio_is_swept_and_comes_back() {
    assert_pasted_audio_is_swept_and_comes_back(false);
}

/// The case a 16-bit blob could not survive: samples from a float source.
#[test]
fn a_paste_from_float_audio_is_swept_and_comes_back() {
    assert_pasted_audio_is_swept_and_comes_back(true);
}

/// A float take with 0.5–1.0 s copied straight from it, and the blob
/// that copy wrote. Straight from the take, because derived audio is
/// 16-bit and a 16-bit blob of it would be exact anyway.
fn copied_from_float() -> (Session, String) {
    let dir = TempDir::new().expect("tempdir");
    let take = write_tone(&dir.path().join("take.wav"), true);
    let store = session::Store::open(dir.path()).expect("store");
    let mut s = Session {
        dir,
        store,
        engine: audio_engine::Engine::new(),
        dispatcher: ToolDispatcher::default_dispatcher(),
        clipboard: None,
    };
    s.call("load", json!({ "path": take }));
    let out = s.call(
        "copy_region",
        json!({ "track": 0, "range": { "start_sec": 0.5, "end_sec": 1.0 } }),
    );
    let hash = out["clipboard_blob"]
        .as_str()
        .expect("blob hash")
        .to_string();
    (s, hash)
}

/// The blob is what the copy took, named by its samples' hash.
#[test]
fn the_clipboard_blob_reads_back_as_exactly_what_was_copied() {
    let (s, hash) = copied_from_float();
    let copied = s.clipboard.clone().expect("clipboard");

    let blob = tools::provenance::load_clipboard_blob(s.dir.path(), &hash).expect("load");

    assert_eq!(blob.sample_rate, copied.sample_rate);
    assert_eq!(blob.channels, copied.channels);
    assert!(
        blob.samples == copied.samples,
        "the same samples, bit for bit"
    );
}

/// Overwrite the blob with what the old 16-bit writer made of it.
fn make_lossy(s: &Session, hash: &str) {
    let path = tools::provenance::clipboard_blob_path(s.dir.path(), hash);
    let clip = s.clipboard.clone().expect("clipboard");
    audio_engine::write_wav(&clip.samples, clip.sample_rate, clip.channels, &path)
        .expect("lossy blob");
}

/// A blob that does not hold the audio it is named for — damaged, or
/// written lossily before blobs were float — is refused, not pasted.
#[test]
fn a_blob_that_is_not_its_name_is_refused() {
    let (s, hash) = copied_from_float();
    make_lossy(&s, &hash);

    let err = tools::provenance::load_clipboard_blob(s.dir.path(), &hash).expect_err("refused");
    assert!(err.contains("does not hold"), "{err}");
}

/// Copying again repairs a lossy blob rather than trusting the name.
#[test]
fn copying_again_replaces_a_blob_that_is_not_its_name() {
    let (mut s, hash) = copied_from_float();
    make_lossy(&s, &hash);

    s.call(
        "copy_region",
        json!({ "track": 0, "range": { "start_sec": 0.5, "end_sec": 1.0 } }),
    );

    tools::provenance::load_clipboard_blob(s.dir.path(), &hash).expect("repaired");
}
