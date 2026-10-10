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
//!      user's — unless that edit changed what the call's arguments
//!      mean (step 5).
//! 4. Progress is guaranteed. The tool is run off the lock at most
//!    [`OFF_LOCK_ATTEMPTS`] times: the first run and one re-run. If both
//!    lose, it is run once more under the locks, which is step 2: nothing
//!    can move the head under it.
//! 5. **A re-run must mean what the first run meant.** A call names a track
//!    by its position (`track: 1`) and a time by its seconds, as the model
//!    saw them. Before #421 a user's edit waited for the tool, so the tool
//!    always ran on what the model had seen. Run again after the user
//!    removed track 0, `track: 1` is a different track; run again after the
//!    user dragged a clip, the same seconds are different audio; and the
//!    model is told the call succeeded. So a call that names tracks (see
//!    [`addressed_tracks`]) remembers, from the head its first run began
//!    on, each named track's id and clips, and every later run — the
//!    re-run and the locked run alike — begins only if each named position
//!    still holds that track with those clips. Otherwise nothing is applied
//!    and the call returns a [`ToolResult::Error`] saying what changed and
//!    what the tracks are now, for the model to read and call again. The
//!    user's edit stays. A track added or removed elsewhere, a fader, a
//!    marker or a clip moved on another track does not stop a re-run: it
//!    does not change which audio the call was about. A call that names no
//!    track (`load`) is always re-run.
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

