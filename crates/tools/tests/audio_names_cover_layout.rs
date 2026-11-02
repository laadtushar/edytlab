//! A file the project writes is named for its rate and channel count as
//! well as its samples (#620).
//!
//! Destructive edits, `generate_tone` and `generate_noise` named their
//! output by the blake3 of the interleaved samples alone, and a file
//! already under a name is reused, not written. So two buffers with the
//! same interleaved values and a different layout (mono at 48 kHz and
//! stereo at 24 kHz, say) got one name, and the second edit was handed the
//! first's file: audio at the wrong rate or channel count, with nothing to
//! say so. A `load` copy and a clipboard blob were already named by
//! `provenance::audio_hash`, which covers all three.

use std::path::{Path, PathBuf};

#[path = "../../../test-support/rust_source_scan.rs"]
mod rust_source_scan;
use rust_source_scan::{lex, without_test_items};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::json;
use session::SessionState;

#[path = "support/harness.rs"]
mod harness;
use harness::Session;

/// Interleaved values shared by the two layouts.
const VALUES: usize = 4_800;

impl Session {
    fn head_state(&self) -> SessionState {
        let head = self.store.head().expect("head");
        self.store.get(head).expect("head node").state
    }

    /// The file track `track`'s first clip plays.
    fn clip_file(&self, track: usize) -> PathBuf {
        self.head_state().tracks[track].clips[0].source_path.clone()
    }
}

/// The same interleaved values, whatever the layout they are read in.
fn value(i: usize) -> i16 {
    (((i * 37) % 20_000) as i32 - 10_000) as i16
}

fn write_wav(path: &Path, sample_rate: u32, channels: u16) {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for i in 0..VALUES {
        w.write_sample(value(i)).unwrap();
    }
    w.finalize().unwrap();
}

fn spec_of(path: &Path) -> (u32, u16, u32) {
    let r = WavReader::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    (r.spec().sample_rate, r.spec().channels, r.duration())
}

/// What `path` renders to when it is the only track: the frames written.
fn rendered_frames(project: &Session, track: usize) -> u64 {
    let mut state = project.head_state();
    state.tracks = vec![state.tracks[track].clone()];
    let out = project.dir.path().join(format!("only-{track}.wav"));
    audio_engine::render_state_to_wav(&state, &out, None)
        .expect("render")
        .frames_written
}

/// A mono track at 48 kHz and a stereo track at 24 kHz holding the same
/// interleaved values, each given the same edit. The outputs have equal
/// samples and a different layout.
#[test]
fn two_edits_with_the_same_samples_and_a_different_layout_get_different_files() {
    let mut p = Session::empty();
    let mono = p.dir.path().join("mono-48k.wav");
    let stereo = p.dir.path().join("stereo-24k.wav");
    write_wav(&mono, 48_000, 1);
    write_wav(&stereo, 24_000, 2);
    p.ok("load", json!({ "path": mono }));
    p.ok("load", json!({ "path": stereo }));

    // Polarity is applied sample by sample, so the two buffers stay equal.
    p.ok("invert", json!({ "track": 0 }));
    p.ok("invert", json!({ "track": 1 }));

    let (a, b) = (p.clip_file(0), p.clip_file(1));
    assert_ne!(
        a, b,
        "one file named for two layouts: the second edit was handed the first's"
    );
    assert_eq!(spec_of(&a), (48_000, 1, VALUES as u32), "the mono edit");
    assert_eq!(
        spec_of(&b),
        (24_000, 2, (VALUES / 2) as u32),
        "the stereo edit keeps its own rate and channel count"
    );

    // And each plays as itself: same duration, since the same number of
    // values is read at half the rate in two channels.
    let (mono_frames, stereo_frames) = (rendered_frames(&p, 0), rendered_frames(&p, 1));
    assert!(
        mono_frames.abs_diff(stereo_frames) <= mono_frames / 20,
        "the stereo track renders {stereo_frames} frames, the mono one {mono_frames}"
    );
}

