//! The mix the window plays, and the preview-cache entry it holds (#607).
//!
//! The transport plays the file `render_preview` returned, an entry in the
//! project's preview cache, and the cache evicts its least recently used
//! entries to stay under a cap. WaveSurfer plays a loaded mix from a blob, so
//! evicting the file does not stop a play that is under way; but the timeline
//! reads it again whenever it mounts, as it does when the Graph or Transcript
//! tab is left and returned to, and an entry the agent's renders, an
//! audition, a graph preview or a compare had evicted meanwhile was a "Could
//! not load the mix" for the mix the user was just playing.
//!
//! So the window's own render holds the entry it returns
//! ([`tools::preview_cache::Held`]), from before it is looked up until the
//! window's next render replaces it or the project changes. A held entry is
//! outside the cap, so the cache is over it by at most this one entry (and
//! the A/B compare's pair, while one is open), and letting the hold go
//! evicts the cache back under it.
//!
//! The window has one mix at a time, so there is one hold. Only the window's
//! `render_preview` command fills it: the agent's tools render into the same
//! cache with no hold of their own (`Tool::mutates` is false for them), so a
//! `render_preview` or `audition_effect` call neither takes the slot nor
//! replaces what is in it. The A/B compare keeps its own holds
//! ([`crate::compare`]).
//!
//! A hold is let go by value: [`WindowMix::replace`] and
//! [`WindowMix::release`] hand back what they displaced, and the caller drops
//! it after unlocking the slot. Dropping a hold evicts, which lists the cache
//! folder and removes files, and nothing else should wait on that.

use tools::preview_cache::Held;

/// The hold on the entry the window plays, if it has played one. Lives in
/// [`crate::state::AppState::window_mix`]. Generic over what a hold is only
/// so its tests can watch holds being dropped.
#[derive(Debug)]
pub struct WindowMix<H = Held> {
    held: Option<H>,
}

impl<H> Default for WindowMix<H> {
    fn default() -> Self {
        Self { held: None }
    }
}

impl<H> WindowMix<H> {
    /// The window rendered the mix it plays now, and `hold` keeps its entry
    /// out of eviction. Returns the hold on the mix it played before, which
    /// is an ordinary cache entry again once the caller drops it.
    ///
    /// The caller takes `hold` before it looks the entry up, and puts it here
    /// only once the render has worked: a render that fails changes nothing
    /// the window shows, so the mix before it stays held.
    #[must_use = "the hold displaced is let go of by dropping it, outside the slot's lock"]
    pub fn replace(&mut self, hold: H) -> Option<H> {
        self.held.replace(hold)
    }

    /// The window is no longer playing a mix of the project that was open
    /// (it was changed). Returns the hold, to be dropped by the caller.
    #[must_use = "the hold is let go of by dropping it, outside the slot's lock"]
    pub fn release(&mut self) -> Option<H> {
        self.held.take()
    }

    /// Whether a mix is held.
    #[cfg(test)]
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A stand-in for a hold that the test can see being dropped: a clone
    /// of `entry`, so `held(entry)` is whether the slot still has it.
    fn hold(entry: &Arc<()>) -> Arc<()> {
        Arc::clone(entry)
    }

    fn held(entry: &Arc<()>) -> bool {
        Arc::strong_count(entry) > 1
    }

    #[test]
    fn a_rendered_mix_is_held_until_it_is_released() {
        let entry = Arc::new(());
        let mut slot = WindowMix::default();

        assert!(slot.replace(hold(&entry)).is_none(), "nothing was held");
        assert!(slot.is_held() && held(&entry));

        drop(slot.release());
        assert!(!slot.is_held() && !held(&entry));
    }

    #[test]
    fn the_next_render_takes_over_and_hands_back_the_one_before() {
        let (first, second) = (Arc::new(()), Arc::new(()));
        let mut slot = WindowMix::default();
        drop(slot.replace(hold(&first)));

        let displaced = slot.replace(hold(&second));

        assert!(displaced.is_some(), "the hold on the first is handed back");
        assert!(
            held(&first),
            "it is held until the caller drops it, so the caller chooses when"
        );
        drop(displaced);
        assert!(
            !held(&first),
            "dropped, the first is an ordinary entry again"
        );
        assert!(held(&second), "and the second is held");
    }

    /// The caller takes the new hold before it drops the old one, so a
    /// render of the mix already held has no moment where the entry is not.
    #[test]
    fn rendering_the_same_entry_again_never_leaves_it_unheld() {
        let entry = Arc::new(());
        let mut slot = WindowMix::default();
        drop(slot.replace(hold(&entry)));

        let again = hold(&entry);
        assert!(held(&entry));
        drop(slot.replace(again));

        assert!(held(&entry));
        assert_eq!(Arc::strong_count(&entry), 2, "one hold, the slot's");
    }

    #[test]
    fn releasing_an_empty_slot_is_nothing() {
        let mut slot = WindowMix::<Arc<()>>::default();
        assert!(slot.release().is_none());
        assert!(!slot.is_held());
    }
}
