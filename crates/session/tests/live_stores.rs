//! A folder has one live `Store` (#501, #534, #566), whichever way it is
//! spelt and whoever opens it.
//!
//! The registry tells folders apart by identity (device and inode, or volume
//! and file index), not by path. A machine running these tests has no
//! case-insensitive disk and CI cannot make a bind mount, so the cases that
//! need two paths to be one folder use a registry built with a test double
//! for the identity (`Registry::with_identity`). The ones that need only a
//! real folder under two spellings (`a/../b`, a symlink) use the process's.

use std::fs;
use std::io;
use std::path::Path;

use chrono::Utc;
use session::live::{identify, Identity};
use session::{
    BusGraph, Error, FolderId, NodeId, Registry, SessionNode, SessionState, Store, TempoMap,
    STORE_DIR,
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

/// A disk that does not tell `Music` from `music`: a folder is named by its
/// last component, folded to lower case. The two spellings are two real
/// folders on the machine running the test; the registry has to take them
/// for one, as it must for a folder reached twice on a case-insensitive disk
/// or through a bind mount.
fn casefolded(dir: &Path) -> io::Result<Identity> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let hash = name
        .bytes()
        .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
    Ok(Identity::unpinned(FolderId::new(0, u128::from(hash))))
}

// =============================================================================
// Two spellings of one folder
// =============================================================================

/// The registry's half of case 1. Two spellings that `canonicalize` does not
/// unify were two keys, and so two live stores, each with its own idea of
/// the head.
#[test]
fn two_spellings_with_one_identity_give_one_live_store() {
    let tmp = TempDir::new().unwrap();
    let registry = Registry::with_identity(casefolded);

    let first = registry
        .open_shared_status(&tmp.path().join("Music"))
        .unwrap();
    assert!(first.is_new());
    let first = first.into_store();
    let second = registry
        .open_shared_status(&tmp.path().join("music"))
        .unwrap();

    assert!(!second.is_new(), "the second spelling built a second store");
    assert!(first.ptr_eq(&second.into_store()));
    assert!(registry.is_open(&tmp.path().join("MUSIC")));
    assert!(
        !registry.is_open(&tmp.path().join("Podcasts")),
        "another folder is not open"
    );
}

/// The other half: the sweep's guard. A staged edit running under one
/// spelling has to show in the count a store opened under the other reads.
#[test]
fn two_spellings_with_one_identity_share_one_in_flight_count() {
    let tmp = TempDir::new().unwrap();
    let registry = Registry::with_identity(casefolded);
    let mut upper = registry
        .open_unregistered(&tmp.path().join("Music"))
        .unwrap();
    upper.append(node(1)).unwrap();
    let running = upper.stage().unwrap();

    let lower = registry
        .open_unregistered(&tmp.path().join("music"))
        .unwrap();
    assert_eq!(
        lower.staged_in_flight(),
        1,
        "the second spelling read zero in flight, so a sweep through it would delete a running \
         tool's output"
    );

    let elsewhere = registry
        .open_unregistered(&tmp.path().join("Podcasts"))
        .unwrap();
    assert_eq!(elsewhere.staged_in_flight(), 0, "another folder's count");

    drop(running);
    assert_eq!(lower.staged_in_flight(), 0);
}

/// A shared store, a private writer and a reader see the same count too:
/// the count belongs to the folder, not to how a store was opened.
#[test]
fn every_kind_of_open_reads_the_same_in_flight_count() {
    let tmp = TempDir::new().unwrap();
    let registry = Registry::with_identity(casefolded);
    let live = registry.open_shared(&tmp.path().join("Music")).unwrap();
    let running = live.lock().unwrap().stage().unwrap();

    let reader = registry.open_read_only(&tmp.path().join("music")).unwrap();
    assert_eq!(reader.staged_in_flight(), 1);
    drop(reader);
    drop(running);
    assert_eq!(live.lock().unwrap().staged_in_flight(), 0);
}

/// Spellings `canonicalize` did unify still do, now through the identity.
#[test]
fn a_dotdot_spelling_and_a_symlink_are_the_one_folder() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let live = Store::open_shared(&a).unwrap();

    let dotdot = Store::open_shared(&b.join("..").join("a")).unwrap();
    assert!(live.ptr_eq(&dotdot), "`b/../a` is a second store for `a`");
    assert!(Store::is_open(&b.join("..").join("a")));
    assert!(!Store::is_open(&b), "another folder");

    #[cfg(unix)]
    {
        let link = tmp.path().join("link-to-a");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        let through_link = Store::open_shared(&link).unwrap();
        assert!(
            live.ptr_eq(&through_link),
            "a symlink to `a` is a second store"
        );
        // The store keeps the spelling it was made with.
        assert_eq!(live.lock().unwrap().project_dir(), a);
    }
}

