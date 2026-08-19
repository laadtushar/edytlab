//! Progress and cancellation for long-running tools (#169 §1).
//!
//! A tool call is one round trip: it returns when it is finished, so
//! there is nowhere for a twelve-file batch to say "on file 7" and
//! nothing for a user to interrupt. `batch_apply` shipped with that
//! acceptance box deliberately unticked.
//!
//! ## Why a registered sink rather than a field on `ToolContext`
//!
//! `ToolContext` is built at 112 call sites. Threading a reporter
//! through all of them to serve the one tool that needs it would be a
//! change whose diff is almost entirely mechanical noise, and it would
//! put a lifetime on every test fixture in the workspace.
//!
//! So this is shaped like `log::set_logger`: the application registers
//! one sink at startup, tools call `report` without knowing who is
//! listening, and in a test or a CLI run with no sink registered every
//! call is a cheap no-op. The precedent is well worn and it is the same
//! trade — a process-wide destination for something inherently
//! process-wide.
//!
//! Cancellation is process-wide for the same reason: there is one user
//! and one foreground batch, and a cancel button means "stop the thing
//! I am watching".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use serde_json::Value;

type Sink = Box<dyn Fn(Value) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Register where progress goes. First call wins; later ones are
/// ignored and report `false`.
///
/// Once, at startup, like a logger. A sink that could be swapped
/// mid-run would let one window's cancel button silence another's
/// progress.
pub fn set_sink(sink: impl Fn(Value) + Send + Sync + 'static) -> bool {
    SINK.set(Box::new(sink)).is_ok()
}

/// Emit a progress event. A no-op when nothing is listening.
pub fn report(event: Value) {
    if let Some(sink) = SINK.get() {
        sink(event);
    }
}

/// Start a cancellable run, clearing any stale request.
///
/// Called by the tool rather than by the canceller: a cancel that
/// arrives after the run it was meant for would otherwise kill the
/// *next* one, which is a genuinely alarming thing for a batch to do.
pub fn begin() {
    CANCELLED.store(false, Ordering::SeqCst);
}

/// Ask the running tool to stop at its next checkpoint.
pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Whether a stop has been asked for.
pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `begin` clears a stale request, so a cancel that lands after its
    /// run cannot kill the next one.
    #[test]
    fn beginning_a_run_clears_an_earlier_cancel() {
        request_cancel();
        assert!(cancelled());
        begin();
        assert!(!cancelled());
    }

    /// Reporting with no sink registered is a no-op, not a panic — that
    /// is the state every test and the CLI run in.
    #[test]
    fn reporting_without_a_sink_is_harmless() {
        report(serde_json::json!({ "hello": "world" }));
    }
}
