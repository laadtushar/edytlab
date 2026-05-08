//! Session crate (Phase 1, M05).
//!
//! Provides the DAG data model for an edytlab session and a JSON-backed
//! content-addressable store. Phase 1 only exercises the linear subset
//! (`append` always parents to `head`); the branching primitives in
//! [`diff`] are Phase 2 stubs but live here so the on-disk schema is
//! forward-compatible from day one.

pub mod diff;
pub mod node;
pub mod state;
pub mod store;

pub use diff::{
    diff as diff_nodes, diff_states, fork as fork_node, merge as merge_nodes,
    revert_to as revert_to_node, BusMeta, DiffOp, DiffTarget, EffectScope, SessionDiff,
};
pub use node::{NodeId, SessionNode};
pub use state::{
    Bus, BusGraph, Clip, EffectInstance, KeyMap, KeySegment, SessionState, TempoMap, TempoSegment,
    Track, TrackId, Transcript, TranscriptWord,
};
pub use store::Store;

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
}

pub type Result<T> = std::result::Result<T, Error>;
