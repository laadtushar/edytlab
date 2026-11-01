//! `render_preview` reuses a render instead of redoing it (#164).
//!
//! The claim worth testing is not "a file appears in a directory" — it
//! is that the second call does no work and hands back *the same
//! bytes*. Determinism is what makes the cache safe (a node id is a
//! hash of the session state, and rendering that state is
//! reproducible), so a test that only checked the path would pass even
//! if the cached file were something else entirely.
//!
//! The other half is the miss: change the session and the node id
//! changes, so there is no invalidation step that could be forgotten.

use std::path::PathBuf;

use serde_json::{json, Value};

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

const SAMPLE_RATE: u32 = 48_000;

impl Session {
    fn preview(&mut self) -> Value {
        let node = self.head_hex();
        self.ok("render_preview", json!({ "node_id": node }))
    }
}

/// Rendering the same head twice must do no work the second time, and
/// the file it hands back must be the same bytes — not merely a file
/// with the same name.
#[test]
fn the_second_render_of_a_head_is_a_cache_hit() {
    let mut h = Session::with_sine(SAMPLE_RATE, 1);

    let first = h.preview();
    assert_eq!(
        first["cached"],
        json!(false),
        "first render should be a miss"
    );
    let path = PathBuf::from(first["path"].as_str().unwrap());
    let bytes = std::fs::read(&path).expect("the rendered preview");
    assert!(!bytes.is_empty());

    let second = h.preview();
    assert_eq!(second["cached"], json!(true), "second render should hit");
    assert_eq!(second["path"], first["path"], "the hit named another file");
    assert_eq!(
        std::fs::read(&path).expect("still there"),
        bytes,
        "a cached preview must be byte-identical to the render it replaces",
    );

    // And the numbers a hit reports come from the file, so they match.
    assert_eq!(second["frames_written"], first["frames_written"]);
    assert_eq!(second["sample_rate"], first["sample_rate"]);
    assert_eq!(second["channels"], first["channels"]);
}

/// Editing changes the session state, which changes the node id, which
/// misses. There is no invalidation step to get wrong.
#[test]
fn an_edit_misses_and_leaves_the_old_entry_intact() {
    let mut h = Session::with_sine(SAMPLE_RATE, 1);

    let before = h.preview();
    let before_path = PathBuf::from(before["path"].as_str().unwrap());

    h.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );

    let after = h.preview();
    assert_eq!(after["cached"], json!(false), "a new head must not hit");
    assert_ne!(after["path"], before["path"]);

    // Undo means going back, so the previous render has to still be
    // there — that is the case the cache exists for.
    assert!(
        before_path.is_file(),
        "the previous head's preview was thrown away"
    );
}

/// Stepping back to an earlier head plays instantly: the entry from
/// before the edit is still a hit.
#[test]
fn undo_then_redo_hits_in_both_directions() {
    let mut h = Session::with_sine(SAMPLE_RATE, 1);

    let first_head = h.head_hex();
    h.preview();

    h.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.2 }),
    );
    let second_head = h.head_hex();
    h.preview();

    let back = h.ok("render_preview", json!({ "node_id": first_head }));
    assert_eq!(back["cached"], json!(true), "undo re-rendered");

    let forward = h.ok("render_preview", json!({ "node_id": second_head }));
    assert_eq!(forward["cached"], json!(true), "redo re-rendered");
}

/// A ranged render is an excerpt, not the mix, and must never be stored
/// under the node's name — serving one as a whole-session preview would
/// be a cache hit that returns the wrong audio. It is stored (#535), in
/// the same bounded cache, under a key that names its range.
#[test]
fn a_ranged_render_is_not_cached_as_the_whole_mix() {
    let mut h = Session::with_sine(SAMPLE_RATE, 1);
    let node = h.head_hex();

    let excerpt = h.ok(
        "render_preview",
        json!({ "node_id": node, "range": [0, 1000] }),
    );
    let excerpt_path = PathBuf::from(excerpt["path"].as_str().unwrap());
    let cache_dir = h.dir.path().join(".audiograph").join("previews");
    assert_eq!(
        excerpt_path.parent(),
        Some(cache_dir.as_path()),
        "a ranged render was written outside the preview cache: {}",
        excerpt_path.display(),
    );
    assert_ne!(
        excerpt_path,
        cache_dir.join(format!("{node}.wav")),
        "an excerpt was stored under the node's name, where the mix belongs",
    );

    // The full render that follows is a genuine miss rather than a hit
    // on the excerpt.
    let full = h.preview();
    assert_eq!(full["cached"], json!(false));
    assert!(
        full["frames_written"].as_u64().unwrap() > 1000,
        "the whole-session render returned an excerpt",
    );
    // And the excerpt is still there, and is still an excerpt.
    assert!(excerpt_path.is_file(), "the mix evicted the excerpt");
    let excerpt_bytes = std::fs::metadata(&excerpt_path).unwrap().len();
    let mix_bytes = std::fs::metadata(full["path"].as_str().unwrap())
        .unwrap()
        .len();
    assert!(excerpt_bytes < mix_bytes, "the excerpt file is the mix");
}

/// `storage_report` has to be able to see the cache, or a bounded cache
/// is one nobody can observe working.
#[test]
fn storage_report_counts_the_preview_cache() {
    let mut h = Session::with_sine(SAMPLE_RATE, 1);

    let before = h.ok("storage_report", json!({}));
    assert_eq!(before["preview_cache"]["files"], json!(0));

    h.preview();

    let after = h.ok("storage_report", json!({}));
    assert_eq!(after["preview_cache"]["files"], json!(1));
    assert!(after["preview_cache"]["bytes"].as_u64().unwrap() > 0);
    assert!(after["preview_cache"]["cap_bytes"].as_u64().unwrap() > 0);
    // The cache is not derived edit history and must not be counted as
    // such — that number drives #98's policy decision.
    assert_eq!(after["history"]["files"], before["history"]["files"]);
}
