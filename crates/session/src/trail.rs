//! The path the user took through the history, kept with the project
//! (#453).
//!
//! Undo goes back along the heads the user was on, not along each node's
//! `parent`: a node's id is the hash of its state alone, so an edit that
//! returns to an earlier state (mute, then unmute) lands on a node that
//! already exists, and that node keeps the parent it had the first time
//! (#398). The frontend decides where undo and redo go
//! (`apps/desktop/src/lib/headTrail.ts`, held by `useSession`), and until
//! now it forgot the path when the app closed. This is the file it saves
//! the path in, so that after a restart undo retraces the same steps.
//!
//! The file is `<project>/.audiograph/trail.json`, beside `head`:
//!
//! ```json
//! { "head": "<hex>", "trail": ["<hex>", ...], "redo": ["<hex>", ...] }
//! ```
//!
//! * `head` is the head the path ends at. A reader compares it with the
//!   head it has opened the project on: a path that ends somewhere else
//!   (a save lost to a crash, a store edited by another client) is not the
//!   path to this head.
//! * `trail` is the heads left behind, oldest first. `redo` is where undo
//!   came from, most recent last.
//!
//! It is in the store's directory rather than in `view.json` because the
//! view is disposable by design (losing it costs a scroll) and this is
//! not: losing it makes undo skip steps. It is still only a convenience,
//! so it never stands between a user and their project. A missing,
//! unreadable or corrupt file is an empty trail, which is exactly what
//! undo has always fallen back on, and a read has no way to fail.
//!
//! A read returns only what is safe to act on: ids that are node ids, of
//! nodes the project still has. A compaction removes nodes
//! ([`Store::remove_node`](crate::Store::remove_node)), and an undo that
//! was pointed at one would be refused on every press.
//!
//! A write goes to a temporary file in the same directory and is renamed
//! over `trail.json`, then the directory is synced, as the head file is
//! written. A reader sees the old file or the new one, never half of one.
//! The trail is bounded to [`MAX_TRAIL`] entries each way, the same bound
//! the frontend keeps, so the file does not grow with the history.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::node::NodeId;
use crate::store::{fsync_dir, node_path};
use crate::{Result, STORE_DIR};

/// The file's name, inside `.audiograph/`.
pub const TRAIL_FILE: &str = "trail.json";

/// How many heads are kept each way. The frontend's `MAX_TRAIL` in
/// `lib/headTrail.ts` is the same number: a longer file would hold steps
/// the app has already forgotten.
pub const MAX_TRAIL: usize = 500;

/// The most a trail file can honestly hold is two lists of [`MAX_TRAIL`]
/// 64-character ids, about 70 KB. A file far past that is not one this
/// code wrote, and is not read into memory: it is an empty trail.
const MAX_FILE_BYTES: u64 = 1 << 20;

/// The saved path, in the form it is stored and sent over IPC: ids are
/// the 64-character hex of a [`NodeId`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadTrail {
    /// The head the path ends at; `None` for a project with no history.
    #[serde(default)]
    pub head: Option<String>,
    /// The heads the user left behind, oldest first, not including `head`.
    #[serde(default)]
    pub trail: Vec<String>,
    /// Where undo came from, most recent last.
    #[serde(default)]
    pub redo: Vec<String>,
}

/// Where the project's trail is kept.
pub fn trail_path(project_dir: &Path) -> PathBuf {
    project_dir.join(STORE_DIR).join(TRAIL_FILE)
}

/// `hex` as the canonical id the store writes, if it is one.
///
/// Checked here rather than left to [`NodeId::from_hex`], which slices
/// the text by byte and so panics on a 64-byte string that holds a
/// multi-byte character, and accepts a sign in front of a digit pair. The
/// text comes from a file anyone can edit and from the webview, and a
/// read of it must not be able to fail.
fn canonical(hex: &str) -> Option<String> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    NodeId::from_hex(hex).ok().map(NodeId::to_hex)
}

/// The last [`MAX_TRAIL`] of `ids`, keeping their order.
fn bounded(mut ids: Vec<String>) -> Vec<String> {
    if ids.len() > MAX_TRAIL {
        ids.drain(..ids.len() - MAX_TRAIL);
    }
    ids
}

/// Keep the entries that are node ids, and for which `keep` holds, then
/// bound the list.
fn clean(ids: &[String], keep: impl Fn(&str) -> bool) -> Vec<String> {
    bounded(
        ids.iter()
            .filter_map(|id| canonical(id))
            .filter(|id| keep(id))
            .collect(),
    )
}

fn has_node(project_dir: &Path, hex: &str) -> bool {
    node_path(project_dir, hex).is_file()
}

/// The trail saved with the project, or an empty one.
///
/// Never fails: no file, a file that cannot be read, a file that is not
/// this format, and a head the project does not have all read as
/// [`HeadTrail::default`]. Entries naming nodes the project no longer has
/// are dropped from the lists, in place, so the path closes up over them.
pub fn read_trail(project_dir: &Path) -> HeadTrail {
    let path = trail_path(project_dir);
    let small_enough = fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_FILE_BYTES);
    let Some(saved) = small_enough
        .then(|| fs::read(&path).ok())
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<HeadTrail>(&bytes).ok())
    else {
        return HeadTrail::default();
    };
    let Some(head) = saved
        .head
        .as_deref()
        .and_then(canonical)
        .filter(|head| has_node(project_dir, head))
    else {
        return HeadTrail::default();
    };
    let exists = |id: &str| has_node(project_dir, id);
    HeadTrail {
        head: Some(head),
        trail: clean(&saved.trail, exists),
        redo: clean(&saved.redo, exists),
    }
}

/// Save `trail` with the project, replacing what was there.
///
/// Returns `false`, and writes nothing, when the trail does not belong to
/// this project: it has no head, or its head is not a node here. The
/// frontend saves without waiting for the answer, so a save for the
/// project being left can arrive after the next one has opened, and must
/// not put one project's path in another's directory. It is not an error,
/// because nothing is wrong with the caller; there is simply nothing to
/// keep.
///
/// Entries that are not node ids are dropped and each list is cut to its
/// last [`MAX_TRAIL`]. Whether the entries' nodes exist is checked when
/// the trail is read, not here: a node can go between the two.
pub fn write_trail(project_dir: &Path, trail: &HeadTrail) -> Result<bool> {
    let Some(head) = trail
        .head
        .as_deref()
        .and_then(canonical)
        .filter(|head| has_node(project_dir, head))
    else {
        return Ok(false);
    };
    let clean_trail = HeadTrail {
        head: Some(head),
        trail: clean(&trail.trail, |_| true),
        redo: clean(&trail.redo, |_| true),
    };

    let store_dir = project_dir.join(STORE_DIR);
    fs::create_dir_all(&store_dir)?;
    let mut tmp = NamedTempFile::new_in(&store_dir)?;
    tmp.write_all(&serde_json::to_vec(&clean_trail)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(trail_path(project_dir))?;
    fsync_dir(&store_dir)?;
    Ok(true)
}
