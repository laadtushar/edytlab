//! JSON-backed content-addressable session store.
//!
//! Layout under `<project_dir>/.audiograph/`:
//! - `nodes/<hex[0..2]>/<hex>.json` — one file per [`SessionNode`], sharded
//!   by the first two hex chars of the id. Mirrors git's `objects/` scheme:
//!   keeps the top-level `nodes/` directory bounded to ~256 entries even
//!   after thousands of edits.
//! - `head` — single-line file containing the hex id of the current head,
//!   or absent / empty when the store has no nodes yet.
//!
//! Durability strategy: every write goes to a sibling `*.tmp` file inside
//! the same directory, then `tempfile::persist` does an atomic rename. The
//! node file is renamed BEFORE the head file. A crash between the two ends
//! up with `(old head + new node file orphaned)` — recoverable — and never
//! `(new head + missing node file)` — corrupt.

use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::node::{NodeId, SessionNode};
use crate::{Error, Result};

const STORE_DIR: &str = ".audiograph";
const NODES_DIR: &str = "nodes";
const HEAD_FILE: &str = "head";

/// Fsync a directory so a prior atomic rename inside it is durable.
///
/// On POSIX, `rename(2)` is atomic but its persistence across power loss
/// is only guaranteed once the directory entry is fsync'd. `tempfile`'s
/// `persist` does the rename but does NOT do the directory fsync, so we
/// do it ourselves after every persist of a node or head file.
///
/// On Windows, NTFS guarantees the metadata journal flushes on rename,
/// and opening a directory handle requires `FILE_FLAG_BACKUP_SEMANTICS`
/// which `std::fs::File::open` does not pass. We treat this as a no-op
/// there; reaching parity will require the `winapi` crate or `windows`.
#[cfg(unix)]
fn fsync_dir(path: &Path) -> io::Result<()> {
    let dir = fs::File::open(path)?;
    dir.sync_all()
}

#[cfg(not(unix))]
fn fsync_dir(_path: &Path) -> io::Result<()> {
    // See module-level note: directory fsync needs platform-specific
    // open flags on Windows. NTFS rename metadata is journaled, so the
    // practical durability gap is small, but we should revisit this.
    Ok(())
}

pub struct Store {
    project_dir: PathBuf,
    head: Option<NodeId>,
}

impl Store {
    /// Open or create the store under `<project_dir>/.audiograph/`.
    ///
    /// **Single-writer assumption.** The store assumes single-writer
    /// access. Concurrent writes from multiple processes are unsupported
    /// in Phase 1; the `final_path.exists()` short-circuit and head
    /// rename ordering both rely on no other writer racing inside the
    /// same store directory. Multi-writer locking lands in Phase 3 with
    /// the MCP server.
    pub fn open(project_dir: &Path) -> Result<Self> {
        let store_dir = project_dir.join(STORE_DIR);
        let nodes_dir = store_dir.join(NODES_DIR);
        fs::create_dir_all(&nodes_dir)?;

        let head_path = store_dir.join(HEAD_FILE);
        let head = if head_path.exists() {
            let raw = fs::read_to_string(&head_path)?;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(NodeId::from_hex(trimmed).map_err(|_| Error::InvalidHeadHex(trimmed.into()))?)
            }
        } else {
            None
        };

        Ok(Self {
            project_dir: project_dir.to_path_buf(),
            head,
        })
    }

    /// Append a node to the linear history. The caller's `parent` and `id`
    /// fields are overwritten: `parent` becomes the current head, `id` is
    /// recomputed from `state`.
    pub fn append(&mut self, mut node: SessionNode) -> Result<NodeId> {
        node.parent = self.head;
        node.id = NodeId::from_state(&node.state)?;
        let id = node.id;

        let hex = id.to_hex();
        let shard_dir = self.shard_dir(&hex);
        fs::create_dir_all(&shard_dir)?;

        let final_path = shard_dir.join(format!("{hex}.json"));
        // Skip the write if this exact content already exists. Content
        // addressing makes the operation idempotent — re-appending the
        // same state should be a no-op for the node file but still update
        // head (the caller may want to re-point head at an old state).
        if !final_path.exists() {
            let json = serde_json::to_vec_pretty(&node)?;
            let mut tmp = NamedTempFile::new_in(&shard_dir)?;
            tmp.write_all(&json)?;
            tmp.as_file().sync_all()?;
            tmp.persist(&final_path)?;
            // Persist the rename itself, not just the file contents.
            fsync_dir(&shard_dir)?;
        }

        self.write_head_atomic(id)?;
        self.head = Some(id);
        Ok(id)
    }

    pub fn get(&self, id: NodeId) -> Result<SessionNode> {
        let hex = id.to_hex();
        let path = self.shard_dir(&hex).join(format!("{hex}.json"));
        if !path.exists() {
            return Err(Error::NodeNotFound(hex));
        }
        let bytes = fs::read(&path)?;
        let node: SessionNode = serde_json::from_slice(&bytes)?;
        Ok(node)
    }

    /// Current head, or `None` if the store has no nodes yet.
    pub fn head(&self) -> Option<NodeId> {
        self.head
    }

    /// Project directory the store was opened against. Tools that
    /// keep their own caches under `<project>/.audiograph/<their-cache>/`
    /// (e.g. the Phase-2 stem cache) read this rather than threading
    /// the path through `ToolContext` separately.
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// Read every node JSON file under `<project>/.audiograph/nodes/`
    /// and return the parsed list.
    ///
    /// Order is unspecified — the caller is responsible for any sorting
    /// (typically by `created_at`). Files that fail to parse are
    /// surfaced as errors rather than silently skipped, so a corrupt
    /// store fails loudly instead of partially.
    ///
    /// This is an O(N) directory scan; M25's frontend graph view caps
    /// the working set at 200 nodes so the cost is bounded. Phase 3
    /// will likely switch to an in-memory index for larger sessions.
    pub fn list_nodes(&self) -> Result<Vec<SessionNode>> {
        let nodes_dir = self.project_dir.join(STORE_DIR).join(NODES_DIR);
        if !nodes_dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for shard in fs::read_dir(&nodes_dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(shard.path())? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let bytes = fs::read(&path)?;
                let node: SessionNode = serde_json::from_slice(&bytes)?;
                out.push(node);
            }
        }
        Ok(out)
    }

    pub fn set_head(&mut self, id: NodeId) -> Result<()> {
        let hex = id.to_hex();
        let path = self.shard_dir(&hex).join(format!("{hex}.json"));
        if !path.exists() {
            return Err(Error::NodeNotFound(hex));
        }
        self.write_head_atomic(id)?;
        self.head = Some(id);
        Ok(())
    }

    fn shard_dir(&self, hex: &str) -> PathBuf {
        self.project_dir
            .join(STORE_DIR)
            .join(NODES_DIR)
            .join(&hex[0..2])
    }

    fn write_head_atomic(&self, id: NodeId) -> Result<()> {
        let store_dir = self.project_dir.join(STORE_DIR);
        let head_path = store_dir.join(HEAD_FILE);
        let mut tmp = NamedTempFile::new_in(&store_dir)?;
        tmp.write_all(id.to_hex().as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(&head_path)?;
        // Persist the head rename itself.
        fsync_dir(&store_dir)?;
        Ok(())
    }
}