/// The same pair in the other order: whichever edit writes first, the other
/// keeps its own layout.
#[test]
fn the_order_the_edits_are_made_in_changes_nothing() {
    let mut p = Session::empty();
    let mono = p.dir.path().join("mono-48k.wav");
    let stereo = p.dir.path().join("stereo-24k.wav");
    write_wav(&mono, 48_000, 1);
    write_wav(&stereo, 24_000, 2);
    p.ok("load", json!({ "path": mono }));
    p.ok("load", json!({ "path": stereo }));

    p.ok("invert", json!({ "track": 1 }));
    p.ok("invert", json!({ "track": 0 }));

    assert_eq!(spec_of(&p.clip_file(0)), (48_000, 1, VALUES as u32));
    assert_eq!(spec_of(&p.clip_file(1)), (24_000, 2, (VALUES / 2) as u32));
}

/// A tone or a noise at two session rates whose samples are bit for bit the
/// same: 1 s at 48 kHz and 2 s at 24 kHz are both 48 000 of them. A noise
/// depends on the number of samples and not on the rate; a tone does on the
/// phase of each, which is `2π · frequency · i / rate`, so halving both the
/// frequency and the rate leaves every one unchanged (a power of two scales
/// a float exactly). The session rate is moved in between by resampling the
/// loaded track, which is what changes it.
fn same_samples_at_two_rates(tool: &str) {
    let mut p = Session::empty();
    let take = p.dir.path().join("take.wav");
    write_wav(&take, 48_000, 1);
    p.ok("load", json!({ "path": take }));

    let args = |seconds: f64, frequency: f64| match tool {
        "generate_tone" => json!({ "frequency_hz": frequency, "duration_sec": seconds }),
        _ => json!({ "duration_sec": seconds }),
    };
    p.ok(tool, args(1.0, 440.0));
    p.ok(
        "resample_track",
        json!({ "track": 0, "target_sample_rate": 24_000 }),
    );
    assert_eq!(
        p.head_state().sample_rate,
        24_000,
        "the session now runs at 24 kHz, or this checks nothing"
    );
    p.ok(tool, args(2.0, 220.0));

    let (first, second) = (p.clip_file(1), p.clip_file(2));
    assert_ne!(first, second, "{tool}: one file for two session rates");
    assert_eq!(spec_of(&first), (48_000, 1, 48_000), "{tool}, at 48 kHz");
    assert_eq!(spec_of(&second), (24_000, 1, 48_000), "{tool}, at 24 kHz");
}

#[test]
fn generate_tone_names_its_file_for_the_session_rate() {
    same_samples_at_two_rates("generate_tone");
}

#[test]
fn generate_noise_names_its_file_for_the_session_rate() {
    same_samples_at_two_rates("generate_noise");
}

// ---------------------------------------------------------------------------
// Where the files live
// ---------------------------------------------------------------------------
//
// An edit's output and a `load` copy are now named the same way, and they
// share a folder: a source the renderer cannot stream (a FLAC, say) is
// transcoded into `.audiograph/derived/<audio_hash>.wav`, where every edit
// writes too. So an edit whose output is the same audio as such a copy
// names the same file, and `AudioStore::put_with` reuses it. That is right, since
// the name now says the audio is the same (rate, channels and samples), and
// nothing that deletes decides by who wrote a file: compaction and the
// sweeps ask which nodes *name* a path. Digital silence is the audio an
// edit reproduces exactly (`silence_region` over it sets zeros to zeros).

fn silent_flac(dir: &Path) -> PathBuf {
    let path = dir.join("silence.flac");
    audio_engine::write_flac(&vec![0.0; 11_025], 44_100, 1, &path).expect("flac");
    path
}

/// `load` of a FLAC, then an edit that leaves it as it was, then an edit
/// that does not (polarity: negative zeros hash differently).
fn load_edit_edit() -> (Session, [session::NodeId; 3]) {
    let mut p = Session::empty();
    let flac = silent_flac(p.dir.path());
    p.ok("load", json!({ "path": flac }));
    let loaded = p.store.head().unwrap();
    p.ok(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.25 }),
    );
    let same = p.store.head().unwrap();
    p.ok("invert", json!({ "track": 0 }));
    let inverted = p.store.head().unwrap();
    (p, [loaded, same, inverted])
}

