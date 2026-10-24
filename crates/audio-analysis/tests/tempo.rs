//! Tempo and beat-grid regressions (#696): a kick on the beat with a hi-hat
//! on the off-beat read as double tempo, and the grid could land on the hats.

mod common;

use audio_analysis::analyze;
use common::*;

/// Signed offset from `t` to the nearest time in `of`, in seconds.
fn signed_nearest(t: f32, of: &[f32]) -> f32 {
    of.iter()
        .map(|k| t - k)
        .fold(f32::INFINITY, |a, b| if b.abs() < a.abs() { b } else { a })
}

/// Distance from `t` to the nearest time in `of`, in seconds.
fn nearest(t: f32, of: &[f32]) -> f32 {
    of.iter()
        .map(|k| (t - k).abs())
        .fold(f32::INFINITY, f32::min)
}

fn run(bpm: f32, kick_amp: f32, hat_amp: f32) {
    let (sig, kicks) = kick_and_hats(bpm, 24.0, 0.25, kick_amp, hat_amp);
    let report = analyze(&decoded(sig)).expect("analyze");
    let reported_bpm = report.bpm.expect("a tempo");
    let beat = 60.0 / bpm;
    // Beats that sit on a kick (within 35 ms) rather than on a hat (half a
    // beat away).
    let on_kick = report
        .beats
        .iter()
        .filter(|b| nearest(**b, &kicks) <= 0.035)
        .count();
    let share = on_kick as f32 / report.beats.len().max(1) as f32;
    let mean_off: f32 = report
        .beats
        .iter()
        .map(|b| signed_nearest(*b, &kicks))
        .sum::<f32>()
        / report.beats.len() as f32;
    println!("OFFSET mean beat-minus-kick {:.1} ms", mean_off * 1000.0);
    assert!(
        mean_off.abs() <= 0.006,
        "{bpm} BPM: the grid sits {:.1} ms from the kicks",
        mean_off * 1000.0
    );
    println!(
        "RESULT bpm_true={bpm} kick={kick_amp} hat={hat_amp} -> bpm={:.1} beats={} on_kick={:.0}% (grid period {:.3}s, true {:.3}s)",
        reported_bpm,
        report.beats.len(),
        share * 100.0,
        report.beats.windows(2).map(|w| w[1] - w[0]).sum::<f32>()
            / report.beats.len().saturating_sub(1).max(1) as f32,
        beat
    );
    assert!(
        (reported_bpm - bpm).abs() <= bpm * 0.015,
        "{bpm} BPM kick+hats read as {reported_bpm} BPM"
    );
    assert!(
        share >= 0.9,
        "{bpm} BPM: only {:.0}% of the beat grid is on a kick",
        share * 100.0
    );
}

#[test]
fn kick_and_offbeat_hats_70_bpm() {
    run(70.0, 0.8, 0.25);
}

#[test]
fn kick_and_offbeat_hats_90_bpm() {
    run(90.0, 0.8, 0.25);
}

#[test]
fn kick_and_offbeat_hats_128_bpm() {
    run(128.0, 0.8, 0.25);
}

/// Hats as loud as the kick: broadband noise gives the most spectral flux, so
/// a tracker that follows total flux follows the hats.
#[test]
fn loud_hats_do_not_take_the_grid() {
    run(90.0, 0.5, 0.9);
    run(128.0, 0.5, 0.9);
}

/// The result says how sure it is and what the other octaves are, on the same
/// scale, so the tool can offer "or 180?" and mean it.
#[test]
fn the_result_carries_a_confidence_and_the_double() {
    let (sig, _) = kick_and_hats(90.0, 24.0, 0.25, 0.8, 0.25);
    let report = analyze(&decoded(sig)).expect("analyze");
    assert!(report.bpm_confidence >= 0.5, "{:?}", report.bpm_confidence);
    // 45 is under the 50 BPM floor, so only the double is offered.
    let alt = report
        .bpm_alternatives
        .iter()
        .find(|a| a.relation == audio_analysis::TempoRelation::Double)
        .expect("double offered");
    assert!((alt.bpm - 180.0).abs() <= 3.0, "{alt:?}");
    assert!(alt.confidence < report.bpm_confidence, "{alt:?}");
    assert!(report
        .bpm_alternatives
        .iter()
        .all(|a| a.relation != audio_analysis::TempoRelation::Half));
}

/// A chord pad and a bass note under the drums, as in a real mix.
#[test]
fn a_pad_and_a_bass_under_the_drums_change_nothing() {
    for bpm in [70.0f32, 90.0, 128.0] {
        let (mut sig, kicks) = kick_and_hats(bpm, 24.0, 0.25, 0.8, 0.25);
        let pad = chord(&[57, 60, 64, 45], 24.0, 3);
        for (s, p) in sig.iter_mut().zip(pad) {
            *s += 0.4 * p;
        }
        let report = analyze(&decoded(sig)).expect("analyze");
        let got = report.bpm.expect("a tempo");
        assert!(
            (got - bpm).abs() <= bpm * 0.015,
            "{bpm} BPM with a pad read as {got}"
        );
        let on_kick = report
            .beats
            .iter()
            .filter(|b| nearest(**b, &kicks) <= 0.035)
            .count();
        assert!(
            on_kick as f32 >= 0.9 * report.beats.len() as f32,
            "{bpm} BPM with a pad: {on_kick} of {} beats on a kick",
            report.beats.len()
        );
    }
}

/// A tempo that drifts a few percent is followed by the grid, not forced onto
/// a rigid one.
#[test]
fn a_drummer_who_pushes_the_beat_is_followed() {
    // Kicks at 100 BPM, each one a random up-to-15 ms early or late.
    let mut noise = Noise::new(3);
    let beat = 0.6f32;
    let mut sig = vec![0.0f32; (20.0 * SR as f32) as usize];
    let mut kicks = Vec::new();
    let mut t = 0.3f32;
    while t < 19.5 {
        let jitter = noise.next() * 0.015;
        add_kick_at(&mut sig, ((t + jitter) * SR as f32) as usize, 0.8);
        kicks.push(t + jitter);
        t += beat;
    }
    let report = analyze(&decoded(sig)).expect("analyze");
    let got = report.bpm.expect("a tempo");
    assert!((got - 100.0).abs() <= 1.5, "read as {got}");
    let close = report
        .beats
        .iter()
        .filter(|b| nearest(**b, &kicks) <= 0.025)
        .count();
    assert!(
        close as f32 >= 0.9 * report.beats.len() as f32,
        "{close} of {} beats within 25 ms of a kick",
        report.beats.len()
    );
}

/// A pad with no drums has no pulse: no tempo, no beats, and the key is still
/// found. It must not fail the whole analysis.
#[test]
fn a_pad_alone_has_a_key_and_no_tempo() {
    let sig = chord(&[57, 60, 64, 45], 8.0, 3);
    let report = analyze(&decoded(sig)).expect("analyze");
    assert_eq!(report.bpm, None);
    assert!(report.bpm_alternatives.is_empty() && report.beats.is_empty());
    assert_eq!(report.key, "A minor");
}
