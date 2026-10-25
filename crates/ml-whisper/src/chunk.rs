//! Long audio, one 30 s window at a time (plan step 5 on #384, #659).
//!
//! The encoder takes exactly 30 s. A recording longer than that is cut into
//! windows, and the rule for where the next one starts is the whole of this
//! module. It is `whisper.transcribe`'s, and the window decides it, from the
//! segment timestamps the model wrote (`segments.rs`):
//!
//! * a window is up to 30 s of audio from `seek`;
//! * a window that reaches the end of the audio keeps every word;
//! * any other window's result says where the next one starts
//!   ([`WindowResult::next`], seconds from its start): the end of its last
//!   word, or, if the model closed its last segment, `None`, a whole window
//!   on. Text the model had not finished (a segment with no closing
//!   timestamp) is not in the result, and the next window hears it from the
//!   start of its first word;
//! * the next window starts where the result says, however little that is (at
//!   least one frame). `whisper.transcribe` seeks to `last_word_end` whenever
//!   it is past the window's start ([`transcribe.py` lines 401 to 416][seek]),
//!   with no floor, so a window whose only finished word is a short one at its
//!   start is followed by a window that starts just after it, and the text the
//!   model had not finished is heard again. Upstream has nothing to stop a
//!   window that finishes the same hallucinated word at its start from being
//!   decoded again 0.2 s on, over and over; this port does: more than
//!   [`MAX_SMALL_ADVANCES`] advances of [`MIN_ADVANCE_S`] or less in a row, and
//!   the next window starts a **whole window** on;
//! * a result with no start for the next window (the model closed its last
//!   segment, or the window decoded to nothing) also moves on a whole window;
//!
//! This replaced a rule of this module's own (#659). Without timestamp tokens a
//! window had no way to say which word it had finished, so a word within half
//! a second of the edge, or one the decoder had not finished, was dropped and
//! heard again, and a first word that repeated the last kept one was dropped as
//! its tail. Both are gone: the timestamps say where a segment ends.
//!
//! Word times in a window are relative to its start; they are offset by
//! `seek` here. The language is fixed by the first window that has speech and
//! then given to every later one, as `whisper.transcribe` does, so a window of
//! music cannot flip the language of the rest. So is the end of the last speech,
//! which the next window's pause rule needs (`segments::place_words`).
//!
//! The windows themselves are decoded by a [`WindowTranscriber`], which the
//! model implements and the tests fake: the rule above is checked on 75 s of
//! made-up audio in CI.
//!
//! [seek]: https://github.com/openai/whisper/blob/31243bad24cc746f07d4c8bfdd2d974872cb1803/whisper/transcribe.py#L401-L416

use crate::mel::{N_SAMPLES, SAMPLE_RATE};
use crate::{Result, WhisperError, Word};

/// Samples in a full window: 30 s at 16 kHz.
pub(crate) const WINDOW_SAMPLES: usize = N_SAMPLES;
/// A window shorter than this (0.1 s) is not worth an encoder pass.
const MIN_WINDOW_SAMPLES: usize = SAMPLE_RATE / 10;
/// An advance of this many seconds or less is a small one ([`MAX_SMALL_ADVANCES`]).
pub(crate) const MIN_ADVANCE_S: f32 = 0.5;
/// The most small advances in a row; the next one is a whole window.
pub(crate) const MAX_SMALL_ADVANCES: usize = 3;
/// The least a window advances when it advances at all: one encoder frame.
const MIN_STEP_S: f32 = 0.02;

/// What one window is asked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WindowRequest {
    /// The language token to use, or `None` to detect it.
    pub language: Option<u32>,
    /// Where the speech before this window ended, in seconds from this
    /// window's start (negative if it ended before the window began).
    pub last_speech_s: f32,
    /// The window reaches the end of the audio, so there is no later window to
    /// hear what it left unfinished.
    pub to_end: bool,
}

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
    /// Where the next window starts, in seconds from this window's start;
    /// `None` is a whole window on.
    pub next: Option<f32>,
    /// Where the last segment that has words ended, in seconds from this
    /// window's start, if any has.
    pub last_speech_s: Option<f32>,
}

