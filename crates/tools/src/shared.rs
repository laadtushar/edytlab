//! Dispatching a tool against the app's shared state, without holding the
//! store's lock for the whole of a long tool (#421).
//!
//! The agent used to lock the dispatcher, the store, the engine and the
//! clipboard, call the tool, and release them all when it returned. A
//! time-stretch of a 32 s stereo track takes about ten seconds in a debug
//! build, and for all of them every command that wanted the store waited:
//! the timeline's reads, the mixer, undo. This module is the replacement.
//!
//! ## How a call runs
//!
//! 1. **Admit** under the dispatcher's lock, alone, and drop it. The lock
//!    is held for a hash lookup and a schema check, never while waiting on
//!    another lock, which is also what removes the order inversion with the
//!    commands that lock the store, then the engine, then the dispatcher.
//! 2. A tool that does not say [`crate::Tool::runs_off_the_lock`] takes the
//!    store, engine and clipboard locks, in that order, for the whole call.
//!    This is what every tool did before, and what a new tool does until
//!    someone opts it in.
//! 3. A tool that says so is run on a staged store handle
//!    (`session::Store::stage`) with no lock held, a fresh engine and an
//!    empty clipboard of its own. Whatever it appends stays in memory. Then
//!    the store's lock is taken for [`session::Store::commit`], which
//!    writes it only if the head is still where the run started:
//!    * **published** — the tool's result is returned;
//!    * **conflict** — the head moved while the tool ran, which means a
//!      user edit (a marker, a fader, an undo, a clip move) landed in the
//!      middle. That edit is kept. The tool's work is thrown away and the
//!      tool is run again on the new head, so its node parents off the
//!      user's.
//! 4. Progress is guaranteed. After [`OFF_LOCK_ATTEMPTS`] lost races the
//!    tool is run once under the locks, which is step 2: nothing can move
//!    the head under it.
//!
//! A run that loses leaves only files under their content-addressed names
//! (see [`session::Store::staged_in_flight`] for why the sweep waits).
//! It never leaves a session node.
//!
//! A panic in an off-lock tool poisons no lock, where one in a locked tool
//! poisons the store's.

use std::collections::HashSet;
use std::sync::Mutex;

use serde_json::Value;

use crate::dispatcher::Prepared;
use crate::{Clipboard, Result, ToolContext, ToolDispatcher, ToolResult};

/// How many times an off-lock tool is run before it is run under the
/// locks instead. Two means one retry after a user edit lands mid-run:
/// a second edit during the retry is rare enough that waiting out a third
/// run is worse than holding the lock once.
pub const OFF_LOCK_ATTEMPTS: usize = 2;

/// The four pieces of app state a tool call touches, as the app shares
/// them. Borrowed, so the app keeps owning them.
pub struct Shared<'a> {
    pub dispatcher: &'a Mutex<ToolDispatcher>,
    pub store: &'a Mutex<session::Store>,
    pub engine: &'a Mutex<audio_engine::Engine>,
    pub clipboard: &'a Mutex<Option<Clipboard>>,
}

impl Shared<'_> {
    /// Dispatch `name` with `args` as [`ToolDispatcher::invoke`] would, with
    /// the same errors, running the tool off the store lock when it allows
    /// it. See the module docs.
    ///
    /// `user_message` is the chat message the call belongs to, which a few
    /// tools read a selection range from. `allowed` is this turn's tool
    /// whitelist, or `None` for unrestricted.
    ///
    /// Poisoned locks panic, as the agent loop always has: a tool that
    /// panicked under a lock left the session in a state nobody has looked
    /// at.
    pub fn dispatch(
        &self,
        name: &str,
        args: Value,
        user_message: &str,
        allowed: Option<&HashSet<String>>,
    ) -> Result<ToolResult> {
        let prepared = {
            let d = self.dispatcher.lock().expect("dispatcher mutex poisoned");
            d.prepare(name, &args, allowed)?
        };

        if prepared.runs_off_the_lock() {
            for attempt in 1..=OFF_LOCK_ATTEMPTS {
                if let Some(result) =
                    self.run_off_the_lock(&prepared, &args, user_message, allowed, attempt)
                {
                    return result;
                }
            }
            tracing::info!(
                tool = name,
                attempts = OFF_LOCK_ATTEMPTS,
                "the session kept changing while the tool ran; running it under the lock"
            );
        }
        self.run_locked(&prepared, args, user_message, allowed)
    }

    /// One attempt with no lock held. `None` means the session changed
    /// under it and nothing was published.
    fn run_off_the_lock(
        &self,
        prepared: &Prepared,
        args: &Value,
        user_message: &str,
        allowed: Option<&HashSet<String>>,
        attempt: usize,
    ) -> Option<Result<ToolResult>> {
        // The lock is held for this one call, and released before the tool.
        let staged = self.store.lock().expect("store mutex poisoned").stage();
        let mut staged = match staged {
            Ok(s) => s,
            Err(e) => {
                return Some(Ok(ToolResult::Error(format!(
                    "could not start {}: {e}",
                    prepared.name()
                ))))
            }
        };

        // Nothing the tool may use is shared. `Engine` holds no state, and
        // the tools that opt in never read the clipboard.
        let mut engine = audio_engine::Engine::new();
        let mut clipboard: Option<Clipboard> = None;
        let result = {
            let mut ctx = ToolContext {
                store: &mut staged,
                engine: &mut engine,
                user_message,
                clipboard: &mut clipboard,
                allowed_tools: allowed,
            };
            ToolDispatcher::run(prepared, args.clone(), &mut ctx)
        };

        // Committed whatever the result was: a tool that appended and then
        // failed leaves its node behind when it runs under the lock, and
        // the two paths should agree.
        let committed = self
            .store
            .lock()
            .expect("store mutex poisoned")
            .commit(staged);
        match committed {
            Ok(session::Commit::Published) => Some(result),
            Ok(session::Commit::Conflict) => {
                tracing::info!(
                    tool = prepared.name(),
                    attempt,
                    "the session changed while the tool ran; running it again on the new head"
                );
                None
            }
            Err(e) => Some(Ok(ToolResult::Error(format!("session append failed: {e}")))),
        }
    }

    /// The tool, under the store, engine and clipboard locks — in that
    /// order, as every other caller takes them.
    fn run_locked(
        &self,
        prepared: &Prepared,
        args: Value,
        user_message: &str,
        allowed: Option<&HashSet<String>>,
    ) -> Result<ToolResult> {
        let mut store = self.store.lock().expect("store mutex poisoned");
        let mut engine = self.engine.lock().expect("engine mutex poisoned");
        let mut clipboard = self.clipboard.lock().expect("clipboard mutex poisoned");
        let mut ctx = ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message,
            clipboard: &mut clipboard,
            allowed_tools: allowed,
        };
        ToolDispatcher::run(prepared, args, &mut ctx)
    }
}
