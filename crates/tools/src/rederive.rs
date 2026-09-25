//! Putting a swept derived file back (#98).
//!
//! The sweep in [`crate::reclaim`] only deletes files a replay has
//! already rebuilt. This is the other half of that promise: given a node
//! whose audio is gone, replay the chain that produced it and get the
//! same bytes back — which [`materialize`] does for every path that
//! reads a node's audio or moves the head to it.
//!
//! ## Why the bytes are the same
//!
//! The CAS name *is* the blake3 of the post-edit samples. So a replay
//! that produces different audio produces a differently-named file, and
//! the miss simply persists rather than silently substituting audio
//! that is not what the node describes. Byte-identity is not something
//! this has to be careful about — it is what the naming scheme checks
//! for free.
//!
//! ## Why it replays into a scratch project
//!
//! Replaying through the dispatcher appends nodes, and the caller wants
//! a *file*, not history. So the chain runs against a throwaway project
//! and the file it produces is moved into the real `derived/` under the
//! name it was always going to have. The real store is never written
//! to.

use std::path::{Path, PathBuf};

use crate::provenance::derived_dir;
use crate::{ToolContext, ToolDispatcher};

/// What replaying a node's chain wrote: a scratch project holding the
/// derived files, and their names.
///
/// The names are content addresses — the blake3 of the samples — so a
/// name that also exists in the real project is the same bytes, rebuilt.
/// That is what lets the sweep ask "can this come back?" by looking,
/// rather than by trusting a record that says it should (#356).
pub struct Replay {
    scratch: tempfile::TempDir,
    produced: std::collections::BTreeSet<std::ffi::OsString>,
}

impl Replay {
    /// The derived files the replay wrote, by name.
    pub fn produced(&self) -> &std::collections::BTreeSet<std::ffi::OsString> {
        &self.produced
    }

    /// Where the replay wrote `name`, if it did.
    fn path(&self, name: &std::ffi::OsStr) -> Option<PathBuf> {
        self.produced
            .contains(name)
            .then(|| derived_dir(self.scratch.path()).join(name))
    }
}

/// Replay the chain that produced `node` into a scratch project.
///
/// `Err` names what stopped it, in a sentence a user can act on: a step
/// with no way back recorded, or a step that failed when run again.
pub fn replay(store: &session::Store, node: session::NodeId) -> Result<Replay, String> {
    let recipe = crate::recipe::export(store, node)?;
    if let Some(blocker) = recipe.blockers().into_iter().next() {
        return Err(blocker);
    }

    // A scratch project so the replay's nodes land somewhere that gets
    // thrown away. Only the audio it writes is wanted.
    let scratch = tempfile::TempDir::new()
        .map_err(|e| format!("could not make a scratch project to rebuild in: {e}"))?;
    let mut scratch_store = session::Store::open(scratch.path())
        .map_err(|e| format!("could not open the scratch project: {e}"))?;
    let mut engine = audio_engine::Engine::new();
    let mut clipboard: Option<crate::Clipboard> = None;

    let dispatcher = ToolDispatcher::default_dispatcher();
    {
        let mut ctx = ToolContext {
            store: &mut scratch_store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        for step in &recipe.steps {
            match dispatcher.invoke(&step.tool, step.params.clone(), &mut ctx) {
                Ok(crate::ToolResult::Ok(_)) => {}
                Ok(crate::ToolResult::Error(msg)) => {
                    return Err(format!("replaying `{}` failed: {msg}", step.tool))
                }
                Err(e) => return Err(format!("replaying `{}` failed: {e}", step.tool)),
            }
        }
    }

    let produced = std::fs::read_dir(derived_dir(scratch.path()))
        .map(|dir| {
            dir.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name())
                .collect()
        })
        .unwrap_or_default();
    Ok(Replay { scratch, produced })
}

