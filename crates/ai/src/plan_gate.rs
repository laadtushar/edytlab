//! The plan gate: Plan first's wait for exactly one answer to a shown plan
//! (ADR 0002).
//!
//! The user is shown a card (the model's plan, or the first step that would
//! change the session, #415) and the turn stops until one of four things
//! happens: they approve it, approve it with a revision, reject it, or
//! nobody answers for five minutes. Those are the whole protocol, and this
//! module is all of it. It used to be four values shared by two crates (a
//! `Notify`, a revision slot, a rejection flag, and a flag the desktop kept
//! to know whether anyone was waiting), and the rules for them lived partly
//! in each. A stray Run click on a card that had gone away banked a permit
//! the next turn's gate took for its own answer (#251, #290).
//!
//! # How it works
//!
//! [`PlanGate::ask`] opens one question: it makes a `tokio::sync::oneshot`
//! pair for this card only, keeps the sender in the gate's slot and hands
//! the receiver to the turn as a [`Waiting`]. [`PlanGate::answer`] takes the
//! sender out of the slot and sends the answer. A oneshot carries one
//! message, so the rest follows from the type and nothing needs a check:
//!
//! * a second answer finds the slot empty and does nothing;
//! * an answer with no question open finds the slot empty and does nothing,
//!   so it can never be kept for the next question;
//! * when the turn stops waiting (the time ran out, or the turn was
//!   dropped) the [`Waiting`] clears the slot, so a late answer finds
//!   nothing either.
//!
//! # Locks
//!
//! The gate is a handle that is cloned to wherever it is needed: the agent,
//! the desktop's `AppState` and the commands that answer. Answering takes
//! the gate's own lock for the length of a pointer swap and nothing else.
//! It never takes the agent's lock, which `send_message` holds across the
//! whole turn, and it needs no runtime: it is a plain call (CLAUDE.md,
//! "Lock order"). The gate's lock is a leaf: nothing is taken while it is
//! held, and the sender, whose drop wakes the turn, is only dropped or
//! used after it is let go.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::oneshot;

/// What the user said to the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Run it as shown.
    Approve,
    /// Run it, but not as shown: this is the user's rewrite of the steps,
    /// worded for the model. For a model-written plan the text is appended
    /// to the user's message. For a held step it is guidance only, because
    /// running the held calls would apply the very thing the user just
    /// changed.
    Revise(String),
    /// Do not run it.
    Reject,
}

/// The one open question, if there is one, and a count of those asked.
#[derive(Debug, Default)]
struct Slot {
    open: Option<Open>,
    /// The last [`Open::ticket`] handed out.
    issued: u64,
}

#[derive(Debug)]
struct Open {
    /// Which question this is. A [`Waiting`] closes only its own: by the
    /// time it is dropped a newer question may have taken the slot.
    ticket: u64,
    tx: oneshot::Sender<Answer>,
}

/// The plan gate. Clones are the same gate.
///
/// See the [module documentation](self).
#[derive(Debug, Clone, Default)]
pub struct PlanGate {
    slot: Arc<Mutex<Slot>>,
}

impl PlanGate {
    /// A gate with no question open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a question and return what the turn waits on.
    ///
    /// Call it **before** the card is shown. The card is what tells the user
    /// there is something to answer, so a gate that is open before it exists
    /// cannot be answered too early, and one that opened after it could be
    /// answered before it was open.
    ///
    /// A question still open is ended by this one: its [`Waiting`] gets no
    /// answer. That does not happen in the app, where one turn runs at a
    /// time, and ending the older question is the safe thing if it ever
    /// did: nothing runs on an answer that was meant for another card.
    pub fn ask(&self) -> Waiting {
        let (tx, rx) = oneshot::channel();
        let (ticket, replaced) = {
            let mut slot = self.lock();
            slot.issued += 1;
            let ticket = slot.issued;
            (ticket, slot.open.replace(Open { ticket, tx }))
        };
        drop(replaced);
        Waiting {
            rx,
            gate: self.clone(),
            ticket,
        }
    }

    /// Answer the open question. Returns whether there was one to take it.
    ///
    /// `false` means nothing was done: no card is open, it has already been
    /// answered, or its turn stopped waiting. That is not an error to
    /// report to the user, only a click that arrived too late or twice.
    pub fn answer(&self, answer: Answer) -> bool {
        // Taken under the lock, sent after it: one answer, and the lock is
        // held for the swap only.
        let open = self.lock().open.take();
        match open {
            Some(open) => open.tx.send(answer).is_ok(),
            None => false,
        }
    }

    /// Whether a question is open and its turn is still waiting for the
    /// answer. This is what tells the window a click would count.
    pub fn is_waiting(&self) -> bool {
        self.lock()
            .open
            .as_ref()
            .is_some_and(|open| !open.tx.is_closed())
    }

