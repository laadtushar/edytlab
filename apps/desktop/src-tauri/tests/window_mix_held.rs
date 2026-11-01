//! The mix the window plays is held in the preview cache (#607).
//!
//! The transport plays `render_preview`'s path, an entry in
//! `<project>/.audiograph/previews/`. That cache evicts its least recently
//! used entries once it is over a cap, and nothing held the entry the window
//! was playing. WaveSurfer plays a loaded mix from a blob, so losing the file
//! does not stop a play that is under way; but the Graph and Transcript tabs
//! unmount the timeline, coming back loads the file again, and an entry the
//! agent's renders, auditions or a compare had evicted meanwhile was a
//! "Could not load the mix" for the mix the user was just playing.
//!
//! The window's own render (`render_preview`) now holds the entry it returns
//! (`AppState::window_mix`): a held entry is skipped by eviction and counted
//! outside the cap, so the cache is over its cap by at most that one entry.
//! The next window render takes the hold over from the one before, and a
//! project change lets it go. The agent's tools render into the same cache
//! with no hold of their own and never take the window's.
//!
//! The environment is process-wide (the cache's cap is read from it), so
//! every test takes [`Sandbox`], which holds a lock for as long as it is
//! alive.
//!
//! Windows: gated like `commands_mock.rs`, for the same reason (Wry's
//! WebView2 imports).

#![cfg(not(target_os = "windows"))]

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::json;
use session::NodeId;

#[macro_use]
mod support;
use support::MixBed;

static ENV: Mutex<()> = Mutex::new(());

const CAP_VAR: &str = tools::preview_cache::CAP_ENV;

/// mtime is the cache's LRU clock, and its resolution is coarse on some
/// filesystems: a gap between renders makes the order one the test chose.
const TICK: Duration = Duration::from_millis(20);

/// A project with a load and five edits after it, opened over IPC.
///
/// Every node is the same length, so every whole-mix render is the same
/// size, which lets a test set the cache's cap in renders.
struct Sandbox {
    _lock: MutexGuard<'static, ()>,
    bed: MixBed,
    /// A second, empty project folder, for a test that changes project.
    other: PathBuf,
    saved_cap: Option<std::ffi::OsString>,
}

sandbox_derefs_to_bed!(Sandbox);

impl Sandbox {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("project");
        let other = root.path().join("other");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        let saved_cap = std::env::var_os(CAP_VAR);
        std::env::remove_var(CAP_VAR);

        Self {
            _lock: lock,
            bed: MixBed::open(root, project),
            other,
            saved_cap,
        }
    }

    /// The size of one whole-mix render, from a render the agent's tool
    /// made, which holds nothing. The entry is removed again.
    fn render_bytes(&self) -> u64 {
        let path = self.agent_render(self.nodes[0]);
        let bytes = std::fs::metadata(&path).expect("rendered").len();
        std::fs::remove_file(path).expect("remove the measuring render");
        bytes
    }

    fn cap(&self, bytes: u64) {
        std::env::set_var(CAP_VAR, bytes.to_string());
    }

    /// Anything in the cache folder that is not a finished entry.
    fn staging_files(&self) -> Vec<String> {
        std::fs::read_dir(self.previews())
            .map(|read| {
                read.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| !n.ends_with(".wav"))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// An entry of `bytes` bytes under a key no node has, rendered through
    /// the cache like any other: pressure on the cap from a render that
    /// needs no audio, for a test whose nodes cannot be rendered again.
    fn fill(&self, bytes: u64, key: u8) {
        std::thread::sleep(TICK);
        tools::PreviewCache::new(&self.project)
            .get_or_render::<_, std::io::Error>(NodeId([key; 32]), |staged| {
                std::fs::write(staged, vec![0u8; bytes as usize])
            })
            .expect("fill the cache");
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        match &self.saved_cap {
            Some(v) => std::env::set_var(CAP_VAR, v),
            None => std::env::remove_var(CAP_VAR),
        }
    }
}

// ---------------------------------------------------------------------
// The window's mix is held
// ---------------------------------------------------------------------

/// The reported case: the mix the window plays is the cache's least
/// recently used entry, and the agent's renders (and auditions, and graph
/// previews) go past the cap. Without the hold the first of them takes it,
/// and coming back to the timeline cannot load it.
///
/// The agent's own renders are ordinary entries all the same: the cache
/// stays bounded by evicting them, and the one the window plays is what is
/// left over the cap.
#[test]
fn the_mix_the_window_plays_outlives_renders_that_evict() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    // Room for one render and a half.
    let cap = one * 3 / 2;
    s.cap(cap);

    let played = s.window_render(s.nodes[1]).expect("render_preview");
    assert_eq!(played, s.entry(s.nodes[1]));
    for node in [s.nodes[2], s.nodes[3], s.nodes[4]] {
        std::thread::sleep(TICK);
        s.agent_render(node);
    }

    assert!(
        s.entry(s.nodes[1]).is_file(),
        "the mix the window plays was evicted by renders the agent made"
    );
    assert!(
        !s.entry(s.nodes[2]).exists() && !s.entry(s.nodes[3]).exists(),
        "the agent's older renders should be what the cache gave up"
    );
    assert!(
        s.bytes_of(&[s.nodes[2], s.nodes[3], s.nodes[4]]) <= cap,
        "the renders nobody holds are over the cap"
    );
    assert_eq!(s.staging_files(), Vec::<String>::new());
}

