//! Word edges pulled in to where there is sound.
//!
//! The cross-attention alignment (`timing.rs`) lays the words end to end: a
//! word ends where the next begins, and the first begins at 0. Silence
//! therefore belongs to a word, and measured on the real model it all belongs
//! to the word **after** it: the first word of every window starts at the
//! window's start however long the lead-in is, and a word after a pause starts
//! when the pause did (a 6.7 s pause came out as the next word's start, 6.7 s
//! early).
//!
//! `whisper.timing.add_word_timestamps` fixes that from the segment timestamps
//! of Whisper's timestamp mode, and `segments.rs` ports it (#659). It fixes the
//! long pauses, and measured on real speech with 1.5 to 6 s pauses put in it
//! (`scripts/word-timing-eval`) it is what keeps a word from starting seconds
//! early. It does not fix a short lead-in, because the base model's segments run
//! end to end (a segment starts where the one before it ended, and the first at
//! 0.0 s whatever the lead-in), so the timestamps know no more about where the
//! speech starts than the aligner does. This module asks the signal, and is
//! kept for what is left after `segments.rs`: on the fixture, whose first word
//! is 0.3 s into the audio, the first word starts at 0.0 s without it and 0.28 s
//! with it, and on real speech with pauses the worst word end is 1.1 s out
//! without it and 0.4 s out with it.
//!
//! [`trim_to_sound`] looks at the stretch of audio each word was given and
//! finds the sound in it:
//!
//! 1. A 10 ms frame is **quiet** if its RMS is more than [`QUIET_DB`] below the
//!    loudest frame of the window, and **sound** otherwise. The reference is
//!    the window, not the word, so a soft word next to a loud one is still
//!    sound.
//! 2. Within the word's stretch, runs of sound are joined unless they are
//!    separated by [`PAUSE_S`] of quiet or more. What is left are the stretches
//!    of sound the word was given, with pauses between them.
//! 3. The longest one is the word. (Its neighbours' edges leak into the
//!    stretch: the aligner's boundary falls a little early or late, so the
//!    stretch can hold the end of the word before, a pause, and then the word.
//!    A leak is a tenth of a second or so; a word is longer.) A tie goes to the
//!    later one.
//! 4. The word's start and end move in to that run, [`PAD_S`] either side.
//!
//! An edge only ever moves **inward**, by whole frames. A stretch with no pause
//! in it is one run and does not change, so words spoken without a pause
//! between them keep the aligner's times, and so does a recording whose
//! background noise is within [`QUIET_DB`] of its speech (nothing is quiet), and
//! a window of digital silence ([`FLOOR`]).
//!
//! What it does not do is move the boundary between two words that are
//! spoken without a pause. That one is the aligner's, and is measured in the
//! PR for #384: about 0.1 to 0.2 s early. It is the same on real speech (#659):
//! a start is a median 0.065 s from the reference and 0.06 to 0.08 s early on
//! average.

use crate::timing::SECONDS_PER_FRAME;
use crate::Word;

/// Samples per level frame: 10 ms at 16 kHz.
const FRAME: usize = 160;
const FRAMES_PER_SECOND: f32 = 100.0;
/// A frame is quiet if it is more than this many dB under the window's loudest.
pub(crate) const QUIET_DB: f32 = 45.0;
/// Sound separated by at least this much quiet is two runs.
const PAUSE_S: f32 = 0.25;
/// A level under this (RMS, about -80 dBFS) is digital silence: no sound to
/// find, whatever it is compared to.
const FLOOR: f32 = 1e-4;
/// Kept around the sound, so an edge is not cut at the first loud frame.
const PAD_S: f32 = 0.02;

/// Whether `audio` is silent: every sample under [`FLOOR`] (about -80 dBFS).
/// Digital silence, which padding and a muted track both are; a recording
/// with any noise of its own is not.
pub(crate) fn is_silent(audio: &[f32]) -> bool {
    audio.iter().all(|s| s.abs() < FLOOR)
}

/// RMS of each whole 10 ms frame of `audio`.
fn levels(audio: &[f32]) -> Vec<f32> {
    audio
        .chunks_exact(FRAME)
        .map(|f| (f.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt())
        .collect()
}

/// A run of sound: frames `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    start: usize,
    end: usize,
}

impl Run {
    fn len(&self) -> usize {
        self.end - self.start
    }
}

/// The longest run of sound among `levels[first..last]`, where runs
/// separated by `pause` frames of quiet or more are different runs. A tie goes
/// to the later.
fn longest_run(levels: &[f32], first: usize, last: usize, quiet: f32, pause: usize) -> Option<Run> {
    let mut best: Option<Run> = None;
    let mut current: Option<Run> = None;
    let mut gap = 0;
    let keep = |run: Run, best: &mut Option<Run>| {
        if best.is_none_or(|b| run.len() >= b.len()) {
            *best = Some(run);
        }
    };
    for (f, &level) in levels.iter().enumerate().take(last).skip(first) {
        if level < quiet {
            gap += 1;
            continue;
        }
        let fresh = Run {
            start: f,
            end: f + 1,
        };
        current = match current {
            Some(run) if gap < pause => Some(Run { end: f + 1, ..run }),
            Some(run) => {
                keep(run, &mut best);
                Some(fresh)
            }
            None => Some(fresh),
        };
        gap = 0;
    }
    if let Some(run) = current {
        keep(run, &mut best);
    }
    best
}

