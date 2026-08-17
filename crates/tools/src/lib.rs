//! Tool dispatcher (Phase 1, M07).
//!
//! Defines the [`Tool`] trait, the [`ToolDispatcher`] registry, and the
//! [`ToolContext`] handed to tools at invocation time. Individual tools
//! land in `tool/` in subsequent milestones (M08, M09).
//!
//! Schemas exported by this crate match the **Anthropic tool-use** shape:
//!
//! ```json
//! {
//!   "name": "tool_name",
//!   "description": "...",
//!   "input_schema": {
//!     "type": "object",
//!     "properties": { ... },
//!     "required": [ ... ]
//!   }
//! }
//! ```
//!
//! The dispatcher validates incoming `args` against each tool's
//! `input_schema` *before* calling [`Tool::invoke`], so individual tool
//! implementations can assume their inputs already conform to the
//! advertised schema.

pub mod dispatcher;
pub mod preview_cache;
pub mod provenance;
pub mod schema;
pub mod tool;
pub mod util;

pub use dispatcher::{Tool, ToolContext, ToolDispatcher, READS_OUTSIDE_THE_SESSION};
pub use preview_cache::{Hit as PreviewHit, PreviewCache};
pub use provenance::{verify_chain, Problem};
pub use util::range_resolver::{resolve as resolve_range, Range, RangeError};

/// Materialise a multi-clip track as a single WAV, for callers outside
/// this crate that need something to draw. Re-exported rather than made
/// module-public so the rest of `tool::util` stays internal.
pub use tool::util::flattened_track_wav;

/// Result returned by an individual tool invocation.
///
/// Maps directly to Anthropic's tool-use response shape: [`ToolResult::Ok`]
/// becomes a successful `tool_result` content block, while
/// [`ToolResult::Error`] becomes a `tool_result` with `is_error: true` so
/// the model can recover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolResult {
    Ok(serde_json::Value),
    Error(String),
}

/// Errors raised by the dispatcher itself, distinct from tool-level
/// failures (which are surfaced as [`ToolResult::Error`]).
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("unknown tool: {0}")]
    Unknown(String),

    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    #[error("malformed tool schema for {tool}: {reason}")]
    MalformedToolSchema { tool: String, reason: String },
}

pub type Result<T> = std::result::Result<T, DispatchError>;
