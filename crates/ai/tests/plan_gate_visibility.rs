//! When no plan arrives, the reason survives (#267) and so does the
//! checkpoint (#415).
//!
//! `fetch_plan` turned every transport error, every non-2xx and every
//! unparseable body into a bare `None`, and the caller fell through to
//! the tool loop on `None`. So a user who turned Plan First on lost the
//! checkpoint they asked for, and from outside it was indistinguishable
//! from the model deciding no plan was needed. No log line either — the
//! failure class was erased before anyone could see it.
//!
//! The turn still goes ahead without a plan: a planning hiccup should not
//! block work the user asked for. What changed is that it is no longer
//! silent (#267), and that with Plan first on the fall-through no longer
//! means "edits run unapproved": the first edit is held for approval
//! instead (#415, `tests/plan_first_gate.rs`). The fall-through without
//! any gate stays only for a request classified as a mashup with Plan
//! first **off**.
//!
//! `PlanUnavailable` itself is `pub(crate)`, so the check that each
//! class renders something a user can act on lives beside it in
//! `agent_loop.rs`. What belongs here is the part of the contract the
//! frontend depends on: the event exists, carries the reason and whether
//! the first edit is held, and is not the same thing as a rejection.

use ai::AgentEvent;

/// The event exists and carries the reason, which is what lets the
/// frontend say no plan arrived rather than staying quiet.
#[test]
fn the_event_carries_the_reason() {
    let event = AgentEvent::PlanUnavailable {
        reason: "the planning request returned HTTP 503".to_string(),
        first_edit_held: true,
    };
    match event {
        AgentEvent::PlanUnavailable { reason, .. } => {
            assert!(reason.contains("503"), "the status was dropped: {reason}");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

/// The event says whether the first edit is held, which is what lets the
/// frontend promise "nothing will change without your approval" only when
/// that is true (#415).
#[test]
fn the_event_says_whether_the_first_edit_is_held() {
    for held in [true, false] {
        let event = AgentEvent::PlanUnavailable {
            reason: "x".to_string(),
            first_edit_held: held,
        };
        match event {
            AgentEvent::PlanUnavailable {
                first_edit_held, ..
            } => assert_eq!(first_edit_held, held),
            other => panic!("wrong variant: {other:?}"),
        }
    }
}

/// `PlanUnavailable` must not be mistaken for `PlanRejected`.
///
/// They mean opposite things: rejected is the user declining, and the
/// turn ends having run nothing. Unavailable is the turn going ahead
/// *without* the user having been asked about a plan.
#[test]
fn it_is_not_the_same_event_as_a_rejection() {
    let unavailable = AgentEvent::PlanUnavailable {
        reason: "x".into(),
        first_edit_held: false,
    };
    assert!(
        !matches!(unavailable, AgentEvent::PlanRejected),
        "a missing plan must not read as a user rejection — one ran the \
         tools and the other did not"
    );
}
