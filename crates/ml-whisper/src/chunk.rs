//! Long audio, one 30 s window at a time (plan step 5 on #384).
//!
//! The encoder takes exactly 30 s. A recording longer than that is cut into
//! windows, and the rule for where the next one starts is the whole of this
//! module:
//!
//! * a window is up to 30 s of audio from `seek`;
//! * if it is the **last** one (it reaches the end of the audio), every word
//!   is kept;
//! * otherwise the last word may have been cut by the window's edge, and the
//!   model's idea of it is a guess. A word counts as cut when the decoder did
//!   not finish by itself (it ran out of tokens, or looped and was stopped), or
//!   when the word ends within [`EDGE_S`] of the window's end. A cut word is
//!   dropped, and the next window starts at the **later of the last complete
//!   word's end and the cut word's start**, so the cut word is heard whole the
//!   second time. (Both are in the audio the window trimmed to the sound
//!   (`refine.rs`), so across a pause the cut word's start is the later one, and
//!   in speech with no pause they are the same instant.) Without a cut word the
//!   next window starts where the last word ended;
//! * a window that starts where the aligner ended a word starts inside that
//!   word's tail, because the aligner puts the boundary between two words that
//!   are spoken without a pause about 0.14 s early. The model may write the
//!   tail down as a word, so a first word that repeats the last kept word, and
//!   starts no more than [`DUPLICATE_S`] after it ended, is dropped. A cut word
//!   that is the same word again ("no no") is not a tail: it is heard whole,
//!   not in part, and is kept;
//! * if no word ends later than [`MIN_ADVANCE_S`] into the window (it decoded to
//!   a hallucination, or nothing past the first fraction of a second), the next
//!   window starts a **whole window** on, as `whisper.transcribe` does when its
//!   seek would not move. Advancing by [`MIN_ADVANCE_S`] instead would decode
//!   the same hopeless audio sixty times over, writing a word each time;
//! * a window that decoded to nothing, or was skipped as no speech, is
//!   passed over whole.
//!
//! Word times in a window are relative to its start; they are offset by
//! `seek` here. The language is fixed by the first window that has speech and
//! then given to every later one, as `whisper.transcribe` does, so a window of
//! music cannot flip the language of the rest.
//!
//! The windows themselves are decoded by a [`WindowTranscriber`], which the
//! model implements and the tests fake: the rule above is checked on 75 s of
//! made-up audio in CI.

use crate::mel::{N_SAMPLES, SAMPLE_RATE};
use crate::{Result, WhisperError, Word};

/// Samples in a full window: 30 s at 16 kHz.
pub(crate) const WINDOW_SAMPLES: usize = N_SAMPLES;
/// A window shorter than this (0.1 s) is not worth an encoder pass.
const MIN_WINDOW_SAMPLES: usize = SAMPLE_RATE / 10;
/// A word that ends within this many seconds of a full window's end may have
/// been cut by it.
pub(crate) const EDGE_S: f32 = 0.5;
/// The next window starts a word's end or start later only if that is more than
/// this many seconds into the window; otherwise it starts a whole window on.
pub(crate) const MIN_ADVANCE_S: f32 = 0.5;
/// A first word that repeats the last kept word and starts within this many
/// seconds of its end is the word's tail, heard again.
pub(crate) const DUPLICATE_S: f32 = 0.1;

/// What decoding one window gave.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WindowResult {
    /// Times in seconds from the window's start.
    pub words: Vec<Word>,
    /// The language token used.
    pub language: u32,
    /// Whether the window had speech: it decoded at least one token. Only such
    /// a window sets the language.
    pub speech: bool,
    /// The decoder ended the window itself, at `<|endoftext|>`.
    pub finished: bool,
}

/// Decodes one window of 16 kHz mono audio, up to 30 s.
pub(crate) trait WindowTranscriber {
    /// `language` is the language token to use, or `None` to detect it.
    fn window(&mut self, audio: &[f32], language: Option<u32>) -> Result<WindowResult>;
}

/// The words of a whole recording.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Chunked {
    pub words: Vec<Word>,
    /// The language token, if any window had speech (or one was given).
    pub language: Option<u32>,
    /// Windows decoded.
    pub windows: usize,
}

