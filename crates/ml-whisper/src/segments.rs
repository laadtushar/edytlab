//! Segments from the timestamp tokens, and where the words go (#659).
//!
//! A window decoded in Whisper's timestamp mode ([`crate::decode::DecodeConfig::with_timestamps`])
//! is a run of segments, each `<|start|> text <|end|>`, with a timestamp token
//! every 20 ms. This module is a port of what `whisper/transcribe.py` and
//! `whisper/timing.py` in <https://github.com/openai/whisper> (MIT), release
//! 20250625 (commit `31243bad24cc746f07d4c8bfdd2d974872cb1803`), do with them
//! when `word_timestamps=True`:
//!
//! * [`split_segments`] is the block of `transcribe()` that cuts the sampled
//!   tokens into segments at every pair of consecutive timestamps
//!   ([`transcribe.py` lines 339 to 416][transcribe]), and says where the next
//!   window would start from the timestamps alone;
//! * [`place_words`] is `add_word_timestamps` after `find_alignment`
//!   ([`timing.py` lines 279 to 388][timing]): words that run on across a
//!   sentence end or a pause are cut back to twice the median word, and the
//!   first word of a segment starts at the segment's own start when the aligner
//!   put it more than half a second early.
//!
//! The aligner (`timing.rs`) lays words end to end, so silence belongs to a word
//! and, measured on the real model, all of it to the word **after** it: the
//! first word of a window began at 0.0 s whatever the lead-in, and a word after
//! a 6.7 s pause began 6.7 s early. What the timestamps give is where the speech
//! before a window ended (the pause rule of [`place_words`] needs it) and where
//! the last finished word ended (where the next window starts). What they do not
//! give, on the pinned `whisper-base`, is where speech starts after a pause: its
//! segments run end to end, each starting where the one before it ended and the
//! first at 0.0 s, so the rule that moves a first word to its segment's start
//! never fired on the recordings measured (#659), and `refine.rs` still does
//! that part.
//!
//! [transcribe]: https://github.com/openai/whisper/blob/31243bad24cc746f07d4c8bfdd2d974872cb1803/whisper/transcribe.py#L339-L416
//! [timing]: https://github.com/openai/whisper/blob/31243bad24cc746f07d4c8bfdd2d974872cb1803/whisper/timing.py#L279-L388

use std::ops::Range;

use crate::timing::{merge_punctuations, WordTiming, SECONDS_PER_FRAME};
use crate::Word;

/// Characters that end a sentence: upstream's `sentence_end_marks`.
const SENTENCE_END_MARKS: &str = ".。!！?？";
/// Upstream's cap on the median word duration, in seconds.
const MAX_MEDIAN_S: f32 = 0.7;

/// One segment of a decoded window.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Segment {
    /// Seconds from the window's start.
    pub start_s: f32,
    pub end_s: f32,
    /// The segment's tokens: indices into the decoded tokens, timestamps
    /// included.
    pub tokens: Range<usize>,
}

/// A decoded window cut into segments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Split {
    pub segments: Vec<Segment>,
    /// The output ends `text <|end|>` with a lone timestamp: the model closed
    /// its last segment, and there is nothing after it. Upstream then moves
    /// on a whole window.
    pub single_timestamp_ending: bool,
    /// Where upstream's `seek` lands from the timestamps alone, in seconds
    /// from the window's start: the end of the last finished segment. `None`
    /// is a whole window (the output was closed, or it had no pair of
    /// timestamps at all).
    pub seek_s: Option<f32>,
}

