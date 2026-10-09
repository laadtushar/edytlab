//! The list of tools Plan first does not hold for approval (#415).
//!
//! With Plan first on and no plan from the model, the first step that
//! includes a call which can change the session is shown to the user
//! before it runs. "Can change the session" is `Tool::mutates`, and it
//! defaults to `true`: a new tool, and every tool an MCP server adds, is
//! held unless someone marks it read-only on purpose.
//!
//! This file is what makes that "on purpose". The read-only list is
//! pinned here by name, so exempting a tool from approval shows up in a
//! diff of this file and gets reviewed, instead of arriving as a one-line
//! override in the middle of a tool's implementation.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::{json, Value};
use tempfile::TempDir;
use tools::{
    schema::{anthropic_tool, object_schema},
    DispatchError, Tool, ToolContext, ToolDispatcher, ToolResult,
};

/// Every tool that never needs approval, alphabetical.
///
/// Deliberately **not** on this list, though they look harmless:
///
/// * `audition_effect` and `render_preview` render audio and write files
///   (the preview cache, and an OS temp file for a ranged render).
///   Holding a preview costs one click; flipping either later is a
///   reviewed one-line change to this list.
/// * `copy_region` overwrites the user's clipboard and writes a CAS blob.
/// * `transcribe` and `split_by_speaker` append nodes.
/// * `separate_stems`, `export_multiple`, `export_recipe` and
///   `render_final` write files.
/// * `compact_session` deletes files.
/// * `name_node`, `fork_node` and `revert_to` change the history.
/// * `mute_track`, `solo_track` and `set_sync_lock` append state.
/// * `apply_recipe` and `batch_apply` run nested edits.
/// * `load` appends a node.
const READ_ONLY: [&str; 7] = [
    "analyze_track",
    "compare_nodes",
    "export_labels",
    "plot_spectrum",
    "select_region",
    "silence_finder",
    "storage_report",
];

#[test]
fn the_read_only_list_is_pinned() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut actual: Vec<String> = dispatcher
        .tool_names()
        .into_iter()
        .filter(|name| !dispatcher.get(name).expect("registered").mutates())
        .collect();
    actual.sort();

    assert_eq!(
        actual,
        READ_ONLY.to_vec(),
        "the set of tools exempt from Plan-first approval changed.\n\
         Adding a tool to this list exempts it from approval: with Plan first on, \
         it will run without the user seeing it first. That must be a reviewed \
         decision. If it is intended, update READ_ONLY in this file, with the \
         reason, in the same change."
    );
}

/// A tool that says nothing about it is held. This is the property the
/// whole design rests on, so it is checked against a bare implementation
/// rather than against a tool that happens not to override it.
#[test]
fn a_tool_is_held_unless_it_says_otherwise() {
    struct Bare;
    impl Tool for Bare {
        fn name(&self) -> &'static str {
            "bare"
        }
        fn schema(&self) -> Value {
            anthropic_tool("bare", "fixture", object_schema(&[]))
        }
        fn invoke(&self, _args: Value, _ctx: &mut ToolContext) -> tools::Result<ToolResult> {
            Ok(ToolResult::Ok(Value::Null))
        }
    }
    assert!(Bare.mutates());
}

