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
//!
//! ## Staged handles
//!
//! A tool that takes seconds should not hold the store's lock for all of
//! them (#421). A [`Store`] is only `{ project_dir, head }` over node files
//! that are written whole and never edited in a way that changes what they
//! mean, so a long edit can run against a *staged* copy of the handle
//! ([`Store::stage`]) with no lock held: reads see the shared head as it
//! was, and appends are kept in memory instead of written. When the edit
//! is done the staged handle is handed back under the lock
//! ([`Store::commit`]), which publishes it only if the shared head is still
//! where the edit started. Node ids hash state alone, so "the head is
//! where it started" is a comparison of ids, not of anything that can
//! drift. If the head moved — the user edited meanwhile — nothing is
//! written, and the caller runs the edit again on the new head.

use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tempfile::NamedTempFile;

use chrono::Utc;

use crate::diff::{self, SessionDiff};
use crate::node::{NodeId, SessionNode};
use crate::state::SessionState;
use crate::{Error, Result};

/// The store's own directory inside a project. Public because other
/// caches live beside it — the preview cache (#164) is under
/// `<project>/.audiograph/previews/`, and `storage_report` has to be
/// able to find them without hardcoding the name a second time.
pub const STORE_DIR: &str = ".audiograph";
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
    /// How many staged handles made from this store are alive. Shared by
    /// the store and every handle staged from it, so a sweep holding the
    /// store can tell that an edit is running without the lock (#421).
    in_flight: Arc<AtomicUsize>,
    /// `Some` for a handle made by [`Store::stage`]: appends go here
    /// instead of to disk.
    staged: Option<Staged>,
}

/// What a staged handle holds instead of writing it.
struct Staged {
    /// The head the handle was staged at. [`Store::commit`] publishes only
    /// if the shared store is still here.
    base: Option<NodeId>,
    /// Nodes appended since, in order, one per id.
    nodes: Vec<SessionNode>,
    /// Counts this handle in [`Store::staged_in_flight`] for as long as
    /// it lives, however it ends: committed, dropped, or unwound.
    _guard: InFlight,
}