/// Cut the sampled `tokens` of one window (`window_s` seconds long) into
/// segments.
///
/// A segment ends where two timestamps follow each other: `<|end|><|start|>`.
/// What follows the last such pair is an **unfinished** segment, text the
/// model did not close, and upstream leaves it out and decodes it again from
/// the end of the last finished one. `keep_tail` keeps it as a segment of its
/// own, for the last window of a recording, where there is no later window to
/// hear it in.
///
/// With no pair at all, the whole output is one segment from the window's
/// start to its last timestamp (or the window's end if it has none past
/// `<|0.00|>`).
pub(crate) fn split_segments(
    tokens: &[u32],
    timestamp_begin: u32,
    window_s: f32,
    keep_tail: bool,
) -> Split {
    let n = tokens.len();
    let is_stamp = |t: u32| t >= timestamp_begin;
    let at = |t: u32| t.saturating_sub(timestamp_begin) as f32 * SECONDS_PER_FRAME;
    let single = n >= 2 && !is_stamp(tokens[n - 2]) && is_stamp(tokens[n - 1]);

    // The index of the second timestamp of each pair: where a segment ends.
    let mut slices: Vec<usize> = (0..n.saturating_sub(1))
        .filter(|&i| is_stamp(tokens[i]) && is_stamp(tokens[i + 1]))
        .map(|i| i + 1)
        .collect();

    if slices.is_empty() {
        if n == 0 {
            return Split {
                segments: Vec::new(),
                single_timestamp_ending: false,
                seek_s: None,
            };
        }
        let end_s = tokens
            .iter()
            .rev()
            .find(|&&t| is_stamp(t))
            .filter(|&&t| t != timestamp_begin)
            .map_or(window_s, |&t| at(t));
        return Split {
            segments: vec![Segment {
                start_s: 0.0,
                end_s,
                tokens: 0..n,
            }],
            single_timestamp_ending: single,
            seek_s: None,
        };
    }

    if single {
        slices.push(n);
    }
    let mut segments = Vec::with_capacity(slices.len() + 1);
    let mut last = 0;
    for current in slices {
        let first = tokens[last];
        // The first token of a segment is a timestamp (the decode rules make
        // the first one so, and every later one follows a pair); a decoder
        // that did not would still get a segment, starting where the last ended.
        let start_s = if is_stamp(first) {
            at(first)
        } else {
            segments.last().map_or(0.0, |s: &Segment| s.end_s)
        };
        segments.push(Segment {
            start_s,
            end_s: at(tokens[current - 1]),
            tokens: last..current,
        });
        last = current;
    }
    let seek_s = (!single).then(|| at(tokens[last - 1]));
    if keep_tail && !single && last < n {
        let first = tokens[last];
        let start_s = if is_stamp(first) {
            at(first)
        } else {
            segments.last().map_or(0.0, |s| s.end_s)
        };
        segments.push(Segment {
            start_s,
            end_s: window_s,
            tokens: last..n,
        });
    }
    Split {
        segments,
        single_timestamp_ending: single,
        seek_s,
    }
}

/// How many text tokens a segment holds, for [`place_words`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SegmentText {
    pub start_s: f32,
    pub end_s: f32,
    /// Tokens under `<|endoftext|>`: what the aligner was given for it.
    pub text_tokens: usize,
}

/// What [`place_words`] made of a window.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    /// In order, seconds from the window's start, inside it.
    pub words: Vec<Word>,
    /// Where the last word ends (upstream's `get_end`), if there is one. The
    /// speech before the next window ends there, and so does the audio the
    /// next window can skip.
    pub last_word_end: Option<f32>,
}

/// The median of `values`, the mean of the two middle ones for an even count.
fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

/// The text of the segments of a decoded window, which is what the aligner is
/// given: the tokens under `<|endoftext|>` (a timestamp, language or task token
/// is not text and has no bytes to spell; upstream's `token < tokenizer.eot`),
/// and how many each segment holds.
pub(crate) fn text_of_segments(
    segments: &[Segment],
    tokens: &[u32],
    eot: u32,
) -> (Vec<u32>, Vec<SegmentText>) {
    let mut text = Vec::new();
    let mut spans = Vec::with_capacity(segments.len());
    for segment in segments {
        let before = text.len();
        text.extend(tokens[segment.tokens.clone()].iter().filter(|&&t| t < eot));
        spans.push(SegmentText {
            start_s: segment.start_s,
            end_s: segment.end_s,
            text_tokens: text.len() - before,
        });
    }
    (text, spans)
}