fn file_of(p: &Session, node: session::NodeId) -> PathBuf {
    p.store.get(node).unwrap().state.tracks[0].clips[0]
        .source_path
        .clone()
}

#[test]
fn an_edit_and_a_load_copy_of_the_same_audio_are_one_file_in_one_folder() {
    let (p, [loaded, same, inverted]) = load_edit_edit();
    let derived = tools::provenance::derived_dir(p.dir.path());

    let copy = file_of(&p, loaded);
    assert_eq!(copy.parent(), Some(derived.as_path()), "the transcode");
    assert_eq!(
        file_of(&p, same),
        copy,
        "the edit reused it: the same audio"
    );
    let other = file_of(&p, inverted);
    assert_eq!(other.parent(), Some(derived.as_path()));
    assert_ne!(other, copy, "and different audio is another file");
}

/// Compaction removes what only the pruned nodes name, from the project's
/// own folder. The load node is pruned; the edit that kept the file by
/// naming it is not, so the file stays.
#[test]
fn compaction_keeps_a_file_a_kept_node_names_whoever_wrote_it() {
    let (mut p, [loaded, same, inverted]) = load_edit_edit();
    let copy = file_of(&p, loaded);
    let kept = file_of(&p, inverted);
    p.store.set_head(same).unwrap();

    // Keep the edit only: the load node (which wrote the copy) is pruned.
    let plan = tools::reclaim::plan_compaction(&p.store, same, 1).expect("plan");
    assert!(plan.nodes.contains(&loaded) && !plan.nodes.contains(&same));
    assert!(
        !plan
            .files
            .iter()
            .any(|f| f.ends_with(copy.file_name().unwrap())),
        "{:?}",
        plan.files
    );
    let report = tools::reclaim::apply_compaction(&mut p.store, &plan).expect("compact");
    assert_eq!((report.removed_files, report.kept_files), (1, 0));
    assert!(copy.is_file(), "the surviving node still plays it");
    assert!(tools::rederive::missing_paths(&p.store, same).is_empty());
    // What only the pruned branch named is what goes.
    assert!(!kept.exists(), "the inverted node's own file was removed");
}

/// The history sweep verifies the shared file through whichever node
/// replays it, removes it once nothing the head names needs it, and a
/// rebuild puts it back for both nodes that name it.
#[test]
fn the_sweep_and_a_rebuild_treat_a_shared_file_as_one_file() {
    let (p, [loaded, same, _]) = load_edit_edit();
    let copy = file_of(&p, loaded);
    let before = std::fs::read(&copy).unwrap();

    let report = tools::reclaim::sweep(&p.store, 0).expect("sweep");
    assert_eq!(report.kept_unverified, 0, "{report:?}");
    assert!(!copy.exists(), "history's audio went: {report:?}");

    tools::rederive::materialize(&p.store, same).expect("rebuilt");
    assert_eq!(std::fs::read(&copy).unwrap(), before);
    assert!(tools::rederive::missing_paths(&p.store, loaded).is_empty());
    assert!(tools::rederive::missing_paths(&p.store, same).is_empty());
}

// ---------------------------------------------------------------------------
// Only the audio store names and writes the project's audio (#719, stage 2)
// ---------------------------------------------------------------------------
//
// The store (`session::audio_store`) makes every name (`AudioName`, which no
// other crate can construct) and every write (`put_with`, `put_verified`,
// `put_then_name`). The type stops a tool from naming a file itself; what it
// cannot stop is a tool building a path into the store's folders by hand and
// writing there. These two scans hold that line, and the delete scan below
// holds the same line for deleting.

/// Source files that may hash with blake3 outside their tests, and why: none
/// of them names audio.
const MAY_HASH: &[(&str, &str)] = &[(
    "tools/src/preview_cache.rs",
    "a node id and a range, not audio",
)];

