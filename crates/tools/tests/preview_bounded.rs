//! A preview never leaves a file outside the project's bounded caches
//! (#535).
//!
//! `render_preview` used to render into an OS temp file first. A whole
//! session was then renamed into `.audiograph/previews/`, but a ranged
//! render, a cache that could not be written, and a render that failed
//! all left that file where it was: uncapped, outside the project, and
//! owned by nobody. That was the one reason Plan first held previews for
//! approval (#519), so this is what makes it safe to stop holding them.
//!
//! The claim is about *where files end up*, so these point the OS temp
//! directory at an empty folder of their own and look at it afterwards.
//! A leak then cannot hide among other tests' files, and cannot be
//! blamed on another process.
//!
//! The environment is process-wide, so every test here takes
//! [`Sandbox`], which holds a lock for as long as it is alive.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

const SAMPLE_RATE: u32 = 48_000;

static ENV: Mutex<()> = Mutex::new(());

/// The variables `std::env::temp_dir` reads, here and on Windows.
const TMP_VARS: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];
const CAP_VAR: &str = tools::preview_cache::CAP_ENV;

/// A project, and an OS temp directory of its own, empty.
struct Sandbox {
    _lock: MutexGuard<'static, ()>,
    _root: TempDir,
    project: PathBuf,
    os_tmp: PathBuf,
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Sandbox {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = TempDir::new().expect("tempdir");
        let project = root.path().join("project");
        let os_tmp = root.path().join("os-tmp");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&os_tmp).unwrap();

        let mut saved: Vec<_> = TMP_VARS.iter().map(|v| (*v, std::env::var_os(v))).collect();
        saved.push((CAP_VAR, std::env::var_os(CAP_VAR)));
        for var in TMP_VARS {
            std::env::set_var(var, &os_tmp);
        }
        std::env::remove_var(CAP_VAR);

        let src = write_sine(&project, "in.wav");
        let store = session::Store::open(&project).expect("open store");
        let mut s = Self {
            _lock: lock,
            _root: root,
            project,
            os_tmp,
            saved,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        };
        ok(s.call("load", json!({ "path": src.to_string_lossy() })));
        s
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

    fn head(&self) -> String {
        self.store.head().expect("a head").to_hex()
    }

    fn cap(&self, bytes: u64) {
        std::env::set_var(CAP_VAR, bytes.to_string());
    }

    fn cache(&self, name: &str) -> PathBuf {
        self.project.join(".audiograph").join(name)
    }

    /// `render_preview` of the head over `range` (whole mix if `None`).
    fn preview(&mut self, range: Option<[u64; 2]>) -> ToolResult {
        let mut args = json!({ "node_id": self.head() });
        if let Some(r) = range {
            args["range"] = json!(r);
        }
        self.call("render_preview", args)
    }

    /// Everything in the OS temp directory, by name.
    fn leaked_into_os_tmp(&self) -> Vec<String> {
        names_in(&self.os_tmp)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for (var, value) in &self.saved {
            match value {
                Some(v) => std::env::set_var(var, v),
                None => std::env::remove_var(var),
            }
        }
    }
}

