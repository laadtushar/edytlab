//! Key regressions (#696): a minor chord read as the wrong key.

mod common;

use audio_analysis::analyze;
use common::*;

fn key_of(parts: &[Vec<f32>]) -> String {
    let sig: Vec<f32> = parts.iter().flatten().copied().collect();
    let r = analyze(&decoded(sig)).expect("analyze");
    println!("RESULT key={}", r.key);
    r.key
}

// MIDI: C4 = 60.
const BB3: i32 = 58;
const DB4: i32 = 61;
const F4: i32 = 65;

#[test]
fn sustained_b_flat_minor_triad() {
    for harmonics in [1, 4] {
        let key = key_of(&[chord(&[BB3, DB4, F4], 6.0, harmonics)]);
        assert_eq!(
            key, "A# minor",
            "{harmonics} partials: B-flat minor read as {key}"
        );
    }
}

#[test]
fn a_minor_progression_with_bass_and_harmonics() {
    // Am - Dm - E - Am, bass root an octave under each chord.
    let p = |notes: &[i32], bass: i32| {
        let mut n = notes.to_vec();
        n.push(bass);
        chord(&n, 2.0, 4)
    };
    let key = key_of(&[
        p(&[57, 60, 64], 45),
        p(&[62, 65, 69], 50),
        p(&[64, 68, 71], 52),
        p(&[57, 60, 64], 45),
    ]);
    assert_eq!(key, "A minor");
}

#[test]
fn c_major_progression_with_bass_and_harmonics() {
    // C - F - G - C.
    let p = |notes: &[i32], bass: i32| {
        let mut n = notes.to_vec();
        n.push(bass);
        chord(&n, 2.0, 4)
    };
    let key = key_of(&[
        p(&[60, 64, 67], 48),
        p(&[60, 65, 69], 41),
        p(&[62, 67, 71], 43),
        p(&[60, 64, 67], 48),
    ]);
    assert_eq!(key, "C major");
}

#[test]
fn the_result_names_a_runner_up_and_both_scores() {
    let sig = chord(&[BB3, DB4, F4, 34], 6.0, 4);
    let r = analyze(&decoded(sig)).expect("analyze");
    assert_eq!(r.key, "A# minor");
    assert!(r.key_score > 0.5 && r.key_score <= 1.0, "{}", r.key_score);
    assert_ne!(r.key_runner_up, r.key);
    assert!(r.key_runner_up_score < r.key_score);
    println!(
        "RESULT {} {:.2}, then {} {:.2}",
        r.key, r.key_score, r.key_runner_up, r.key_runner_up_score
    );
}
