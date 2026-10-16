//! The A/B compare the window has open, and the preview-cache entries it
//! holds (#575).
//!
//! `prepare_compare` renders each side into the project's preview cache,
//! which evicts its least recently used entries to stay under a cap. The
//! A/B bar loads a side from its file on every switch, for as long as it
//! is open, so a side evicted meanwhile is a switch that fails. The two
//! entries are therefore held ([`tools::preview_cache::Held`]) from before
//! they are rendered until the bar ends the compare (`end_compare`). A held
//! entry is outside the cap, so the cache can be over it by this one pair;
//! letting the pair go evicts it back under.
//!
//! The window has one A/B bar, so there is one compare at a time: starting
//! one ends the one before. Each gets an id, and only the id of the open
//! compare ends it, so a bar that closes as a newer one opens cannot let go
//! of the newer one's renders.

use tools::preview_cache::Held;

/// The compare whose renders are held, if any. Lives in
/// [`crate::state::AppState::compare`]. Generic over what a hold is only so
/// its tests can watch holds being dropped.
#[derive(Debug)]
pub struct CompareHolds<H = Held> {
    /// The id the last compare to begin was given. Ids only grow, and the
    /// first is 1.
    last: u64,
    /// The open compare, and the holds on its two sides.
    open: Option<(u64, Vec<H>)>,
}

impl<H> Default for CompareHolds<H> {
    fn default() -> Self {
        Self {
            last: 0,
            open: None,
        }
    }
}

impl<H> CompareHolds<H> {
    /// A compare begins: whatever was open is over, and its renders are
    /// ordinary cache entries again. Returns the new compare's id.
    pub fn begin(&mut self) -> u64 {
        self.open = None;
        self.last += 1;
        self.last
    }

    /// The compare `id` has rendered both sides: hold `sides` until it
    /// ends. If another compare has begun since, `id` is already over and
    /// `sides` are let go of at once; the window has moved on and will not
    /// play them.
    pub fn open(&mut self, id: u64, sides: Vec<H>) {
        if id == self.last {
            self.open = Some((id, sides));
        }
    }

    /// The bar for compare `id` has closed. Ending a compare that is not
    /// the open one (an older one, or one never begun) changes nothing.
    pub fn end(&mut self, id: u64) {
        if matches!(self.open, Some((open, _)) if open == id) {
            self.open = None;
        }
    }

    /// The id of the open compare, if one is.
    #[cfg(test)]
    pub fn open_id(&self) -> Option<u64> {
        self.open.as_ref().map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A stand-in for a hold that the test can see being dropped: a clone
    /// of `side`, so `held(side)` is whether the compare still has it.
    fn hold(side: &Arc<()>) -> Arc<()> {
        Arc::clone(side)
    }

    fn held(side: &Arc<()>) -> bool {
        Arc::strong_count(side) > 1
    }

    fn sides() -> [Arc<()>; 2] {
        [Arc::new(()), Arc::new(())]
    }

    #[test]
    fn an_open_compare_holds_its_sides_until_it_ends() {
        let [a, b] = sides();
        let mut holds = CompareHolds::default();

        let id = holds.begin();
        assert_eq!(id, 1, "the first compare of a launch is 1");
        holds.open(id, vec![hold(&a), hold(&b)]);
        assert_eq!(holds.open_id(), Some(id));
        assert!(held(&a) && held(&b));

        holds.end(id);
        assert_eq!(holds.open_id(), None);
        assert!(!held(&a) && !held(&b));
    }

    #[test]
    fn a_new_compare_ends_the_one_before() {
        let ([a, b], [c, d]) = (sides(), sides());
        let mut holds = CompareHolds::default();
        let first = holds.begin();
        holds.open(first, vec![hold(&a), hold(&b)]);

        let second = holds.begin();
        assert!(second > first, "ids only grow");
        assert!(!held(&a) && !held(&b), "the first pair is let go");
        holds.open(second, vec![hold(&c), hold(&d)]);
        assert!(held(&c) && held(&d));
    }

    /// The bar for the first compare closes as the second opens: its end
    /// arrives after the second has begun, and must not end the second.
    #[test]
    fn ending_an_older_compare_leaves_the_open_one() {
        let ([a, b], [c, d]) = (sides(), sides());
        let mut holds = CompareHolds::default();
        let first = holds.begin();
        holds.open(first, vec![hold(&a), hold(&b)]);
        let second = holds.begin();
        holds.open(second, vec![hold(&c), hold(&d)]);

        holds.end(first);
        holds.end(second + 1);

        assert_eq!(holds.open_id(), Some(second));
        assert!(held(&c) && held(&d));
    }

    /// Two compares render at once, and the older finishes last. The
    /// window shows the newer one: the older one's renders are not held,
    /// and its result does not take the newer one's place.
    #[test]
    fn a_compare_that_finishes_after_a_newer_one_began_holds_nothing() {
        let ([a, b], [c, d]) = (sides(), sides());
        let mut holds = CompareHolds::default();
        let older = holds.begin();
        let newer = holds.begin();

        holds.open(newer, vec![hold(&c), hold(&d)]);
        holds.open(older, vec![hold(&a), hold(&b)]);

        assert_eq!(holds.open_id(), Some(newer));
        assert!(held(&c) && held(&d));
        assert!(!held(&a) && !held(&b));
    }

    /// A compare that failed to render never opens; a later begin and end
    /// find nothing of it.
    #[test]
    fn a_compare_that_never_opened_holds_nothing() {
        let mut holds = CompareHolds::<Arc<()>>::default();
        let failed = holds.begin();
        holds.end(failed);
        assert_eq!(holds.open_id(), None);
    }
}
