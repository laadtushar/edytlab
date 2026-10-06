//! Putting a swept derived file back (#98).
//!
//! The sweep in [`crate::reclaim`] only deletes files a replay has
//! already rebuilt. This is the other half of that promise: given a node
//! whose audio is gone, rebuild it and get the same bytes back — which
//! [`materialize`] does for every path that reads a node's audio or moves
//! the head to it.
//!
//! ## One step at a time, from the audio before it
//!
//! A node's audio is rebuilt by replaying *that node's own step* on its
//! parent's state, the audio just before the edit. If the parent's audio
//! is gone too, it is rebuilt first the same way, by any node that names
//! it — and so on back to audio that is on disk.
//!
//! Replaying whole chains from a root broke wherever the chain back to it
//! did (#377, #356): compaction cuts kept history off from its `load`; an
//! `apply_diff` names a node and track ids a scratch project does not
//! have; a source moved since its `load` cannot be read again. A single
//! step needs none of that, only the audio it read. And it is the same
//! check the sweep makes before deleting — "does this step, run on its
//! parent, write this file?" — so a rebuild later follows the path that
//! was proved, whichever node's chain that runs through:
//!
//! * the sweep deletes a file only once a step run on its parent has
//!   written it;
//! * that parent's audio is either still on disk, or was itself deleted
//!   only after the same proof;
//! * so by induction back to audio that is on disk, everything deleted
//!   can be rebuilt.
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
//! a *file*, not history. So the step runs against a throwaway project,
//! seeded with the parent's state, and the file it produces is copied
//! into the real `derived/` under the name it was always going to have.
//! The real store is never written to.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::provenance::derived_dir;
use crate::{ToolContext, ToolDispatcher};

/// What replaying a node's step wrote: a scratch project holding the
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

/// Replay `node`'s own step on its parent's state, in a scratch project.
///
/// The parent's audio must all be on disk: it is what the step reads. A
/// root's step — a `load` — runs on an empty project.
///
/// `Err` names what stopped it, in a sentence a user can act on: a step
/// with no way back recorded, audio the step reads that is not on disk,
/// or a step that failed when run again.
pub fn replay(store: &session::Store, node: session::NodeId) -> Result<Replay, String> {
    let n = store
        .get(node)
        .map_err(|e| format!("failed to read node {}: {e}", node.to_hex()))?;
    let step = match &n.op {
        Some(op) => crate::recipe::RecipeStep {
            tool: op.tool.clone(),
            params: op.params.clone(),
            inputs: op.inputs.clone(),
            replayable: op.reproducible,
            label: n.label.clone(),
        },
        None => crate::recipe::RecipeStep {
            tool: "<unrecorded>".to_string(),
            params: serde_json::Value::Null,
            inputs: serde_json::Value::Null,
            replayable: false,
            label: n.label.clone(),
        },
    };
    let recipe = crate::recipe::export_steps(vec![step]);
    if let Some(blocker) = recipe.blockers().into_iter().next() {
        return Err(blocker);
    }
    let seed = match n.parent {
        Some(p) => {
            let parent = store
                .get(p)
                .map_err(|e| format!("failed to read node {}: {e}", p.to_hex()))?;
            if let Some(gone) = parent
                .state
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .find(|c| !c.source_path.is_file())
            {
                return Err(format!(
                    "the audio before this edit is not on disk: {}",
                    gone.source_path.display()
                ));
            }
            Some(parent.state)
        }
        None => None,
    };

    // A scratch project so the replay's nodes land somewhere that gets
    // thrown away. Only the audio it writes is wanted.
    let scratch = tempfile::TempDir::new()
        .map_err(|e| format!("could not make a scratch project to rebuild in: {e}"))?;
    let mut scratch_store = session::Store::open(scratch.path())
        .map_err(|e| format!("could not open the scratch project: {e}"))?;
    // The parent's state is the scratch project's first node. Its clips
    // name the real project's audio, which is on disk, so the step reads
    // from there and only what it writes lands in the scratch project.
    if let Some(state) = seed {
        scratch_store
            .append(session::SessionNode {
                id: session::NodeId([0; 32]),
                parent: None,
                created_at: chrono::Utc::now(),
                label: Some("replay seed".to_string()),
                reasoning: None,
                state,
                op: None,
            })
            .map_err(|e| format!("could not seed the scratch project: {e}"))?;
    }
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

/// One rebuild: which nodes name each file, and which nodes it has
/// already tried, so a file shared around the graph is not chased in a
/// circle.
struct Rebuild<'a> {
    store: &'a session::Store,
    derived: PathBuf,
    naming: HashMap<PathBuf, Vec<session::NodeId>>,
    tried: HashSet<session::NodeId>,
}