/// A word being placed: `WordTiming` with the text trimmed.
struct Placing {
    text: String,
    start_s: f32,
    end_s: f32,
    probability: f32,
}

/// Upstream's `add_word_timestamps`, from the `find_alignment` result on.
///
/// `alignment` is the aligned words of the window's text tokens, in order,
/// **before** punctuation is attached; `segments` say how many of those tokens
/// each segment holds. `last_speech_s` is where the speech before this window
/// ended, in this window's seconds (negative if it ended before the window).
/// Times are clamped into `0..=window_s`, and a word is at least one encoder
/// frame long.
pub(crate) fn place_words(
    mut alignment: Vec<WordTiming>,
    segments: &[SegmentText],
    mut last_speech_s: f32,
    window_s: f32,
    prepend: &str,
    append: &str,
) -> Placed {
    if alignment.is_empty() {
        return Placed {
            words: Vec::new(),
            last_word_end: None,
        };
    }
    let mut durations: Vec<f32> = alignment
        .iter()
        .map(|t| t.end_s - t.start_s)
        .filter(|&d| d != 0.0)
        .collect();
    let have_durations = !durations.is_empty();
    let median_s = median(&mut durations).min(MAX_MEDIAN_S);
    let max_s = median_s * 2.0;

    // Words at sentence boundaries are not longer than twice the median word.
    if have_durations {
        for i in 1..alignment.len() {
            if alignment[i].end_s - alignment[i].start_s > max_s {
                if SENTENCE_END_MARKS.contains(alignment[i].text.as_str()) {
                    alignment[i].end_s = alignment[i].start_s + max_s;
                } else if SENTENCE_END_MARKS.contains(alignment[i - 1].text.as_str()) {
                    alignment[i].start_s = alignment[i].end_s - max_s;
                }
            }
        }
    }
    let alignment = merge_punctuations(alignment, prepend, append);

    let mut out: Vec<Word> = Vec::new();
    let mut word_index = 0;
    for segment in segments {
        let mut saved = 0;
        let mut words: Vec<Placing> = Vec::new();
        while word_index < alignment.len() && saved < segment.text_tokens {
            let timing = &alignment[word_index];
            let text = timing.text.trim();
            if !text.is_empty() {
                words.push(Placing {
                    text: text.to_string(),
                    start_s: timing.start_s,
                    end_s: timing.end_s,
                    probability: timing.probability,
                });
            }
            saved += timing.tokens.len();
            word_index += 1;
        }
        if words.is_empty() {
            continue;
        }

        // The first and the second word after a pause are not longer than
        // twice the median word.
        if words[0].end_s - last_speech_s > median_s * 4.0
            && (words[0].end_s - words[0].start_s > max_s
                || (words.len() > 1 && words[1].end_s - words[0].start_s > max_s * 2.0))
        {
            if words.len() > 1 && words[1].end_s - words[1].start_s > max_s {
                let boundary = (words[1].end_s / 2.0).max(words[1].end_s - max_s);
                words[0].end_s = boundary;
                words[1].start_s = boundary;
            }
            words[0].start_s = (words[0].end_s - max_s).max(0.0);
        }

        // The segment's own start, if the first word began long before it.
        if segment.start_s < words[0].end_s && segment.start_s - 0.5 > words[0].start_s {
            words[0].start_s = (words[0].end_s - median_s).min(segment.start_s).max(0.0);
        }

        // The segment's own end, if the last word ran on long past it.
        let last = words.len() - 1;
        let mut seg_end = segment.end_s;
        if seg_end > words[last].start_s && seg_end + 0.5 < words[last].end_s {
            words[last].end_s = (words[last].start_s + median_s).max(seg_end);
        } else {
            seg_end = words[last].end_s;
        }
        last_speech_s = seg_end;

        out.extend(words.into_iter().map(|w| inside(w, window_s)));
    }
    Placed {
        last_word_end: out.last().map(|w| w.end_s),
        words: out,
    }
}

