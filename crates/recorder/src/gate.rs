//! Who may use the microphone and speakers right now (#203).
//!
//! A latency measurement plays sweeps and listens for them, so anything
//! else recording at the time hears them (and a take made during one has
//! test tones in it), and a recording that starts mid-measurement makes the
//! measurement hear the person. A timer that is armed will start recording
//! on its own, by the clock, perhaps in the middle of a measurement. A
//! punch-in plays the mix to the performer and records them, so it is the
//! same kind of user: nothing else may record or play test tones while it
//! runs, and it may not start over anything that does. So the four exclude
//! one another, and the exclusion is one small piece of state here, not
//! flags in four commands:
//!
//! - a calibration refuses to start while one is running, while a punch is
//!   running, while a timer is armed (waiting or recording), and (the
//!   caller checks its own `Recorder` slot after taking the run, see
//!   below) while a take is running;
//! - a punch-in refuses to start under exactly the same conditions, with
//!   its own wording;
//! - `start_recording` and `timer_record` refuse while a calibration or a
//!   punch-in runs.
//!
//! Each side sets its own flag and then reads the other's, all `SeqCst`, so
//! of two that start at the same instant at least one sees the other and
//! refuses.
//!
//! Each run's cancel is its own flag too. The calibration's used to share
//! `tools::progress`'s, which is the agent's `batch_apply` and the timer's:
//! a Cancel pressed for one stopped the other, and a calibration beginning
//! cleared a cancel that was meant for a batch. The punch-in's is a third:
//! it is a take with a performer in the room, so a Cancel meant for the
//! agent's batch must not throw it away, and the Cancel pressed for it must
//! not stop a batch (the agent can run one while the person punches).

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
    #[error("A punch-in is already recording.")]
    AlreadyPunching,
    #[error(
        "A punch-in is recording. Wait for it to finish, or cancel it, before recording or \
         measuring the latency."
    )]
    Punching,
    #[error("A timed recording is waiting to start or running. Cancel it before punching in.")]
    TimerArmedForPunch,
}

pub struct Gate {
    calibrating: AtomicBool,
    cancel: AtomicBool,
    punching: AtomicBool,
    punch_cancel: AtomicBool,
    timers: AtomicUsize,
}

/// The process's gate.
pub static GATE: Gate = Gate::new();

impl Gate {
    pub const fn new() -> Self {
        Self {
            calibrating: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            punching: AtomicBool::new(false),
            punch_cancel: AtomicBool::new(false),
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
        if self.punching.load(Ordering::SeqCst) {
            self.calibrating.store(false, Ordering::SeqCst);
            return Err(Busy::Punching);
        }
        if self.timers.load(Ordering::SeqCst) > 0 {
            self.calibrating.store(false, Ordering::SeqCst);
            return Err(Busy::TimerArmed);
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(CalibrationRun { gate: self })
    }

    /// Take the right to punch in: the same conditions as a calibration,
    /// since both play to the speakers and listen to the microphone. A
    /// cancel left over from an earlier punch is cleared here. The caller
    /// then checks that no take is running and drops the run if one is.
    pub fn begin_punch(&self) -> Result<PunchRun<'_>, Busy> {
        if self
            .punching
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(Busy::AlreadyPunching);
        }
        if self.calibrating.load(Ordering::SeqCst) {
            self.punching.store(false, Ordering::SeqCst);
            return Err(Busy::Calibrating);
        }
        if self.timers.load(Ordering::SeqCst) > 0 {
            self.punching.store(false, Ordering::SeqCst);
            return Err(Busy::TimerArmedForPunch);
        }
        self.punch_cancel.store(false, Ordering::SeqCst);
        Ok(PunchRun { gate: self })
    }

    /// Ask the running punch-in to stop at its next checkpoint, throwing
    /// the take away. Does nothing when none is running, so a stray cancel
    /// is not remembered for the next one.
    pub fn cancel_punch(&self) {
        if self.punching.load(Ordering::SeqCst) {
            self.punch_cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the running punch-in was asked to stop; its `cancel`
    /// callback.
    pub fn punch_cancelled(&self) -> bool {
        self.punch_cancel.load(Ordering::SeqCst)
    }

    pub fn punching(&self) -> bool {
        self.punching.load(Ordering::SeqCst)
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
        } else if self.punching.load(Ordering::SeqCst) {
            Err(Busy::Punching)
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
        if self.punching.load(Ordering::SeqCst) {
            self.timers.fetch_sub(1, Ordering::SeqCst);
            return Err(Busy::Punching);
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

/// A punch-in in progress; dropping it frees the gate.
pub struct PunchRun<'a> {
    gate: &'a Gate,
}

impl Drop for PunchRun<'_> {
    fn drop(&mut self) {
        self.gate.punching.store(false, Ordering::SeqCst);
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

    #[test]
    fn a_punch_excludes_recording_timers_and_measuring_and_the_other_way_round() {
        let g = Gate::new();
        let punch = g.begin_punch().unwrap();
        assert_eq!(g.begin_punch().err(), Some(Busy::AlreadyPunching));
        assert_eq!(g.check_can_record(), Err(Busy::Punching));
        assert_eq!(g.arm_timer().err(), Some(Busy::Punching));
        assert_eq!(g.begin_calibration().err(), Some(Busy::Punching));
        let e = g.check_can_record().unwrap_err().to_string();
        assert!(e.contains("punch-in"), "{e}");
        drop(punch);
        assert_eq!(g.check_can_record(), Ok(()));
        assert!(g.begin_calibration().is_ok(), "free once the punch is done");

        // And a punch is refused over each of the others.
        let cal = g.begin_calibration().unwrap();
        assert_eq!(g.begin_punch().err(), Some(Busy::Calibrating));
        drop(cal);
        let timer = g.arm_timer().unwrap();
        let e = g.begin_punch().err().unwrap();
        assert_eq!(e, Busy::TimerArmedForPunch);
        assert!(e.to_string().contains("punching in"), "{e}");
        drop(timer);
        assert!(g.begin_punch().is_ok());
    }

    #[test]
    fn a_refused_attempt_leaves_no_trace_in_the_other_flags() {
        let g = Gate::new();
        let punch = g.begin_punch().unwrap();
        let _ = g.begin_calibration();
        let _ = g.arm_timer();
        drop(punch);
        // Neither refused attempt may keep the gate closed.
        assert!(g.begin_punch().is_ok());
        let cal_gate = Gate::new();
        let cal = cal_gate.begin_calibration().unwrap();
        let _ = cal_gate.begin_punch();
        drop(cal);
        assert!(cal_gate.begin_punch().is_ok(), "the refused punch held it");
    }

    #[test]
    fn the_punch_has_its_own_cancel() {
        let g = Gate::new();
        g.cancel_punch();
        assert!(!g.punch_cancelled(), "nothing was running to cancel");
        let run = g.begin_punch().unwrap();
        // The calibration's cancel is not the punch's, either way.
        g.cancel_calibration();
        assert!(!g.punch_cancelled());
        g.cancel_punch();
        assert!(g.punch_cancelled());
        assert!(!g.calibration_cancelled());
        drop(run);
        let _next = g.begin_punch().unwrap();
        assert!(
            !g.punch_cancelled(),
            "a cancel for the last punch must not stop the next"
        );
    }
}