/// What a full window gives: the words to keep, where the next window starts
/// and the word that was cut off, if any.
#[derive(Debug, Clone, PartialEq)]
struct Cut {
    keep: Vec<Word>,
    /// Seconds from this window's start; `None` means a whole window on.
    next: Option<f32>,
    dropped: Option<Word>,
}

/// `Some(seconds)` if the next window starts that far into this one, `None`
/// (a whole window on) if that is [`MIN_ADVANCE_S`] or less.
fn advance(seconds: f32) -> Option<f32> {
    (seconds > MIN_ADVANCE_S).then_some(seconds)
}

/// Split a full window's words into the ones to keep and where the next
/// window starts.
fn cut_window(mut words: Vec<Word>, window_s: f32, finished: bool) -> Cut {
    let Some(last) = words.pop() else {
        return Cut {
            keep: words,
            next: None,
            dropped: None,
        };
    };
    let cut = !finished || last.end_s > window_s - EDGE_S;
    if !cut {
        let next = advance(last.end_s);
        words.push(last);
        return Cut {
            keep: words,
            next,
            dropped: None,
        };
    }
    // The cut word is heard again from its start, or from the end of the word
    // before it if that is later (a word's start is never before its
    // predecessor's end, but the aligner's guesses for the last word of a
    // window are rough).
    let resume = words
        .last()
        .map_or(last.start_s, |complete| complete.end_s.max(last.start_s));
    Cut {
        keep: words,
        next: advance(resume),
        dropped: Some(last),
    }
}

/// A word as the comparison sees it: letters and digits, lower case.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `first`, the first word of a window, is the tail of `kept`, the
/// last word kept before it, heard again.
///
/// `dropped` is the word the previous window cut off. When `first` is that
/// word's text it is that word, heard whole, and not a tail, even if the same
/// word was just said ("no no").
fn is_tail_of(kept: &Word, first: &Word, dropped: Option<&Word>) -> bool {
    let text = plain(&first.text);
    !text.is_empty()
        && text == plain(&kept.text)
        && dropped.is_none_or(|d| plain(&d.text) != text)
        && first.start_s <= kept.end_s + DUPLICATE_S
}

/// Transcribe `audio` (16 kHz mono) window by window.
///
/// `cancel` is asked before each window. [`run_windows_observed`] without
/// the progress report: the form the chunking tests use.
#[cfg(test)]
pub(crate) fn run_windows<T: WindowTranscriber>(
    transcriber: &mut T,
    audio: &[f32],
    language: Option<u32>,
    cancel: &dyn Fn() -> bool,
) -> Result<Chunked> {
    run_windows_observed(transcriber, audio, language, cancel, &|_, _| {})
}

/// How many windows there are in all, as far as is known at `seek`: the ones
/// decoded so far plus the ones the audio left would fill if each took the
/// full 30 s. An estimate that can only grow: a window that ends in a word
/// cut by its edge starts the next one earlier than 30 s on, so the audio
/// takes more windows than its length over 30 s.
fn windows_in_all(decoded: usize, total: usize, seek: usize) -> usize {
    decoded + (total - seek).div_ceil(WINDOW_SAMPLES).max(1)
}

