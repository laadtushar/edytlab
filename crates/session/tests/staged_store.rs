//! A staged store handle: appends held in memory, published only if the
//! shared head has not moved (#421).
//!
//! The point of staging is that a long edit can run with no lock on the
//! shared store. These tests pin what makes that safe: nothing reaches disk
//! until the commit, a commit onto a head that moved writes nothing at all,
//! and a handle cannot do anything that rewrites history.

use std::fs;
use std::path::Path;

use chrono::Utc;
use session::{
    Annotation, AnnotationId, AnnotationKind, BusGraph, Commit, Error, NodeId, NodeOp, SessionNode,
    SessionState, Store, TempoMap, STORE_DIR,
};
use tempfile::TempDir;

fn node(length_samples: u64) -> SessionNode {
    SessionNode {
        id: NodeId([0u8; 32]),
        parent: None,
        created_at: Utc::now(),
        label: None,
        reasoning: None,
        state: SessionState {
            tracks: Vec::new(),
            bus_routing: BusGraph::default(),
            master_chain: Vec::new(),
            tempo_map: TempoMap::default(),
            key_map: None,
            transcript: None,
            sample_rate: 48_000,
            length_samples,
            annotations: Vec::new(),
            sync_lock: false,
        },
        op: None,
    }
}

fn op(tool: &str) -> NodeOp {
    NodeOp::new(tool.into(), serde_json::json!({}), "test".into())
}

/// The files under `<project>/.audiograph/`, as `(relative path, bytes)`,
/// sorted: what "nothing was written" is checked against.
fn snapshot(project: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let root = project.join(STORE_DIR);
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

fn node_file(project: &Path, id: NodeId) -> std::path::PathBuf {
    let hex = id.to_hex();
    project
        .join(STORE_DIR)
        .join("nodes")
        .join(&hex[0..2])
        .join(format!("{hex}.json"))
}

fn marker(name: &str) -> Annotation {
    Annotation {
        id: AnnotationId::new(),
        name: name.to_string(),
        kind: AnnotationKind::Marker { time_sec: 1.0 },
    }
}

#[test]
fn a_staged_append_writes_nothing_until_commit() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();
    let before = snapshot(dir.path());

    let mut staged = shared.stage().unwrap();
    assert!(staged.is_staged());
    assert!(!shared.is_staged());
    assert_eq!(staged.head(), Some(base));

    let n = staged.append(node(2)).unwrap();
    assert_ne!(n, base);
    assert_eq!(staged.head(), Some(n));
    assert_eq!(staged.get(n).unwrap().parent, Some(base));
    assert_eq!(staged.get(n).unwrap().state.length_samples, 2);
    // The base is still readable through the handle.
    assert_eq!(staged.get(base).unwrap().state.length_samples, 1);

    // Disk and the shared store are exactly as they were.
    assert_eq!(snapshot(dir.path()), before, "a staged append wrote a file");
    assert!(!node_file(dir.path(), n).exists());
    assert_eq!(shared.head(), Some(base));
    assert!(
        shared.get(n).is_err(),
        "the shared store saw an unpublished node"
    );
    assert_eq!(Store::open(dir.path()).unwrap().head(), Some(base));
}

#[test]
fn commit_publishes_nodes_then_head() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();

    let mut staged = shared.stage().unwrap();
    let a = staged.append(node(2)).unwrap();
    let b = staged.append(node(3)).unwrap();
    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);

    assert_eq!(shared.head(), Some(b));
    assert_eq!(shared.get(a).unwrap().parent, Some(base));
    assert_eq!(shared.get(b).unwrap().parent, Some(a));

    // A store opened from scratch sees the same thing: it is on disk.
    let reopened = Store::open(dir.path()).unwrap();
    assert_eq!(reopened.head(), Some(b));
    assert_eq!(reopened.get(b).unwrap().parent, Some(a));
    assert_eq!(reopened.list_nodes().unwrap().len(), 3);
}

#[test]
fn commit_refuses_when_the_head_moved() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();

    let mut staged = shared.stage().unwrap();
    let tool = staged.append(node(2)).unwrap();

    // The user edits while the tool runs.
    let user = shared.add_annotation(base, marker("user")).unwrap();
    assert_eq!(shared.head(), Some(user));
    let before = snapshot(dir.path());

    assert_eq!(shared.commit(staged).unwrap(), Commit::Conflict);

    assert_eq!(
        snapshot(dir.path()),
        before,
        "a refused commit wrote a file"
    );
    assert!(!node_file(dir.path(), tool).exists());
    assert_eq!(shared.head(), Some(user), "the user's edit was displaced");
    assert_eq!(Store::open(dir.path()).unwrap().head(), Some(user));
}

