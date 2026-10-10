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
//!
//! ## One list, asked from outside
//!
//! The registered kinds are a table (`STREAMABLE`) that [`build`]
//! instantiates from, and [`support`] / [`streamable_kinds`] answer from
//! the same table. Anything that decides whether a kind may be put in a
//! chain (the `add_effect` tool, #520) asks here instead of keeping its
//! own list, because a second list is how a tool comes to accept what the
//! renderer then fails on: a chain holding one such effect fails every
//! render until it is removed, and on the master bus that is the whole mix.

use audio_dsp::dynamics::DEFAULT_RELEASE_MS;
use audio_dsp::{Biquad, BiquadCoeffs, Gain, Limiter, Processor};
use session::EffectInstance;

use crate::{Error, Result};

/// How to make one streamable effect: the arguments are its parameters,
/// the project rate and the channel count.
type Make = fn(&serde_json::Value, u32, usize) -> Box<dyn Processor>;

/// An effect kind the streaming renderer can run.
struct Streamable {
    kind: &'static str,
    make: Make,
}

/// Every kind the renderer can run in a chain, and the only place that
/// says so. A kind belongs here when its output is the same however the
/// signal is divided into chunks (see the module docs); until then it
/// belongs in `NOT_YET_STREAMING`.
const STREAMABLE: &[Streamable] = &[
    Streamable {
        kind: "gain",
        make: |p, _, _| Box::new(Gain::from_db(param(p, "db", 0.0))),
    },
    // The release is in milliseconds and the coefficient comes from the
    // project rate, so the same `release_ms` is the same duration at 44.1
    // and at 96 kHz.
    Streamable {
        kind: "limiter",
        make: |p, sample_rate, _| {
            Box::new(Limiter::new(
                param(p, "ceiling_db", 0.0),
                param(p, "release_ms", DEFAULT_RELEASE_MS),
                sample_rate,
            ))
        },
    },
    Streamable {
        kind: "low_pass_filter",
        make: |p, sample_rate, channels| {
            Box::new(Biquad::new(
                BiquadCoeffs::low_pass(param(p, "cutoff_hz", 20_000.0), sample_rate),
                channels,
            ))
        },
    },
    Streamable {
        kind: "high_pass_filter",
        make: |p, sample_rate, channels| {
            Box::new(Biquad::new(
                BiquadCoeffs::high_pass(param(p, "cutoff_hz", 20.0), sample_rate),
                channels,
            ))
        },
    },
    Streamable {
        kind: "notch_filter",
        make: |p, sample_rate, channels| {
            Box::new(Biquad::new(
                BiquadCoeffs::notch(
                    param(p, "center_hz", 1_000.0),
                    param(p, "q", 1.0),
                    sample_rate,
                ),
                channels,
            ))
        },
    },
];

/// What the renderer can do with an effect kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindSupport {
    /// It runs in a chain.
    Streamable,
    /// It exists (as a destructive tool, say) but cannot run on a stream
    /// yet.
    NotYetStreamable,
    /// No such effect. Kinds are matched exactly, so a different case or a
    /// stray space is a different, unknown kind.
    Unknown,
}

/// Whether the renderer can run `kind` in a chain.
pub fn support(kind: &str) -> KindSupport {
    if STREAMABLE.iter().any(|s| s.kind == kind) {
        KindSupport::Streamable
    } else if NOT_YET_STREAMING.contains(&kind) {
        KindSupport::NotYetStreamable
    } else {
        KindSupport::Unknown
    }
}

