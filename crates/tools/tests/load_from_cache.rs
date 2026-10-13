//! A track loaded from one of the project's own caches keeps its audio
//! (#580).
//!
//! `load` points a clip at a WAV the renderer can open, where the file
//! lies, so that a render of an untouched track is the source's own
//! bytes. For a user's file that is right. For a file in
//! `.audiograph/previews/` (`render_preview`) or `.audiograph/auditions/`
//! (`audition_effect`) it was a trap: both caches evict their oldest
//! entries to stay under a cap, and an eviction deleted the audio of a
//! clip that named the entry. "Render a preview, then load it as a track"
//! is one agent turn, so the track would play until the cache next filled.
//!
//! These run the real tools and the cache's own eviction, then render the
//! session, because the claim is that the session still plays, not where a
//! path points.
//!
//! The cap is an environment variable and the environment is process-wide,
//! so every test here holds [`Project`]'s lock for as long as it lives.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::preview_cache::{PreviewCache, CACHE_DIR as PREVIEW_DIR, CAP_ENV};
use tools::tool::audition::AUDITION_DIR;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

static ENV: Mutex<()> = Mutex::new(());

/// A project with one track, `in.wav`, loaded from the project folder.
struct Project {
    _lock: MutexGuard<'static, ()>,
    saved_cap: Option<OsString>,
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Project {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved_cap = std::env::var_os(CAP_ENV);
        std::env::remove_var(CAP_ENV);

        let dir = TempDir::new().expect("tempdir");
        let src = write_sine(&dir.path().join("in.wav"), 440.0);
        let store = session::Store::open(dir.path()).expect("open store");
        let mut p = Self {
            _lock: lock,
            saved_cap,
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        ok(p.call("load", json!({ "path": src.to_string_lossy() })));
        p
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

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn store_dir(&self) -> PathBuf {
        self.root().join(session::STORE_DIR)
    }

    fn head(&self) -> String {
        self.store.head().expect("a head").to_hex()
    }

    fn state(&self) -> session::SessionState {
        self.store
            .get(self.store.head().expect("a head"))
            .expect("head node")
            .state
    }

    /// Where the clip of track `track` reads its audio.
    fn clip_source(&self, track: usize) -> PathBuf {
        self.state().tracks[track].clips[0].source_path.clone()
    }

    /// The head, rendered to a WAV, as the bytes it came out as.
    fn render(&self) -> Result<Vec<u8>, String> {
        let out = self.root().join("render.wav");
        let _ = std::fs::remove_file(&out);
        audio_engine::render_state_to_wav(&self.state(), &out, None).map_err(|e| e.to_string())?;
        std::fs::read(&out).map_err(|e| e.to_string())
    }

    /// Evict everything from the cache folder `subdir` the way the cache
    /// does when it is over its cap: its own `evict_to_cap`, with a cap
    /// no entry fits under.
    fn evict(&self, subdir: &str) {
        std::env::set_var(CAP_ENV, "1");
        let cache = PreviewCache::in_dir(self.root(), subdir);
        cache.evict_to_cap();
        std::env::remove_var(CAP_ENV);
        assert!(
            cache.is_empty(),
            "the cache should have evicted every entry: {:?}",
            std::fs::read_dir(cache.dir()).map(|d| d.flatten().count())
        );
    }

    /// Files in `.audiograph/derived/`, by name.
    fn derived(&self) -> Vec<String> {
        names_in(&self.store_dir().join("derived"))
    }

    /// Load `path` as a new track and return the track's index.
    fn load(&mut self, path: &Path) -> usize {
        let v = ok(self.call("load", json!({ "path": path.to_string_lossy() })));
        v["track_index"].as_u64().expect("a track index") as usize
    }

    /// `render_preview` of the head: the cache entry it made.
    fn preview(&mut self) -> PathBuf {
        let head = self.head();
        path_of(&ok(self.call("render_preview", json!({ "node_id": head }))))
    }

    /// `audition_effect` of a gain change on track 0: the cache entry it made.
    fn audition(&mut self) -> PathBuf {
        path_of(&ok(self.call(
            "audition_effect",
            json!({ "track": 0, "kind": "gain", "params": { "db": -6.0 } }),
        )))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        match &self.saved_cap {
            Some(v) => std::env::set_var(CAP_ENV, v),
            None => std::env::remove_var(CAP_ENV),
        }
    }
}

fn write_sine(path: &Path, freq: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn path_of(v: &Value) -> PathBuf {
    PathBuf::from(v["path"].as_str().expect("a path"))
}

/// Whether `path` is inside the folder `dir` once both are resolved.
fn is_inside(path: &Path, dir: &Path) -> bool {
    let (Ok(path), Ok(dir)) = (path.canonicalize(), dir.canonicalize()) else {
        return false;
    };
    path.starts_with(dir)
}

// ---------------------------------------------------------------------
// The bug
// ---------------------------------------------------------------------

/// **The bug, through `render_preview`.** The track is loaded from a
/// preview, the cache evicts the preview, and the session still renders
/// what it rendered before.
#[test]
fn a_preview_loaded_as_a_track_outlives_the_cache_entry() {
    let mut p = Project::new();
    let preview = p.preview();
    assert!(
        is_inside(&preview, &p.store_dir().join(PREVIEW_DIR)),
        "render_preview should have put it in the preview cache: {}",
        preview.display()
    );

    let track = p.load(&preview);
    let before = p.render().expect("renders while the entry is there");
    p.evict(PREVIEW_DIR);
    assert!(!preview.exists(), "the entry should be gone");

    let source = p.clip_source(track);
    assert!(
        !is_inside(&source, &p.store_dir().join(PREVIEW_DIR)),
        "the clip names a cache entry: {}",
        source.display()
    );
    let after = p
        .render()
        .unwrap_or_else(|e| panic!("the track lost its audio with the cache entry: {e}"));
    assert_eq!(before, after, "the render changed when the cache emptied");
}

/// **The same through `audition_effect`**, whose cache is a sibling
/// folder with the same eviction.
#[test]
fn an_audition_loaded_as_a_track_outlives_the_cache_entry() {
    let mut p = Project::new();
    let audition = p.audition();
    assert!(
        is_inside(&audition, &p.store_dir().join(AUDITION_DIR)),
        "audition_effect should have put it in the audition cache: {}",
        audition.display()
    );

    let track = p.load(&audition);
    let before = p.render().expect("renders while the entry is there");
    p.evict(AUDITION_DIR);
    assert!(!audition.exists(), "the entry should be gone");

    let source = p.clip_source(track);
    assert!(
        !is_inside(&source, &p.store_dir().join(AUDITION_DIR)),
        "the clip names a cache entry: {}",
        source.display()
    );
    let after = p
        .render()
        .unwrap_or_else(|e| panic!("the track lost its audio with the cache entry: {e}"));
    assert_eq!(before, after, "the render changed when the cache emptied");
}

// ---------------------------------------------------------------------
// What the copy is
// ---------------------------------------------------------------------

/// The copy is the project's own audio in `derived/`, named by what the
/// audio is, and it is the file's own bytes: a clip that read the entry
/// reads the same samples, in the same format, from the copy.
#[test]
fn the_copy_is_the_entry_s_own_bytes_in_derived() {
    let mut p = Project::new();
    let preview = p.preview();
    let bytes = std::fs::read(&preview).unwrap();

    let track = p.load(&preview);

    let source = p.clip_source(track);
    assert!(
        is_inside(&source, &p.store_dir().join("derived")),
        "a copy belongs in derived/: {}",
        source.display()
    );
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    // Named by the audio, as a transcode is.
    let decoded = audio_decoder::decode_file(&preview).unwrap();
    let hash =
        tools::provenance::audio_hash(&decoded.samples, decoded.sample_rate, decoded.channels);
    assert_eq!(
        source.file_name().unwrap().to_string_lossy(),
        format!("{hash}.wav")
    );
}

/// Loading the same cached audio again lands on the same file, as the same
/// audio from two places does for a transcode (#160).
#[test]
fn the_same_preview_loaded_twice_is_stored_once() {
    let mut p = Project::new();
    let preview = p.preview();

    let first = p.load(&preview);
    let derived_after_one = p.derived();
    let second = p.load(&preview);

    assert_eq!(p.clip_source(first), p.clip_source(second));
    assert_eq!(p.derived(), derived_after_one, "no second copy");
}

/// Different audio is a different file.
#[test]
fn a_preview_and_an_audition_are_two_files() {
    let mut p = Project::new();
    let preview = p.preview();
    let audition = p.audition();

    let a = p.load(&preview);
    let b = p.load(&audition);

    assert_ne!(p.clip_source(a), p.clip_source(b));
}

/// The copy outlives its entry, so nothing may delete it on the strength
/// of the entry. The history sweep removes audio only when a replay of the
/// step that made it writes the same file again; a replay of this `load`
/// reads the cache entry, which the cache is free to evict a moment after
/// the sweep has looked. So with the entry still there and the head on an
/// earlier node, the sweep at its most eager must leave the copy, and the
/// track must still play when the head comes back to it.
#[test]
fn the_history_sweep_keeps_a_copy_whose_entry_is_still_in_the_cache() {
    let mut p = Project::new();
    let before_load = p.store.head().expect("a head");
    let preview = p.preview();
    let track = p.load(&preview);
    let copy = p.clip_source(track);
    let loaded = p.store.head().expect("a head");
    assert!(copy.is_file());
    assert!(preview.is_file(), "the entry is still in the cache");

    // Only history names the copy now.
    p.store.set_head(before_load).expect("move the head back");
    let report = tools::reclaim::sweep(&p.store, 0).expect("sweep");
    assert!(
        copy.is_file(),
        "the sweep deleted the only copy of audio the cache is about to evict: {report:?}"
    );
    // It was weighed and kept, not overlooked: a replay of the `load`
    // does not write the copy, so the sweep cannot vouch for it.
    assert_eq!(report.kept_unverified, 1, "{report:?}");

    p.evict(PREVIEW_DIR);
    p.store.set_head(loaded).expect("move the head forward");
    p.render()
        .unwrap_or_else(|e| panic!("the track lost its audio: {e}"));
}

// ---------------------------------------------------------------------
// What does not change
// ---------------------------------------------------------------------

/// **A user's file stays where it is.** The copy is for the project's own
/// caches. A WAV from anywhere else is referenced in place, which is what
/// keeps a render of an untouched track the source's own bytes.
#[test]
fn a_user_file_outside_the_project_is_still_referenced_in_place() {
    let mut p = Project::new();
    let elsewhere = TempDir::new().expect("tempdir");
    let wav = write_sine(&elsewhere.path().join("take.wav"), 220.0);
    let derived_before = p.derived();

    let track = p.load(&wav);

    assert_eq!(p.clip_source(track), wav, "the clip should name the file");
    assert_eq!(p.derived(), derived_before, "nothing is copied");
}

/// The passthrough that referencing in place exists for: one track playing
/// one untouched source renders to that source's bytes.
#[test]
fn a_user_file_still_renders_to_its_own_bytes() {
    let root = TempDir::new().expect("tempdir");
    let project = root.path().join("fresh");
    std::fs::create_dir_all(&project).unwrap();
    let wav = write_sine(&root.path().join("take.wav"), 330.0);

    let mut store = session::Store::open(&project).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut ctx = ToolContext {
        store: &mut store,
        engine: &mut engine,
        user_message: "",
        clipboard: &mut clipboard,
        allowed_tools: None,
    };
    ok(dispatcher
        .invoke("load", json!({ "path": wav.to_string_lossy() }), &mut ctx)
        .unwrap());

    let state = store.get(store.head().unwrap()).unwrap().state;
    let out = root.path().join("out.wav");
    audio_engine::render_state_to_wav(&state, &out, None).expect("render");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&wav).unwrap(),
        "an untouched single source renders to its own bytes"
    );
}

/// A folder the user happens to call `previews` is theirs. Only the
/// project's `.audiograph/` is the app's.
#[test]
fn a_users_folder_named_previews_is_not_the_cache() {
    let mut p = Project::new();
    let folder = p.root().join("previews");
    std::fs::create_dir_all(&folder).unwrap();
    let wav = write_sine(&folder.join("take.wav"), 550.0);
    let derived_before = p.derived();

    let track = p.load(&wav);

    assert_eq!(p.clip_source(track), wav);
    assert_eq!(p.derived(), derived_before);
}