fn write_sine(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SAMPLE_RATE as usize {
        let t = n as f32 / SAMPLE_RATE as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.25;
        w.write_sample((s * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    path
}

fn ok(result: ToolResult) -> Value {
    match result {
        ToolResult::Ok(v) => v,
        ToolResult::Error(msg) => panic!("expected Ok, got Error({msg})"),
    }
}

fn err(result: ToolResult) -> String {
    match result {
        ToolResult::Error(msg) => msg,
        ToolResult::Ok(v) => panic!("expected Error, got Ok({v})"),
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

/// Bytes held by the finished entries of a cache folder.
fn wav_bytes_in(dir: &Path) -> u64 {
    names_in(dir)
        .iter()
        .filter(|n| n.ends_with(".wav"))
        .map(|n| std::fs::metadata(dir.join(n)).unwrap().len())
        .sum()
}

/// What is left in a cache folder that is not a finished entry: staging
/// files, in any state.
fn staging_in(dir: &Path) -> Vec<String> {
    names_in(dir)
        .into_iter()
        .filter(|n| !n.ends_with(".wav"))
        .collect()
}

// ---------------------------------------------------------------------
// A ranged render
// ---------------------------------------------------------------------

/// The case the issue is about: a ranged render used to be the one that
/// always left its file in the OS temp directory.
#[test]
fn a_ranged_render_lands_in_the_preview_cache_and_nowhere_else() {
    let mut s = Sandbox::new();

    let excerpt = ok(s.preview(Some([100, 5_000])));
    let path = path_of(&excerpt);

    assert!(path.is_file(), "the excerpt is not at {}", path.display());
    assert_eq!(
        path.parent(),
        Some(s.cache("previews").as_path()),
        "the excerpt is outside the preview cache"
    );
    assert_eq!(excerpt["frames_written"], json!(4_900));
    assert_eq!(excerpt["cached"], json!(false));

    assert_eq!(
        s.leaked_into_os_tmp(),
        Vec::<String>::new(),
        "a ranged render left a file in the OS temp directory"
    );
    assert_eq!(
        staging_in(&s.cache("previews")),
        Vec::<String>::new(),
        "a staging file was left in the cache"
    );
}

/// An excerpt is cached like any other preview: asking again is a hit,
/// and the hit reports the excerpt's numbers, not the mix's.
#[test]
fn the_same_range_again_is_a_hit_on_the_excerpt() {
    let mut s = Sandbox::new();

    let first = ok(s.preview(Some([100, 5_000])));
    let second = ok(s.preview(Some([100, 5_000])));

    assert_eq!(second["cached"], json!(true), "the same range re-rendered");
    assert_eq!(second["path"], first["path"]);
    assert_eq!(second["frames_written"], json!(4_900));
    assert_eq!(
        std::fs::read(path_of(&second)).unwrap(),
        std::fs::read(path_of(&first)).unwrap()
    );
    assert_eq!(s.leaked_into_os_tmp(), Vec::<String>::new());
}

/// The key has to say which part of the mix a file holds. Under the
/// node's name alone, an excerpt would be served as the whole mix (the
/// reason a ranged render was not cached at all before this).
#[test]
fn a_ranged_render_does_not_collide_with_the_full_one_or_with_another_range() {
    let mut s = Sandbox::new();
    let node = s.head();

    let a = ok(s.preview(Some([0, 4_000])));
    let b = ok(s.preview(Some([0, 6_000])));
    let c = ok(s.preview(Some([1_000, 4_000])));
    let paths = [path_of(&a), path_of(&b), path_of(&c)];
    assert_ne!(paths[0], paths[1], "two ranges shared a file");
    assert_ne!(paths[0], paths[2], "two ranges shared a file");
    assert_ne!(paths[1], paths[2], "two ranges shared a file");

    // The whole mix after the excerpts is a miss, under the node's name,
    // and is the whole mix.
    let full = ok(s.preview(None));
    assert_eq!(full["cached"], json!(false), "the mix hit on an excerpt");
    assert_eq!(
        path_of(&full),
        s.cache("previews").join(format!("{node}.wav"))
    );
    assert_eq!(full["frames_written"], json!(SAMPLE_RATE));
    assert!(!paths.contains(&path_of(&full)));

    // And the excerpt is still its own, with its own numbers.
    let again = ok(s.preview(Some([0, 4_000])));
    assert_eq!(again["cached"], json!(true));
    assert_eq!(again["frames_written"], json!(4_000));
    assert_eq!(again["path"], a["path"]);
}

/// "Bounded" is the point: a run of different excerpts fills the cache
/// to its cap and no further, and the one just asked for survives.
#[test]
fn ranged_renders_stay_within_the_cache_cap() {
    let mut s = Sandbox::new();
    // Each excerpt below is 8 to 26 kB; this holds a few of them.
    let cap = 40_000;
    s.cap(cap);

    let mut last = PathBuf::new();
    for i in 0..10u64 {
        let result = ok(s.preview(Some([0, 4_000 + 1_000 * i])));
        last = path_of(&result);
        // mtime is the LRU clock, and its resolution is coarse on some
        // filesystems.
        std::thread::sleep(std::time::Duration::from_millis(15));
        assert!(
            wav_bytes_in(&s.cache("previews")) <= cap,
            "after {} renders the cache held {} bytes, over its {cap}-byte cap",
            i + 1,
            wav_bytes_in(&s.cache("previews")),
        );
    }

    assert!(last.is_file(), "the excerpt just rendered was evicted");
    let kept = names_in(&s.cache("previews")).len();
    assert!(kept < 10, "nothing was ever evicted ({kept} files kept)");
    assert_eq!(s.leaked_into_os_tmp(), Vec::<String>::new());
    assert_eq!(staging_in(&s.cache("previews")), Vec::<String>::new());
}

/// A mix too big for the cap cannot be kept. Finding that out used to cost
/// the cache everything in it, so every Play of a session past the cap
/// emptied the cache before it failed. It fails now with the cache as it
/// was.
#[test]
fn a_mix_over_the_cap_is_an_error_that_costs_the_cache_nothing() {
    let mut s = Sandbox::new();
    // Each excerpt is about 9 kB, the whole one-second mix 96 kB.
    s.cap(40_000);
    let a = path_of(&ok(s.preview(Some([0, 4_000]))));
    std::thread::sleep(std::time::Duration::from_millis(15));
    let b = path_of(&ok(s.preview(Some([0, 4_500]))));
    std::thread::sleep(std::time::Duration::from_millis(15));

    let msg = err(s.preview(None));
    assert!(msg.contains("cap"), "should say why: {msg}");

    assert!(
        a.is_file() && b.is_file(),
        "a preview that could not fit emptied the cache"
    );
    assert_eq!(s.leaked_into_os_tmp(), Vec::<String>::new());
    assert_eq!(staging_in(&s.cache("previews")), Vec::<String>::new());
}

// ---------------------------------------------------------------------
// A render that fails
// ---------------------------------------------------------------------

/// The temp file used to be created before the render, so a render that
/// failed (here: a range that starts past the end of the mix) left an
/// empty file behind.
#[test]
fn a_render_that_fails_leaves_no_file_behind() {
    let mut s = Sandbox::new();

    let msg = err(s.preview(Some([10_000_000, 10_000_100])));
    assert!(msg.contains("render failed"), "unexpected error: {msg}");

    assert_eq!(
        s.leaked_into_os_tmp(),
        Vec::<String>::new(),
        "a failed render left a file in the OS temp directory"
    );
    assert_eq!(
        names_in(&s.cache("previews")),
        Vec::<String>::new(),
        "a failed render left something in the cache"
    );
}

/// A cache that cannot be written used to be forgiven by handing back
/// the temp file, which is exactly the file this is about. There is no
/// bounded place to put the preview, so the call fails and says why.
#[test]
fn a_cache_that_cannot_be_written_is_an_error_not_a_stray_file() {
    let mut s = Sandbox::new();
    // A file where the folder should be: the one way to make a cache
    // unwritable that a test running as root cannot get around.
    std::fs::write(s.cache("previews"), b"in the way").unwrap();

    for range in [None, Some([0, 4_000])] {
        let msg = err(s.preview(range));
        assert!(
            msg.contains("cache"),
            "the error should say the cache is the problem, got: {msg}"
        );
        assert_eq!(
            s.leaked_into_os_tmp(),
            Vec::<String>::new(),
            "range {range:?}: an unwritable cache left a file in the OS temp directory"
        );
    }
}

// ---------------------------------------------------------------------
// audition_effect writes under the same rule
// ---------------------------------------------------------------------

#[test]
fn an_audition_lands_in_its_cache_and_nowhere_else() {
    let mut s = Sandbox::new();

    let result = ok(s.call(
        "audition_effect",
        json!({ "track": 0, "kind": "gain", "params": { "db": 3.0 } }),
    ));
    let path = path_of(&result);

    assert_eq!(
        path.parent(),
        Some(s.cache("auditions").as_path()),
        "an audition was written outside its cache"
    );
    assert!(path.is_file());
    assert_eq!(s.leaked_into_os_tmp(), Vec::<String>::new());
    assert_eq!(staging_in(&s.cache("auditions")), Vec::<String>::new());
    assert_eq!(
        names_in(&s.cache("previews")),
        Vec::<String>::new(),
        "an audition was written into the preview cache"
    );
}

#[test]
fn an_audition_that_fails_leaves_no_file_behind() {
    let mut s = Sandbox::new();

    // Starts past the end of the one-second session.
    let msg = err(s.call(
        "audition_effect",
        json!({
            "track": 0, "kind": "gain", "params": { "db": 3.0 },
            "start_sec": 30.0, "end_sec": 31.0,
        }),
    ));
    assert!(msg.contains("audition render failed"), "got: {msg}");

    assert_eq!(s.leaked_into_os_tmp(), Vec::<String>::new());
    assert_eq!(
        names_in(&s.cache("auditions")),
        Vec::<String>::new(),
        "a failed audition left something in its cache"
    );
}