/// A head that left and came back is the same state, so what was staged on
/// it is still correct. Ids hash state alone, which is what makes this a
/// comparison and not a guess.
#[test]
fn a_head_that_moved_and_came_back_is_not_a_conflict() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();

    let mut staged = shared.stage().unwrap();
    let tool = staged.append(node(2)).unwrap();

    let away = shared.append(node(9)).unwrap();
    assert_ne!(away, base);
    shared.set_head(base).unwrap();

    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);
    assert_eq!(shared.head(), Some(tool));
    assert_eq!(shared.get(tool).unwrap().parent, Some(base));
}

#[test]
fn a_read_only_stage_commits_as_a_no_op_even_after_the_head_moved() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();

    let staged = shared.stage().unwrap();
    assert_eq!(staged.get(base).unwrap().state.length_samples, 1);

    let user = shared.add_annotation(base, marker("user")).unwrap();
    let before = snapshot(dir.path());

    // Nothing was staged, so there is nothing to conflict with: the caller
    // does not run a read again because someone else wrote.
    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);
    assert_eq!(shared.head(), Some(user));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn staging_an_empty_store_and_committing_the_first_node() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    assert_eq!(shared.head(), None);

    let mut staged = shared.stage().unwrap();
    assert_eq!(staged.head(), None);
    let first = staged.append(node(5)).unwrap();
    assert_eq!(staged.get(first).unwrap().parent, None);
    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);
    assert_eq!(shared.head(), Some(first));

    // And the same on an empty store that someone else filled meanwhile.
    let dir2 = TempDir::new().unwrap();
    let mut shared2 = Store::open(dir2.path()).unwrap();
    let mut staged2 = shared2.stage().unwrap();
    staged2.append(node(5)).unwrap();
    let other = shared2.append(node(7)).unwrap();
    assert_eq!(shared2.commit(staged2).unwrap(), Commit::Conflict);
    assert_eq!(shared2.head(), Some(other));
}

#[test]
fn staged_set_op_lands_on_commit_and_the_first_writer_wins() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();

    // A new node: the op set on the handle is in the file after the commit,
    // and a second set_op on the handle does not replace the first.
    let mut staged = shared.stage().unwrap();
    let n = staged.append(node(2)).unwrap();
    staged.set_op(n, op("first")).unwrap();
    staged.set_op(n, op("second")).unwrap();
    assert_eq!(staged.get(n).unwrap().op.unwrap().tool, "first");
    assert!(shared.get(n).is_err(), "an op was published early");
    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);
    assert_eq!(shared.get(n).unwrap().op.unwrap().tool, "first");

    // A node that is already on disk with an op keeps it.
    shared.set_head(base).unwrap();
    let mut again = shared.stage().unwrap();
    let same = again.append(node(2)).unwrap();
    assert_eq!(same, n, "the same state is the same node");
    again.set_op(same, op("later")).unwrap();
    assert_eq!(shared.commit(again).unwrap(), Commit::Published);
    assert_eq!(shared.head(), Some(n));
    assert_eq!(shared.get(n).unwrap().op.unwrap().tool, "first");

    // A node that is already on disk without an op takes the staged one.
    shared.set_head(base).unwrap();
    let plain = shared.append(node(3)).unwrap();
    assert!(shared.get(plain).unwrap().op.is_none());
    shared.set_head(base).unwrap();
    let mut third = shared.stage().unwrap();
    let again_plain = third.append(node(3)).unwrap();
    assert_eq!(again_plain, plain);
    third.set_op(plain, op("filled")).unwrap();
    assert_eq!(shared.commit(third).unwrap(), Commit::Published);
    assert_eq!(shared.get(plain).unwrap().op.unwrap().tool, "filled");

    // Only a node the handle appended may be given an op.
    let mut fourth = shared.stage().unwrap();
    assert!(matches!(
        fourth.set_op(base, op("nope")),
        Err(Error::Staged(_))
    ));
    assert!(shared.get(base).unwrap().op.is_none());
}

