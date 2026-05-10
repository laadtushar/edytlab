//! Reverse the sample order in a sub-range, or the entire buffer
//! when no range is provided.

use crate::util::range_resolver::{resolve as resolve_range, RangeError};
use crate::Range;
use serde::Deserialize;

pub fn apply_reverse(samples: &mut [f32], sample_rate: u32, range: Option<Range>) {
    match range {
        Some(r) => {
            let start = (r.start_sec * sample_rate as f64) as usize;
            let end = ((r.end_sec * sample_rate as f64) as usize).min(samples.len());
            if end > start {
                samples[start..end].reverse();
            }
        }
        None => samples.reverse(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReverseParams {
    pub range: Option<Range>,
}

pub fn dispatch_reverse(
    params: ReverseParams,
    user_message: &str,
    samples: &mut [f32],
    sample_rate: u32,
) -> Result<(), ReverseError> {
    let range = resolve_range(params.range, user_message, false).map_err(ReverseError::Range)?;
    apply_reverse(samples, sample_rate, range);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ReverseError {
    #[error("{0}")]
    Range(#[from] RangeError),
}