#[test]
fn two_different_folders_never_share_a_store() {
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let of_a = Store::open_shared(a.path()).unwrap();
    let of_b = Store::open_shared(b.path()).unwrap();
    assert!(!of_a.ptr_eq(&of_b));
    assert!(of_a.ptr_eq(&Store::open_shared(a.path()).unwrap()));
}

// =============================================================================
// The registry is a view
// =============================================================================

#[test]
fn the_registry_does_not_keep_a_store_alive_and_forgets_it() {
    let tmp = TempDir::new().unwrap();
    let registry = Registry::with_identity(identify);
    let held = registry.open_shared(tmp.path()).unwrap();
    let weak = std::sync::Arc::downgrade(held.as_arc());
    assert!(registry.is_open(tmp.path()));

    drop(held);

    assert_eq!(weak.strong_count(), 0, "the registry owned the store");
    assert!(!registry.is_open(tmp.path()));

    // The folder moves on while nothing has it open; the next open reads it.
    let mut elsewhere = registry.open_unregistered(tmp.path()).unwrap();
    let moved = elsewhere.append(node(9)).unwrap();
    let reopened = registry.open_shared_status(tmp.path()).unwrap();
    assert!(reopened.is_new());
    assert_eq!(reopened.into_store().lock().unwrap().head(), Some(moved));
}

/// A store a tool panicked under cannot be locked again. It is not handed to
/// the next open, which gets a working one in its place.
#[test]
fn a_poisoned_store_is_replaced_not_handed_out() {
    let tmp = TempDir::new().unwrap();
    let registry = Registry::with_identity(identify);
    let broken = registry.open_shared(tmp.path()).unwrap();
    {
        let broken = broken.handle();
        let _ = std::thread::spawn(move || {
            let _held = broken.lock().unwrap();
            panic!("a tool panicked with the store locked");
        })
        .join();
    }
    assert!(broken.is_poisoned());
    assert!(
        !registry.is_open(tmp.path()),
        "a poisoned store is not live"
    );

    let fresh = registry.open_shared_status(tmp.path()).unwrap();
    assert!(fresh.is_new());
    let fresh = fresh.into_store();
    assert!(!fresh.ptr_eq(&broken));
    assert!(registry.open_shared(tmp.path()).unwrap().ptr_eq(&fresh));
}

/// An identity is only a name while the folder is there. A live store pins
/// its folder, so a deleted folder's identity (its inode) is not given to
/// the next one made while the store lives.
#[test]
fn a_live_stores_folder_identity_is_not_given_to_another_folder() {
    let tmp = TempDir::new().unwrap();
    let gone = tmp.path().join("gone");
    let live = Store::open_shared(&gone).unwrap();
    let id_of_gone = identify(&gone).unwrap().id();

    fs::remove_dir_all(&gone).unwrap();
    for n in 0..64 {
        let next = tmp.path().join(format!("next-{n}"));
        fs::create_dir_all(&next).unwrap();
        assert_ne!(
            identify(&next).unwrap().id(),
            id_of_gone,
            "a new folder took the identity of a live store's"
        );
    }
    drop(live);
}

// =============================================================================
// A folder replaced at the same path
// =============================================================================

/// Put another folder at `project`'s path, as a restore from a backup or a
/// trash-and-recreate does, and say where the old one went.
fn replace_folder(project: &Path) -> std::path::PathBuf {
    let aside = project.with_extension("old");
    fs::rename(project, &aside).unwrap();
    fs::create_dir(project).unwrap();
    aside
}