/// Source files that may spell a path into the store's folders outside their
/// tests, and why: each reads, lists, checks, or (the stage 3b callers) moves
/// files the store wrote; none writes a new one, and none deletes one (the
/// sweeps and compaction list and delete through the store, stage 3a).
const MAY_SPELL_STORE_PATHS: &[(&str, &str)] = &[
    (
        "tools/src/provenance.rs",
        "re-exports the folders and finds a clipboard blob to read it back",
    ),
    ("tools/src/rederive.rs", "a replay's put-back (stage 3b)"),
    ("tools/src/repair.rs", "repair (stage 3b)"),
    (
        "tools/src/destination.rs",
        "refuses an export into the store's folders",
    ),
    (
        "tools/src/tool/storage.rs",
        "the storage report lists the folders",
    ),
    (
        "desktop/project.rs",
        "Save As checks the copy's audio (stage 3b)",
    ),
];

/// What spells a path into one of the store's folders. `.dir(` with an
/// argument is `AudioFolder::dir(project)` (a cache's `dir()` takes none).
const STORE_PATH_SPELLINGS: &[&str] = &[
    "derived_dir(",
    "clipboard_dir(",
    "generated_dir(",
    "DERIVED_DIR",
    "CLIPBOARD_DIR",
    "GENERATED_DIR",
    "clipboard_blob_path(",
    "join(\"generated\")",
    "join(\"derived\")",
    "join(\"clipboard\")",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The lines of `source` that are not test code: every item marked
/// `#[cfg(test)]` is left out, so an allowance for a file is about what it
/// ships, and a writer added to a file whose only store paths were in its
/// tests is seen.
fn production_lines(source: &str) -> Vec<&str> {
    let kept: std::collections::BTreeSet<usize> = without_test_items(&lex(source))
        .iter()
        .map(|t| t.line)
        .collect();
    source
        .lines()
        .enumerate()
        .filter(|(i, _)| kept.contains(&(i + 1)))
        .map(|(_, line)| line)
        .collect()
}

fn spells_store_path(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or("");
    STORE_PATH_SPELLINGS.iter().any(|s| code.contains(s))
        || code
            .match_indices(".dir(")
            .any(|(at, _)| !code[at + ".dir(".len()..].starts_with(')'))
}

/// Every source file of the crates that write project audio, as
/// (`tools/src/...` or `desktop/...`, its non-test lines).
fn writer_sources() -> Vec<(String, Vec<String>)> {
    let tools = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        (tools.join("src"), "tools/src"),
        (tools.join("../../apps/desktop/src-tauri/src"), "desktop"),
    ];
    let mut out = Vec::new();
    for (root, label) in roots {
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        for file in files {
            let rel = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&file).unwrap();
            let lines = production_lines(&text)
                .into_iter()
                .map(String::from)
                .collect();
            out.push((format!("{label}/{rel}"), lines));
        }
    }
    assert!(out.len() > 50, "found the sources: {}", out.len());
    out
}

fn allowed(list: &[(&str, &str)], rel: &str) -> bool {
    list.iter().any(|(allowed, _)| *allowed == rel)
}

fn hashes(lines: &[String]) -> bool {
    lines
        .iter()
        .any(|l| l.split("//").next().unwrap_or("").contains("blake3::"))
}

#[test]
fn only_the_store_names_audio() {
    let offenders: Vec<String> = writer_sources()
        .into_iter()
        .filter(|(rel, lines)| !allowed(MAY_HASH, rel) && hashes(lines))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "these hash on their own; name audio with `session::audio_store::AudioName` \
         (or add the file to MAY_HASH with the reason it hashes something that is not \
         audio): {offenders:?}"
    );
}

