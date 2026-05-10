//! Linear fade-in / fade-out within a region.

use crate::util::range_resolver::{resolve as resolve_range, RangeError};
use crate::Range;
use serde::Deserialize;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    In,
    Out,
}

pub fn apply_fade(samples: &mut [f32], sample_rate: u32, range: Range, kind: Kind) {
    let start = (range.start_sec * sample_rate as f64) as usize;
    let end = ((range.end_sec * sample_rate as f64) as usize).min(samples.len());
    if end <= start {
        return;
    }
    let len = (end - start) as f32;
    for (i, sample) in samples[start..end].iter_mut().enumerate() {
        let t = i as f32 / len;
        let gain = match kind {
            Kind::In => t,
            Kind::Out => 1.0 - t,
        };
        *sample *= gain;
    }
}

#[derive(Debug, Deserialize)]
pub struct FadeParams {
    pub range: Option<Range>,
    #[serde(default = "default_kind")]
    pub kind: KindParam,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindParam {
    In,
    Out,
}

fn default_kind() -> KindParam {
    KindParam::Out
}

impl From<KindParam> for Kind {
    fn from(p: KindParam) -> Self {
        match p {
            KindParam::In => Kind::In,
            KindParam::Out => Kind::Out,
        }
    }
}

pub fn dispatch_fade(
    params: FadeParams,
    user_message: &str,
    samples: &mut [f32],
    sample_rate: u32,
) -> Result<(), FadeError> {
    let range = resolve_range(params.range, user_message, true)
        .map_err(FadeError::Range)?
        .expect("required => Some on Ok");
    apply_fade(samples, sample_rate, range, params.kind.into());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum FadeError {
    #[error("{0}")]
    Range(#[from] RangeError),
}