impl<'a> Rebuild<'a> {
    fn new(store: &'a session::Store) -> Self {
        let mut naming: HashMap<PathBuf, Vec<session::NodeId>> = HashMap::new();
        for n in store.list_nodes().unwrap_or_default() {
            for track in &n.state.tracks {
                for clip in &track.clips {
                    let ids = naming.entry(key(&clip.source_path)).or_default();
                    if !ids.contains(&n.id) {
                        ids.push(n.id);
                    }
                }
            }
        }
        Self {
            store,
            derived: key(&derived_dir(store.project_dir())),
            naming,
            tried: HashSet::new(),
        }
    }

    /// `node`'s derived files that are not on disk.
    fn missing(&self, node: session::NodeId) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for path in missing_paths(self.store, node) {
            let in_derived = path.parent().is_some_and(|p| key(p) == self.derived);
            if in_derived && !out.contains(&path) {
                out.push(path);
            }
        }
        out
    }

    /// Put `path` back, through `prefer` first and then any other node
    /// that names it. The name is the content, so any step that writes it
    /// writes these bytes — and a branch made by `apply_diff` shares its
    /// parent's audio but cannot itself be replayed, so asking only the
    /// branch would call a file lost that its parent rebuilds.
    fn file(&mut self, path: &Path, prefer: session::NodeId) -> Result<(), String> {
        if path.is_file() {
            return Ok(());
        }
        let name = path
            .file_name()
            .ok_or_else(|| "the missing path has no file name".to_string())?
            .to_string_lossy()
            .to_string();
        let mut candidates = vec![prefer];
        for id in self.naming.get(&key(path)).cloned().unwrap_or_default() {
            if id != prefer {
                candidates.push(id);
            }
        }
        let mut first_error: Option<String> = None;
        for id in candidates {
            if !self.tried.insert(id) {
                continue;
            }
            match self.node(id) {
                Ok(()) if path.is_file() => return Ok(()),
                Ok(()) => {
                    first_error.get_or_insert_with(|| {
                        format!(
                            "the replay ran but did not reproduce {name} — the edit is not \
                             deterministic, so the file cannot be recovered this way"
                        )
                    });
                }
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        Err(format!(
            "cannot rebuild {name}: {}",
            first_error.unwrap_or_else(|| "nothing records how to make it".to_string())
        ))
    }

    /// Rebuild `node`'s missing audio: its parent's first, then its own
    /// step on that.
    fn node(&mut self, node: session::NodeId) -> Result<(), String> {
        let n = self
            .store
            .get(node)
            .map_err(|e| format!("failed to read node {}: {e}", node.to_hex()))?;
        if let Some(parent) = n.parent {
            for path in self.missing(parent) {
                self.file(&path, parent)?;
            }
        }
        let replayed = replay(self.store, node)?;
        for path in self.missing(node) {
            if let Some(rebuilt) = path.file_name().and_then(|name| replayed.path(name)) {
                put_back(&rebuilt, &path)?;
            }
        }
        Ok(())
    }
}

/// Regenerate `missing`, a file `node` names.
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
    Rebuild::new(store).file(missing, node)?;
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
    let mut rebuild = Rebuild::new(store);
    let missing = rebuild.missing(node);
    for path in &missing {
        rebuild.file(path, node)?;
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