/// The positions of the tracks a call names, from its arguments.
///
/// By convention a tool that takes a track by its position in the session
/// calls the argument `track`, or lists them in `track_indices`. Rather than
/// listing those names, this reads every top-level argument whose name
/// contains `track` and whose value is a whole number or a list of them, so
/// an argument called `source_track` is covered the day a tool adds one.
/// `off_lock_tools.rs` checks that every tool that runs off the lock names
/// its tracks in a way this reads. Ascending, without repeats.
pub fn addressed_tracks(args: &Value) -> Vec<usize> {
    let Some(map) = args.as_object() else {
        return Vec::new();
    };
    let position = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
    let mut out: Vec<usize> = Vec::new();
    for (_, value) in map.iter().filter(|(key, _)| key.contains("track")) {
        match value {
            Value::Array(items) => out.extend(items.iter().filter_map(position)),
            other => out.extend(position(other)),
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// A track a call named, as the first run of the call saw it.
struct Seen {
    index: usize,
    id: session::TrackId,
    name: String,
    clips: Vec<session::Clip>,
}

/// The tracks a call names, as they were at the head its first run began
/// on. Later runs are measured against this, not against each other: a
/// change that is harmless on its own, then another, can still add up to
/// a track that is not the one the call named.
struct Named {
    tracks: Vec<Seen>,
}

impl Named {
    /// What `store` holds, at its head, at each of `positions`. A position
    /// with no track is left out: the tool reports that itself, and
    /// appends nothing.
    fn capture(store: &session::Store, positions: &[usize]) -> Self {
        let tracks = head_state(store)
            .iter()
            .flat_map(|state| {
                positions.iter().filter_map(|&index| {
                    let track = state.tracks.get(index)?;
                    Some(Seen {
                        index,
                        id: track.id.clone(),
                        name: track.name.clone(),
                        clips: track.clips.clone(),
                    })
                })
            })
            .collect();
        Self { tracks }
    }

    /// The error to return instead of running the call on the head of
    /// `store`, or `None` if every track the call named is still where it
    /// was, with its clips.
    ///
    /// Clips are compared whole. A tool that takes no time at all would be
    /// safe to run on a moved clip, but most take seconds (`fade`, the
    /// `start_sec` of the filters, the `[apply to …]` selection), and
    /// "which tools" is a list that goes stale. A user who edits the very
    /// track the agent is working on, in the seconds it takes, gets an
    /// error to read and a call to make again, never a wrong edit.
    fn refusal(&self, tool: &str, store: &session::Store) -> Option<String> {
        let Some(state) = head_state(store) else {
            return Some(format!(
                "{tool} was not applied: the session could not be read after it changed while \
                 the tool ran. Nothing was changed by this call."
            ));
        };
        let what = self
            .tracks
            .iter()
            .find_map(|seen| match state.tracks.get(seen.index) {
                Some(now) if now.id == seen.id && now.clips == seen.clips => None,
                Some(now) if now.id == seen.id => Some(format!(
                    "the clips on track {} (\"{}\") were changed (moved, trimmed, split or \
                     replaced)",
                    seen.index, seen.name
                )),
                Some(now) => Some(format!(
                    "track {} stopped being \"{}\" and became \"{}\"",
                    seen.index, seen.name, now.name
                )),
                None => Some(format!(
                    "track {} (\"{}\") stopped existing",
                    seen.index, seen.name
                )),
            })?;
        let now: Vec<String> = state
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{i} \"{}\"", t.name))
            .collect();
        tracing::info!(
            tool,
            %what,
            "the session changed what the call names; not running it again"
        );
        Some(format!(
            "{tool} was not applied. While it ran, {what}. The call names tracks by their \
             position and times by their seconds, and the session has changed since you chose \
             them, so running it again could edit different audio than you meant. Nothing was \
             changed by this call; the change that was made is kept. The tracks are now: {}. If \
             the edit is still wanted, call {tool} again.",
            if now.is_empty() {
                "none".to_string()
            } else {
                now.join(", ")
            }
        ))
    }
}

/// The state at `store`'s head, if there is one and it reads.
fn head_state(store: &session::Store) -> Option<session::SessionState> {
    store
        .head()
        .and_then(|head| store.get(head).ok())
        .map(|node| node.state)
}

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
            // What the call names, as its first run saw it (module docs,
            // step 5). Filled in by the first attempt.
            let positions = addressed_tracks(&args);
            let mut named: Option<Named> = None;
            for attempt in 1..=OFF_LOCK_ATTEMPTS {
                if let Some(result) = self.run_off_the_lock(
                    &prepared,
                    &args,
                    (&positions, &mut named),
                    user_message,
                    allowed,
                    attempt,
                ) {
                    return result;
                }
            }
            tracing::info!(
                tool = name,
                attempts = OFF_LOCK_ATTEMPTS,
                "the session kept changing while the tool ran; running it under the lock"
            );
            return self.run_locked(&prepared, args, named.as_ref(), user_message, allowed);
        }
        self.run_locked(&prepared, args, None, user_message, allowed)
    }

    /// One attempt with no lock held. `None` means the session changed
    /// under it and nothing was published.
    ///
    /// `named` is the positions of the tracks the call names, and what the
    /// first attempt saw at them, which this attempt records if it is the
    /// first. An attempt that finds one of them changed returns the refusal
    /// without running the tool.
    fn run_off_the_lock(
        &self,
        prepared: &Prepared,
        args: &Value,
        (positions, named): (&[usize], &mut Option<Named>),
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

        // The base this attempt runs on is fixed by `stage`, and is read
        // here, off the lock, from the handle: the state the tool will see.
        if !positions.is_empty() {
            match named {
                None => *named = Some(Named::capture(&staged, positions)),
                Some(first) => {
                    if let Some(refusal) = first.refusal(prepared.name(), &staged) {
                        return Some(Ok(ToolResult::Error(refusal)));
                    }
                }
            }
        }

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
    ///
    /// `named` is what an earlier off-lock run of the same call saw, if
    /// there was one. The head is checked against it under the lock, where
    /// nothing can move it before the tool has run.
    fn run_locked(
        &self,
        prepared: &Prepared,
        args: Value,
        named: Option<&Named>,
        user_message: &str,
        allowed: Option<&HashSet<String>>,
    ) -> Result<ToolResult> {
        let mut store = self.store.lock().expect("store mutex poisoned");
        if let Some(refusal) = named.and_then(|n| n.refusal(prepared.name(), &store)) {
            return Ok(ToolResult::Error(refusal));
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_call_names_the_tracks_in_track_and_track_indices() {
        assert_eq!(addressed_tracks(&json!({ "track": 3, "factor": 2.0 })), [3]);
        assert_eq!(
            addressed_tracks(&json!({ "track_indices": [4, 1, 4], "name": "mix" })),
            [1, 4]
        );
    }

    /// Any argument about a track counts, so a tool that calls it
    /// something else is still covered.
    #[test]
    fn any_argument_about_a_track_counts() {
        assert_eq!(
            addressed_tracks(&json!({ "source_track": 2, "music_track": 0, "track_index": 5 })),
            [0, 2, 5]
        );
    }

    #[test]
    fn a_call_that_names_none_names_none() {
        assert!(addressed_tracks(&json!({})).is_empty());
        assert!(addressed_tracks(&json!({ "path": "take.wav" })).is_empty());
        // Not positions: a name, a missing value, a negative number, a
        // fraction.
        assert!(addressed_tracks(&json!({
            "track": null, "track_name": "vocals", "track_gain": -3, "track_pan": 0.5
        }))
        .is_empty());
        assert!(addressed_tracks(&json!(["track"])).is_empty());
    }
}
