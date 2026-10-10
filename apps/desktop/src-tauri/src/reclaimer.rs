//! Keeping a project's derived audio under its cap, in the background (#98).
//!
//! Every destructive edit writes a new audio file under `derived/`, and
//! undo means every one of them stays named. Past
//! [`tools::reclaim::DEFAULT_DERIVED_CAP_BYTES`], the oldest audio only
//! undo history holds is removed — and only files a replay of their
//! edits has just rebuilt, so going back to one rebuilds it
//! (`tools::rederive::materialize`, on every path that reads a node).
//!
//! ## Why two handles
//!
//! Deciding what may go is slow: each history file is verified by
//! replaying the edits that made it. A command that takes the store's lock
//! holds it for its whole length, and so does any agent tool that does not
//! run off the lock (#421), so a sweep that verified under that lock would
//! stall the app: every read of the session would wait behind it. So the
//! plan is made on a store handle of this thread's own, which only reads,
//! and only the deletes take the lock. Node files are written whole and
//! renamed into place, so a second reader never sees half of one. Under
//! the lock no locked edit is half-way through, and `apply_sweep` checks
//! each file against the store as it is then: an undo onto a planned file,
//! or an edit that came to name one, keeps it.
//!
//! A tool that does run off the lock is half-way through, though, and the
//! lock does not show it: it has written its output file and not yet the
//! node that names it. `apply_sweep` therefore does nothing while one is
//! in flight (`Store::staged_in_flight`), and this pass tries again a
//! minute later.

use std::time::Duration;

use tools::reclaim::SweepReport;

use crate::state::AppState;

/// How often the size is checked. Under the cap a check is one directory
/// listing, so this bounds how far past the cap a busy session can get
/// before anything is removed, at a cost nobody will notice.
pub const CHECK_EVERY: Duration = Duration::from_secs(60);

/// One pass over the open project: plan off the lock, apply under it.
///
/// `None` when there was nothing to do — no project, under the cap, or
/// nothing over it that may go. Failures cost disk, not work, so they are
/// logged and read as nothing done.
pub fn reclaim_once(state: &AppState, cap_bytes: u64) -> Option<SweepReport> {
    let handle = state.store_handle()?;
    let project_dir = handle.lock().ok()?.project_dir().to_path_buf();
    let reader = match session::Store::open(&project_dir) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "could not open the project to plan a sweep");
            return None;
        }
    };
    let plan = match tools::reclaim::plan_sweep(&reader, cap_bytes) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "planning the derived-audio sweep failed");
            return None;
        }
    };
    if plan.is_empty() {
        return None;
    }

    let store = handle.lock().ok()?;
    if store.staged_in_flight() > 0 {
        tracing::debug!("derived-audio sweep deferred: a tool is running off the store lock");
        return None;
    }
    match tools::reclaim::apply_sweep(&store, &plan) {
        Ok(r) => {
            if r.removed_files > 0 {
                tracing::info!(
                    removed = r.removed_files,
                    freed_bytes = r.freed_bytes,
                    remaining_bytes = r.remaining_bytes,
                    "swept derived audio over the cap"
                );
            }
            if r.kept_unverified > 0 {
                // A file provenance calls rebuildable that a replay did
                // not rebuild: a gap in replay, worth knowing about.
                tracing::warn!(
                    kept = r.kept_unverified,
                    "history a replay could not rebuild was kept"
                );
            }
            Some(r)
        }
        Err(e) => {
            tracing::warn!(error = %e, "sweeping derived audio failed");
            None
        }
    }
}

/// Check the open project now and then every [`CHECK_EVERY`], for as
/// long as the app runs. Whichever project is open at each check is the
/// one checked, so opening another needs nothing here.
pub fn spawn(state: AppState) {
    let spawned = std::thread::Builder::new()
        .name("reclaim-derived-audio".into())
        .spawn(move || loop {
            reclaim_once(&state, tools::reclaim::DEFAULT_DERIVED_CAP_BYTES);
            std::thread::sleep(CHECK_EVERY);
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start the derived-audio sweep");
    }
}