/// Decodes one window of 16 kHz mono audio, up to 30 s.
pub(crate) trait WindowTranscriber {
    fn window(&mut self, audio: &[f32], request: WindowRequest) -> Result<WindowResult>;
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
/// full 30 s. An estimate that can only grow: a window that ends at the last
/// finished word starts the next one earlier than 30 s on, so the audio takes
/// more windows than its length over 30 s.
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
    // Where the speech so far ended, in seconds from the start of the audio.
    let mut last_speech_s = 0.0f32;
    // Advances of MIN_ADVANCE_S or less since the last bigger one.
    let mut small = 0usize;
    while total - seek >= MIN_WINDOW_SAMPLES {
        if cancel() {
            return Err(WhisperError::Cancelled);
        }
        let end = (seek + WINDOW_SAMPLES).min(total);
        let slice = &audio[seek..end];
        let offset = seek as f32 / rate;
        progress(windows, windows_in_all(windows, total, seek));
        let request = WindowRequest {
            language,
            last_speech_s: last_speech_s - offset,
            to_end: end == total,
        };
        let result = transcriber.window(slice, request)?;
        windows += 1;
        if result.speech && language.is_none() {
            language = Some(result.language);
        }
        words.extend(result.words.into_iter().map(|w| Word {
            start_s: w.start_s + offset,
            end_s: w.end_s + offset,
            ..w
        }));
        if let Some(s) = result.last_speech_s {
            last_speech_s = s + offset;
        }
        if end == total {
            break;
        }
        seek = match result.next.filter(|&s| s > 0.0) {
            Some(s) => {
                let s = s.max(MIN_STEP_S);
                small = if s > MIN_ADVANCE_S { 0 } else { small + 1 };
                if small > MAX_SMALL_ADVANCES {
                    small = 0;
                    end
                } else {
                    // A word's end can lie past the window's if the aligner
                    // overshoots; the window's end is as far as it can go.
                    seek + ((s * rate).round() as usize).min(end - seek)
                }
            }
            None => {
                small = 0;
                end
            }
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

    /// One call a fake saw: where the window started and what it was asked.
    type Call = (f32, WindowRequest);

    /// A fake model that hears the words above, the way Whisper hears them in
    /// timestamp mode: a word that ends inside the window is finished and in the
    /// result, and the next window starts at the last one's end; a word that
    /// runs past the edge is left out (the model has not closed its segment),
    /// unless the window reaches the end of the audio, where the model hears it
    /// as ending there.
    struct Hears {
        /// Seconds before which nothing is heard (music, silence).
        speech_from: f32,
        calls: RefCell<Vec<Call>>,
        /// The model closes its last segment: the next window is a whole one on.
        closes: bool,
    }

    impl Hears {
        fn new() -> Self {
            Self {
                speech_from: 0.0,
                calls: RefCell::new(Vec::new()),
                closes: false,
            }
        }
    }

    impl WindowTranscriber for Hears {
        fn window(&mut self, audio: &[f32], request: WindowRequest) -> Result<WindowResult> {
            let start = audio[0] / RATE as f32;
            let len = audio.len() as f32 / RATE as f32;
            self.calls.borrow_mut().push((start, request));
            let mut words = Vec::new();
            for k in 0.. {
                let (s, e) = word_span(k);
                if s >= start + len {
                    break;
                }
                if s < start.max(self.speech_from) {
                    continue;
                }
                let cut = e > start + len - 0.01;
                if cut && !request.to_end {
                    break;
                }
                words.push(Word {
                    text: format!("w{k}"),
                    start_s: s - start,
                    end_s: (e - start).min(len),
                    confidence: 0.9,
                });
            }
            let last = words.last().map(|w| w.end_s);
            Ok(WindowResult {
                speech: !words.is_empty(),
                language: 7,
                next: if self.closes { None } else { last },
                last_speech_s: last,
                words,
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

        // Window 1 [0, 30): word 14 ends at 29.4, the last that ends inside it,
        // so the next window starts there. Window 2 [29.4, 59.4): word 29 ends
        // at 59.4, the edge, so it is not finished; word 28 ends at 57.4. Window
        // 3 [57.4, 75) reaches the end of the audio.
        let starts: Vec<f32> = model.calls.borrow().iter().map(|c| c.0).collect();
        assert_eq!(starts.len(), 3, "{starts:?}");
        assert_eq!(out.windows, 3);
        for (got, want) in starts.iter().zip([0.0, 29.4, 57.4]) {
            assert!((got - want).abs() < 1e-3, "{starts:?}");
        }
        let to_end: Vec<bool> = model.calls.borrow().iter().map(|c| c.1.to_end).collect();
        assert_eq!(to_end, vec![false, false, true]);
    }

    #[test]
    fn a_window_the_model_closed_is_followed_by_a_whole_one() {
        let mut model = Hears::new();
        model.closes = true;
        let out = run_windows(&mut model, &ramp(75.0), None, &never).unwrap();
        let starts: Vec<f32> = model.calls.borrow().iter().map(|c| c.0).collect();
        for (got, want) in starts.iter().zip([0.0, 30.0, 60.0]) {
            assert!((got - want).abs() < 1e-3, "{starts:?}");
        }
        assert_eq!(out.windows, 3);
        // A word that crosses 30 s is in no window whole, and so is missing: the
        // price of a model that says it has finished.
        assert!(out.words.len() >= 35, "{}", out.words.len());
    }

    #[test]
    fn the_end_of_the_speech_so_far_is_given_to_the_next_window_in_its_own_seconds() {
        let mut model = Hears::new();
        run_windows(&mut model, &ramp(75.0), None, &never).unwrap();
        let calls = model.calls.borrow();
        // Nothing before the first window.
        assert_eq!(calls[0].1.last_speech_s, 0.0);
        // Window 1 ended its speech at 29.4 s and window 2 starts there.
        assert!(calls[1].1.last_speech_s.abs() < 1e-3, "{:?}", calls[1].1);
        // Window 2's last word ended at 57.4 s (its seconds 28.0); window 3
        // starts there too.
        assert!(calls[2].1.last_speech_s.abs() < 1e-3, "{:?}", calls[2].1);
    }

    #[test]
    fn a_window_that_starts_before_the_last_speech_ended_sees_it_as_negative_time() {
        // The first window says the speech ended at 20 s but the next starts at
        // 12 s: it ended 8 s into the next one.
        struct Overlap(Vec<f32>);
        impl WindowTranscriber for Overlap {
            fn window(&mut self, _: &[f32], request: WindowRequest) -> Result<WindowResult> {
                self.0.push(request.last_speech_s);
                Ok(WindowResult {
                    words: Vec::new(),
                    language: 1,
                    speech: true,
                    next: Some(12.0),
                    last_speech_s: Some(20.0),
                })
            }
        }
        let mut model = Overlap(Vec::new());
        run_windows(&mut model, &ramp(40.0), None, &never).unwrap();
        assert!((model.0[1] - 8.0).abs() < 1e-3, "{:?}", model.0);
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
            calls.iter().map(|c| c.1.language).collect::<Vec<_>>(),
            vec![None, None, None]
        );
        assert_eq!(out.language, Some(7));
        assert_eq!(texts(&out.words), vec!["w35", "w36", "w37"]);
    }

    #[test]
    fn the_language_of_the_first_window_with_speech_is_given_to_the_rest() {
        let mut model = Hears::new();
        run_windows(&mut model, &ramp(75.0), None, &never).unwrap();
        let langs: Vec<_> = model.calls.borrow().iter().map(|c| c.1.language).collect();
        assert_eq!(langs, vec![None, Some(7), Some(7)]);

        let mut model = Hears::new();
        run_windows(&mut model, &ramp(75.0), Some(3), &never).unwrap();
        let langs: Vec<_> = model.calls.borrow().iter().map(|c| c.1.language).collect();
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
        model.closes = true; // whole windows: 75 s is exactly three
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
        // A window that ends at its last finished word starts the next one
        // before the 30 s mark: more windows than length over 30 s.
        let mut model = Hears::new();
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

    /// A model that writes two words in the first 0.2 s of every window and
    /// says the next window starts at the end of the second. It records where
    /// each window began.
    struct Hallucinates(Vec<f32>);

    impl WindowTranscriber for Hallucinates {
        fn window(&mut self, audio: &[f32], _: WindowRequest) -> Result<WindowResult> {
            self.0.push(audio[0] / RATE as f32);
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
                next: Some(0.2),
                last_speech_s: Some(0.2),
            })
        }
    }

    #[test]
    fn a_window_that_keeps_finishing_the_same_word_at_its_start_is_not_decoded_sixty_times() {
        // The next window would start 0.2 s on, every time. Upstream would do
        // that for as long as the audio lasts; this stops after
        // MAX_SMALL_ADVANCES of them in a row and moves a whole window.
        let mut model = Hallucinates(Vec::new());
        let out = run_windows(&mut model, &ramp(60.0), None, &never).unwrap();
        let want = [0.0, 0.2, 0.4, 0.6, 30.6];
        assert_eq!(model.0.len(), want.len(), "{:?}", model.0);
        for (got, want) in model.0.iter().zip(want) {
            assert!((got - want).abs() < 1e-3, "{:?}", model.0);
        }
        assert_eq!(out.windows, want.len());
    }

    #[test]
    fn a_short_word_at_the_start_of_a_window_moves_the_next_window_just_past_it() {
        // Upstream's seek has no floor: one finished word that ends 0.3 s in
        // is followed by a window that starts 0.3 s in, and whatever the model
        // had not finished is heard again. It used to be a whole window, and
        // 30 s of speech went with it.
        struct OneShort(Vec<f32>);
        impl WindowTranscriber for OneShort {
            fn window(&mut self, audio: &[f32], _: WindowRequest) -> Result<WindowResult> {
                let start = audio[0] / RATE as f32;
                self.0.push(start);
                let words = if start == 0.0 {
                    vec![Word {
                        text: "yes".into(),
                        start_s: 0.0,
                        end_s: 0.3,
                        confidence: 1.0,
                    }]
                } else {
                    Vec::new()
                };
                Ok(WindowResult {
                    language: 1,
                    speech: true,
                    next: Some(0.3),
                    last_speech_s: Some(0.3),
                    words,
                })
            }
        }
        let mut model = OneShort(Vec::new());
        run_windows(&mut model, &ramp(40.0), None, &never).unwrap();
        assert!((model.0[1] - 0.3).abs() < 1e-3, "{:?}", model.0);
    }

    #[test]
    fn a_next_start_of_nothing_is_a_whole_window_and_a_tiny_one_is_a_frame() {
        struct Says(Option<f32>, Vec<f32>);
        impl WindowTranscriber for Says {
            fn window(&mut self, audio: &[f32], _: WindowRequest) -> Result<WindowResult> {
                self.1.push(audio[0] / RATE as f32);
                Ok(WindowResult {
                    words: Vec::new(),
                    language: 1,
                    speech: true,
                    next: self.0,
                    last_speech_s: None,
                })
            }
        }
        for next in [None, Some(0.0), Some(-1.0)] {
            let mut model = Says(next, Vec::new());
            run_windows(&mut model, &ramp(40.0), None, &never).unwrap();
            assert!((model.1[1] - 30.0).abs() < 1e-3, "{next:?}: {:?}", model.1);
        }
        let mut model = Says(Some(0.001), Vec::new());
        run_windows(&mut model, &ramp(40.0), None, &never).unwrap();
        assert!((model.1[1] - MIN_STEP_S).abs() < 1e-3, "{:?}", model.1);
    }

    #[test]
    fn a_result_that_starts_the_next_window_past_the_end_is_held_to_the_window() {
        // The aligner overshoots: the next window cannot start after this one ends.
        struct Over(Vec<f32>);
        impl WindowTranscriber for Over {
            fn window(&mut self, audio: &[f32], _: WindowRequest) -> Result<WindowResult> {
                self.0.push(audio[0] / RATE as f32);
                Ok(WindowResult {
                    words: Vec::new(),
                    language: 1,
                    speech: true,
                    next: Some(45.0),
                    last_speech_s: None,
                })
            }
        }
        let mut model = Over(Vec::new());
        let out = run_windows(&mut model, &ramp(70.0), None, &never).unwrap();
        assert_eq!(model.0.len(), 3, "{:?}", model.0);
        assert!((model.0[1] - 30.0).abs() < 1e-3 && (model.0[2] - 60.0).abs() < 1e-3);
        assert_eq!(out.windows, 3);
    }

    #[test]
    fn the_first_request_is_the_given_language_and_no_speech_yet() {
        let mut model = Hears::new();
        run_windows(&mut model, &ramp(5.0), Some(4), &never).unwrap();
        let calls = model.calls.borrow();
        assert_eq!(
            calls[0].1,
            WindowRequest {
                language: Some(4),
                last_speech_s: 0.0,
                to_end: true
            }
        );
    }
}