/// Regenerate `missing` by replaying the chain that produced `node`.
///
/// Returns `Ok(true)` when the file is present afterwards — including
/// when it turned out to be there all along, so callers can use this as
/// "make sure this exists" without checking first.
///
/// `Err` names what stopped it, in a sentence a user can act on. A
/// refusal is the right outcome for a file with no way back: the
/// alternative is a render that quietly differs from what the node
/// says it is.
pub fn ensure_present(
    store: &session::Store,
    node: session::NodeId,
    missing: &Path,
) -> Result<bool, String> {
    if missing.is_file() {
        return Ok(true);
    }
    let Some(name) = missing.file_name() else {
        return Err("the missing path has no file name".to_string());
    };

    // `node` first, then every other node that names the same file. The
    // name is the content, so any chain that writes it writes these
    // bytes — and the sweep deletes a file once *some* chain has proved
    // it can (#356). A branch made by `apply_diff` shares its parent's
    // audio but cannot itself be replayed; asking only the branch would
    // call a file lost that its parent rebuilds.
    let others = store
        .list_nodes()
        .unwrap_or_default()
        .into_iter()
        .filter(|n| {
            n.id != node
                && n.state
                    .tracks
                    .iter()
                    .any(|t| t.clips.iter().any(|c| c.source_path == missing))
        })
        .map(|n| n.id);

    let mut first_error: Option<String> = None;
    let mut rebuilt: Option<(Replay, PathBuf)> = None;
    for id in std::iter::once(node).chain(others) {
        match replay(store, id) {
            // The replay wrote its output under the scratch project's
            // derived directory, named by content. If the bytes match
            // what the node describes, the name matches too — so this is
            // a lookup, not a search.
            Ok(r) => match r.path(name) {
                Some(path) => {
                    rebuilt = Some((r, path));
                    break;
                }
                None => {
                    first_error.get_or_insert_with(|| {
                        format!(
                            "the replay ran but did not reproduce {} — the edit is not \
                             deterministic, so the file cannot be recovered this way",
                            name.to_string_lossy()
                        )
                    });
                }
            },
            Err(e) => {
                first_error.get_or_insert_with(|| {
                    format!("cannot rebuild {}: {e}", name.to_string_lossy())
                });
            }
        }
    }
    let Some((_replay, rebuilt)) = rebuilt else {
        return Err(first_error.unwrap_or_else(|| {
            format!("nothing records how to rebuild {}", name.to_string_lossy())
        }));
    };
    put_back(&rebuilt, missing)?;
    Ok(true)
}

/// Put back every derived file `node` names that is not on disk, before
/// anything reads the node's audio or moves the head to it (#98, #356).
///
/// The history sweep deletes audio that only older nodes name, once a
/// replay has proved it can come back. This is the other half: every
/// path that renders a node, or makes one the head — undo, the history
/// view, an A/B accept, `revert_to`, `fork_node`, `apply_diff`, a render
/// or export of any node — calls this first, so a swept file is rebuilt
/// before anything opens it. The head's audio is never swept, so the
/// head's own reads need nothing.
///
/// Only files in the project's `derived/` are rebuilt. Those are the
/// only files the app ever deletes, and the only ones a replay can
/// write; a missing file anywhere else is the user's own source, moved
/// or deleted, and is left for the read to report as it always has.
///
/// Returns the files it rebuilt. `Err` names the first file it could not
/// rebuild and why, and the caller refuses rather than go on to render
/// silence or move the head to a state that cannot play.
pub fn materialize(store: &session::Store, node: session::NodeId) -> Result<Vec<PathBuf>, String> {
    let derived = key(&derived_dir(store.project_dir()));
    let mut missing: Vec<PathBuf> = Vec::new();
    for path in missing_paths(store, node) {
        let in_derived = path.parent().is_some_and(|p| key(p) == derived);
        if in_derived && !missing.contains(&path) {
            missing.push(path);
        }
    }
    if missing.is_empty() {
        return Ok(missing);
    }

    // One replay of the node's chain writes every file along it, so it
    // puts back every missing file at once rather than one replay each.
    if let Ok(r) = replay(store, node) {
        for path in &missing {
            if let Some(rebuilt) = path.file_name().and_then(|name| r.path(name)) {
                put_back(&rebuilt, path)?;
            }
        }
    }
    // What that replay did not write, another chain that names the file
    // may (see `ensure_present`), and if none does, this says why.
    for path in &missing {
        ensure_present(store, node, path)?;
    }
    Ok(missing)
}

/// Copy a rebuilt file into place under its name, atomically.
///
/// The name is the content's hash and presence is all a reader checks, so
/// a copy cut short under the final name would be wrong audio that every
/// later check calls present. The copy goes to a temporary file beside it
/// and is renamed into place only once it is whole.
fn put_back(rebuilt: &Path, missing: &Path) -> Result<(), String> {
    let could_not =
        |e: &dyn std::fmt::Display| format!("could not put {} back: {e}", missing.display());
    let parent = missing
        .parent()
        .ok_or_else(|| could_not(&"it has no parent directory"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    let mut part = tempfile::Builder::new()
        .prefix(".rebuilt-")
        .suffix(".wav.part")
        .tempfile_in(parent)
        .map_err(|e| could_not(&e))?;
    let mut src = std::fs::File::open(rebuilt).map_err(|e| could_not(&e))?;
    std::io::copy(&mut src, part.as_file_mut()).map_err(|e| could_not(&e))?;
    part.as_file().sync_all().map_err(|e| could_not(&e))?;
    part.persist(missing)
        .map(|_| ())
        .map_err(|e| could_not(&e.error))
}

/// A path as the file system names it, so a clip path written through a
/// different spelling of the project directory still compares equal.
fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Every clip path on `node` that is missing from disk.
pub fn missing_paths(store: &session::Store, node: session::NodeId) -> Vec<PathBuf> {
    let Ok(n) = store.get(node) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for track in &n.state.tracks {
        for clip in &track.clips {
            if !clip.source_path.is_file() {
                out.push(clip.source_path.clone());
            }
        }
    }
    out
}
