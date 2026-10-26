//! Who may use the microphone and speakers right now (#203).
//!
//! A latency measurement plays sweeps and listens for them, so anything
//! else recording at the time hears them (and a take made during one has
//! test tones in it), and a recording that starts mid-measurement makes the
//! measurement hear the person. A timer that is armed will start recording
//! on its own, by the clock, perhaps in the middle of a measurement. So the
//! three exclude one another, and the exclusion is one small piece of state
//! here, not three flags in three commands:
//!
//! - a calibration refuses to start while one is running, while a timer is
//!   armed (waiting or recording), and (the caller checks its own
//!   `Recorder` slot after taking the run, see below) while a take is
//!   running;
//! - `start_recording` and `timer_record` refuse while a calibration runs.
//!
//! Each side sets its own flag and then reads the other's, all `SeqCst`, so
//! of two that start at the same instant at least one sees the other and
//! refuses.
//!
//! The calibration's cancel is its own flag too. It used to share
//! `tools::progress`'s, which is the agent's `batch_apply` and the timer's:
//! a Cancel pressed for one stopped the other, and a calibration beginning
//! cleared a cancel that was meant for a batch.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Busy {
    #[error("A latency measurement is already running.")]
    AlreadyCalibrating,
    #[error(
        "A latency measurement is running. Wait for it to finish, or cancel it in Settings, \
         before recording."
    )]
    Calibrating,
    #[error(
        "A timed recording is waiting to start or running. Cancel it before measuring the latency."
    )]
    TimerArmed,
}

pub struct Gate {
    calibrating: AtomicBool,
    cancel: AtomicBool,
    timers: AtomicUsize,
}

/// The process's gate.
pub static GATE: Gate = Gate::new();

impl Gate {
    pub const fn new() -> Self {
        Self {
            calibrating: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            timers: AtomicUsize::new(0),
        }
    }

    /// Take the right to measure. A cancel left over from an earlier run is
    /// cleared here, by the run it was not meant for no longer being
    /// there to receive it. The caller then checks that no take is running
    /// and drops the run if one is.
    pub fn begin_calibration(&self) -> Result<CalibrationRun<'_>, Busy> {
        if self
            .calibrating
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Busy::AlreadyCalibrating);
        }
        if self.timers.load(Ordering::SeqCst) > 0 {
            self.calibrating.store(false, Ordering::SeqCst);
            return Err(Busy::TimerArmed);
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(CalibrationRun { gate: self })
    }

    /// Ask the running calibration to stop at its next checkpoint. Does
    /// nothing when none is running, so a stray cancel is not remembered
    /// for the next one (and `begin_calibration` clears it as well).
    pub fn cancel_calibration(&self) {
        if self.calibrating.load(Ordering::SeqCst) {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the running calibration was asked to stop; its `cancel`
    /// callback.
    pub fn calibration_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn calibrating(&self) -> bool {
        self.calibrating.load(Ordering::SeqCst)
    }

    /// Whether a recording may start now. For `start_recording`, which
    /// calls it with its recorder slot locked, so a calibration that begins
    /// meanwhile finds the take.
    pub fn check_can_record(&self) -> Result<(), Busy> {
        if self.calibrating.load(Ordering::SeqCst) {
            Err(Busy::Calibrating)
        } else {
            Ok(())
        }
    }

    /// Arm a timed recording. Held until it ends, however it ends.
    pub fn arm_timer(&self) -> Result<TimerArm<'_>, Busy> {
        self.timers.fetch_add(1, Ordering::SeqCst);
        if self.calibrating.load(Ordering::SeqCst) {
            self.timers.fetch_sub(1, Ordering::SeqCst);
            return Err(Busy::Calibrating);
        }
        Ok(TimerArm { gate: self })
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

/// A calibration in progress; dropping it frees the gate.
pub struct CalibrationRun<'a> {
    gate: &'a Gate,
}

impl Drop for CalibrationRun<'_> {
    fn drop(&mut self) {
        self.gate.calibrating.store(false, Ordering::SeqCst);
    }
}

/// A timer in progress; dropping it frees the gate.
pub struct TimerArm<'a> {
    gate: &'a Gate,
}

impl Drop for TimerArm<'_> {
    fn drop(&mut self) {
        self.gate.timers.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_calibration_runs_at_a_time() {
        let g = Gate::new();
        let first = g.begin_calibration().expect("free at first");
        assert_eq!(g.begin_calibration().err(), Some(Busy::AlreadyCalibrating));
        drop(first);
        assert!(g.begin_calibration().is_ok(), "free again once it ends");
    }

    #[test]
    fn recording_is_refused_while_calibrating_and_allowed_after() {
        let g = Gate::new();
        assert_eq!(g.check_can_record(), Ok(()));
        let run = g.begin_calibration().unwrap();
        let e = g.check_can_record().unwrap_err();
        assert_eq!(e, Busy::Calibrating);
        assert!(e.to_string().contains("latency measurement"), "{e}");
        drop(run);
        assert_eq!(g.check_can_record(), Ok(()));
    }

    #[test]
    fn a_timer_is_refused_while_calibrating_and_leaves_no_trace() {
        let g = Gate::new();
        let run = g.begin_calibration().unwrap();
        assert_eq!(g.arm_timer().err(), Some(Busy::Calibrating));
        drop(run);
        // The refused timer must not count as armed.
        assert!(g.begin_calibration().is_ok());
    }

    #[test]
    fn calibration_is_refused_while_a_timer_is_armed() {
        let g = Gate::new();
        let timer = g.arm_timer().unwrap();
        let e = g.begin_calibration().err().unwrap();
        assert_eq!(e, Busy::TimerArmed);
        assert!(e.to_string().contains("timed recording"), "{e}");
        drop(timer);
        assert!(g.begin_calibration().is_ok(), "free once the timer is done");
    }

    #[test]
    fn a_refused_calibration_does_not_hold_the_gate() {
        let g = Gate::new();
        let timer = g.arm_timer().unwrap();
        let _ = g.begin_calibration();
        assert_eq!(
            g.check_can_record(),
            Ok(()),
            "recording must not stay blocked"
        );
        drop(timer);
    }

    #[test]
    fn two_timers_are_both_counted() {
        let g = Gate::new();
        let (a, b) = (g.arm_timer().unwrap(), g.arm_timer().unwrap());
        drop(a);
        assert_eq!(g.begin_calibration().err(), Some(Busy::TimerArmed));
        drop(b);
        assert!(g.begin_calibration().is_ok());
    }

    #[test]
    fn cancel_reaches_the_running_calibration_only() {
        let g = Gate::new();
        g.cancel_calibration();
        assert!(!g.calibration_cancelled(), "nothing was running to cancel");
        let run = g.begin_calibration().unwrap();
        assert!(!g.calibration_cancelled());
        g.cancel_calibration();
        assert!(g.calibration_cancelled());
        drop(run);
        let _next = g.begin_calibration().unwrap();
        assert!(
            !g.calibration_cancelled(),
            "a cancel for the last run must not stop the next"
        );
    }
}
