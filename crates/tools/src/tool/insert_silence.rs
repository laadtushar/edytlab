//! Splice silence into a buffer at a given offset.

use serde::Deserialize;

pub fn apply_insert_silence(
    samples: &mut Vec<f32>,
    sample_rate: u32,
    at_sec: f64,
    duration_sec: f64,
) -> Result<(), InsertSilenceError> {
    if duration_sec < 0.0 {
        return Err(InsertSilenceError::NegativeDuration(duration_sec));
    }
    if at_sec < 0.0 {
        return Err(InsertSilenceError::NegativeOffset(at_sec));
    }
    let offset = ((at_sec * sample_rate as f64) as usize).min(samples.len());
    let count = (duration_sec * sample_rate as f64) as usize;
    samples.splice(offset..offset, std::iter::repeat_n(0.0, count));
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct InsertSilenceParams {
    pub at: f64,
    pub duration: f64,
}

pub fn dispatch_insert_silence(
    params: InsertSilenceParams,
    samples: &mut Vec<f32>,
    sample_rate: u32,
) -> Result<(), InsertSilenceError> {
    apply_insert_silence(samples, sample_rate, params.at, params.duration)
}

#[derive(Debug, thiserror::Error)]
pub enum InsertSilenceError {
    #[error("duration must be >= 0; got {0}")]
    NegativeDuration(f64),
    #[error("at must be >= 0; got {0}")]
    NegativeOffset(f64),
}