/// Eight seconds of 120 BPM clicks: enough beats for `analyze_track` to
/// find a tempo, which a steady tone does not give it.
fn write_clicks(path: &Path) {
    let sr = 44_100u32;
    let spec = WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let total = sr as usize * 8;
    let period = sr as usize / 2;
    let click_len = sr as usize / 100;
    let mut w = WavWriter::create(path, spec).expect("wav writer");
    for n in 0..total {
        let into_click = n % period;
        let s = if into_click < click_len {
            let env = 1.0 - into_click as f32 / click_len as f32;
            (into_click as f32 * 0.5).sin() * env
        } else {
            0.0
        };
        w.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Relative path -> size of every file under `dir`.
fn files_under(dir: &Path) -> BTreeMap<String, u64> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, u64>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("under root")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, entry.metadata().expect("metadata").len());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// "Read-only" is a claim about what a call leaves behind, so check what
/// it leaves behind: the head, the number of nodes, and every file in the
/// project. The one allowance is `derived/track-*.wav`, the
/// content-addressed flattened-track cache that `Tool::mutates` documents
/// as not counting.
#[test]
fn every_read_only_tool_leaves_the_session_untouched() {
    let dir = TempDir::new().expect("tempdir");
    let src = dir.path().join("take.wav");
    write_clicks(&src);

    let mut store = session::Store::open(dir.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut clipboard: Option<tools::Clipboard> = None;

    {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let loaded = dispatcher
            .invoke("load", json!({ "path": src.to_string_lossy() }), &mut ctx)
            .expect("load dispatches");
        assert!(
            matches!(loaded, ToolResult::Ok(_)),
            "load failed: {loaded:?}"
        );
    }

    let head = store.head().expect("a head after load");
    let hex = head.to_hex();
    let calls: [(&str, Value); 7] = [
        ("analyze_track", json!({ "track": 0 })),
        ("compare_nodes", json!({ "a": hex, "b": hex })),
        ("export_labels", json!({})),
        (
            "plot_spectrum",
            json!({ "track": 0, "start_sec": 0.0, "end_sec": 0.5 }),
        ),
        ("select_region", json!({ "from_beat": 1, "to_beat": 2 })),
        (
            "silence_finder",
            json!({ "track": 0, "threshold_db": -40.0 }),
        ),
        ("storage_report", json!({})),
    ];
    // The table above must cover exactly the pinned list, or a tool could
    // be added to one and never exercised.
    let mut named: Vec<&str> = calls.iter().map(|(n, _)| *n).collect();
    named.sort_unstable();
    assert_eq!(named, READ_ONLY.to_vec());

    let nodes_before = store.list_nodes().expect("list nodes").len();
    let files_before = files_under(dir.path());

    for (name, args) in calls {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let result = dispatcher
            .invoke(name, args, &mut ctx)
            .unwrap_or_else(|e| panic!("{name} was refused: {e}"));
        assert!(
            matches!(result, ToolResult::Ok(_)),
            "{name} should succeed on this session, so that its happy path is what \
             was checked: {result:?}"
        );
        assert!(
            clipboard.is_none(),
            "{name} wrote to the clipboard, so it mutates"
        );
    }

    assert_eq!(store.head(), Some(head), "a read-only tool moved the head");
    assert_eq!(
        store.list_nodes().expect("list nodes").len(),
        nodes_before,
        "a read-only tool appended a node"
    );

    let mut files_after = files_under(dir.path());
    // The flattened-track cache is the one thing a read is allowed to
    // leave. Everything else must be exactly as it was.
    files_after.retain(|path, _| {
        files_before.contains_key(path)
            || !(path.starts_with("derived/track-") && path.ends_with(".wav"))
    });
    assert_eq!(
        files_after, files_before,
        "a read-only tool changed the project's files"
    );
}

/// A call that would be refused runs nothing, so it needs no approval.
/// `would_mutate` and `invoke` share one pre-dispatch path; this holds
/// them to it.
#[test]
fn would_mutate_is_false_for_refused_calls() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let valid = json!({ "track": 0, "db": 3.0 });

    // The control: a valid call to an edit is one that mutates.
    assert!(dispatcher.would_mutate("set_track_gain", &valid, None));

    // Unknown tool.
    assert!(!dispatcher.would_mutate("no_such_tool", &json!({}), None));

    // Outside the whitelist, though the call itself is valid.
    let only_reads: HashSet<String> = ["storage_report".to_string()].into();
    assert!(!dispatcher.would_mutate("set_track_gain", &valid, Some(&only_reads)));

    // Arguments that fail the tool's schema.
    let bad = json!({ "track": "zero", "db": 3.0 });
    assert!(!dispatcher.would_mutate("set_track_gain", &bad, None));

    // And invoke refuses each of them, for the reason the gate relied on.
    let dir = TempDir::new().expect("tempdir");
    let mut store = session::Store::open(dir.path()).expect("open store");
    let mut engine = audio_engine::Engine::new();
    let mut clipboard: Option<tools::Clipboard> = None;
    let mut call = |name: &str, args: Value, allowed: Option<&HashSet<String>>| {
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: allowed,
        };
        dispatcher.invoke(name, args, &mut ctx)
    };
    assert!(matches!(
        call("no_such_tool", json!({}), None),
        Err(DispatchError::Unknown(_))
    ));
    assert!(matches!(
        call("set_track_gain", valid.clone(), Some(&only_reads)),
        Err(DispatchError::NotPermitted(_))
    ));
    assert!(matches!(
        call("set_track_gain", bad, None),
        Err(DispatchError::SchemaValidation(_))
    ));
}