/// Pull the edges of `words` (seconds from the start of `audio`, 16 kHz mono)
/// in to the sound inside them. See the module docs.
pub(crate) fn trim_to_sound(words: &mut [Word], audio: &[f32]) {
    let levels = levels(audio);
    let loudest = levels.iter().copied().fold(0.0f32, f32::max);
    if loudest < FLOOR {
        return;
    }
    let quiet = loudest * 10f32.powf(-QUIET_DB / 20.0);
    let pause = (PAUSE_S * FRAMES_PER_SECOND).round() as usize;
    for w in words {
        let first = (w.start_s * FRAMES_PER_SECOND).floor().max(0.0) as usize;
        let last = ((w.end_s * FRAMES_PER_SECOND).ceil() as usize).min(levels.len());
        if first >= last {
            continue;
        }
        let Some(run) = longest_run(&levels, first, last, quiet, pause) else {
            continue;
        };
        let start_s = w.start_s.max(run.start as f32 / FRAMES_PER_SECOND - PAD_S);
        let end_s = w.end_s.min(run.end as f32 / FRAMES_PER_SECOND + PAD_S);
        if end_s >= start_s + SECONDS_PER_FRAME {
            w.start_s = start_s;
            w.end_s = end_s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 16_000;

    fn word(start_s: f32, end_s: f32) -> Word {
        Word {
            text: "w".into(),
            start_s,
            end_s,
            confidence: 1.0,
        }
    }

    /// `seconds` of audio, silent except for `bursts` (start, end, amplitude)
    /// of a 300 Hz tone, over a noise floor of `floor` amplitude.
    fn audio(seconds: f32, bursts: &[(f32, f32, f32)], floor: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut x = vec![0.0f32; n];
        // A cheap deterministic noise.
        let mut state = 12_345u32;
        for s in &mut x {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *s = ((state >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 2.0 * floor;
        }
        for &(a, b, amp) in bursts {
            let (from, to) = (
                (a * RATE as f32) as usize,
                ((b * RATE as f32) as usize).min(n),
            );
            for (i, s) in x.iter_mut().enumerate().take(to).skip(from) {
                *s += (2.0 * std::f32::consts::PI * 300.0 * i as f32 / RATE as f32).sin() * amp;
            }
        }
        x
    }

    fn near(got: f32, want: f32) -> bool {
        (got - want).abs() < 0.011
    }

    #[test]
    fn only_digital_silence_is_silent() {
        assert!(is_silent(&[]));
        assert!(is_silent(&vec![0.0; 1000]));
        assert!(is_silent(&vec![0.00005; 1000]));
        let mut x = vec![0.0; 1000];
        x[500] = 0.001;
        assert!(!is_silent(&x), "one sample over the floor is sound");
        assert!(
            !is_silent(&audio(1.0, &[], 0.001)),
            "room noise is not silence"
        );
    }

    #[test]
    fn a_lead_in_is_trimmed_from_the_first_word() {
        // The aligner starts the first word at 0 and ends it at 0.9; the
        // sound is 0.30 to 0.80.
        let x = audio(1.5, &[(0.30, 0.80, 0.3)], 0.0);
        let mut words = vec![word(0.0, 0.9)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[0].start_s, 0.28), "{:?}", words[0]);
        assert!(near(words[0].end_s, 0.82), "{:?}", words[0]);
    }

    #[test]
    fn a_word_stretched_over_the_pause_after_it_is_trimmed_back() {
        let x = audio(3.0, &[(0.2, 0.7, 0.3), (2.0, 2.5, 0.3)], 0.0);
        // Word one is given the whole pause (end at 2.0, where word two
        // starts, as the aligner lays them out).
        let mut words = vec![word(0.0, 2.0), word(2.0, 3.0)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[0].end_s, 0.72), "{:?}", words[0]);
        // Word two already starts where its sound does: an edge moves inward
        // only, so it stays at 2.0 (not 1.98, where the padding would put it).
        assert!((words[1].start_s - 2.0).abs() < 1e-6, "{:?}", words[1]);
        assert!(near(words[1].end_s, 2.52), "{:?}", words[1]);
    }

    #[test]
    fn a_word_after_a_pause_loses_the_pause_and_the_tail_of_the_word_before() {
        // The aligner's boundary fell 0.1 s inside the first word's sound, so
        // the second word's stretch is that tail, 2.2 s of quiet, and the word.
        // This is the case on the real model: a 6.7 s pause came out as the
        // start of the word after it.
        let x = audio(4.0, &[(0.2, 0.8, 0.3), (3.0, 3.5, 0.3)], 0.0);
        let mut words = vec![word(0.0, 0.7), word(0.7, 3.5)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[1].start_s, 2.98), "{:?}", words[1]);
        assert!((words[1].end_s - 3.5).abs() < 1e-6, "{:?}", words[1]);
        // The first word's stretch is all sound from 0.2 on.
        assert!(near(words[0].start_s, 0.2 - PAD_S), "{:?}", words[0]);
        assert!((words[0].end_s - 0.7).abs() < 1e-6, "{:?}", words[0]);
    }

    #[test]
    fn a_soft_word_after_a_loud_one_is_still_sound() {
        // The word is 40 dB under the loud one: inside the 45 dB.
        let x = audio(4.0, &[(0.2, 0.8, 0.3), (3.0, 3.5, 0.003)], 0.0);
        let mut words = vec![word(0.7, 3.5)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[0].start_s, 2.98), "{:?}", words[0]);
    }

    #[test]
    fn a_stretch_without_a_pause_is_not_changed() {
        // Sound all the way through, with a dip of 0.1 s that is not a pause.
        let x = audio(2.0, &[(0.0, 0.9, 0.3), (1.0, 2.0, 0.3)], 0.0);
        let mut words = vec![word(0.25, 1.25), word(1.25, 1.9)];
        let before = words.clone();
        trim_to_sound(&mut words, &x);
        assert_eq!(words, before, "one run each: nothing to trim");
    }

    #[test]
    fn the_longer_run_is_the_word_and_a_tie_goes_to_the_later() {
        // 0.1 s, a pause, 0.5 s: the second.
        let x = audio(3.0, &[(0.5, 0.6, 0.3), (1.5, 2.0, 0.3)], 0.0);
        let mut words = vec![word(0.4, 2.2)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[0].start_s, 1.48), "{:?}", words[0]);
        // 0.5 s, a pause, 0.1 s: the first (a word and the head of the next).
        let x = audio(3.0, &[(0.5, 1.0, 0.3), (2.0, 2.1, 0.3)], 0.0);
        let mut words = vec![word(0.4, 2.2)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[0].end_s, 1.02), "{:?}", words[0]);
        // Equal: the later.
        let x = audio(3.0, &[(0.5, 0.8, 0.3), (1.5, 1.8, 0.3)], 0.0);
        let mut words = vec![word(0.4, 2.2)];
        trim_to_sound(&mut words, &x);
        assert!(near(words[0].start_s, 1.48), "{:?}", words[0]);
    }

    #[test]
    fn digital_silence_and_a_loud_noise_floor_leave_the_aligners_times() {
        let mut words = vec![word(0.0, 1.0)];
        trim_to_sound(&mut words, &vec![0.0; RATE * 2]);
        assert_eq!((words[0].start_s, words[0].end_s), (0.0, 1.0));

        // Noise 30 dB under the tone: inside the 45 dB range, so it counts as
        // sound and the word is not trimmed.
        let x = audio(1.5, &[(0.30, 0.80, 0.3)], 0.01);
        let mut words = vec![word(0.0, 1.0)];
        trim_to_sound(&mut words, &x);
        assert_eq!((words[0].start_s, words[0].end_s), (0.0, 1.0));

        // Noise 50 dB under the tone: quiet enough to trim.
        let x = audio(1.5, &[(0.30, 0.80, 0.3)], 0.001);
        let mut words = vec![word(0.0, 1.0)];
        trim_to_sound(&mut words, &x);
        assert!(words[0].start_s > 0.2, "{:?}", words[0]);
    }

    #[test]
    fn spans_outside_the_audio_or_empty_are_left_alone() {
        let x = audio(1.0, &[(0.2, 0.5, 0.3)], 0.0);
        let mut words = vec![word(2.0, 3.0), word(0.5, 0.5), word(0.95, 1.2)];
        let before = words.clone();
        trim_to_sound(&mut words, &x);
        assert_eq!(words[0], before[0]);
        assert_eq!(words[1], before[1]);
        // The last one reaches past the end of the audio: its span is the part
        // inside, which is quiet, so it keeps the aligner's times.
        assert_eq!(words[2], before[2]);
        trim_to_sound(&mut [], &x);
        trim_to_sound(&mut [word(0.0, 1.0)], &[]);
    }

    #[test]
    fn a_word_never_ends_up_shorter_than_a_frame() {
        // A click of one frame.
        let mut x = vec![0.0f32; RATE];
        for s in &mut x[8000..8160] {
            *s = 0.5;
        }
        let mut words = vec![word(0.4, 0.6)];
        trim_to_sound(&mut words, &x);
        assert!(
            words[0].end_s - words[0].start_s >= SECONDS_PER_FRAME,
            "{:?}",
            words[0]
        );
    }
}
