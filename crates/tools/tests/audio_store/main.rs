//! The project's audio store as its callers in `tools` use it (ADR 0008,
//! #719).
//!
//! The store's own cases (names and the pre-#620 names, the folders, the
//! write-once puts, which files it owns, the deleter and the in-flight guard,
//! the copies into a project and out of it, what a replay rebuilt and under
//! which names, and the stale staging files of a writer) are tested once, in
//! `session::audio_store`'s own tests, which need the module's private
//! constructors. What is here is what only a caller can show, so each test
//! runs tools: it builds its project with the agent's tools and then sweeps
//! it, compacts it, rebuilds it, repairs it or copies it through
//! `tools::reclaim`, `tools::rederive` and `tools::repair`.
//!
//! - [`legacy_names`]: a project written before #620 still sweeps and
//!   rebuilds under its older names.
//! - [`compaction_ownership`]: `compact_session` deletes only what the store
//!   owns in `derived/` (#589).
//! - [`copied_project`]: the sweeps, a rebuild and the repair on a copy of a
//!   project with the original still on disk, and after it is gone (#612).
//! - [`reclaim`]: the sweeps' policy (#98): what is removed, what is kept, and
//!   that nothing is removed while a handle is staged.

#[path = "../support/harness.rs"]
mod harness;

mod compaction_ownership;
mod copied_project;
mod legacy_names;
mod reclaim;
