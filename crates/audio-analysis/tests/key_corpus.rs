//! The key estimate against a synthetic corpus, and the choice of profile set.
//!
//! Every key (12 roots, major and minor) is written as chord progressions,
//! voiced as triads with the root an octave and a half below and four
//! partials per note, and given to every profile set at several bass
//! weights. `cargo test -p audio-analysis --test key_corpus -- --nocapture`
//! prints the table that chose `ProfileSet::DEFAULT` and `BASS_WEIGHT`.
//!
//! This corpus is clean: sustained chords, no vocals, no reverb, no mix. It
//! can tell a profile that confuses a key with its relative major from one
//! that does not; it cannot say how any of them does on a record.

mod common;

use audio_analysis::key::{chroma, estimate_key_from_chroma, Chroma, ProfileSet, BASS_WEIGHT};
use common::*;

const RATE: u32 = 22_050;
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Major,
    Minor,
}

/// A chord on scale degree given in semitones above the tonic, with its
/// quality and the bass note's semitones above the tonic.
type Chord = (i32, &'static [i32]);

const MAJOR_TRIAD: &[i32] = &[0, 4, 7];
const MINOR_TRIAD: &[i32] = &[0, 3, 7];

/// Progressions as (semitones above the tonic, triad shape).
fn progression(mode: Mode, kind: Kind) -> Vec<Chord> {
    match (mode, kind) {
        // I - IV - V - I
        (Mode::Major, Kind::Cadence) => vec![
            (0, MAJOR_TRIAD),
            (5, MAJOR_TRIAD),
            (7, MAJOR_TRIAD),
            (0, MAJOR_TRIAD),
        ],
        // i - iv - V - i (harmonic minor: the V is major)
        (Mode::Minor, Kind::Cadence) => vec![
            (0, MINOR_TRIAD),
            (5, MINOR_TRIAD),
            (7, MAJOR_TRIAD),
            (0, MINOR_TRIAD),
        ],
        (m, Kind::PedalLoop) => progression(m, Kind::Loop),
        (Mode::Minor, Kind::Loop) => vec![
            (0, MINOR_TRIAD),
            (8, MAJOR_TRIAD),
            (3, MAJOR_TRIAD),
            (10, MAJOR_TRIAD),
            (0, MINOR_TRIAD),
            (8, MAJOR_TRIAD),
            (3, MAJOR_TRIAD),
            (10, MAJOR_TRIAD),
        ],
        (Mode::Major, Kind::Loop) => vec![
            (0, MAJOR_TRIAD),
            (9, MINOR_TRIAD),
            (5, MAJOR_TRIAD),
            (7, MAJOR_TRIAD),
            (0, MAJOR_TRIAD),
            (9, MINOR_TRIAD),
            (5, MAJOR_TRIAD),
            (7, MAJOR_TRIAD),
        ],
        // I - V - I - V - I - V
        (Mode::Major, Kind::Alternate) => (0..6)
            .map(|i| (if i % 2 == 0 { 0 } else { 7 }, MAJOR_TRIAD))
            .collect(),
        // i - v - i - v - i - v (natural minor v)
        (Mode::Minor, Kind::Alternate) => (0..6)
            .map(|i| (if i % 2 == 0 { 0 } else { 7 }, MINOR_TRIAD))
            .collect(),
        // I - I - V - IV - I - I: a vamp that stays on the tonic.
        (Mode::Major, Kind::Vamp) => vec![
            (0, MAJOR_TRIAD),
            (0, MAJOR_TRIAD),
            (7, MAJOR_TRIAD),
            (5, MAJOR_TRIAD),
            (0, MAJOR_TRIAD),
            (0, MAJOR_TRIAD),
        ],
        // i - i - VII - VI - i - i, all natural minor: every note is a note
        // of the relative major, and only the weight on the tonic (and the
        // bass) says minor. The hard case.
        (Mode::Minor, Kind::Vamp) => vec![
            (0, MINOR_TRIAD),
            (0, MINOR_TRIAD),
            (10, MAJOR_TRIAD),
            (8, MAJOR_TRIAD),
            (0, MINOR_TRIAD),
            (0, MINOR_TRIAD),
        ],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Cadence,
    Vamp,
    /// Tonic and dominant, back and forth: a dominant-key bias reads it as
    /// the dominant's key.
    Alternate,
    /// Four chords from the tonic, round and round: i - VI - III - VII
    /// (A minor, F, C, G) or I - vi - IV - V (C, A minor, F, G). The same
    /// four chords either way, so which key is meant is a matter of where it
    /// starts and what the bass does; reported, not asserted.
    Loop,
    /// The same loop over a bass that stays on the tonic. The chords say
    /// relative major; the bass says tonic.
    PedalLoop,
}

/// A progression in `tonic` (pitch class 0..12), 1.5 s a chord, optionally
/// with a kick and hats under it and optionally detuned.
fn render(tonic: usize, mode: Mode, kind: Kind, drums: bool, cents: f32, noise: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for (semis, shape) in progression(mode, kind) {
        let root_pc = tonic as i32 + semis;
        // Triad in the octave from C4 up, root position, plus the root two
        // octaves under C4... (an octave and a half under the triad).
        let triad_root = 60 + root_pc.rem_euclid(12);
        let mut notes: Vec<i32> = shape.iter().map(|s| triad_root + s).collect();
        let bass_pc = if kind == Kind::PedalLoop {
            tonic as i32
        } else {
            root_pc
        };
        notes.push(36 + bass_pc.rem_euclid(12));
        out.extend(chord_at(RATE, &notes, 1.5, 4, cents));
    }
    if drums {
        // 120 BPM kick and hats from the same helpers as the tempo tests,
        // at the corpus' rate.
        let (d, _) = kick_and_hats_at(RATE, 120.0, out.len() as f32 / RATE as f32, 0.0, 0.5, 0.2);
        for (o, d) in out.iter_mut().zip(d) {
            *o = *o * 0.5 + d * 0.5;
        }
    }
    if noise > 0.0 {
        let mut n = Noise::new(11 + tonic as u64);
        for o in out.iter_mut() {
            *o += noise * n.next();
        }
    }
    out
}

struct Case {
    label: &'static str,
    kind: Kind,
    drums: bool,
    cents: f32,
    noise: f32,
    /// Whether the shipped configuration must get 95% of them.
    asserted: bool,
}

const CASES: [Case; 8] = [
    Case {
        label: "cadence",
        kind: Kind::Cadence,
        drums: false,
        cents: 0.0,
        noise: 0.0,
        asserted: true,
    },
    Case {
        label: "+drums",
        kind: Kind::Cadence,
        drums: true,
        cents: 0.0,
        noise: 0.0,
        asserted: true,
    },
    Case {
        label: "35c flat",
        kind: Kind::Cadence,
        drums: false,
        cents: -35.0,
        noise: 0.0,
        asserted: true,
    },
    Case {
        label: "noisy",
        kind: Kind::Cadence,
        drums: false,
        cents: 0.0,
        noise: 0.15,
        asserted: true,
    },
    Case {
        label: "vamp",
        kind: Kind::Vamp,
        drums: false,
        cents: 0.0,
        noise: 0.0,
        asserted: false,
    },
    Case {
        label: "I-V-I-V",
        kind: Kind::Alternate,
        drums: false,
        cents: 0.0,
        noise: 0.0,
        asserted: false,
    },
    Case {
        label: "4-chord loop",
        kind: Kind::Loop,
        drums: false,
        cents: 0.0,
        noise: 0.0,
        asserted: false,
    },
    Case {
        label: "loop+pedal",
        kind: Kind::PedalLoop,
        drums: false,
        cents: 0.0,
        noise: 0.0,
        asserted: false,
    },
];

/// The chroma and the right answer of every key in a case.
fn chromas(case: &Case) -> Vec<(Chroma, String, String)> {
    let mut out = Vec::new();
    for tonic in 0..12 {
        for mode in [Mode::Major, Mode::Minor] {
            let sig = render(tonic, mode, case.kind, case.drums, case.cents, case.noise);
            let want = format!(
                "{} {}",
                NAMES[tonic],
                if mode == Mode::Major {
                    "major"
                } else {
                    "minor"
                }
            );
            let relative = match mode {
                Mode::Major => format!("{} minor", NAMES[(tonic + 9) % 12]),
                Mode::Minor => format!("{} major", NAMES[(tonic + 3) % 12]),
            };
            out.push((chroma(&sig, RATE).expect("chroma"), want, relative));
        }
    }
    out
}

/// (right, total, misses that named the relative key).
fn score(
    set: &[(Chroma, String, String)],
    profiles: ProfileSet,
    bass: f32,
) -> (usize, usize, usize) {
    let (mut right, mut relative) = (0, 0);
    for (c, want, rel) in set {
        let k = estimate_key_from_chroma(c, profiles, bass).expect("key");
        if &k.key == want {
            right += 1;
        } else if &k.key == rel {
            relative += 1;
        }
    }
    (right, set.len(), relative)
}

#[test]
fn profile_sets_and_bass_weights_on_the_corpus() {
    let sets: Vec<_> = CASES.iter().map(chromas).collect();
    println!("KEYTABLE right of 24 per case (misses that named the relative key in brackets)");
    print!("KEYTABLE {:<30}", "");
    for c in &CASES {
        print!(" {:>12}", c.label);
    }
    println!();
    for profiles in ProfileSet::ALL {
        for bass in [0.0f32, 0.5, 1.0] {
            print!("KEYTABLE {:<24} b={:<3}", profiles.name(), bass);
            for set in &sets {
                let (r, t, rel) = score(set, profiles, bass);
                print!(" {r:>8}/{t} ({rel})");
            }
            println!();
        }
    }

    // The shipped configuration holds up on the clean cases, with drums under
    // them, with the recording out of tune and with noise over it.
    for (c, set) in CASES.iter().zip(&sets).filter(|(c, _)| c.asserted) {
        let (r, t, _) = score(set, ProfileSet::DEFAULT, BASS_WEIGHT);
        assert!(
            r as f32 >= t as f32 * 0.95,
            "{}: {r}/{t} right with {} at bass weight {BASS_WEIGHT}",
            c.label,
            ProfileSet::DEFAULT.name()
        );
    }
}