/// [`run_windows`] that says which window it is about to decode.
///
/// `progress(index, total)` is called before each window, after `cancel` has
/// answered no: `index` counts from 0 and is the number of windows already
/// decoded, `total` is the estimate of [`windows_in_all`], so `total` is
/// always more than `index`. A cancelled run reports nothing for the window
/// it did not start.
pub(crate) fn run_windows_observed<T: WindowTranscriber>(
    transcriber: &mut T,
    audio: &[f32],
    mut language: Option<u32>,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(usize, usize),
) -> Result<Chunked> {
    let total = audio.len();
    let rate = SAMPLE_RATE as f32;
    let mut words = Vec::new();
    let mut windows = 0;
    let mut seek = 0usize;
    // The word the window before this one cut off, with absolute times.
    let mut last_dropped: Option<Word> = None;
    while total - seek >= MIN_WINDOW_SAMPLES {
        if cancel() {
            return Err(WhisperError::Cancelled);
        }
        let end = (seek + WINDOW_SAMPLES).min(total);
        let slice = &audio[seek..end];
        progress(windows, windows_in_all(windows, total, seek));
        let result = transcriber.window(slice, language)?;
        windows += 1;
        if result.speech && language.is_none() {
            language = Some(result.language);
        }

        let last_window = end == total;
        let window_s = slice.len() as f32 / rate;
        let (keep, next, dropped) = if last_window {
            (result.words, None, None)
        } else {
            let cut = cut_window(result.words, window_s, result.finished);
            (cut.keep, cut.next, cut.dropped)
        };
        let offset = seek as f32 / rate;
        let mut keep: Vec<Word> = keep
            .into_iter()
            .map(|w| Word {
                start_s: w.start_s + offset,
                end_s: w.end_s + offset,
                ..w
            })
            .collect();
        // This window began inside the last kept word if that word is its
        // first, again.
        if let (Some(kept), Some(first)) = (words.last(), keep.first()) {
            if is_tail_of(kept, first, last_dropped.as_ref()) {
                keep.remove(0);
            }
        }
        words.extend(keep);
        last_dropped = dropped.map(|w| Word {
            start_s: w.start_s + offset,
            end_s: w.end_s + offset,
            ..w
        });
        if last_window {
            break;
        }
        seek = match next {
            Some(s) => {
                let advance = (s * rate).round() as usize;
                // A word's end can lie past the window's if the aligner
                // overshoots; the window's end is as far as it can go.
                seek + advance.min(end - seek)
            }
            None => end,
        };
    }
    Ok(Chunked {
        words,
        language,
        windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const RATE: usize = SAMPLE_RATE;

    /// Audio whose sample `i` is `i`, so a window can tell where it starts.
    fn ramp(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| i as f32)
            .collect()
    }

    /// Word `k` is spoken from `2k + 0.1` s to `2k + 1.4` s.
    fn word_span(k: usize) -> (f32, f32) {
        (2.0 * k as f32 + 0.1, 2.0 * k as f32 + 1.4)
    }

    /// A fake model that hears the words above, the way Whisper hears them: a
    /// word that starts in the window is in it, and one that runs past the end
    /// is heard as ending there.
    struct Hears {
        /// Seconds before which nothing is heard (music, silence).
        speech_from: f32,
        /// `(window start in seconds, language passed in)` for each call.
        calls: RefCell<Vec<(f32, Option<u32>)>>,
        finished: bool,
    }

    impl Hears {
        fn new() -> Self {
            Self {
                speech_from: 0.0,
                calls: RefCell::new(Vec::new()),
                finished: true,
            }
        }
    }

    impl WindowTranscriber for Hears {
        fn window(&mut self, audio: &[f32], language: Option<u32>) -> Result<WindowResult> {
            let start = audio[0] / RATE as f32;
            let len = audio.len() as f32 / RATE as f32;
            self.calls.borrow_mut().push((start, language));
            let mut words = Vec::new();
            for k in 0.. {
                let (s, e) = word_span(k);
                if s >= start + len {
                    break;
                }
                if s < start.max(self.speech_from) {
                    continue;
                }
                words.push(Word {
                    text: format!("w{k}"),
                    start_s: s - start,
                    end_s: (e - start).min(len),
                    confidence: 0.9,
                });
            }
            Ok(WindowResult {
                speech: !words.is_empty(),
                words,
                language: 7,
                finished: self.finished,
            })
        }
    }

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|w| w.text.as_str()).collect()
    }

    fn never() -> bool {
        false
    }

    #[test]
    fn seventy_five_seconds_are_three_windows_and_every_word_is_heard_once() {
        let mut model = Hears::new();
        let out = run_windows(&mut model, &ramp(75.0), None, &never).unwrap();

        // Words 0..=37 start before 75 s (word 37 at 74.1 s).
        let expected: Vec<String> = (0..=37).map(|k| format!("w{k}")).collect();
        assert_eq!(texts(&out.words), expected, "each word once, in order");
        for (k, w) in out.words.iter().enumerate() {
            let (s, e) = word_span(k);
            assert!(
                (w.start_s - s).abs() < 1e-3,
                "{}: start {} vs {s}",
                w.text,
                w.start_s
            );
            // The last word is cut by the end of the audio.
            let e = if k == 37 { 75.0 } else { e };
            assert!(
                (w.end_s - e).abs() < 1e-3,
                "{}: end {} vs {e}",
                w.text,
                w.end_s
            );
        }

        // Window 1 [0, 30): word 14 ends at 29.4, 0.6 s from the edge, so it is
        // complete and the next window starts where it ended. Window 2
        // [29.4, 59.4): word 29 ends exactly at the edge, so it is cut, and the
        // next window starts at the later of the end of word 28 (57.4) and the
        // start of word 29 (58.1): 58.1. Window 3 reaches the end of the audio.
        let starts: Vec<f32> = model.calls.borrow().iter().map(|c| c.0).collect();
        assert_eq!(starts.len(), 3, "{starts:?}");
        assert_eq!(out.windows, 3);
        for (got, want) in starts.iter().zip([0.0, 29.4, 58.1]) {
            assert!((got - want).abs() < 1e-3, "{starts:?}");
        }
    }

    #[test]
    fn a_word_cut_by_the_window_is_heard_whole_in_the_next() {
        struct LateWord(Vec<f32>);
        impl WindowTranscriber for LateWord {
            fn window(&mut self, audio: &[f32], _: Option<u32>) -> Result<WindowResult> {
                let start = audio[0] / RATE as f32;
                self.0.push(start);
                // One word starts at 27 s and ends at 33 s: cut by the first
                // window, whole in the second.
                let (s, e) = (27.0f32, 33.0f32);
                let len = audio.len() as f32 / RATE as f32;
                let mut words = vec![Word {
                    text: "early".into(),
                    start_s: 10.0 - start,
                    end_s: 11.0 - start,
                    confidence: 1.0,
                }];
                if start > 10.0 {
                    words.clear();
                }
                if s >= start {
                    words.push(Word {
                        text: "long".into(),
                        start_s: s - start,
                        end_s: (e - start).min(len),
                        confidence: 1.0,
                    });
                }
                Ok(WindowResult {
                    words,
                    language: 1,
                    speech: true,
                    finished: true,
                })
            }
        }
        let mut model = LateWord(Vec::new());
        let out = run_windows(&mut model, &ramp(45.0), None, &never).unwrap();
        // First window [0, 30): "long" runs to the edge, so it is dropped and
        // the next window starts where it did (27 s), which is later than where
        // "early" ended (11 s).
        assert!(model.0.len() >= 2, "{:?}", model.0);
        assert!((model.0[1] - 27.0).abs() < 1e-3, "{:?}", model.0);
        assert_eq!(texts(&out.words), vec!["early", "long"]);
        assert!((out.words[1].start_s - 27.0).abs() < 1e-3);
        assert!((out.words[1].end_s - 33.0).abs() < 1e-3);
    }

    #[test]
    fn a_decoder_that_did_not_finish_loses_its_last_word_to_the_next_window() {
        let mut model = Hears::new();
        model.finished = false;
        let out = run_windows(&mut model, &ramp(45.0), None, &never).unwrap();
        // Every word is still heard exactly once: the last of each window is
        // dropped and heard again.
        let n = out.words.len();
        let expected: Vec<String> = (0..n).map(|k| format!("w{k}")).collect();
        assert_eq!(texts(&out.words), expected);
        assert!(model.calls.borrow().len() >= 2);
    }

    #[test]
    fn windows_without_speech_are_passed_over_whole_and_leave_the_language_open() {
        let mut model = Hears::new();
        model.speech_from = 70.0; // music for the first 70 s
        let out = run_windows(&mut model, &ramp(75.0), None, &never).unwrap();
        let calls = model.calls.borrow().clone();
        assert_eq!(
            calls.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![0.0, 30.0, 60.0],
            "a silent window costs exactly 30 s"
        );
        // The language is still unknown until a window has speech, and is
        // then the one that window found.
        assert_eq!(
            calls.iter().map(|c| c.1).collect::<Vec<_>>(),
            vec![None, None, None]
        );
        assert_eq!(out.language, Some(7));
        assert_eq!(texts(&out.words), vec!["w35", "w36", "w37"]);
    }

    #[test]
    fn the_language_of_the_first_window_with_speech_is_given_to_the_rest() {
        let mut model = Hears::new();
        run_windows(&mut model, &ramp(75.0), None, &never).unwrap();
        let langs: Vec<_> = model.calls.borrow().iter().map(|c| c.1).collect();
        assert_eq!(langs, vec![None, Some(7), Some(7)]);

        let mut model = Hears::new();
        run_windows(&mut model, &ramp(75.0), Some(3), &never).unwrap();
        let langs: Vec<_> = model.calls.borrow().iter().map(|c| c.1).collect();
        assert_eq!(
            langs,
            vec![Some(3), Some(3), Some(3)],
            "a given language is kept"
        );
    }

    #[test]
    fn a_short_recording_is_one_window_and_a_sliver_is_none() {
        let mut model = Hears::new();
        let out = run_windows(&mut model, &ramp(5.0), None, &never).unwrap();
        assert_eq!(out.windows, 1);
        assert_eq!(texts(&out.words), vec!["w0", "w1", "w2"]);

        let mut model = Hears::new();
        let out = run_windows(&mut model, &ramp(0.05), None, &never).unwrap();
        assert_eq!(out.windows, 0, "50 ms is not worth an encoder pass");
        assert!(out.words.is_empty());

        let out = run_windows(&mut model, &[], None, &never).unwrap();
        assert_eq!(out.windows, 0);
    }

    #[test]
    fn cancel_is_asked_before_every_window_and_stops_the_run() {
        let asked = Cell::new(0);
        let cancel = || {
            asked.set(asked.get() + 1);
            asked.get() > 1 // allow the first window, refuse the second
        };
        let mut model = Hears::new();
        let err = run_windows(&mut model, &ramp(75.0), None, &cancel).unwrap_err();
        assert!(matches!(err, WhisperError::Cancelled), "{err:?}");
        assert_eq!(model.calls.borrow().len(), 1);
    }

    /// Every `(index, total)` the run reported, in order.
    fn reports(
        model: &mut Hears,
        seconds: f32,
        cancel: &dyn Fn() -> bool,
    ) -> (Result<Chunked>, Vec<(usize, usize)>) {
        let seen = RefCell::new(Vec::new());
        let out = run_windows_observed(model, &ramp(seconds), None, cancel, &|index, total| {
            seen.borrow_mut().push((index, total))
        });
        (out, seen.into_inner())
    }

    #[test]
    fn each_window_is_reported_before_it_is_decoded() {
        let mut model = Hears::new();
        let (out, seen) = reports(&mut model, 75.0, &never);
        out.unwrap();
        assert_eq!(seen, vec![(0, 3), (1, 3), (2, 3)], "75 s is three windows");
        // One report per decoded window, and the window it names is the one
        // the model was then asked for.
        assert_eq!(model.calls.borrow().len(), seen.len());
    }

    #[test]
    fn a_short_recording_is_one_window_of_one() {
        let mut model = Hears::new();
        let (out, seen) = reports(&mut model, 5.0, &never);
        out.unwrap();
        assert_eq!(seen, vec![(0, 1)]);
    }

    #[test]
    fn the_total_may_grow_when_a_window_ends_early_but_never_falls_behind_the_index() {
        // A decoder that did not finish loses its last word to the next
        // window, which then starts before the 30 s mark: more windows than
        // length over 30 s.
        let mut model = Hears::new();
        model.finished = false;
        let (out, seen) = reports(&mut model, 90.0, &never);
        out.unwrap();
        assert!(
            seen.len() > 3,
            "90 s is three windows of 30 s, and took more: {seen:?}"
        );
        assert_eq!(seen[0], (0, 3), "{seen:?}");
        assert!(seen.last().unwrap().1 > 3, "the estimate grew: {seen:?}");
        for (n, (index, total)) in seen.iter().enumerate() {
            assert_eq!(*index, n, "the index counts decoded windows: {seen:?}");
            assert!(total > index, "{seen:?}");
        }
        let totals: Vec<usize> = seen.iter().map(|s| s.1).collect();
        assert!(totals.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    }

    #[test]
    fn nothing_is_reported_for_the_window_a_cancel_stops() {
        let asked = Cell::new(0);
        let cancel = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };
        let mut model = Hears::new();
        let (out, seen) = reports(&mut model, 75.0, &cancel);
        assert!(matches!(out, Err(WhisperError::Cancelled)));
        assert_eq!(seen, vec![(0, 3)]);
    }

    #[test]
    fn a_window_that_never_gets_anywhere_still_ends() {
        // A model that hears one word ending at 0.1 s and gives up each time
        // (not finished): the next window must still start later, and the run
        // must end. (`a_hallucinating_window_costs_a_whole_window...` has the
        // arithmetic.)
        struct Stuck(usize);
        impl WindowTranscriber for Stuck {
            fn window(&mut self, _: &[f32], _: Option<u32>) -> Result<WindowResult> {
                self.0 += 1;
                Ok(WindowResult {
                    words: vec![
                        Word {
                            text: "a".into(),
                            start_s: 0.0,
                            end_s: 0.1,
                            confidence: 1.0,
                        },
                        Word {
                            text: "b".into(),
                            start_s: 0.1,
                            end_s: 0.2,
                            confidence: 1.0,
                        },
                    ],
                    language: 1,
                    speech: true,
                    finished: false,
                })
            }
        }
        let mut model = Stuck(0);
        let out = run_windows(&mut model, &ramp(60.0), None, &never).unwrap();
        assert_eq!(model.0, 2, "a whole window each time");
        assert_eq!(out.windows, model.0);
    }

    #[test]
    fn a_lone_cut_word_late_in_the_window_is_heard_from_its_start() {
        // Silence, then a word that starts at 29.0 s and runs past the edge.
        struct Late;
        impl WindowTranscriber for Late {
            fn window(&mut self, audio: &[f32], _: Option<u32>) -> Result<WindowResult> {
                let start = audio[0] / RATE as f32;
                let len = audio.len() as f32 / RATE as f32;
                let words = if start < 29.0 {
                    vec![Word {
                        text: "late".into(),
                        start_s: 29.0 - start,
                        end_s: len,
                        confidence: 1.0,
                    }]
                } else {
                    vec![Word {
                        text: "late".into(),
                        start_s: 0.0,
                        end_s: 1.2,
                        confidence: 1.0,
                    }]
                };
                Ok(WindowResult {
                    words,
                    language: 1,
                    speech: true,
                    finished: true,
                })
            }
        }
        let out = run_windows(&mut Late, &ramp(40.0), None, &never).unwrap();
        assert_eq!(texts(&out.words), vec!["late"]);
        assert!(
            (out.words[0].start_s - 29.0).abs() < 1e-3,
            "{:?}",
            out.words
        );
        assert_eq!(out.windows, 2);
    }

    #[test]
    fn a_hallucinating_window_costs_a_whole_window_not_half_a_second() {
        // A model that writes two words in the first 0.2 s of every window and
        // then gives up (not finished). The kept word ends at 0.1 s, and a
        // window must not start 0.5 s on from it, sixty times over: it
        // advances a whole window, as `whisper.transcribe` does when no word
        // ends past the start.
        struct Hallucinates(usize);
        impl WindowTranscriber for Hallucinates {
            fn window(&mut self, _: &[f32], _: Option<u32>) -> Result<WindowResult> {
                self.0 += 1;
                let word = |text: &str, start_s: f32| Word {
                    text: text.into(),
                    start_s,
                    end_s: start_s + 0.1,
                    confidence: 1.0,
                };
                Ok(WindowResult {
                    words: vec![word("a", 0.0), word("b", 0.1)],
                    language: 1,
                    speech: true,
                    finished: false,
                })
            }
        }
        let mut model = Hallucinates(0);
        let out = run_windows(&mut model, &ramp(60.0), None, &never).unwrap();
        // [0, 30) then [30, 60), which is the last and keeps both its words.
        assert_eq!(model.0, 2, "{} windows", model.0);
        assert_eq!(out.windows, 2);
        assert_eq!(texts(&out.words), vec!["a", "a", "b"]);
        let starts: Vec<f32> = out.words.iter().map(|w| w.start_s).collect();
        assert_eq!(starts.len(), 3);
        assert!((starts[0] - 0.0).abs() < 1e-3 && (starts[1] - 30.0).abs() < 1e-3);
    }

    /// Continuous speech: word `k` is spoken from `0.7 k` to `0.7 (k + 1)` s
    /// with no pause, and the aligner puts every boundary between two words
    /// [`EARLY`] seconds before the word before has finished (measured on the
    /// real model: 0.14 s). `texts` says what each word is.
    struct Continuous {
        texts: Vec<String>,
        /// The least of a word's tail the model still writes down. A window
        /// that starts inside a word hears no more of it than that.
        least_tail: f32,
        calls: Vec<f32>,
    }

    const EARLY: f32 = 0.14;

    impl Continuous {
        fn new(least_tail: f32) -> Self {
            Self {
                texts: (0..200).map(|k| format!("w{k}")).collect(),
                least_tail,
                calls: Vec::new(),
            }
        }
    }

    impl WindowTranscriber for Continuous {
        fn window(&mut self, audio: &[f32], _: Option<u32>) -> Result<WindowResult> {
            let from = audio[0] / RATE as f32;
            let len = audio.len() as f32 / RATE as f32;
            self.calls.push(from);
            let mut words: Vec<Word> = Vec::new();
            for k in 0..self.texts.len() {
                let (start, end) = (0.7 * k as f32, 0.7 * (k + 1) as f32);
                if end - from < self.least_tail {
                    continue; // over before the window began
                }
                if start >= from + len {
                    break;
                }
                // The first word of a window starts with it; any other starts
                // where the aligner puts the boundary.
                let start_s = if words.is_empty() {
                    0.0
                } else {
                    (start - EARLY - from).max(0.0)
                };
                let end_s = (end - EARLY - from).clamp(start_s + 0.02, len);
                words.push(Word {
                    text: self.texts[k].clone(),
                    start_s,
                    end_s,
                    confidence: 0.9,
                });
            }
            Ok(WindowResult {
                speech: !words.is_empty(),
                words,
                language: 7,
                finished: true,
            })
        }
    }

    #[test]
    fn the_tail_of_a_kept_word_is_not_heard_as_a_word_of_its_own() {
        // The aligner ends a word 0.14 s before it has finished when the next
        // follows with no pause, so a window that starts where the last kept
        // word ends starts inside that word's tail, and the model, which writes
        // down any tail of 0.1 s or more, writes the word again.
        let mut model = Continuous::new(0.1);
        let out = run_windows(&mut model, &ramp(100.0), None, &never).unwrap();

        // Words 0..=142 start before 100 s (word 142 at 99.4 s).
        let expected: Vec<String> = (0..=142).map(|k| format!("w{k}")).collect();
        let got: Vec<String> = out.words.iter().map(|w| w.text.clone()).collect();
        assert_eq!(got, expected, "each word once, in order");
        assert!(out.windows >= 4, "the recording has more than one window");
        for pair in out.words.windows(2) {
            assert!(pair[0].start_s <= pair[1].start_s, "{pair:?}");
        }
    }

    #[test]
    fn a_word_said_twice_in_a_row_is_not_taken_for_a_tail() {
        // "no no" where the window cuts: the second is cut off, dropped, and
        // heard whole in the next window, where it has the first one's text.
        // Here the model hears none of the first one's tail (it needs 0.2 s),
        // so the word at the start of the second window is the real second
        // "no", and has to stay.
        let mut model = Continuous::new(0.2);
        model.texts[41] = "no".into();
        model.texts[42] = "no".into();
        let out = run_windows(&mut model, &ramp(50.0), None, &never).unwrap();
        let got: Vec<&str> = out.words.iter().map(|w| w.text.as_str()).collect();
        let expected: Vec<String> = (0..=71)
            .map(|k| {
                if k == 41 || k == 42 {
                    "no".into()
                } else {
                    format!("w{k}")
                }
            })
            .collect();
        assert_eq!(got, expected, "both of the words, each once");
        // And the window really did start where the second one was cut.
        assert!(model.calls.len() >= 2, "{:?}", model.calls);
    }

    #[test]
    fn cutting_rules_on_their_own() {
        let w = |t: &str, s: f32, e: f32| Word {
            text: t.into(),
            start_s: s,
            end_s: e,
            confidence: 1.0,
        };
        let cut = |words: Vec<Word>, finished: bool| cut_window(words, 30.0, finished);
        let kept = |c: &Cut| c.keep.len();
        // No words: pass over the whole window.
        let c = cut(Vec::new(), true);
        assert_eq!((kept(&c), c.next, c.dropped), (0, None, None));
        // Last word well inside: keep all, resume at its end.
        let c = cut(vec![w("a", 1.0, 2.0), w("b", 3.0, 20.0)], true);
        assert_eq!((kept(&c), c.next, c.dropped), (2, Some(20.0), None));
        // Last word at the edge: drop it, resume at its start, which is the
        // later of that and the previous word's end.
        let c = cut(vec![w("a", 1.0, 2.0), w("b", 3.0, 29.9)], true);
        assert_eq!((texts(&c.keep), c.next), (vec!["a"], Some(3.0)));
        assert_eq!(c.dropped.map(|d| d.text), Some("b".to_string()));
        // ...unless the previous word's end is the later (the aligner's guess
        // for the last word of a window is rough).
        let c = cut(vec![w("a", 1.0, 4.0), w("b", 3.0, 29.9)], true);
        assert_eq!((texts(&c.keep), c.next), (vec!["a"], Some(4.0)));
        // Decoder gave up: the last word goes even if it ends early.
        let c = cut(vec![w("a", 1.0, 2.0), w("b", 3.0, 10.0)], false);
        assert_eq!((texts(&c.keep), c.next), (vec!["a"], Some(3.0)));
        // Only a cut word, started early: nothing is learned, move on a whole window.
        let c = cut(vec![w("a", 0.2, 30.0)], true);
        assert_eq!((kept(&c), c.next), (0, None));
        // Only a cut word, started late: hear it from its start.
        let c = cut(vec![w("a", 20.0, 30.0)], true);
        assert_eq!((kept(&c), c.next), (0, Some(20.0)));
    }

    #[test]
    fn no_word_ending_past_the_minimum_means_a_whole_window() {
        let w = |t: &str, s: f32, e: f32| Word {
            text: t.into(),
            start_s: s,
            end_s: e,
            confidence: 1.0,
        };
        // Kept words that end within MIN_ADVANCE_S of the start: a whole window,
        // not 0.5 s on.
        let c = cut_window(vec![w("a", 0.0, 0.3)], 30.0, true);
        assert_eq!((c.keep.len(), c.next), (1, None));
        let c = cut_window(vec![w("a", 0.0, 0.1), w("b", 0.1, 0.2)], 30.0, false);
        assert_eq!((c.keep.len(), c.next), (1, None));
        // The boundary itself is "not past" it, and a hair later is.
        let c = cut_window(vec![w("a", 0.0, MIN_ADVANCE_S)], 30.0, true);
        assert_eq!(c.next, None);
        let c = cut_window(vec![w("a", 0.0, MIN_ADVANCE_S + 0.01)], 30.0, true);
        assert_eq!(c.next, Some(MIN_ADVANCE_S + 0.01));
    }

    #[test]
    fn a_tail_is_a_repeat_of_the_last_kept_word_that_starts_where_it_ended() {
        let w = |t: &str, s: f32, e: f32| Word {
            text: t.into(),
            start_s: s,
            end_s: e,
            confidence: 1.0,
        };
        let kept = w("friend,", 28.0, 29.0);
        // The same word, from where the kept one ended: its tail.
        assert!(is_tail_of(&kept, &w("Friend", 29.0, 29.05), None));
        assert!(is_tail_of(
            &kept,
            &w("friend", 29.0 + DUPLICATE_S, 29.4),
            None
        ));
        // Later than that, or another word: not.
        assert!(!is_tail_of(
            &kept,
            &w("friend", 29.0 + DUPLICATE_S + 0.01, 29.5),
            None
        ));
        assert!(!is_tail_of(&kept, &w("friends", 29.0, 29.05), None));
        // Punctuation alone is not a word to compare.
        assert!(!is_tail_of(&w(",", 1.0, 1.1), &w(".", 1.1, 1.2), None));
        // The word the last window cut off, heard whole, is not a tail even
        // when it is the same word.
        let cut = w("friend", 29.0, 30.0);
        assert!(!is_tail_of(&kept, &w("friend", 29.0, 29.5), Some(&cut)));
        assert!(is_tail_of(
            &kept,
            &w("friend", 29.0, 29.5),
            Some(&w("other", 29.0, 30.0))
        ));
    }
}
