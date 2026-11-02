//! Which effect kinds the streaming renderer can run in a chain.
//!
//! The lists are written out on purpose. The engine's registry is what the
//! tools consult; spelling the kinds here makes a change to either one a
//! visible edit to a test rather than a silent one.
#![allow(dead_code)]

use serde_json::json;
use session::EffectInstance;

/// What the streaming renderer can run in a chain today.
pub const STREAMABLE: [&str; 5] = [
    "gain",
    "limiter",
    "low_pass_filter",
    "high_pass_filter",
    "notch_filter",
];

/// Effects that exist as destructive tools but cannot run in a chain yet.
pub const NOT_YET_STREAMABLE: [&str; 12] = [
    "wah_wah",
    "tremolo",
    "phaser",
    "echo",
    "reverb",
    "noise_gate",
    "leveler",
    "de_esser",
    "distortion",
    "stereo_widener",
    "eq",
    "compressor",
];

/// A stored entry of `kind`, switched on, with no params.
pub fn entry(kind: &str) -> EffectInstance {
    EffectInstance {
        kind: kind.to_string(),
        params: json!({}),
        bypassed: false,
    }
}
