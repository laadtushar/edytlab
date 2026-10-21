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

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use serde_json::{json, Value};
use session::SessionState;
use tempfile::TempDir;
use tools::{ToolContext, ToolDispatcher, ToolResult};

/// Interleaved values shared by the two layouts.
const VALUES: usize = 4_800;

struct Project {
    dir: TempDir,
    store: session::Store,
    engine: audio_engine::Engine,
    dispatcher: ToolDispatcher,
    clipboard: Option<tools::Clipboard>,
}

impl Project {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let store = session::Store::open(dir.path()).expect("store");
        Self {
            dir,
            store,
            engine: audio_engine::Engine::new(),
            dispatcher: ToolDispatcher::default_dispatcher(),
            clipboard: None,
        }
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
            ToolResult::Error(m) => panic!("`{tool}` failed: {m}"),
        }
    }

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
fn rendered_frames(project: &Project, track: usize) -> u64 {
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
    let mut p = Project::new();
    let mono = p.dir.path().join("mono-48k.wav");
    let stereo = p.dir.path().join("stereo-24k.wav");
    write_wav(&mono, 48_000, 1);
    write_wav(&stereo, 24_000, 2);
    p.call("load", json!({ "path": mono }));
    p.call("load", json!({ "path": stereo }));

    // Polarity is applied sample by sample, so the two buffers stay equal.
    p.call("invert", json!({ "track": 0 }));
    p.call("invert", json!({ "track": 1 }));

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
    let mut p = Project::new();
    let mono = p.dir.path().join("mono-48k.wav");
    let stereo = p.dir.path().join("stereo-24k.wav");
    write_wav(&mono, 48_000, 1);
    write_wav(&stereo, 24_000, 2);
    p.call("load", json!({ "path": mono }));
    p.call("load", json!({ "path": stereo }));

    p.call("invert", json!({ "track": 1 }));
    p.call("invert", json!({ "track": 0 }));

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
    let mut p = Project::new();
    let take = p.dir.path().join("take.wav");
    write_wav(&take, 48_000, 1);
    p.call("load", json!({ "path": take }));

    let args = |seconds: f64, frequency: f64| match tool {
        "generate_tone" => json!({ "frequency_hz": frequency, "duration_sec": seconds }),
        _ => json!({ "duration_sec": seconds }),
    };
    p.call(tool, args(1.0, 440.0));
    p.call(
        "resample_track",
        json!({ "track": 0, "target_sample_rate": 24_000 }),
    );
    assert_eq!(
        p.head_state().sample_rate,
        24_000,
        "the session now runs at 24 kHz, or this checks nothing"
    );
    p.call(tool, args(2.0, 220.0));

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
// names the same file, and `put_cas_file` reuses it. That is right, since
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
fn load_edit_edit() -> (Project, [session::NodeId; 3]) {
    let mut p = Project::new();
    let flac = silent_flac(p.dir.path());
    p.call("load", json!({ "path": flac }));
    let loaded = p.store.head().unwrap();
    p.call(
        "silence_region",
        json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.25 }),
    );
    let same = p.store.head().unwrap();
    p.call("invert", json!({ "track": 0 }));
    let inverted = p.store.head().unwrap();
    (p, [loaded, same, inverted])
}

fn file_of(p: &Project, node: session::NodeId) -> PathBuf {
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
// One function names audio
// ---------------------------------------------------------------------------

/// Files of this crate's `src` that call blake3 themselves, and why each
/// may. Every other file names audio through `provenance::audio_hash`, so
/// a writer cannot leave the rate or the channel count out again (#620).
const MAY_HASH: &[(&str, &str)] = &[
    ("provenance.rs", "audio_hash itself, and the older name"),
    ("preview_cache.rs", "a node id and a range, not audio"),
    (
        "tool/mix_to_new_track.rs",
        "the whole WAV file, header included",
    ),
    (
        "tool/util.rs",
        "a lane copy is keyed by its clips, not its samples",
    ),
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

#[test]
fn only_audio_hash_names_audio() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut offenders = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if MAY_HASH.iter().any(|(allowed, _)| *allowed == rel) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        if text.contains("blake3::") {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "these hash on their own; name audio with `provenance::audio_hash` (or add \
         the file to MAY_HASH with the reason it hashes something that is not audio): \
         {offenders:?}"
    );
}
