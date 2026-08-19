//! Microphone capture → WAV file writer.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("no input device available")]
    NoDevice,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("wav write error: {0}")]
    Wav(#[from] hound::Error),
}

pub struct Recorder {
    samples: Arc<Mutex<Vec<f32>>>,
    stream: Option<cpal::Stream>,
    sample_rate: u32,
    channels: u16,
}

// cpal::Stream is not Send by default on some platforms; we hold it
// inside a Mutex<Option<Recorder>> so only one thread touches it.
// SAFETY: we never share the stream across threads without a Mutex guard.
unsafe impl Send for Recorder {}

impl Recorder {
    pub fn start() -> Result<Self, RecorderError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or(RecorderError::NoDevice)?;
        let config = device
            .default_input_config()
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let samples_clone = Arc::clone(&samples);
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Ok(mut s) = samples_clone.lock() {
                        s.extend_from_slice(data);
                    }
                },
                |e| tracing::error!("stream error: {e}"),
                None,
            )
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        stream
            .play()
            .map_err(|e| RecorderError::Stream(e.to_string()))?;
        Ok(Self {
            samples,
            stream: Some(stream),
            sample_rate,
            channels,
        })
    }

    pub fn stop_and_save(mut self, path: &PathBuf) -> Result<(PathBuf, u32, u16), RecorderError> {
        drop(self.stream.take());
        let samples = self.samples.lock().unwrap().clone();
        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for s in &samples {
            writer.write_sample(*s)?;
        }
        writer.finalize()?;
        Ok((path.clone(), self.sample_rate, self.channels))
    }

    pub fn duration_sec(&self) -> f64 {
        let n = self.samples.lock().unwrap().len();
        n as f64 / (self.sample_rate as f64 * self.channels as f64)
    }
}

pub fn format_seconds(secs: f64) -> String {
    let m = (secs / 60.0) as u64;
    let s = secs as u64 % 60;
    format!("{m}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::format_seconds;

    #[test]
    fn formats_duration() {
        assert_eq!(format_seconds(65.3), "1:05");
        assert_eq!(format_seconds(3.0), "0:03");
    }
}

// =============================================================================
// Timer record (#203 §2)
// =============================================================================

/// When an unattended recording should begin and how long it should run.
///
/// Both halves are optional and independent: "start in ten minutes" and
/// "record for thirty seconds" are different requests, and either alone
/// is useful. Neither means "record now until I say stop", which is the
/// existing behaviour and stays the default.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Schedule {
    /// Seconds from when the schedule was armed until capture begins.
    pub start_after_sec: Option<f64>,
    /// Seconds of capture once begun.
    pub duration_sec: Option<f64>,
}

/// What the caller should do at this instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing yet — still counting down to the start.
    Wait,
    /// Begin capturing.
    Start,
    /// Keep capturing.
    Continue,
    /// Stop and save.
    Stop,
}

/// Decide what to do, given how long the schedule has been armed and
/// whether capture is already running.
///
/// A pure function on purpose. The device call it drives cannot be
/// exercised without a sound card, but *when to start and when to stop*
/// is the part with the off-by-one in it — an unattended recording that
/// stops a second early has clipped the last word of a take nobody was
/// in the room for. So the decision is separated from the device and
/// tested directly.
///
/// `elapsed_sec` is measured from the moment the schedule was armed,
/// not from when capture began; `Stop` accounts for the start delay so
/// `duration_sec` is time *recorded*, which is what a person means by
/// "record for thirty seconds".
pub fn next_action(schedule: &Schedule, elapsed_sec: f64, recording: bool) -> Action {
    let start_at = schedule.start_after_sec.unwrap_or(0.0).max(0.0);

    if !recording {
        return if elapsed_sec >= start_at {
            Action::Start
        } else {
            Action::Wait
        };
    }

    match schedule.duration_sec {
        // No duration: runs until stopped by hand, which is the
        // existing behaviour.
        None => Action::Continue,
        Some(d) if elapsed_sec >= start_at + d.max(0.0) => Action::Stop,
        Some(_) => Action::Continue,
    }
}

impl Schedule {
    /// Whether this schedule asks for anything at all. An empty one is
    /// ordinary press-to-record, and the caller should not arm a timer
    /// for it.
    pub fn is_armed(&self) -> bool {
        self.start_after_sec.is_some() || self.duration_sec.is_some()
    }

