//! Session crate (Phase 1, M05).
//!
//! Provides the DAG data model for an edytlab session and a JSON-backed
//! content-addressable store. Phase 1 only exercises the linear subset
//! (`append` always parents to `head`); the branching primitives in
//! [`diff`] are Phase 2 stubs but live here so the on-disk schema is
//! forward-compatible from day one.

pub mod annotation;
pub mod diff;
pub mod live;
pub mod meta;
pub mod node;
pub mod relocate;
pub mod state;
pub mod store;
pub mod trail;

pub use annotation::{Annotation, AnnotationId, AnnotationKind};
pub use diff::{
    diff as diff_nodes, diff_states, fork as fork_node, merge as merge_nodes,
    revert_to as revert_to_node, BusMeta, DiffOp, DiffTarget, EffectScope, SessionDiff,
};
pub use live::{FolderId, LiveStore, Opened, Registry};
pub use node::{NodeId, NodeOp, SessionNode};
pub use state::{
    Bus, BusGraph, Clip, EffectInstance, EnvelopePoint, KeyMap, KeySegment, Send, SessionState,
    TempoMap, TempoSegment, Track, TrackId, Transcript, TranscriptWord,
};
pub use store::{Commit, Store, STORE_DIR};

/// Unified error type for the session crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json (de)serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tempfile persist error: {0}")]
    Persist(#[from] tempfile::PersistError),

    #[error("invalid hex in head file: {0}")]
    InvalidHeadHex(String),

    #[error("hex decode error: {0}")]
    HexDecode(String),

    #[error("node not found: {0}")]
    NodeNotFound(String),

    /// `merge(a, b)` failed because both branches modified at least one
    /// shared [`DiffTarget`] (e.g. both set `tracks[t].effects[e]`).
    /// `targets` lists every conflicting target as a debug string for
    /// human-readable diagnostics.
    #[error("merge conflict on {} target(s): [{}]", targets.len(), targets.join(", "))]
    MergeConflict { targets: Vec<String> },

    /// A staged store (see [`Store::stage`]) was asked to do something only
    /// the shared store may: rewrite history, move the head, or commit
    /// into the wrong place. The payload names what was asked.
    #[error("{0} is not allowed on a staged store")]
    Staged(&'static str),

    /// A store was asked to write that was opened to read
    /// ([`Store::open_read_only`]). The payload names what was asked.
    #[error("{0} is not allowed on a read-only store")]
    ReadOnly(&'static str),

    /// [`Store::open_private`] (or [`Store::open_shared`], when the holder is
    /// a private writer) found the folder already has a live store, and two
    /// writers on one project folder is what the registry exists to prevent.
    #[error(
        "the project folder {} is already open in this process; another writer on it would \
         overwrite its history",
        path.display()
    )]
    AlreadyOpen { path: std::path::PathBuf },

    /// The path now names a different folder from the one a live store was
    /// opened on (the project was restored over itself, or trashed and made
    /// again), and that store is still held, so a new one would be a second
    /// writer on the new folder's path. See `crate::live`.
    #[error(
        "the project folder {} was replaced while it was open, and the old one is still in use \
         (the assistant may be working in it); wait for the assistant's turn to end, then open \
         the project again",
        path.display()
    )]
    FolderReplaced { path: std::path::PathBuf },

    /// [`Store::reread`] was given a folder other than the store's own.
    #[error("{} is not the folder this store is on", path.display())]
    OtherFolder { path: std::path::PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;
