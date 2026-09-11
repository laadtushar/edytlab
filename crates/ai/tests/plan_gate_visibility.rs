//! When the plan gate is skipped, the reason survives (#267).
//!
//! `fetch_plan` turned every transport error, every non-2xx and every
//! unparseable body into a bare `None`, and the caller falls through to
//! the tool loop on `None`. So a user who turned Plan First on lost the
//! checkpoint they asked for, and from outside it was indistinguishable
//! from the model deciding no plan was needed. No log line either — the
//! failure class was erased before anyone could see it.
//!
//! The fall-through itself is deliberate and stays: a planning hiccup
//! should not block work the user asked for. What changed is that it is
//! no longer silent.
//!
//! `PlanUnavailable` itself is `pub(crate)`, so the check that each
//! class renders something a user can act on lives beside it in
//! `agent_loop.rs`. What belongs here is the part of the contract the
//! frontend depends on: the event exists, carries the reason, and is
//! not the same thing as a rejection.

use ai::AgentEvent;

/// The event exists and carries the reason, which is what lets the
/// frontend say the gate was skipped rather than staying quiet.
#[test]
fn the_event_carries_the_reason() {
    let event = AgentEvent::PlanUnavailable {
        reason: "the planning request returned HTTP 503".to_string(),
    };
    match event {
        AgentEvent::PlanUnavailable { reason } => {
            assert!(reason.contains("503"), "the status was dropped: {reason}");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

/// `PlanUnavailable` must not be mistaken for `PlanRejected`.
///
/// They mean opposite things: rejected is the user declining, and the
/// turn ends having run nothing. Unavailable is the turn going ahead
/// *without* the user having been asked.
#[test]
fn it_is_not_the_same_event_as_a_rejection() {
    let unavailable = AgentEvent::PlanUnavailable { reason: "x".into() };
    assert!(
        !matches!(unavailable, AgentEvent::PlanRejected),
        "a skipped gate must not read as a user rejection — one ran the \
         tools and the other did not"
    );
}