    /// Seconds until the next transition, for a countdown a user can
    /// read. `None` when nothing is pending.
    pub fn remaining(&self, elapsed_sec: f64, recording: bool) -> Option<f64> {
        let start_at = self.start_after_sec.unwrap_or(0.0).max(0.0);
        if !recording {
            let left = start_at - elapsed_sec;
            return (left > 0.0).then_some(left);
        }
        let d = self.duration_sec?;
        let left = (start_at + d.max(0.0)) - elapsed_sec;
        (left > 0.0).then_some(left)
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    fn armed(start: Option<f64>, dur: Option<f64>) -> Schedule {
        Schedule {
            start_after_sec: start,
            duration_sec: dur,
        }
    }

    /// No schedule is press-to-record: begin at once, run until stopped.
    #[test]
    fn an_empty_schedule_starts_now_and_runs_until_stopped() {
        let s = Schedule::default();
        assert!(!s.is_armed());
        assert_eq!(next_action(&s, 0.0, false), Action::Start);
        assert_eq!(next_action(&s, 3_600.0, true), Action::Continue);
    }

    /// A delayed start waits, then begins — the "walk into the booth
    /// first" case.
    #[test]
    fn a_delayed_start_waits_and_then_begins() {
        let s = armed(Some(10.0), None);
        assert_eq!(next_action(&s, 0.0, false), Action::Wait);
        assert_eq!(next_action(&s, 9.9, false), Action::Wait);
        assert_eq!(next_action(&s, 10.0, false), Action::Start);
    }

    /// **The off-by-one that matters.** `duration_sec` is time
    /// *recorded*, not time since the timer was armed — so a delayed
    /// start does not eat into the take. An unattended recording that
    /// stops early has clipped the last word of something nobody was in
    /// the room for.
    #[test]
    fn the_duration_is_time_recorded_not_time_since_armed() {
        let s = armed(Some(10.0), Some(30.0));

        // Nine seconds into a thirty-second take, twenty-one to go.
        assert_eq!(next_action(&s, 19.0, true), Action::Continue);
        // Twenty-nine seconds in — still recording.
        assert_eq!(next_action(&s, 39.0, true), Action::Continue);
        // Thirty seconds of audio captured.
        assert_eq!(next_action(&s, 40.0, true), Action::Stop);
    }

    /// A duration with no delay stops after exactly that long.
    #[test]
    fn a_duration_alone_stops_on_time() {
        let s = armed(None, Some(5.0));
        assert_eq!(next_action(&s, 0.0, false), Action::Start);
        assert_eq!(next_action(&s, 4.9, true), Action::Continue);
        assert_eq!(next_action(&s, 5.0, true), Action::Stop);
    }

    /// A delay with no duration begins late and then behaves like
    /// press-to-record.
    #[test]
    fn a_delay_without_a_duration_never_stops_itself() {
        let s = armed(Some(2.0), None);
        assert_eq!(next_action(&s, 2.0, false), Action::Start);
        assert_eq!(next_action(&s, 10_000.0, true), Action::Continue);
    }

    /// Negative values are a caller slip, not an instruction to stop
    /// before starting.
    #[test]
    fn negative_times_are_treated_as_zero() {
        let s = armed(Some(-5.0), Some(-1.0));
        assert_eq!(next_action(&s, 0.0, false), Action::Start);
        assert_eq!(next_action(&s, 0.0, true), Action::Stop);
    }

    /// The countdown a user reads: time to the start before capture,
    /// time to the stop during it, and nothing when nothing is pending.
    #[test]
    fn the_countdown_tracks_whichever_transition_is_next() {
        let s = armed(Some(10.0), Some(30.0));

        assert_eq!(s.remaining(4.0, false), Some(6.0));
        assert_eq!(s.remaining(10.0, false), None, "no countdown once due");
        assert_eq!(s.remaining(25.0, true), Some(15.0));
        assert_eq!(s.remaining(40.0, true), None, "nor once the take is done");

        // Press-to-record has nothing to count down.
        assert_eq!(Schedule::default().remaining(0.0, false), None);
        assert_eq!(Schedule::default().remaining(99.0, true), None);
    }
}