/// A hit is held too: Play on a mix that is already cached (the usual case
/// after undo or redo) is the window's render like any other.
#[test]
fn a_cached_mix_is_held_when_the_window_asks_for_it() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one * 3 / 2);
    s.agent_render(s.nodes[1]);

    let played = s.window_render(s.nodes[1]).expect("render_preview");
    assert_eq!(played, s.entry(s.nodes[1]));
    for node in [s.nodes[2], s.nodes[3]] {
        std::thread::sleep(TICK);
        s.agent_render(node);
    }

    assert!(
        s.entry(s.nodes[1]).is_file(),
        "a cache hit was not held by the window"
    );
}

/// The window plays one mix at a time: its next render takes over from the
/// one before, which counts against the cap again and goes first.
#[test]
fn a_new_window_render_releases_the_one_before() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one * 3 / 2);

    s.window_render(s.nodes[1]).expect("render_preview");
    std::thread::sleep(TICK);
    s.window_render(s.nodes[2]).expect("render_preview");
    std::thread::sleep(TICK);
    s.agent_render(s.nodes[3]);

    assert!(
        s.entry(s.nodes[2]).is_file(),
        "the mix the window plays now was evicted"
    );
    assert!(
        s.entry(s.nodes[3]).is_file(),
        "the newest render was evicted"
    );
    assert!(
        !s.entry(s.nodes[1]).exists(),
        "the mix the window played before is still held"
    );
}

/// Asking again for the mix the window already plays (Play after a
/// cancelled play, Preview pressed twice) keeps holding it: the new hold is
/// taken before the old one is let go.
#[test]
fn rendering_the_same_mix_again_keeps_it_held() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one * 3 / 2);

    s.window_render(s.nodes[1]).expect("render_preview");
    s.window_render(s.nodes[1]).expect("render_preview again");
    for node in [s.nodes[2], s.nodes[3]] {
        std::thread::sleep(TICK);
        s.agent_render(node);
    }

    assert!(s.entry(s.nodes[1]).is_file(), "the mix was let go");
}

/// A window render that fails changes nothing the window shows (its mix is
/// still the one before), so the one before stays held. Here node 1's audio
/// was swept from history and its source is gone, so it cannot be rebuilt;
/// the head's audio a sweep never takes.
#[test]
fn a_window_render_that_fails_keeps_the_mix_still_playing() {
    let s = Sandbox::new();
    let (playing, lost) = (s.nodes[5], s.nodes[1]);
    {
        let store = session::Store::open(&s.project).expect("store");
        tools::reclaim::sweep(&store, 0).expect("sweep");
        assert!(
            !tools::rederive::missing_paths(&store, lost).is_empty(),
            "the sweep removed node 1's audio"
        );
    }
    std::fs::remove_file(s.project.join("take.wav")).expect("remove the take");
    let played = s
        .window_render(playing)
        .expect("render_preview of the head");
    let one = std::fs::metadata(&played).expect("rendered").len();
    s.cap(one * 3 / 2);

    let failed = s.window_render(lost);
    assert!(failed.is_err(), "expected an error, got {failed:?}");
    // The failed attempt took nothing and gave nothing back. Renders by
    // others push the cache over its cap, which would take the mix if it
    // were not held, being the oldest.
    s.fill(one, 1);
    s.fill(one, 2);

    assert!(
        s.entry(playing).is_file(),
        "a failed render let go of the mix the window still plays"
    );
    assert!(!s.entry(lost).exists(), "a render that failed was cached");
    assert_eq!(s.staging_files(), Vec::<String>::new());
}

// ---------------------------------------------------------------------
// The hold ends with the project
// ---------------------------------------------------------------------

/// The mix belongs to the project it was rendered in. Opening another one
/// lets it go, and the project that was left evicts back under its cap at
/// once, as it does when any last hold goes.
#[test]
fn a_project_change_releases_the_window_s_mix() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one * 3 / 2);
    s.window_render(s.nodes[1]).expect("render_preview");
    std::thread::sleep(TICK);
    s.agent_render(s.nodes[2]);
    assert!(
        s.entry(s.nodes[1]).is_file(),
        "the window's mix was evicted before the project changed"
    );

    s.call("open_project", json!({ "path": s.other }))
        .expect("open another project");

    assert!(
        !s.entry(s.nodes[1]).exists(),
        "the old project's mix is still held after the window left it"
    );
    assert!(
        s.entry(s.nodes[2]).is_file(),
        "the newest render should have been kept"
    );
}

// ---------------------------------------------------------------------
// A/B compare keeps its own holds
// ---------------------------------------------------------------------

/// Starting and ending a compare neither takes nor gives the window's hold:
/// the compare has its pair, the window its mix, and the window's mix is
/// the one that counts when the bar is closed.
#[test]
fn a_compare_leaves_the_window_s_hold_alone() {
    let s = Sandbox::new();
    let one = s.render_bytes();
    s.cap(one * 3 / 2);
    s.window_render(s.nodes[1]).expect("render_preview");

    let answer = s
        .call(
            "prepare_compare",
            json!({ "a": s.nodes[2].to_hex(), "b": s.nodes[3].to_hex() }),
        )
        .expect("prepare_compare");
    std::thread::sleep(TICK);
    s.agent_render(s.nodes[4]);
    s.call("end_compare", json!({ "compareId": answer["compare_id"] }))
        .expect("end_compare");

    assert!(
        s.entry(s.nodes[1]).is_file(),
        "the compare let go of the window's mix"
    );
}