/// The data-loss class. `/p` is open and a turn holds the store; `/p` is
/// replaced, and an open of it built a second store with nothing in flight,
/// while the first still wrote, by path, into the new folder. The second's
/// sweep could then delete a file the first's off-lock tool had just written.
#[test]
fn a_folder_replaced_at_the_same_path_is_refused_while_the_old_one_is_held() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("p");
    let old = Store::open_shared(&project).unwrap();
    let running = old.lock().unwrap().stage().unwrap();
    assert!(
        Store::replaced_holder(&project).is_none(),
        "nothing replaced yet"
    );

    replace_folder(&project);

    let refused = |what: &str, got: Result<(), Error>| match got {
        Err(Error::FolderReplaced { path }) => assert_eq!(path, project, "{what}"),
        other => panic!("{what}: expected FolderReplaced, got {other:?}"),
    };
    refused("shared", Store::open_shared(&project).map(drop));
    refused("private", Store::open_private(&project).map(drop));
    refused("raw", Store::open(&project).map(drop));
    // The old store, and the edit running in it, are as they were.
    let holder = Store::replaced_holder(&project).expect("the old store is who holds it");
    assert!(holder.ptr_eq(&old));
    assert_eq!(old.lock().unwrap().staged_in_flight(), 1);
    // A reader writes nothing, so it may look at the new folder.
    let reader = Store::open_read_only(&project).expect("a reader is no second writer");
    assert_eq!(reader.head(), None);

    // Once the old folder has no store left, the path opens as a new project.
    drop((reader, holder, running, old));
    let fresh = Store::open_shared_status(&project).unwrap();
    assert!(fresh.is_new());
    assert_eq!(fresh.into_store().lock().unwrap().staged_in_flight(), 0);
}

/// A folder moved away and another made at its path is the same thing: the
/// store writes by the path it was opened with.
#[test]
fn a_folder_moved_away_and_a_new_one_made_at_its_path_is_refused_too() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("p");
    let moved = tmp.path().join("moved");
    let old = Store::open_shared(&project).unwrap();
    fs::rename(&project, &moved).unwrap();

    assert!(matches!(
        Store::open_shared(&project),
        Err(Error::FolderReplaced { .. })
    ));
    // The moved folder is still the old store's folder, found by identity.
    let again = Store::open_shared_status(&moved).unwrap();
    assert!(!again.is_new());
    assert!(again.into_store().ptr_eq(&old));
}

/// A record's spelling is the path its stores write through, and that moves:
/// the project folder is renamed, the next open reads it through its new
/// name (`reread_into`), and the store writes through that name from then on.
/// Replacing the folder at the *new* path has to be refused while the store
/// lives, which a spelling fixed when the folder was first opened cannot do.
#[test]
fn a_folder_renamed_and_then_replaced_at_its_new_path_is_refused_while_the_store_lives() {
    let tmp = TempDir::new().unwrap();
    let (p, q) = (tmp.path().join("p"), tmp.path().join("q"));
    let live = Store::open_shared(&p).unwrap();
    fs::rename(&p, &q).unwrap();
    // The window opens the project from its new place; the store follows it.
    let again = Store::open_shared_status(&q).unwrap();
    assert!(!again.is_new());
    assert!(again.into_store().ptr_eq(&live));
    live.lock().unwrap().reread_into(&q).unwrap();
    assert_eq!(live.lock().unwrap().project_dir(), q);

    replace_folder(&q);

    assert!(
        matches!(Store::open_shared(&q), Err(Error::FolderReplaced { .. })),
        "a second store was built beside the one that writes through this path"
    );
    drop(live);
    Store::open_shared(&q).expect("nothing writes through it any more");
}

/// And the old spelling is free once nothing writes through it: a new
/// project made at the path the folder used to have is not a "replaced"
/// folder.
#[test]
fn a_new_folder_at_the_spelling_a_store_has_left_is_not_refused() {
    let tmp = TempDir::new().unwrap();
    let (p, q) = (tmp.path().join("p"), tmp.path().join("q"));
    let live = Store::open_shared(&p).unwrap();
    fs::rename(&p, &q).unwrap();
    live.lock().unwrap().reread_into(&q).unwrap();

    let fresh = Store::open_shared_status(&p).expect("nothing writes through the old path");

    assert!(fresh.is_new());
    assert!(!fresh.into_store().ptr_eq(&live));
}

/// A staged edit started before the move still writes through the old path,
/// by the project directory it was staged with, until it is done.
#[test]
fn a_staged_edit_keeps_the_spelling_it_started_with_blocked() {
    let tmp = TempDir::new().unwrap();
    let (p, q) = (tmp.path().join("p"), tmp.path().join("q"));
    let live = Store::open_shared(&p).unwrap();
    let running = live.lock().unwrap().stage().unwrap();
    fs::rename(&p, &q).unwrap();
    live.lock().unwrap().reread_into(&q).unwrap();

    assert!(matches!(
        Store::open_shared(&p),
        Err(Error::FolderReplaced { .. })
    ));
    drop(running);
    Store::open_shared(&p).expect("the edit is over");
}

