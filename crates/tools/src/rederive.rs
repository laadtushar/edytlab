//! Putting a swept derived file back (#98).
//!
//! The sweep in [`crate::reclaim`] only deletes files whose whole
//! ancestor chain records a reproducible op. This is the other half of
//! that promise: given a node whose audio is gone, replay the chain
//! that produced it and get the same bytes back.
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

    let recipe = crate::recipe::export(store, node)?;
    if let Some(blocker) = recipe.blockers().into_iter().next() {
        return Err(format!(
            "cannot rebuild {}: {blocker}",
            missing.file_name().unwrap_or_default().to_string_lossy()
        ));
    }

    // A scratch project so the replay's nodes land somewhere that gets
    // thrown away. Only the audio it writes is wanted.
    let scratch = tempfile::TempDir::new()
        .map_err(|e| format!("could not make a scratch project to rebuild in: {e}"))?;
    let mut scratch_store = session::Store::open(scratch.path())
        .map_err(|e| format!("could not open the scratch project: {e}"))?;
    let mut engine = audio_engine::Engine::new();
    let mut clipboard: Option<Vec<f32>> = None;

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

    // The replay wrote its output under the scratch project's derived
    // directory, named by content. If the bytes match what the node
    // describes, the name matches too — so this is a lookup, not a
    // search, and a mismatch means the replay did not reproduce the
    // edit rather than something to paper over.
    let Some(name) = missing.file_name() else {
        return Err("the missing path has no file name".to_string());
    };
    let rebuilt = derived_dir(scratch.path()).join(name);
    if !rebuilt.is_file() {
        return Err(format!(
            "the replay ran but did not reproduce {} — the edit is not deterministic, so the \
             file cannot be recovered this way",
            name.to_string_lossy()
        ));
    }

    if let Some(parent) = missing.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::copy(&rebuilt, missing)
        .map_err(|e| format!("could not put {} back: {e}", missing.display()))?;

    Ok(true)
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