/// A [`Word`] from a placed one: inside the window, and at least one encoder
/// frame long.
fn inside(w: Placing, window_s: f32) -> Word {
    let start_s = w.start_s.clamp(0.0, window_s);
    let end_s = w
        .end_s
        .max(start_s + SECONDS_PER_FRAME)
        .min(window_s.max(start_s + SECONDS_PER_FRAME));
    Word {
        text: w.text,
        start_s,
        end_s,
        confidence: w.probability.clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEGIN: u32 = 1000;
    /// The timestamp token at `seconds`.
    fn ts(seconds: f32) -> u32 {
        BEGIN + (seconds / SECONDS_PER_FRAME).round() as u32
    }

    fn split(tokens: &[u32], keep_tail: bool) -> Split {
        split_segments(tokens, BEGIN, 30.0, keep_tail)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    // ---- split_segments ----------------------------------------------------

    #[test]
    fn segments_end_where_two_timestamps_follow_each_other() {
        // <1.0> a b <3.0> <3.0> c <5.0> <eot>: two segments, closed.
        let tokens = [ts(1.0), 5, 6, ts(3.0), ts(3.0), 7, ts(5.0)];
        let s = split(&tokens, false);
        assert!(s.single_timestamp_ending);
        assert_eq!(s.seek_s, None, "a closed window moves on whole");
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[0].tokens, 0..4);
        assert_eq!(s.segments[1].tokens, 4..7);
        assert!(near(s.segments[0].start_s, 1.0) && near(s.segments[0].end_s, 3.0));
        assert!(near(s.segments[1].start_s, 3.0) && near(s.segments[1].end_s, 5.0));
    }

    #[test]
    fn an_unfinished_last_segment_is_left_for_the_next_window() {
        // <0.0> a <2.0> <2.0> b c   (no closing timestamp)
        let tokens = [ts(0.0), 5, ts(2.0), ts(2.0), 6, 7];
        let s = split(&tokens, false);
        assert!(!s.single_timestamp_ending);
        assert_eq!(s.segments.len(), 1, "b c is not a segment");
        assert_eq!(s.segments[0].tokens, 0..3);
        assert!(near(s.seek_s.unwrap(), 2.0), "{:?}", s.seek_s);
    }

    #[test]
    fn the_last_window_keeps_its_unfinished_tail() {
        let tokens = [ts(0.0), 5, ts(2.0), ts(2.4), 6, 7];
        let s = split(&tokens, true);
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.segments[1].tokens, 3..6);
        assert!(near(s.segments[1].start_s, 2.4));
        assert!(near(s.segments[1].end_s, 30.0), "up to the window's end");
        // The seek still follows the finished one.
        assert!(near(s.seek_s.unwrap(), 2.0));
    }

    #[test]
    fn with_no_pair_of_timestamps_the_output_is_one_segment() {
        // <0.0> text <6.0>: a single segment, closed by a lone timestamp.
        let s = split(&[ts(0.0), 5, 6, ts(6.0)], false);
        assert!(s.single_timestamp_ending);
        assert_eq!(s.segments.len(), 1);
        assert!(near(s.segments[0].start_s, 0.0));
        assert!(near(s.segments[0].end_s, 6.0));
        assert_eq!(s.seek_s, None);

        // <0.0> text and nothing else: the segment runs to the window's end,
        // and the end of the window is not a timestamp the model gave.
        let s = split(&[ts(0.0), 5, 6], false);
        assert!(!s.single_timestamp_ending);
        assert!(near(s.segments[0].end_s, 30.0));
        assert_eq!(s.seek_s, None);

        // Nothing at all.
        let s = split(&[], false);
        assert!(s.segments.is_empty());
        assert_eq!(s.seek_s, None);
    }

    #[test]
    fn only_text_tokens_go_to_the_aligner_and_each_segment_counts_its_own() {
        // <0.0> a <de> b <2.0> <2.0> c <3.0>, with a language token (above eot,
        // below the timestamps) in the middle of the first segment.
        let (eot, de) = (900, 950);
        let tokens = [ts(0.0), 5, de, 6, ts(2.0), ts(2.0), 7, ts(3.0)];
        let s = split(&tokens, false);
        let (text, spans) = text_of_segments(&s.segments, &tokens, eot);
        assert_eq!(text, vec![5, 6, 7]);
        assert_eq!(
            spans.iter().map(|s| s.text_tokens).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(near(spans[1].start_s, 2.0) && near(spans[1].end_s, 3.0));
    }

    // ---- place_words -------------------------------------------------------

    fn timing(text: &str, tokens: Range<usize>, start_s: f32, end_s: f32) -> WordTiming {
        WordTiming {
            text: text.to_string(),
            tokens,
            start_s,
            end_s,
            probability: 0.9,
        }
    }

    const PREPEND: &str = "\"'“¿([{-";
    const APPEND: &str = "\"'.。,，!！?？:：”)]}、";

    fn place(alignment: Vec<WordTiming>, segments: &[SegmentText], last_speech_s: f32) -> Placed {
        place_words(alignment, segments, last_speech_s, 30.0, PREPEND, APPEND)
    }

    #[test]
    fn words_keep_the_aligners_times_when_nothing_is_long() {
        let alignment = vec![
            timing(" one", 0..1, 1.0, 1.4),
            timing(" two", 1..2, 1.4, 1.8),
            timing(" three", 2..3, 1.8, 2.2),
        ];
        let seg = [SegmentText {
            start_s: 1.0,
            end_s: 2.2,
            text_tokens: 3,
        }];
        let p = place(alignment, &seg, 0.0);
        let times: Vec<(f32, f32)> = p.words.iter().map(|w| (w.start_s, w.end_s)).collect();
        assert_eq!(times, vec![(1.0, 1.4), (1.4, 1.8), (1.8, 2.2)]);
        assert_eq!(p.words[0].text, "one", "the leading space is trimmed");
        assert!(near(p.last_word_end.unwrap(), 2.2));
    }

    #[test]
    fn a_first_word_that_began_long_before_the_segment_starts_at_the_segment() {
        // The aligner puts the first word at 0.0 (the lead-in is its), the
        // segment says speech began at 6.7 s. Medians are 0.4 s.
        let alignment = vec![
            timing(" hello", 0..1, 0.0, 7.1),
            timing(" there", 1..2, 7.1, 7.5),
            timing(" friend", 2..3, 7.5, 7.9),
            timing(" and", 3..4, 7.9, 8.3),
        ];
        let seg = [SegmentText {
            start_s: 6.7,
            end_s: 8.3,
            text_tokens: 4,
        }];
        let p = place(alignment, &seg, 0.0);
        // The pause rule first: the word after a pause is not longer than
        // twice the median (0.8 s), so it is 0.8 s ending at 7.1: 6.3 s.
        // Then the segment's start (6.7 s) is later than that and used only if
        // the word began more than half a second before it.
        let w = &p.words[0];
        assert!(w.end_s <= 7.1 + 1e-4);
        assert!(
            w.start_s >= 6.3 - 1e-4,
            "the word is cut back to near where it was said, not 0 s: {w:?}"
        );
    }

    #[test]
    fn a_segment_start_is_used_when_the_first_word_began_over_half_a_second_before_it() {
        // No pause rule (last speech just before), median 0.4: the first word
        // runs 3.0 to 4.0 (1.0 s, under the cap of 2 x median only if the
        // median allows); make the median 0.5 so the cap is 1.0 and the pause
        // rule is not what moves it.
        let alignment = vec![
            timing(" a", 0..1, 3.0, 4.0),
            timing(" b", 1..2, 4.0, 4.5),
            timing(" c", 2..3, 4.5, 5.0),
            timing(" d", 3..4, 5.0, 5.5),
        ];
        let seg = [SegmentText {
            start_s: 3.8,
            end_s: 5.5,
            text_tokens: 4,
        }];
        // last speech at 3.0: the first word ends 1.0 s after it, under four medians.
        let p = place(alignment, &seg, 3.0);
        let w = &p.words[0];
        // 3.8 - 0.5 = 3.3 > 3.0, so the first word starts at the segment's
        // start, but not within a median of its own end: min(4.0 - 0.5, 3.8).
        assert!(near(w.start_s, 3.5), "{w:?}");
    }

    #[test]
    fn a_last_word_that_runs_past_the_segment_ends_at_the_segment() {
        let alignment = vec![
            timing(" a", 0..1, 1.0, 1.4),
            timing(" b", 1..2, 1.4, 1.8),
            timing(" c", 2..3, 1.8, 2.2),
            timing(" d", 3..4, 2.2, 5.0),
        ];
        let seg = [SegmentText {
            start_s: 1.0,
            end_s: 2.6,
            text_tokens: 4,
        }];
        let p = place(alignment, &seg, 1.0);
        let last = p.words.last().unwrap();
        // 2.6 + 0.5 < 5.0: ends at max(2.2 + median, 2.6) = 2.6.
        assert!(near(last.end_s, 2.6), "{last:?}");
        assert!(near(p.last_word_end.unwrap(), 2.6));
    }

    #[test]
    fn the_word_after_a_sentence_end_is_not_longer_than_twice_the_median() {
        let alignment = vec![
            timing(" It", 0..1, 0.0, 0.4),
            timing(" ends", 1..2, 0.4, 0.8),
            timing(".", 2..3, 0.8, 0.8),
            timing(" Then", 3..4, 0.8, 4.0),
            timing(" more", 4..5, 4.0, 4.4),
        ];
        let seg = [SegmentText {
            start_s: 0.0,
            end_s: 4.4,
            text_tokens: 5,
        }];
        let p = place(alignment, &seg, 0.0);
        let then = p.words.iter().find(|w| w.text == "Then").unwrap();
        // The median of the non-zero durations (0.4, 0.4, 3.2, 0.4) is 0.4.
        assert!(near(then.end_s - then.start_s, 0.8), "{then:?}");
        assert!(near(then.end_s, 4.0), "the end stays, the start moves up");
        assert!(p.words.iter().any(|w| w.text == "ends."));
    }

    #[test]
    fn words_go_to_the_segment_that_owns_their_tokens() {
        let alignment = vec![
            timing(" a", 0..1, 1.0, 1.4),
            timing(" b", 1..3, 1.4, 1.9),
            timing(" c", 3..4, 4.0, 4.4),
        ];
        let segs = [
            SegmentText {
                start_s: 1.0,
                end_s: 1.9,
                text_tokens: 3,
            },
            SegmentText {
                start_s: 4.0,
                end_s: 4.4,
                text_tokens: 1,
            },
        ];
        let p = place(alignment, &segs, 0.0);
        assert_eq!(
            p.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(near(p.last_word_end.unwrap(), 4.4));
    }

    #[test]
    fn times_are_inside_the_window_and_a_word_has_a_frame() {
        let alignment = vec![timing(" a", 0..1, 29.9, 31.0)];
        let seg = [SegmentText {
            start_s: 29.9,
            end_s: 31.0,
            text_tokens: 1,
        }];
        let p = place_words(alignment, &seg, 0.0, 30.0, PREPEND, APPEND);
        let w = &p.words[0];
        assert!(w.end_s <= 30.0 + 1e-4 && w.start_s < w.end_s, "{w:?}");
    }

    #[test]
    fn no_words_has_no_last_word_end() {
        let p = place(Vec::new(), &[], 12.5);
        assert!(p.words.is_empty());
        assert_eq!(p.last_word_end, None);
    }
}