/// Decrements the shared in-flight count when dropped.
struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How [`Store::commit`] ended.
#[must_use = "a Conflict means nothing was written and the work must be redone"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commit {
    /// The staged appends are now in the store, or there were none.
    Published,
    /// The store's head moved while the handle was staged, so nothing was
    /// written. The staged work was done against a state that is no longer
    /// the head.
    Conflict,
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
    ///
    /// Staged handles ([`Store::stage`]) keep this assumption: they never
    /// write, only [`Store::commit`] does, and it takes `&mut self` on the
    /// one shared store, so there is still one writer at a time.
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
            in_flight: Arc::new(AtomicUsize::new(0)),
            staged: None,
        })
    }

    /// A handle for running an edit without holding this store's lock
    /// (#421).
    ///
    /// The handle starts at the current head. Reads (`head`, `get`) work as
    /// on the store itself. `append` moves the handle's head but writes
    /// nothing: the node is kept in memory until [`Store::commit`]. Writes
    /// that rewrite history — `set_head`, `set_label`, `remove_node`,
    /// `detach_parent`, `append_branches`, and so `fork` — are refused with
    /// [`Error::Staged`], because the shared store cannot be told afterwards
    /// what they would have done in the middle of someone else's edit.
    /// `set_op` works on a node the handle appended.
    ///
    /// Take the store's lock to call this, and only for this call. The
    /// handle counts as in flight (see [`Store::staged_in_flight`]) from
    /// here until it is committed or dropped.
    pub fn stage(&self) -> Result<Store> {
        if self.staged.is_some() {
            return Err(Error::Staged("stage"));
        }
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let guard = InFlight(Arc::clone(&self.in_flight));
        Ok(Store {
            project_dir: self.project_dir.clone(),
            head: self.head,
            in_flight: Arc::clone(&self.in_flight),
            staged: Some(Staged {
                base: self.head,
                nodes: Vec::new(),
                _guard: guard,
            }),
        })
    }

    /// Whether this is a handle made by [`Store::stage`].
    pub fn is_staged(&self) -> bool {
        self.staged.is_some()
    }

    /// How many staged handles made from this store are alive.
    ///
    /// Anything that deletes files from the project has to wait while this
    /// is not zero: a staged edit writes its output before it is
    /// committed, and until then no node names it. It is incremented in
    /// [`Store::stage`] and decremented when the handle goes — in
    /// [`Store::commit`] on the normal path — and both happen under the
    /// store's lock, so a caller holding that lock reads a count that
    /// cannot change under it.
    pub fn staged_in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Publish what a staged handle appended, if the head has not moved.
    ///
    /// * Nothing appended (a read-only edit, or one that failed before it
    ///   wrote anything): nothing to do, whatever the head is now.
    ///   Returns [`Commit::Published`].
    /// * The head is still where `staged` started: its nodes are written in
    ///   order — skipping any that already exist, with an `op` the existing
    ///   record lacks merged in, first writer winning as in
    ///   [`Store::set_op`] — and the head file is written last. A crash in
    ///   between leaves the old head and some unreferenced node files, as
    ///   in [`Store::append`], never a head with a missing node.
    ///   Returns [`Commit::Published`].
    /// * The head moved: nothing is written. Returns [`Commit::Conflict`],
    ///   and the caller runs the edit again on the new head. Comparing ids
    ///   is exact, and a head that went somewhere and came back is the
    ///   same state, so it is no conflict.
    ///
    /// Consumes `staged`, which stops counting as in flight either way.
    ///
    /// Errors, without writing the head, if `self` is itself staged, if
    /// `staged` is not a staged handle, or if it was staged from another
    /// project.
    pub fn commit(&mut self, mut staged: Store) -> Result<Commit> {
        if self.staged.is_some() {
            return Err(Error::Staged("commit into a staged store"));
        }
        let Some(run) = staged.staged.take() else {
            return Err(Error::Staged("commit of an unstaged store"));
        };
        if staged.project_dir != self.project_dir {
            return Err(Error::Staged("commit into another project"));
        }
        if run.nodes.is_empty() {
            return Ok(Commit::Published);
        }
        if self.head != run.base {
            return Ok(Commit::Conflict);
        }

        for node in &run.nodes {
            let hex = node.id.to_hex();
            let exists = self.shard_dir(&hex).join(format!("{hex}.json")).exists();
            if !exists {
                self.write_node_idempotent(node)?;
            } else if let Some(op) = node.op.clone() {
                // The node is already there, from an earlier route to the
                // same state. Its record stands, unless it has none.
                self.set_op(node.id, op)?;
            }
        }
        if let Some(head) = staged.head {
            self.write_head_atomic(head)?;
            self.head = Some(head);
        }
        Ok(Commit::Published)
    }

    /// Append a node to the linear history. The caller's `parent` and `id`
    /// fields are overwritten: `parent` becomes the current head, `id` is
    /// recomputed from `state`.
    ///
    /// On a staged handle the node is kept in memory instead of written
    /// (see [`Store::stage`]).
    pub fn append(&mut self, mut node: SessionNode) -> Result<NodeId> {
        node.parent = self.head;
        node.id = NodeId::from_state(&node.state)?;
        let id = node.id;

        if let Some(run) = self.staged.as_mut() {
            // One entry per id, the first: as on disk, where a state
            // reached twice keeps the parent it had the first time. Whether
            // the file is already there is decided at commit.
            if !run.nodes.iter().any(|n| n.id == id) {
                run.nodes.push(node);
            }
            self.head = Some(id);
            return Ok(id);
        }

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

    /// Rename and/or move the annotation with id `target`, appending a
    /// new node with the change. Returns the new head id.
    ///
    /// Both parts are optional so one call covers a rename, a move, or
    /// both: the label lane offers all three and a caller should not
    /// have to read the current value back just to leave it alone.
    ///
    /// A no-op — unknown id, or nothing actually different — returns
    /// `head` unchanged rather than appending a node. Dragging a label
    /// and putting it back where it started should not cost an undo
    /// step, and neither should a rename dialog dismissed unedited.
    pub fn update_annotation(
        &mut self,
        head: NodeId,
        target: crate::annotation::AnnotationId,
        name: Option<String>,
        kind: Option<crate::annotation::AnnotationKind>,
    ) -> Result<NodeId> {
        let mut node = self.get(head)?;
        let Some(existing) = node.state.annotations.iter_mut().find(|a| a.id == target) else {
            return Ok(head);
        };

        let mut changed = false;
        if let Some(name) = name {
            if name != existing.name {
                existing.name = name;
                changed = true;
            }
        }
        if let Some(kind) = kind {
            if kind != existing.kind {
                existing.kind = kind;
                changed = true;
            }
        }
        if !changed {
            return Ok(head);
        }
        self.append(node)
    }

    /// Delete a node's file from the store.
    ///
    /// The only way history is ever removed, and it exists solely for
    /// `compact_session` (#98). Nothing else should reach for it: the
    /// DAG's whole value is that an edit stays there to undo to, and a
    /// node deleted while something still parents off it leaves a chain
    /// that cannot be walked. The caller is responsible for pruning
    /// leaves-first and for never removing the head or its ancestors.
    ///
    /// A node that is already gone is not an error — a compaction
    /// interrupted halfway can be run again.
    pub fn remove_node(&mut self, id: NodeId) -> Result<bool> {
        if self.staged.is_some() {
            return Err(Error::Staged("remove_node"));
        }
        let hex = id.to_hex();
        let path = self.shard_dir(&hex).join(format!("{hex}.json"));
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path)?;
        Ok(true)
    }

    /// Make `id` a root by clearing its parent link.
    ///
    /// The companion to [`Self::remove_node`]: after a compaction the
    /// oldest surviving node still points at one that is gone, and a
    /// walk back along parents then fails with "node not found" — which
    /// reads as a corrupt store rather than as the beginning of
    /// history. Cutting the link makes the kept chain a well-formed DAG
    /// that simply ends.
    ///
    /// Safe because a node's id is the hash of its **state** alone;
    /// `parent` is metadata the store maintains, so rewriting it does
    /// not change what the node is or where it lives on disk.
    pub fn detach_parent(&mut self, id: NodeId) -> Result<()> {
        if self.staged.is_some() {
            return Err(Error::Staged("detach_parent"));
        }
        let mut node = self.get(id)?;
        if node.parent.is_none() {
            return Ok(());
        }
        node.parent = None;

        let hex = id.to_hex();
        let shard_dir = self.shard_dir(&hex);
        let json = serde_json::to_vec_pretty(&node)?;
        let mut tmp = NamedTempFile::new_in(&shard_dir)?;
        tmp.write_all(&json)?;
        tmp.as_file().sync_all()?;
        tmp.persist(shard_dir.join(format!("{hex}.json")))?;
        fsync_dir(&shard_dir)?;
        Ok(())
    }

    pub fn get(&self, id: NodeId) -> Result<SessionNode> {
        // A staged handle sees what it appended before what is on disk.
        if let Some(node) = self
            .staged
            .as_ref()
            .and_then(|run| run.nodes.iter().find(|n| n.id == id))
        {
            return Ok(node.clone());
        }
        let hex = id.to_hex();
        let path = self.shard_dir(&hex).join(format!("{hex}.json"));
        if !path.exists() {
            return Err(Error::NodeNotFound(hex));
        }
        let bytes = fs::read(&path)?;
        let mut node: SessionNode = serde_json::from_slice(&bytes)?;
        // A project that has been copied or moved holds its own audio
        // under paths the nodes do not name. See `crate::relocate`; in a
        // project that has not moved this does nothing.
        crate::relocate::rebind(&mut node.state, &self.project_dir);
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
    ///
    /// On a staged handle the nodes it appended and that are not on disk
    /// yet are included.
    pub fn list_nodes(&self) -> Result<Vec<SessionNode>> {
        let mut out = self.list_disk_nodes()?;
        if let Some(run) = self.staged.as_ref() {
            let on_disk: std::collections::HashSet<NodeId> = out.iter().map(|n| n.id).collect();
            out.extend(
                run.nodes
                    .iter()
                    .filter(|n| !on_disk.contains(&n.id))
                    .cloned(),
            );
        }
        Ok(out)
    }

    fn list_disk_nodes(&self) -> Result<Vec<SessionNode>> {
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
                let mut node: SessionNode = serde_json::from_slice(&bytes)?;
                crate::relocate::rebind(&mut node.state, &self.project_dir);
                out.push(node);
            }
        }
        Ok(out)
    }

    pub fn set_head(&mut self, id: NodeId) -> Result<()> {
        if self.staged.is_some() {
            return Err(Error::Staged("set_head"));
        }
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

    /// Move head to a node whose state matches `target`.
    ///
    /// Node ids hash the state alone, so appending `target`'s state lands
    /// on `target` itself: the result is `target`, still parented to
    /// whatever it was parented to when first reached (see
    /// [`Store::append`]). Nothing is added and every node after it is
    /// kept, so no history is lost; what is not recorded is the route
    /// that led here.
    pub fn revert_to(&mut self, target: NodeId) -> Result<NodeId> {
        diff::revert_to(self, target)
    }

    /// Update `SessionNode::label` on an existing node *in place*. This
    /// is a metadata-only change and does NOT affect [`NodeId`] (the id
    /// is content-hashed over `state` only). The on-disk file is
    /// rewritten atomically via the same tempfile-then-rename pattern
    /// as `append`.
    pub fn set_label(&mut self, id: NodeId, label: Option<String>) -> Result<()> {
        if self.staged.is_some() {
            return Err(Error::Staged("set_label"));
        }
        let mut node = self.get(id)?;
        node.label = label;
        self.write_node_overwrite(&node)?;
        Ok(())
    }

    /// Record how a node was produced (#98).
    ///
    /// Safe to call after the node is written, because [`NodeId`] hashes
    /// only `state` — provenance is a sibling field and cannot change
    /// the node's identity.
    ///
    /// **First writer wins.** Node ids are content-addressed on state, so
    /// two different routes to the same state land on the same node. The
    /// existing record describes the run that actually produced the file
    /// on disk, and overwriting it with a later, equivalent derivation
    /// would trade a fact for a guess.
    ///
    /// On a staged handle this applies to a node the handle appended, in
    /// memory, and lands with [`Store::commit`]. Any other node is refused:
    /// the handle holds no lock, so it must not rewrite files.
    pub fn set_op(&mut self, id: NodeId, op: crate::node::NodeOp) -> Result<()> {
        if let Some(run) = self.staged.as_mut() {
            let Some(node) = run.nodes.iter_mut().find(|n| n.id == id) else {
                return Err(Error::Staged(
                    "set_op on a node the staged run did not append",
                ));
            };
            if node.op.is_none() {
                node.op = Some(op);
            }
            return Ok(());
        }
        let mut node = self.get(id)?;
        if node.op.is_some() {
            return Ok(());
        }
        node.op = Some(op);
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
        if self.staged.is_some() {
            return Err(Error::Staged("append_branches"));
        }
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
                op: None,
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