#[test]
fn a_state_reached_twice_in_one_run_is_one_node_with_its_first_parent() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();

    let mut staged = shared.stage().unwrap();
    let a = staged.append(node(2)).unwrap();
    let b = staged.append(node(3)).unwrap();
    let a_again = staged.append(node(2)).unwrap();
    assert_eq!(a_again, a);
    assert_eq!(staged.head(), Some(a));
    assert_eq!(staged.get(a).unwrap().parent, Some(base));
    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);

    // The head is where the run ended, not where it wrote last.
    assert_eq!(shared.head(), Some(a));
    assert_eq!(shared.get(b).unwrap().parent, Some(a));
    assert_eq!(shared.get(a).unwrap().parent, Some(base));
    assert_eq!(shared.list_nodes().unwrap().len(), 3);
}

#[test]
fn a_staged_store_refuses_history_rewrites() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let base = shared.append(node(1)).unwrap();
    let next = shared.append(node(2)).unwrap();
    let before = snapshot(dir.path());

    let mut staged = shared.stage().unwrap();
    assert!(matches!(staged.set_head(base), Err(Error::Staged(_))));
    assert!(matches!(
        staged.set_label(next, Some("x".into())),
        Err(Error::Staged(_))
    ));
    assert!(matches!(staged.remove_node(base), Err(Error::Staged(_))));
    assert!(matches!(staged.detach_parent(next), Err(Error::Staged(_))));
    assert!(matches!(
        staged.append_branches(base, vec![(node(3).state, None)]),
        Err(Error::Staged(_))
    ));
    assert!(matches!(staged.fork(base), Err(Error::Staged(_))));
    assert!(matches!(
        shared.stage().unwrap().stage(),
        Err(Error::Staged(_))
    ));

    assert_eq!(staged.head(), Some(next), "a refused call moved the head");
    assert_eq!(snapshot(dir.path()), before);
    drop(staged);

    // What appends still works on a handle: annotations are appends.
    let mut staged = shared.stage().unwrap();
    let marked = staged.add_annotation(next, marker("tool")).unwrap();
    assert_eq!(staged.get(marked).unwrap().state.annotations.len(), 1);
    assert_eq!(shared.commit(staged).unwrap(), Commit::Published);
    assert_eq!(shared.head(), Some(marked));
}

#[test]
fn in_flight_counts_staged_handles() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    shared.append(node(1)).unwrap();
    assert_eq!(shared.staged_in_flight(), 0);

    let a = shared.stage().unwrap();
    assert_eq!(shared.staged_in_flight(), 1);
    assert_eq!(a.staged_in_flight(), 1, "a handle reads the same count");
    let b = shared.stage().unwrap();
    assert_eq!(shared.staged_in_flight(), 2);

    assert_eq!(shared.commit(a).unwrap(), Commit::Published);
    assert_eq!(shared.staged_in_flight(), 1);

    drop(b);
    assert_eq!(shared.staged_in_flight(), 0);

    // A conflicted commit stops counting too.
    let mut c = shared.stage().unwrap();
    c.append(node(2)).unwrap();
    shared.append(node(3)).unwrap();
    assert_eq!(shared.commit(c).unwrap(), Commit::Conflict);
    assert_eq!(shared.staged_in_flight(), 0);

    // So does one that unwinds.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = shared.stage().unwrap();
        panic!("a tool panicked mid-run");
    }));
    assert!(result.is_err());
    assert_eq!(shared.staged_in_flight(), 0);
}

#[test]
fn commit_into_the_wrong_place_is_refused() {
    let dir = TempDir::new().unwrap();
    let other_dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    let mut other = Store::open(other_dir.path()).unwrap();
    shared.append(node(1)).unwrap();
    other.append(node(1)).unwrap();

    // From another project.
    let mut staged = other.stage().unwrap();
    staged.append(node(2)).unwrap();
    let before = snapshot(dir.path());
    assert!(matches!(shared.commit(staged), Err(Error::Staged(_))));
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(
        other.staged_in_flight(),
        0,
        "a refused handle still stops counting"
    );

    // An unstaged store.
    let plain = Store::open(dir.path()).unwrap();
    assert!(matches!(shared.commit(plain), Err(Error::Staged(_))));

    // Into a staged store.
    let mut into = shared.stage().unwrap();
    let from = shared.stage().unwrap();
    assert!(matches!(into.commit(from), Err(Error::Staged(_))));
}

#[test]
fn list_nodes_on_a_handle_includes_what_it_appended() {
    let dir = TempDir::new().unwrap();
    let mut shared = Store::open(dir.path()).unwrap();
    shared.append(node(1)).unwrap();

    let mut staged = shared.stage().unwrap();
    staged.append(node(2)).unwrap();
    assert_eq!(staged.list_nodes().unwrap().len(), 2);
    assert_eq!(shared.list_nodes().unwrap().len(), 1);
}
