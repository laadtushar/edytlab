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
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::node::{NodeId, SessionNode};
use crate::{Error, Result};

const STORE_DIR: &str = ".audiograph";
const NODES_DIR: &str = "nodes";
const HEAD_FILE: &str = "head";

pub struct Store {
    project_dir: PathBuf,
    head: Option<NodeId>,
}

impl Store {
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

    pub fn head(&self) -> NodeId {
        // Spec returns `NodeId` (not `Option<NodeId>`). Empty stores are
        // a programming error from the caller's perspective in Phase 1 —
        // the orchestrator always seeds an initial node before issuing
        // reads. Panic with a clear message rather than papering over it.
        self.head
            .expect("Store::head called on empty store; seed an initial node first")
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
        Ok(())
    }
}
