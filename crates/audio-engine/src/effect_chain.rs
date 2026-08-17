//! Render-time effect chains — master, bus and per-track.
//!
//! One registry serves all three. It began as the master chain's
//! (#110), which was the first place a chain of `EffectInstance` had to
//! become processors; buses reused it, and per-track chains (#102) made
//! the "master" in its name simply wrong. A second copy of the match
//! below would have been the duplication #80 and #81 came from.
//!
//! `SessionState::master_chain` and `Track.effects` had both round-
//! tripped through save/load and the diff/merge layer since Phase 1
//! while the render path ignored one and hard-errored on the other. A
//! session with master effects rendered as if they were not there, with
//! no error at all — silent wrong output, which is worse than a
//! rejection, and why an unusable chain now fails the render.
//!
//! ## Why the registry is short
//!
//! The renderer processes a second at a time, and a chain element must
//! give the same output however the signal is divided (see
//! [`audio_dsp::Processor`]). `render.rs` documents that its master
//! chunk size does not affect output bytes, and that stays true only if
//! every element here honours it.
//!
//! The algorithms in `audio_dsp::effects` are one-shot functions over a
//! whole buffer, and most of them are *not* safe to call per chunk:
//!
//! * `tremolo` and `phaser` derive an LFO from the frame index, which
//!   restarts at zero in every chunk — the modulation would reset four
//!   times a minute.
//! * `echo`, `reverb`, `noise_gate`, `leveler` and `de_esser` carry
//!   state (delay lines, comb buffers, envelope followers) that would be
//!   cleared at every seam.
//! * `distortion`'s tone control is a one-pole filter with the same
//!   problem, even though its waveshaper is memoryless.
//!
//! Rather than run them and produce audio that is subtly wrong in a way
//! nobody would attribute to chunking, they are rejected with a message
//! that says what is missing. They become available as each is
//! converted to a `Processor` — see #101's note on the streaming shape.
//!
//! Registered today: `gain`, `limiter`, and the three filters, which are
//! either memoryless or already have a streaming form.

use audio_dsp::{Biquad, BiquadCoeffs, Gain, Limiter, Processor};
use session::EffectInstance;

use crate::{Error, Result};

/// Effect kinds that exist as algorithms but cannot yet run on a
/// stream. Listed explicitly so the error can distinguish "not
/// implemented yet" from "no such effect", which are different mistakes
/// with different fixes.
const NOT_YET_STREAMING: &[&str] = &[
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

/// Read an `f32` parameter, falling back when absent.
fn param(params: &serde_json::Value, name: &str, default: f32) -> f32 {
    params
        .get(name)
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(default)
}

/// Instantiate one effect chain for one render.
///
/// Shared by the master chain, the bus chains and — as of #102 — the
/// per-track chains. The registry was never master-specific; only its
/// name was, and a second copy of this match would have been the
/// duplication #80 and #81 came from.
///
/// Order is the `Vec`'s declaration order, which the determinism
/// invariant in `render.rs` requires ("apply effects in declaration
/// order, never `HashMap`-iteration order").
///
/// Bypassed entries are skipped entirely rather than instantiated and
/// stepped over, so a bypassed effect costs nothing and — more
/// importantly — is byte-identical to an absent one.
pub fn build(
    chain: &[EffectInstance],
    sample_rate: u32,
    channels: usize,
) -> Result<Vec<Box<dyn Processor>>> {
    let mut out: Vec<Box<dyn Processor>> = Vec::new();
    for effect in chain {
        if effect.bypassed {
            continue;
        }
        let p = &effect.params;
        let processor: Box<dyn Processor> = match effect.kind.as_str() {
            "gain" => Box::new(Gain::from_db(param(p, "db", 0.0))),
            "limiter" => Box::new(Limiter::from_db(param(p, "ceiling_db", 0.0))),
            "low_pass_filter" => Box::new(Biquad::new(
                BiquadCoeffs::low_pass(param(p, "cutoff_hz", 20_000.0), sample_rate),
                channels,
            )),
            "high_pass_filter" => Box::new(Biquad::new(
                BiquadCoeffs::high_pass(param(p, "cutoff_hz", 20.0), sample_rate),
                channels,
            )),
            "notch_filter" => Box::new(Biquad::new(
                BiquadCoeffs::notch(
                    param(p, "center_hz", 1_000.0),
                    param(p, "q", 1.0),
                    sample_rate,
                ),
                channels,
            )),
            other if NOT_YET_STREAMING.contains(&other) => {
                return Err(Error::EffectNotStreamable(other.to_string()));
            }
            other => return Err(Error::UnknownEffect(other.to_string())),
        };
        out.push(processor);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(kind: &str, params: serde_json::Value) -> EffectInstance {
        EffectInstance {
            kind: kind.to_string(),
            params,
            bypassed: false,
        }
    }

    #[test]
    fn an_empty_chain_builds_to_nothing() {
        assert!(build(&[], 44_100, 2).unwrap().is_empty());
    }

    #[test]
    fn a_bypassed_effect_is_not_instantiated() {
        let mut e = effect("gain", serde_json::json!({ "db": -6.0 }));
        e.bypassed = true;
        assert!(
            build(&[e], 44_100, 2).unwrap().is_empty(),
            "a bypassed effect must be absent, not a no-op step"
        );
    }

    /// The bug this ticket exists to fix was silence: a master chain the
    /// renderer ignored. An effect it cannot run must say so.
    #[test]
    fn an_unknown_kind_fails_the_render() {
        let err = build(&[effect("wobbulator", serde_json::json!({}))], 44_100, 2).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("wobbulator"), "got {msg}");
    }

    /// "Not implemented on the stream yet" and "no such effect" are
    /// different mistakes and want different fixes, so they get
    /// different errors.
    #[test]
    fn a_known_but_unstreamable_effect_says_so() {
        let err = build(&[effect("reverb", serde_json::json!({}))], 44_100, 2).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("reverb"), "got {msg}");
        assert!(
            msg != build(&[effect("nope", serde_json::json!({}))], 44_100, 2)
                .unwrap_err()
                .to_string(),
            "an unimplemented effect and an unknown one must not read the same"
        );
    }

    #[test]
    fn every_registered_kind_builds() {
        for (kind, params) in [
            ("gain", serde_json::json!({ "db": -3.0 })),
            ("limiter", serde_json::json!({ "ceiling_db": -1.0 })),
            (
                "low_pass_filter",
                serde_json::json!({ "cutoff_hz": 5000.0 }),
            ),
            ("high_pass_filter", serde_json::json!({ "cutoff_hz": 80.0 })),
            (
                "notch_filter",
                serde_json::json!({ "center_hz": 50.0, "q": 4.0 }),
            ),
        ] {
            let built = build(&[effect(kind, params)], 44_100, 2)
                .unwrap_or_else(|e| panic!("{kind} failed to build: {e}"));
            assert_eq!(built.len(), 1, "{kind}");
        }
    }

    /// Missing parameters must fall back to something inert rather than
    /// to zero, which for a gain would silence the master bus.
    #[test]
    fn a_gain_with_no_parameters_is_unity() {
        let mut chain = build(&[effect("gain", serde_json::json!({}))], 44_100, 1).unwrap();
        let mut buf = vec![0.5f32; 8];
        chain[0].process(&mut buf, 1);
        assert_eq!(buf, vec![0.5f32; 8]);
    }
}