/// The kinds the renderer can run in a chain, in registry order.
pub fn streamable_kinds() -> impl Iterator<Item = &'static str> + Clone {
    STREAMABLE.iter().map(|s| s.kind)
}

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
/// name was, and a second copy of this table would have been the
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
        let Some(streamable) = STREAMABLE.iter().find(|s| s.kind == effect.kind) else {
            return Err(if NOT_YET_STREAMING.contains(&effect.kind.as_str()) {
                Error::EffectNotStreamable(effect.kind.clone())
            } else {
                Error::UnknownEffect(effect.kind.clone())
            });
        };
        out.push((streamable.make)(&effect.params, sample_rate, channels));
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
        let cases = [
            ("gain", serde_json::json!({ "db": -3.0 })),
            ("limiter", serde_json::json!({ "ceiling_db": -1.0 })),
            (
                "limiter",
                serde_json::json!({ "ceiling_db": -1.0, "release_ms": 120.0 }),
            ),
            (
                "low_pass_filter",
                serde_json::json!({ "cutoff_hz": 5000.0 }),
            ),
            ("high_pass_filter", serde_json::json!({ "cutoff_hz": 80.0 })),
            (
                "notch_filter",
                serde_json::json!({ "center_hz": 50.0, "q": 4.0 }),
            ),
        ];
        for (kind, params) in cases.iter().cloned() {
            let built = build(&[effect(kind, params)], 44_100, 2)
                .unwrap_or_else(|e| panic!("{kind} failed to build: {e}"));
            assert_eq!(built.len(), 1, "{kind}");
        }
        // A kind added to the table without a case above would build
        // untested; make that an edit to this test.
        for kind in streamable_kinds() {
            assert!(
                cases.iter().any(|(k, _)| *k == kind),
                "{kind} is registered but has no case in this test"
            );
        }
    }

    /// The table, [`support`] and [`build`] are one list seen three ways;
    /// this is the check that they stay so (#520). The `add_effect` tool
    /// asks [`support`] before storing a kind, so a disagreement here is a
    /// tool that accepts what the renderer then fails on.
    #[test]
    fn support_agrees_with_what_build_accepts() {
        for kind in streamable_kinds() {
            assert_eq!(support(kind), KindSupport::Streamable, "{kind}");
            build(&[effect(kind, serde_json::json!({}))], 44_100, 2)
                .unwrap_or_else(|e| panic!("{kind} is streamable but failed to build: {e}"));
        }
        for kind in NOT_YET_STREAMING {
            assert_eq!(support(kind), KindSupport::NotYetStreamable, "{kind}");
            let err = build(&[effect(kind, serde_json::json!({}))], 44_100, 2)
                .err()
                .unwrap_or_else(|| panic!("{kind} is not streamable but built"));
            assert!(
                matches!(err, Error::EffectNotStreamable(_)),
                "{kind}: {err}"
            );
        }
        // Kinds are matched exactly: a case or a space makes a new kind.
        for kind in ["wobbulator", "Gain", "GAIN", " gain", "gain ", ""] {
            assert_eq!(support(kind), KindSupport::Unknown, "{kind:?}");
            let err = build(&[effect(kind, serde_json::json!({}))], 44_100, 2)
                .err()
                .unwrap_or_else(|| panic!("{kind:?} is unknown but built"));
            assert!(matches!(err, Error::UnknownEffect(_)), "{kind:?}: {err}");
        }
    }

    /// A kind in both lists would be reported as one thing and built as
    /// the other.
    #[test]
    fn a_kind_is_never_both_streamable_and_not_yet() {
        for kind in NOT_YET_STREAMING {
            assert!(
                !streamable_kinds().any(|k| k == *kind),
                "{kind} is in both lists"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for kind in streamable_kinds() {
            assert!(seen.insert(kind), "{kind} is registered twice");
        }
    }

    /// A bypassed effect is not built, so the renderer never has to run
    /// it: an unsupported kind that is switched off is harmless.
    #[test]
    fn a_bypassed_unstreamable_effect_does_not_fail_the_build() {
        let mut e = effect("reverb", serde_json::json!({}));
        e.bypassed = true;
        assert!(build(&[e], 44_100, 2).unwrap().is_empty());
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

    /// The gain a limiter applies to a steady 0.1, sample by sample,
    /// after one frame that needs it to halve.
    ///
    /// Measured through the registry rather than by constructing a
    /// `Limiter`, so what is under test is that `build` passes the
    /// parameters and the rate on.
    fn recovery_curve(params: serde_json::Value, sample_rate: u32, frames: usize) -> Vec<f32> {
        let mut chain = build(&[effect("limiter", params)], sample_rate, 1).unwrap();
        let mut buf = vec![0.1f32; frames + 1];
        // Ceiling is 0 dBFS, so a 2.0 must be halved.
        buf[0] = 2.0;
        chain[0].process(&mut buf, 1);
        buf[1..].iter().map(|s| s / 0.1).collect()
    }

    /// `release_ms` has to reach the limiter. A short release is back at
    /// unity while a long one is still holding the gain down.
    #[test]
    fn a_limiters_release_comes_from_its_params() {
        let frames = 24_000; // half a second at 48 kHz
        let quick = recovery_curve(
            serde_json::json!({ "ceiling_db": 0.0, "release_ms": 10.0 }),
            48_000,
            frames,
        );
        let slow = recovery_curve(
            serde_json::json!({ "ceiling_db": 0.0, "release_ms": 2_000.0 }),
            48_000,
            frames,
        );
        assert!(
            *quick.last().unwrap() > 0.999,
            "10 ms release should be back at unity after 500 ms, got {}",
            quick.last().unwrap()
        );
        assert!(
            *slow.last().unwrap() < 0.65,
            "a 2 s release should still be holding the gain well down after 500 ms, got {}",
            slow.last().unwrap()
        );
    }

    /// Omitting `release_ms` is allowed — every limiter saved before the
    /// parameter existed has none — and must mean the default, not zero
    /// (which would be a hard clip again).
    #[test]
    fn a_limiter_without_release_ms_uses_the_default() {
        let omitted = recovery_curve(serde_json::json!({ "ceiling_db": 0.0 }), 48_000, 4_800);
        let explicit = recovery_curve(
            serde_json::json!({
                "ceiling_db": 0.0,
                "release_ms": audio_dsp::dynamics::DEFAULT_RELEASE_MS
            }),
            48_000,
            4_800,
        );
        assert_eq!(omitted, explicit);
        assert!(
            omitted[0] < 0.51,
            "the frame after the spike should still be held down, got {}",
            omitted[0]
        );
    }

    /// A release time is a duration. The coefficient is derived from the
    /// project rate, so 100 ms is 100 ms whether the project is 8 kHz or
    /// 96 kHz. A coefficient fixed in samples would make it 12 times
    /// longer at 8 kHz than at 96.
    #[test]
    fn a_limiters_release_is_the_same_duration_at_every_rate() {
        let release_ms = 100.0f32;
        for rate in [8_000u32, 22_050, 44_100, 48_000, 96_000, 192_000] {
            let frames = (rate as usize) / 2;
            let gain = recovery_curve(
                serde_json::json!({ "ceiling_db": 0.0, "release_ms": release_ms }),
                rate,
                frames,
            );
            // The reduction starts at 0.5 and decays by 1/e per release
            // time; find when it has fallen to 0.5 / e.
            let target = 0.5 / std::f32::consts::E;
            let at = gain
                .iter()
                .position(|g| 1.0 - g <= target)
                .unwrap_or_else(|| panic!("{rate} Hz never recovered"));
            let ms = (at + 1) as f32 * 1000.0 / rate as f32;
            assert!(
                (ms - release_ms).abs() < release_ms * 0.03,
                "at {rate} Hz the reduction fell to 1/e after {ms:.1} ms, not {release_ms} ms"
            );
        }
    }
}