#[test]
fn only_the_store_spells_paths_into_its_folders() {
    let offenders: Vec<String> = writer_sources()
        .into_iter()
        .filter(|(rel, lines)| {
            !allowed(MAY_SPELL_STORE_PATHS, rel) && lines.iter().any(|l| spells_store_path(l))
        })
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "these build paths into the store's folders by hand; write through \
         `session::audio_store::AudioStore` (or add the file to MAY_SPELL_STORE_PATHS \
         with the reason it only reads): {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Only the audio store deletes the project's audio (#719, stage 3a)
// ---------------------------------------------------------------------------
//
// The sweeps, compaction and a repair's leftovers delete through
// `session::Store::audio_deleter`: only a file `AudioStore::owns` places in
// one of its folders, and never while a handle on the project is staged. A
// delete written by hand anywhere else could skip either check, and a path
// does not say lexically where it points, so every delete in these crates'
// shipped code is listed here with what it deletes.

/// Source files that may delete files outside their tests, and why: none of
/// them deletes in the store's folders.
const MAY_DELETE: &[(&str, &str)] = &[
    (
        "tools/src/preview_cache.rs",
        "the preview cache's own entries and staging files",
    ),
    (
        "tools/src/tool/render_preview.rs",
        "a cached preview that does not decode",
    ),
    (
        "desktop/commands.rs",
        "a skill or an agent profile the user deletes",
    ),
    ("desktop/state.rs", "the active agent profile's sidecar"),
];

/// What deletes a file or a folder.
const DELETES: &[&str] = &["remove_file(", "remove_dir(", "remove_dir_all("];

fn deletes(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or("");
    DELETES.iter().any(|d| code.contains(d))
}

#[test]
fn only_the_store_deletes_the_projects_audio() {
    let offenders: Vec<String> = writer_sources()
        .into_iter()
        .filter(|(rel, lines)| !allowed(MAY_DELETE, rel) && lines.iter().any(|l| deletes(l)))
        .map(|(rel, _)| rel)
        .collect();
    assert!(
        offenders.is_empty(),
        "these delete files by hand; delete the project's audio through \
         `session::Store::audio_deleter` (or add the file to MAY_DELETE with what it \
         deletes, which must not be in the store's folders): {offenders:?}"
    );
}

#[test]
fn every_allowance_is_still_needed() {
    // An allowance nothing uses any more is a door left open.
    let sources = writer_sources();
    let find = |rel: &str| sources.iter().find(|(r, _)| r == rel).map(|(_, l)| l);
    for (rel, why) in MAY_HASH {
        let lines = find(rel).unwrap_or_else(|| panic!("{rel} is gone ({why})"));
        assert!(hashes(lines), "{rel} no longer hashes ({why})");
    }
    for (rel, why) in MAY_SPELL_STORE_PATHS {
        let lines = find(rel).unwrap_or_else(|| panic!("{rel} is gone ({why})"));
        assert!(
            lines.iter().any(|l| spells_store_path(l)),
            "{rel} no longer spells a store path ({why})"
        );
    }
    for (rel, why) in MAY_DELETE {
        let lines = find(rel).unwrap_or_else(|| panic!("{rel} is gone ({why})"));
        assert!(
            lines.iter().any(|l| deletes(l)),
            "{rel} no longer deletes ({why})"
        );
    }
}

#[test]
fn the_scan_sees_code_and_not_tests_or_comments() {
    let source = "fn a() { let p = derived_dir(x); }\n\
                  // derived_dir(x) in a comment\n\
                  fn b() { let d = AudioFolder::Generated.dir(&p); }\n\
                  fn c() { let d = cache.dir(); }\n\
                  #[cfg(test)]\n\
                  mod tests { fn t() { let p = clipboard_dir(x); } }\n";
    let flagged: Vec<String> = production_lines(source)
        .into_iter()
        .filter(|l| spells_store_path(l))
        .map(String::from)
        .collect();
    assert_eq!(flagged.len(), 2, "{flagged:?}");
    assert!(flagged[0].contains("derived_dir(x); }"));
    assert!(flagged[1].contains("Generated.dir("));

    let source = "fn a() { std::fs::remove_file(&p).ok(); }\n\
                  // std::fs::remove_file(&p) in a comment\n\
                  fn b() { fs::remove_dir_all(&d)?; }\n\
                  #[cfg(test)]\n\
                  mod tests { fn t() { std::fs::remove_file(&p).unwrap(); } }\n";
    let flagged: Vec<String> = production_lines(source)
        .into_iter()
        .filter(|l| deletes(l))
        .map(String::from)
        .collect();
    assert_eq!(flagged.len(), 2, "{flagged:?}");
}
