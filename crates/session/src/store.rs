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

use chrono::Utc;

use crate::diff::{self, SessionDiff};
use crate::node::{NodeId, SessionNode};
use crate::state::SessionState;
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

    /// Return the annotation list visible at `head`.
    ///
    /// Annotations are content-addressed alongside the rest of the state,
    /// so this is just a thin accessor on top of [`Store::get`]: it reads
    /// the node, clones out its `state.annotations`. Forks see only their
    /// own annotations, and reverting head to an older node automatically
    /// restores that node's annotations without any side-channel bookkeeping.
    pub fn annotations_for(&self, head: NodeId) -> Result<Vec<crate::annotation::Annotation>> {
        let node = self.get(head)?;
        Ok(node.state.annotations.clone())
    }

    /// Append a new node whose state extends `head`'s annotation list
    /// with `annotation`. Returns the id of the new node.
    ///
    /// History is immutable: `head` is unchanged. Callers chaining edits
    /// should feed the returned id back in as the next `head`.
    pub fn add_annotation(
        &mut self,
        head: NodeId,
        annotation: crate::annotation::Annotation,
    ) -> Result<NodeId> {
        let mut node = self.get(head)?;
        node.state.annotations.push(annotation);
        self.append(node)
    }

    /// Remove the annotation with id `target` from `head`'s annotation
    /// list, appending a new node with the trimmed list. Returns the new
    /// head id.
    ///
    /// If no annotation in `head` has id `target` (already removed, or
    /// caller raced another edit) this is a no-op and returns `head`
    /// unchanged — no new node is appended.
    pub fn remove_annotation(
        &mut self,
        head: NodeId,
        target: crate::annotation::AnnotationId,
    ) -> Result<NodeId> {
        let mut node = self.get(head)?;
        let before = node.state.annotations.len();
        node.state.annotations.retain(|a| a.id != target);
        if node.state.annotations.len() == before {
            return Ok(head);
        }
        self.append(node)
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

    // =========================================================================
    // M24: branching DAG operations.
    // =========================================================================

    /// Fork the DAG at `parent`: subsequent appends will parent off
    /// `parent`'s state. See [`crate::diff::fork`] for the rationale on
    /// content-addressing semantics — there's no separate "duplicate
    /// node" because two byte-identical states share an id.
    pub fn fork(&mut self, parent: NodeId) -> Result<NodeId> {
        diff::fork(self, parent)
    }

    /// Compute the structural delta `b - a` from on-disk node states.
    pub fn diff(&self, a: NodeId, b: NodeId) -> Result<SessionDiff> {
        diff::diff(self, a, b)
    }

    /// Merge two nodes; see [`crate::diff::merge`] for semantics.
    pub fn merge(&mut self, a: NodeId, b: NodeId) -> Result<NodeId> {
        diff::merge(self, a, b)
    }

    /// Revert head to a node whose state matches `target`. Appends a
    /// new node parented to current head (history is preserved).
    pub fn revert_to(&mut self, target: NodeId) -> Result<NodeId> {
        diff::revert_to(self, target)
    }

    /// Update `SessionNode::label` on an existing node *in place*. This
    /// is a metadata-only change and does NOT affect [`NodeId`] (the id
    /// is content-hashed over `state` only). The on-disk file is
    /// rewritten atomically via the same tempfile-then-rename pattern
    /// as `append`.
    pub fn set_label(&mut self, id: NodeId, label: Option<String>) -> Result<()> {
        let mut node = self.get(id)?;
        node.label = label;
        self.write_node_overwrite(&node)?;
        Ok(())
    }

    /// Atomically append several states as siblings of `parent`. All
    /// new node files are written via tempfile-then-rename; the head
    /// pointer is updated only after every node is durable on disk.
    /// Returns the ids of the newly appended nodes in input order.
    ///
    /// Used by the `apply_diff` tool to emit N alternative branches in
    /// a single transactional step. If any individual write fails,
    /// previously-renamed node files are left on disk (orphaned but
    /// harmless — content addressing makes them re-discoverable) and
    /// the head pointer is *not* moved, mirroring the crash-safety
    /// guarantee of [`Store::append`].
    pub fn append_branches(
        &mut self,
        parent: NodeId,
        states: Vec<(SessionState, Option<String>)>,
    ) -> Result<Vec<NodeId>> {
        if states.is_empty() {
            return Ok(Vec::new());
        }
        // Validate parent exists before doing any writes.
        let _ = self.get(parent)?;

        let mut written: Vec<NodeId> = Vec::with_capacity(states.len());
        for (state, label) in states {
            let id = NodeId::from_state(&state)?;
            let node = SessionNode {
                id,
                parent: Some(parent),
                created_at: Utc::now(),
                label,
                reasoning: None,
                state,
            };
            self.write_node_idempotent(&node)?;
            written.push(id);
        }
        // Move head to the last branch only after all writes succeeded.
        // Callers that want a different head can call `set_head`.
        if let Some(last) = written.last().copied() {
            self.write_head_atomic(last)?;
            self.head = Some(last);
        }
        Ok(written)
    }

    /// Idempotent atomic write — used by `append_branches`. Skips the
    /// rename if the content-addressed file already exists, matching
    /// `append`'s short-circuit so two byte-identical branches don't
    /// double-write.
    fn write_node_idempotent(&self, node: &SessionNode) -> Result<()> {
        let hex = node.id.to_hex();
        let shard_dir = self.shard_dir(&hex);
        fs::create_dir_all(&shard_dir)?;
        let final_path = shard_dir.join(format!("{hex}.json"));
        if final_path.exists() {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(node)?;
        let mut tmp = NamedTempFile::new_in(&shard_dir)?;
        tmp.write_all(&json)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&final_path)?;
        fsync_dir(&shard_dir)?;
        Ok(())
    }

    /// Force-overwrite atomic write — used by `set_label`. Always
    /// rewrites the file, even if a copy already exists on disk.
    fn write_node_overwrite(&self, node: &SessionNode) -> Result<()> {
        let hex = node.id.to_hex();
        let shard_dir = self.shard_dir(&hex);
        fs::create_dir_all(&shard_dir)?;
        let final_path = shard_dir.join(format!("{hex}.json"));
        let json = serde_json::to_vec_pretty(node)?;
        let mut tmp = NamedTempFile::new_in(&shard_dir)?;
        tmp.write_all(&json)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&final_path)?;
        fsync_dir(&shard_dir)?;
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