/// What holds the old folder need not be a shared store: a private writer
/// is as much a writer, and there is no handle of the window's to let go.
#[test]
fn a_replaced_folder_held_by_a_private_writer_is_refused_with_no_holder_to_release() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("p");
    let private = Store::open_private(&project).unwrap();
    replace_folder(&project);

    assert!(matches!(
        Store::open_shared(&project),
        Err(Error::FolderReplaced { .. })
    ));
    assert!(Store::replaced_holder(&project).is_none());
    drop(private);
    Store::open_shared(&project).expect("the old folder is let go");
}

/// The same folder under another spelling is not a replaced one.
#[test]
fn a_second_spelling_of_the_same_folder_is_not_a_replaced_folder() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    fs::create_dir_all(&b).unwrap();
    let live = Store::open_shared(&a).unwrap();
    let dotdot = b.join("..").join("a");
    assert!(Store::replaced_holder(&dotdot).is_none());
    assert!(live.ptr_eq(&Store::open_shared(&dotdot).unwrap()));
}

/// What a folder's identity must be, on whatever platform this runs on (on
/// Windows it is the volume serial number and the 128-bit file id): the same
/// folder gives the same one however it is spelt, and another folder does not.
#[test]
fn a_folders_identity_is_stable_and_its_own() {
    let tmp = TempDir::new().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    let id = |dir: &Path| identify(dir).unwrap().id();

    assert_eq!(id(&a), id(&a));
    assert_eq!(id(&a), id(&b.join("..").join("a")));
    assert_ne!(id(&a), id(&b));
    assert!(identify(&tmp.path().join("missing")).is_err());
}

// =============================================================================
// Private writers
// =============================================================================

/// `batch_apply`'s open: refused beside a live store, which was a second
/// writer on a project the app had open.
#[test]
fn a_private_open_is_refused_beside_a_live_store() {
    let tmp = TempDir::new().unwrap();
    let live = Store::open_shared(tmp.path()).unwrap();

    match Store::open_private(tmp.path()) {
        Err(Error::AlreadyOpen { path }) => assert_eq!(path, tmp.path()),
        Err(other) => panic!("expected AlreadyOpen, got {other:?}"),
        Ok(_) => panic!("a second writer was opened on a live folder"),
    }

    drop(live);
    Store::open_private(tmp.path()).expect("the folder is free again");
}

#[test]
fn a_private_open_is_refused_under_another_spelling() {
    let registry = Registry::with_identity(casefolded);
    let tmp = TempDir::new().unwrap();
    let _live = registry.open_shared(&tmp.path().join("Music")).unwrap();

    assert!(matches!(
        registry.open_private(&tmp.path().join("music")),
        Err(Error::AlreadyOpen { .. })
    ));
}

/// The refusal does not only hold at the instant of the open: a private
/// writer holds a claim on the folder, so the window cannot open it shared
/// while a batch is writing to it.
#[test]
fn a_private_store_keeps_its_folder_from_being_opened_shared() {
    let tmp = TempDir::new().unwrap();
    let private = Store::open_private(tmp.path()).unwrap();

    assert!(Store::is_open(tmp.path()));
    assert!(matches!(
        Store::open_shared(tmp.path()),
        Err(Error::AlreadyOpen { .. })
    ));
    assert!(matches!(
        Store::open_private(tmp.path()),
        Err(Error::AlreadyOpen { .. })
    ));

    drop(private);
    assert!(!Store::is_open(tmp.path()));
    Store::open_shared(tmp.path()).expect("the folder is free again");
}

/// A private open that cannot read the folder gives back its claim.
#[test]
fn a_private_open_that_fails_leaves_no_claim() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("p");
    fs::create_dir_all(project.join(STORE_DIR)).unwrap();
    fs::write(project.join(STORE_DIR).join("head"), "not hex").unwrap();

    assert!(matches!(
        Store::open_private(&project),
        Err(Error::InvalidHeadHex(_))
    ));
    assert!(!Store::is_open(&project), "the failed open kept its claim");
}

// =============================================================================
// Readers
// =============================================================================