    /// End the question `ticket`, if it is still the open one.
    fn close(&self, ticket: u64) {
        let closed = {
            let mut slot = self.lock();
            match slot.open {
                Some(ref open) if open.ticket == ticket => slot.open.take(),
                _ => None,
            }
        };
        drop(closed);
    }

    /// The slot holds an `Option` and a counter, and no operation leaves
    /// either half-changed, so a lock a panic poisoned is still good.
    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What a turn waits on after [`PlanGate::ask`].
///
/// Dropping it ends the question, wherever it is dropped from, so a turn
/// that is cancelled, fails or times out leaves the gate closed with no code
/// at the call site.
#[derive(Debug)]
#[must_use = "a question nobody waits on is closed again when this is dropped"]
pub struct Waiting {
    rx: oneshot::Receiver<Answer>,
    gate: PlanGate,
    ticket: u64,
}

impl Waiting {
    /// Wait for the answer, for at most `limit`.
    ///
    /// `None` is no answer: the time ran out, or a newer question took this
    /// one's place. Either way nothing was approved.
    pub async fn wait(mut self, limit: Duration) -> Option<Answer> {
        match tokio::time::timeout(limit, &mut self.rx).await {
            Ok(Ok(answer)) => Some(answer),
            // The sender went away unanswered: this question was replaced.
            Ok(Err(_)) => None,
            Err(_elapsed) => {
                // An answer can land in the instant the time runs out.
                // Closing the receiver first stops any later one, and then
                // what was sent before the close is still there to take, so
                // `answer` returning `true` always means this turn got it.
                self.rx.close();
                self.rx.try_recv().ok()
            }
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.gate.close(self.ticket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Longer than anything a test really waits for: the clock is paused,
    /// so a wait this long ends the moment the runtime has nothing else to
    /// do, and the test can still say the wait lasted this long.
    const LONG: Duration = Duration::from_secs(300);

    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}

    /// Nothing is kept in the slot: not merely a question nobody can
    /// answer any more, but none at all.
    fn slot_is_empty(gate: &PlanGate) -> bool {
        gate.lock().open.is_none()
    }

    /// The gate is shared between the turn, the commands that answer it and
    /// the app's state, and a turn parked on it is a `Send` future.
    #[test]
    fn the_handle_is_shared_across_threads_and_the_wait_can_be_spawned() {
        assert_send_sync::<PlanGate>();
        assert_send::<Waiting>();
    }

    #[tokio::test(start_paused = true)]
    async fn one_answer_is_delivered() {
        let gate = PlanGate::new();
        let waiting = gate.ask();
        assert!(gate.answer(Answer::Approve), "an open question takes it");
        assert_eq!(waiting.wait(LONG).await, Some(Answer::Approve));
    }

    #[tokio::test(start_paused = true)]
    async fn every_kind_of_answer_arrives_as_given() {
        for given in [
            Answer::Approve,
            Answer::Reject,
            Answer::Revise("1. reverse — track: 1".to_string()),
        ] {
            let gate = PlanGate::new();
            let waiting = gate.ask();
            assert!(gate.answer(given.clone()));
            assert_eq!(waiting.wait(LONG).await, Some(given));
        }
    }

    /// The answer reaches a turn that is already parked on the gate, which
    /// is the order of things in the app: the turn waits, the user clicks.
    #[tokio::test(start_paused = true)]
    async fn an_answer_reaches_a_turn_that_is_already_waiting() {
        let gate = PlanGate::new();
        let waiting = gate.ask();
        let turn = tokio::spawn(async move { waiting.wait(LONG).await });
        tokio::task::yield_now().await;

        assert!(gate.answer(Answer::Reject));
        assert_eq!(turn.await.expect("the turn"), Some(Answer::Reject));
    }

    /// A double-click on Run. The question has its answer, so the second
    /// finds nothing to answer and changes nothing.
    #[tokio::test(start_paused = true)]
    async fn a_second_answer_is_ignored() {
        let gate = PlanGate::new();
        let waiting = gate.ask();
        assert!(gate.answer(Answer::Approve));
        assert!(!gate.answer(Answer::Reject), "the question was answered");
        assert!(!gate.answer(Answer::Revise("late".to_string())));
        assert_eq!(waiting.wait(LONG).await, Some(Answer::Approve));
    }

    /// #251, by construction. A Run click with no card open used to bank a
    /// permit that the next turn's gate took as its own answer, with
    /// whatever revision came with the click. With nothing open there is no
    /// slot to put it in, so the next question starts unanswered.
    #[tokio::test(start_paused = true)]
    async fn an_answer_with_no_question_open_is_ignored_and_not_kept() {
        let gate = PlanGate::new();
        assert!(!gate.answer(Answer::Approve));
        assert!(!gate.answer(Answer::Revise("left over".to_string())));
        assert!(!gate.is_waiting());

        let waiting = gate.ask();
        assert!(gate.is_waiting());
        assert_eq!(
            waiting.wait(LONG).await,
            None,
            "an earlier click answered a card it never saw"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_timeout_closes_the_gate_after_the_whole_limit() {
        let gate = PlanGate::new();
        let started = tokio::time::Instant::now();

        assert_eq!(gate.ask().wait(LONG).await, None);

        let waited = started.elapsed();
        assert!(waited >= LONG, "gave up after {waited:?}");
        assert!(waited < LONG + Duration::from_secs(1), "waited {waited:?}");
        assert!(!gate.is_waiting());
        assert!(slot_is_empty(&gate));
    }

    #[tokio::test(start_paused = true)]
    async fn a_late_answer_after_a_timeout_is_ignored() {
        let gate = PlanGate::new();
        assert_eq!(gate.ask().wait(LONG).await, None);

        assert!(!gate.answer(Answer::Approve), "nobody is waiting for it");
        assert!(!gate.answer(Answer::Revise("too late".to_string())));

        // And it was not kept for the next question either.
        assert_eq!(gate.ask().wait(LONG).await, None);
    }

    /// The turn is dropped while it waits: the window closed, the task was
    /// aborted, an error unwound it. Nothing is left for a click to answer.
    #[tokio::test(start_paused = true)]
    async fn a_cancelled_turn_clears_the_gate() {
        let gate = PlanGate::new();
        let waiting = gate.ask();
        let turn = tokio::spawn(async move { waiting.wait(LONG).await });
        tokio::task::yield_now().await;
        assert!(gate.is_waiting());

        turn.abort();
        assert!(turn.await.expect_err("aborted").is_cancelled());

        assert!(!gate.is_waiting());
        assert!(slot_is_empty(&gate));
        assert!(!gate.answer(Answer::Approve));
    }

    #[test]
    fn dropping_the_receiver_clears_the_gate() {
        let gate = PlanGate::new();
        let waiting = gate.ask();
        assert!(gate.is_waiting());

        drop(waiting);

        assert!(!gate.is_waiting());
        assert!(slot_is_empty(&gate));
        assert!(!gate.answer(Answer::Reject));
    }

    #[tokio::test(start_paused = true)]
    async fn is_waiting_is_right_in_every_state() {
        let gate = PlanGate::new();
        assert!(!gate.is_waiting(), "nothing asked yet");

        let waiting = gate.ask();
        assert!(gate.is_waiting(), "asked");

        assert!(gate.answer(Answer::Approve));
        assert!(
            !gate.is_waiting(),
            "answered, though the turn has not read it yet"
        );
        assert_eq!(waiting.wait(LONG).await, Some(Answer::Approve));
        assert!(!gate.is_waiting(), "answered and read");

        let waiting = gate.ask();
        assert!(gate.is_waiting(), "asked again");
        assert_eq!(waiting.wait(LONG).await, None);
        assert!(!gate.is_waiting(), "timed out");

        drop(gate.ask());
        assert!(!gate.is_waiting(), "cancelled");
    }

    /// A question that is still open when another is asked is ended by it.
    /// Ending the older wait afterwards must not close the newer question.
    #[tokio::test(start_paused = true)]
    async fn a_new_question_replaces_an_unanswered_one() {
        let gate = PlanGate::new();
        let first = gate.ask();
        let second = gate.ask();

        assert_eq!(first.wait(LONG).await, None, "replaced, not answered");
        assert!(gate.is_waiting(), "the newer question was closed with it");

        assert!(gate.answer(Answer::Approve));
        assert_eq!(second.wait(LONG).await, Some(Answer::Approve));
    }

    /// Answering is a plain call: no runtime, no `.await`, no lock of the
    /// agent's. A command thread can do it while a turn holds everything
    /// else (CLAUDE.md, "Lock order").
    #[tokio::test(start_paused = true)]
    async fn any_clone_answers_from_any_thread_without_a_runtime() {
        let gate = PlanGate::new();
        let waiting = gate.ask();

        let from_a_command = gate.clone();
        let delivered = std::thread::spawn(move || from_a_command.answer(Answer::Approve))
            .join()
            .expect("the answering thread");

        assert!(delivered);
        assert_eq!(waiting.wait(LONG).await, Some(Answer::Approve));
    }

    /// The gate carries a revision as it was given. What the text says is
    /// the desktop's (`revision_text`, which a test there pins); this is
    /// the same wording, multi-line, with an apostrophe and an em dash,
    /// and it must come out the other side byte for byte.
    #[tokio::test(start_paused = true)]
    async fn revise_carries_its_text_untouched() {
        let texts = [
            "I've updated the plan. Please follow these revised steps instead:\n\
             1. reverse — track: 1\n2. normalize — track: 2",
            "  padded\n\n",
            "",
        ];
        for text in texts {
            let gate = PlanGate::new();
            let waiting = gate.ask();
            gate.answer(Answer::Revise(text.to_string()));
            assert_eq!(
                waiting.wait(LONG).await,
                Some(Answer::Revise(text.to_string()))
            );
        }
    }
}
