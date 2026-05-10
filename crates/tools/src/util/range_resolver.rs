//! Resolve a `Range` for a tool from either:
//!   1. A typed `range` parameter the LLM filled in (preferred), or
//!   2. The `[apply to MM:SS-MM:SS]` text prefix in the user message.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub start_sec: f64,
    pub end_sec: f64,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RangeError {
    #[error("range is required for this tool but neither a typed param nor a parseable text prefix was provided")]
    MissingRange,
    #[error("range is invalid: start ({start_sec}) must be < end ({end_sec})")]
    InvalidOrder { start_sec: f64, end_sec: f64 },
}

use regex::Regex;
use std::sync::OnceLock;

/// Captures `MM:SS[.ms]` two times in `[apply to <start>-<end>]`.
/// Whitespace tolerant; case-insensitive on the keyword.
static PREFIX_RE: OnceLock<Regex> = OnceLock::new();

fn prefix_re() -> &'static Regex {
    PREFIX_RE.get_or_init(|| {
        Regex::new(r"(?i)\[apply\s+to\s+(\d+):(\d+(?:\.\d+)?)-(\d+):(\d+(?:\.\d+)?)\]")
            .expect("range prefix regex is statically valid")
    })
}

pub fn resolve(
    typed: Option<Range>,
    message: &str,
    required: bool,
) -> Result<Option<Range>, RangeError> {
    if let Some(r) = typed {
        validate_order(&r)?;
        return Ok(Some(r));
    }
    if let Some(caps) = prefix_re().captures(message) {
        let start = caps[1].parse::<f64>().unwrap() * 60.0 + caps[2].parse::<f64>().unwrap();
        let end = caps[3].parse::<f64>().unwrap() * 60.0 + caps[4].parse::<f64>().unwrap();
        let r = Range {
            start_sec: start,
            end_sec: end,
        };
        validate_order(&r)?;
        return Ok(Some(r));
    }
    if required {
        return Err(RangeError::MissingRange);
    }
    Ok(None)
}

fn validate_order(r: &Range) -> Result<(), RangeError> {
    if r.start_sec >= r.end_sec {
        return Err(RangeError::InvalidOrder {
            start_sec: r.start_sec,
            end_sec: r.end_sec,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_param_wins_over_text_prefix() {
        let typed = Some(Range {
            start_sec: 1.0,
            end_sec: 2.0,
        });
        let msg = "[apply to 0:10-0:20] fade out";
        let r = resolve(typed, msg, true).unwrap().unwrap();
        assert_eq!(r.start_sec, 1.0);
        assert_eq!(r.end_sec, 2.0);
    }

    #[test]
    fn text_prefix_used_when_typed_is_none() {
        let r = resolve(None, "[apply to 0:23.45-0:45.10] fade out", true)
            .unwrap()
            .unwrap();
        assert!((r.start_sec - 23.45).abs() < 1e-6);
        assert!((r.end_sec - 45.10).abs() < 1e-6);
    }

    #[test]
    fn missing_when_required_and_nothing_present() {
        let err = resolve(None, "fade out", true).unwrap_err();
        assert_eq!(err, RangeError::MissingRange);
    }

    #[test]
    fn missing_returns_none_when_not_required() {
        let r = resolve(None, "reverse the whole thing", false).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn invalid_order_rejected() {
        let typed = Some(Range {
            start_sec: 5.0,
            end_sec: 2.0,
        });
        let err = resolve(typed, "", true).unwrap_err();
        assert!(matches!(err, RangeError::InvalidOrder { .. }));
    }

    #[test]
    fn parses_compact_format() {
        let r = resolve(None, "[apply to 1:00-1:30]", true)
            .unwrap()
            .unwrap();
        assert_eq!(r.start_sec, 60.0);
        assert_eq!(r.end_sec, 90.0);
    }
}