#[test]
fn a_read_only_store_reads_beside_a_live_store_and_cannot_write() {
    let tmp = TempDir::new().unwrap();
    let live = Store::open_shared(tmp.path()).unwrap();
    let first = live.lock().unwrap().append(node(1)).unwrap();

    let mut reader = Store::open_read_only(tmp.path()).expect("beside a live store");
    assert!(reader.is_read_only());
    assert_eq!(reader.head(), Some(first));
    assert_eq!(reader.list_nodes().unwrap().len(), 1);
    assert_eq!(reader.get(first).unwrap().id, first);

    let before = snapshot(tmp.path());
    let refused = |what: &str, got: Result<(), Error>| match got {
        Err(Error::ReadOnly(op)) => assert_eq!(op, what),
        other => panic!("{what}: expected ReadOnly, got {other:?}"),
    };
    refused("append", reader.append(node(2)).map(drop));
    refused("set_head", reader.set_head(first));
    refused("set_label", reader.set_label(first, Some("x".into())));
    refused(
        "set_op",
        reader.set_op(
            first,
            session::NodeOp::new("t".into(), serde_json::json!({}), "m".into()),
        ),
    );
    refused("remove_node", reader.remove_node(first).map(drop));
    refused("detach_parent", reader.detach_parent(first));
    refused(
        "append_branches",
        reader
            .append_branches(first, vec![(node(3).state, None)])
            .map(drop),
    );
    refused("stage", reader.stage().map(drop));
    let staged = live.lock().unwrap().stage().unwrap();
    refused("commit", reader.commit(staged).map(drop));
    refused("set_head", reader.fork(first).map(drop));

    assert_eq!(reader.head(), Some(first), "a refused write moved the head");
    assert_eq!(snapshot(tmp.path()), before, "a refused write reached disk");
}

#[test]
fn a_read_only_open_makes_nothing() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("missing");
    assert!(Store::open_read_only(&missing).is_err());
    assert!(!missing.exists(), "a reader made a folder");

    let bare = tmp.path().join("bare");
    fs::create_dir_all(&bare).unwrap();
    let reader = Store::open_read_only(&bare).unwrap();
    assert_eq!(reader.head(), None);
    assert!(reader.list_nodes().unwrap().is_empty());
    assert!(!bare.join(STORE_DIR).exists(), "a reader made a store");
}

// =============================================================================
// Reading the folder again into a live store
// =============================================================================

#[test]
fn reread_reads_the_folder_into_the_same_handle() {
    let tmp = TempDir::new().unwrap();
    let live = Store::open_shared(tmp.path()).unwrap();
    let first = live.lock().unwrap().append(node(1)).unwrap();

    // The folder moves on behind the live store's back.
    let mut elsewhere = Store::open(tmp.path()).unwrap();
    let moved = elsewhere.append(node(2)).unwrap();
    assert_eq!(live.lock().unwrap().head(), Some(first));

    let again = Store::open_shared_status(tmp.path()).unwrap();
    assert!(!again.is_new());
    let same = again.into_store();
    assert!(same.ptr_eq(&live));
    same.lock().unwrap().reread_into(tmp.path()).unwrap();
    assert_eq!(live.lock().unwrap().head(), Some(moved));
    assert!(live.ptr_eq(&Store::open_shared(tmp.path()).unwrap()));
}

#[test]
fn reread_will_not_take_another_folders_history() {
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let live = Store::open_shared(a.path()).unwrap();
    let mut other = Store::open(b.path()).unwrap();
    other.append(node(5)).unwrap();

    let got = live.lock().unwrap().reread_into(b.path());
    match got {
        Err(Error::OtherFolder { path }) => assert_eq!(path, b.path()),
        Err(other) => panic!("expected OtherFolder, got {other:?}"),
        Ok(()) => panic!("a store was given another project's history"),
    }
    assert_eq!(live.lock().unwrap().project_dir(), a.path());
}

/// A live store keeps the spelling of the folder it was made with, and a
/// spelling can stop reaching the folder while the store lives. The folder
/// is then read through the spelling the open was given, not through the
/// dead one, which would make an empty folder where the symlink was.
#[cfg(unix)]
#[test]
fn reread_goes_through_the_given_spelling_when_the_stores_own_is_gone() {
    let tmp = TempDir::new().unwrap();
    let (real, link) = (tmp.path().join("a"), tmp.path().join("l"));
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let live = Store::open_shared(&link).unwrap();
    let first = live.lock().unwrap().append(node(1)).unwrap();

    // While the spelling still reaches the folder it is the one kept.
    live.lock().unwrap().reread_into(&real).unwrap();
    assert_eq!(live.lock().unwrap().project_dir(), link);

    fs::remove_file(&link).unwrap();
    live.lock().unwrap().reread_into(&real).unwrap();

    assert!(!link.exists(), "the dead spelling was read through");
    assert_eq!(live.lock().unwrap().project_dir(), real);
    assert_eq!(live.lock().unwrap().head(), Some(first));
}

// =============================================================================
// Helpers
// =============================================================================

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
