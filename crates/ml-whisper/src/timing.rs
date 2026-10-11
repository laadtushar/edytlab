//! Word timing from the decoder's cross-attention.
//!
//! Plan step 4 on #384. The model does not say when a word was spoken, but
//! the decoder's cross-attention does, roughly: while it writes a token, a few
//! heads (the "alignment heads", `[[3,1],[4,2],[4,3],[4,7],[5,1],[5,2],[5,4],
//! [5,6]]` for `base`) look at the encoder frames where that token was said.
//! This module is a port of `find_alignment`, `median_filter`, `dtw_cpu` and
//! `merge_punctuations` from `whisper/timing.py` in
//! <https://github.com/openai/whisper> (MIT), release 20250625, without the
//! model and without PyTorch: it takes the attention weights as numbers.
//! Everything is tested on made-up weights; no decoder is involved.
//!
//! ## From weights to token times
//!
//! 1. **Input.** One [`Matrix`] per alignment head, `rows` x `cols`: row `k`
//!    is the attention, over the encoder's frames, of the decoder step that
//!    **predicts** token `k` (for `tokens.len()` tokens plus the end, so
//!    `tokens.len() + 1` rows: the step after the prompt, then one per
//!    sampled token, the last of which predicts `<|endoftext|>`). These are
//!    the rows the decode loop's decoder steps produce, one step each. A frame
//!    is 20 ms ([`SECONDS_PER_FRAME`]).
//! 2. **Crop** to the `content_frames` that hold audio (upstream
//!    `num_frames // 2`; see [`encoder_frames`]), so the path cannot run into
//!    the padding of a short window.
//! 3. **Normalise.** For each head and each frame, z-score the values over the
//!    tokens (population standard deviation): upstream's
//!    `std_mean(weights, dim=-2)`. The plan's note says "over time", but
//!    `dim=-2` of `[heads, tokens, frames]` is the token axis, which is what
//!    is done here. A frame where all tokens have the same value becomes 0
//!    rather than NaN.
//! 4. **Smooth** each row with a median filter of width 7 along the frames
//!    (reflect-padded), then **average** over the heads.
//! 5. **DTW** over the negated matrix finds the monotone path from the first
//!    row and frame to the last that collects the most attention; the time at
//!    which it first reaches row `k` is the start of token `k`, and of the
//!    previous token's end ([`token_start_times`]).
//!
//! The weights are expected **already softmaxed over the frames**, as Hugging
//! Face's `output_attentions` are and as the `_timestamped` export is
//! expected to emit: *not yet checked against the real export*, which needs
//! the ONNX Runtime. If it turns out to emit raw scores, call
//! [`Matrix::softmax_rows`] first. Upstream softmaxes after cropping to the
//! content frames, Hugging Face before; this crops after the softmax, as the
//! pinned export's tooling does.
//!
//! ## From token times to words
//!
//! [`group_words`] joins tokens into words as `split_to_word_tokens` does:
//! first into runs that decode to whole UTF-8 characters, then, for languages
//! with spaces, a run starts a new word when it begins with a space or is
//! punctuation. [`align_words`] gives each word the start of its first token
//! and the start of the token after its last, and the mean of its tokens'
//! probabilities as confidence. [`merge_punctuations`] then attaches opening
//! quotes and brackets to the word after and closing punctuation to the word
//! before, as upstream does (their times are not extended, as upstream).
//!
//! Not ported: upstream's `add_word_timestamps` also shortens words at
//! sentence ends and segment edges from the median word length, using the
//! segment timestamps of the timestamp mode; the plan's notimestamps mode has
//! no segments, so that heuristic is left to the chunking step.

use std::ops::Range;

/// One encoder frame is 20 ms: 50 per second.
pub const SECONDS_PER_FRAME: f32 = 0.02;
/// The same as frames per second, exactly: dividing by this rounds a time to
/// the nearest `f32` once, where multiplying by 0.02 would round twice.
const FRAMES_PER_SECOND: f32 = 50.0;
/// Audio samples (16 kHz) per encoder frame: 160 per mel frame, two mel frames
/// per encoder frame.
pub const SAMPLES_PER_FRAME: usize = 320;
/// Upstream's `medfilt_width`.
pub const MEDIAN_FILTER_WIDTH: usize = 7;
/// Upstream's `prepend_punctuations`: attach to the word after.
pub const PREPEND_PUNCTUATIONS: &str = "\"'“¿([{-";
/// Upstream's `append_punctuations`: attach to the word before.
pub const APPEND_PUNCTUATIONS: &str = "\"'.。,，!！?？:：”)]}、";

/// Why alignment could not run.
#[derive(Debug, thiserror::Error)]
pub enum TimingError {
    /// No attention heads were given.
    #[error("no attention heads to align with")]
    NoHeads,

    /// The inputs disagree about a size.
    #[error("{0}")]
    Shape(String),

    /// A token id the byte lookup does not know.
    #[error("token {0} has no bytes")]
    UnknownToken(u32),

    /// The median filter's width must be odd.
    #[error("median filter width {0} is not odd")]
    EvenFilterWidth(usize),

    /// A NaN or infinity in the attention weights.
    #[error("attention weights contain a NaN or an infinity")]
    NonFinite,
}

/// A row-major matrix of `f32`.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<f32>,
}

impl Matrix {
    /// Wrap `data`, which must be `rows * cols` long.
    pub fn new(rows: usize, cols: usize, data: Vec<f32>) -> Result<Self, TimingError> {
        if data.len() != rows * cols {
            return Err(TimingError::Shape(format!(
                "{} values for a {rows} x {cols} matrix",
                data.len()
            )));
        }
        Ok(Self { rows, cols, data })
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    pub fn row(&self, i: usize) -> &[f32] {
        &self.data[i * self.cols..(i + 1) * self.cols]
    }

    /// Softmax each row in place. For attention scores that have not been
    /// through the attention softmax.
    pub fn softmax_rows(&mut self) {
        for row in self.data.chunks_exact_mut(self.cols.max(1)) {
            let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let total: f64 = row.iter().map(|&v| f64::from(v - max).exp()).sum();
            for v in row.iter_mut() {
                *v = (f64::from(*v - max).exp() / total) as f32;
            }
        }
    }
}

/// Encoder frames that hold audio, for `samples` of 16 kHz audio in a window:
/// upstream's `num_frames // 2` for `num_frames` mel frames. 1500 for a full
/// 30 s window.
pub fn encoder_frames(samples: usize) -> usize {
    samples / SAMPLES_PER_FRAME
}

/// Median filter every row of `m` along its columns: width `width` (odd),
/// reflect-padded by `width / 2` at both ends, as upstream's `median_filter`.
/// A row no longer than `width / 2` is left as it is.
pub fn median_filter_rows(m: &mut Matrix, width: usize) -> Result<(), TimingError> {
    if width == 0 || width % 2 == 0 {
        return Err(TimingError::EvenFilterWidth(width));
    }
    let pad = width / 2;
    if m.cols <= pad {
        return Ok(());
    }
    let cols = m.cols;
    let last = cols as isize - 1;
    let mut window = vec![0.0f32; width];
    for row in m.data.chunks_exact_mut(cols) {
        let source = row.to_vec();
        for (j, out) in row.iter_mut().enumerate() {
            for (k, slot) in window.iter_mut().enumerate() {
                // Reflect about the first and last value, without repeating
                // them; the pad is shorter than the row, so once is enough.
                let mut i = j as isize + k as isize - pad as isize;
                if i < 0 {
                    i = -i;
                } else if i > last {
                    i = 2 * last - i;
                }
                *slot = source[i as usize];
            }
            window.sort_by(f32::total_cmp);
            *out = window[pad];
        }
    }
    Ok(())
}

/// A DTW path from cell `(0, 0)` to the last cell, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtwPath {
    /// Row of each cell on the path.
    pub text: Vec<usize>,
    /// Column of each cell on the path.
    pub time: Vec<usize>,
}

/// Dynamic time warping over `cost`: the path from the first cell to the last
/// that moves one step right, down, or diagonally and has the least total
/// cost. A port of `dtw_cpu`, including its tie-breaking (a tie goes to the
/// horizontal step) and its float32 running costs.
pub fn dtw(cost: &Matrix) -> DtwPath {
    let (n, m) = (cost.rows, cost.cols);
    if n == 0 || m == 0 {
        return DtwPath {
            text: Vec::new(),
            time: Vec::new(),
        };
    }
    let stride = m + 1;
    // Running costs are float32 and the sums float64 rounded back, as in
    // upstream (numpy float32 array, float64 input), so ties fall alike.
    let mut total = vec![f32::INFINITY; (n + 1) * stride];
    let mut trace = vec![0u8; (n + 1) * stride];
    total[0] = 0.0;
    for j in 1..=m {
        for i in 1..=n {
            let c0 = total[(i - 1) * stride + j - 1];
            let c1 = total[(i - 1) * stride + j];
            let c2 = total[i * stride + j - 1];
            let (c, t) = if c0 < c1 && c0 < c2 {
                (c0, 0)
            } else if c1 < c0 && c1 < c2 {
                (c1, 1)
            } else {
                (c2, 2)
            };
            total[i * stride + j] =
                (f64::from(cost.data[(i - 1) * m + j - 1]) + f64::from(c)) as f32;
            trace[i * stride + j] = t;
        }
    }

    // Backtrace from the last cell; the first row and column lead home.
    let (mut i, mut j) = (n, m);
    let (mut text, mut time) = (Vec::new(), Vec::new());
    while i > 0 || j > 0 {
        text.push(i.saturating_sub(1));
        time.push(j.saturating_sub(1));
        let step = if i == 0 {
            2
        } else if j == 0 {
            1
        } else {
            trace[i * stride + j]
        };
        match step {
            0 => {
                i -= 1;
                j -= 1;
            }
            1 => i -= 1,
            _ => j -= 1,
        }
    }
    text.reverse();
    time.reverse();
    DtwPath { text, time }
}

/// The matrix DTW runs on, before the sign flip: the heads cropped to
/// `content_frames`, z-scored over tokens, median-filtered, and averaged.
pub fn averaged_matrix(
    heads: &[Matrix],
    content_frames: usize,
    filter_width: usize,
) -> Result<Matrix, TimingError> {
    let first = heads.first().ok_or(TimingError::NoHeads)?;
    let (rows, cols) = (first.rows, first.cols);
    if let Some(other) = heads.iter().find(|h| (h.rows, h.cols) != (rows, cols)) {
        return Err(TimingError::Shape(format!(
            "heads of {rows} x {cols} and {} x {}",
            other.rows, other.cols
        )));
    }
    if rows == 0 || content_frames == 0 || content_frames > cols {
        return Err(TimingError::Shape(format!(
            "{content_frames} content frames of {rows} rows x {cols} frames"
        )));
    }

    let mut sum = vec![0.0f32; rows * content_frames];
    for head in heads {
        // Crop to the content frames.
        let mut cropped = Vec::with_capacity(rows * content_frames);
        for r in 0..rows {
            cropped.extend_from_slice(&head.row(r)[..content_frames]);
        }
        if cropped.iter().any(|v| !v.is_finite()) {
            return Err(TimingError::NonFinite);
        }
        // Z-score each frame over the tokens (population standard deviation).
        for f in 0..content_frames {
            let column = (0..rows).map(|r| cropped[r * content_frames + f]);
            let (lo, hi) = column
                .clone()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v), hi.max(v))
                });
            let mean = column.clone().map(f64::from).sum::<f64>() / rows as f64;
            let var = column.map(|v| (f64::from(v) - mean).powi(2)).sum::<f64>() / rows as f64;
            let std = var.sqrt();
            for r in 0..rows {
                let v = &mut cropped[r * content_frames + f];
                // All equal: no information, and 0 / 0 would be NaN.
                *v = if hi > lo {
                    ((f64::from(*v) - mean) / std) as f32
                } else {
                    0.0
                };
            }
        }
        let mut smoothed = Matrix {
            rows,
            cols: content_frames,
            data: cropped,
        };
        median_filter_rows(&mut smoothed, filter_width)?;
        for (s, v) in sum.iter_mut().zip(&smoothed.data) {
            *s += v;
        }
    }
    let n = heads.len() as f32;
    Ok(Matrix {
        rows,
        cols: content_frames,
        data: sum.into_iter().map(|v| v / n).collect(),
    })
}

/// Seconds at which the alignment path first reaches each row: row `k` is the
/// start of token `k`, and of the end of token `k - 1`. One value per row.
pub fn token_start_times(heads: &[Matrix], content_frames: usize) -> Result<Vec<f32>, TimingError> {
    let averaged = averaged_matrix(heads, content_frames, MEDIAN_FILTER_WIDTH)?;
    let cost = Matrix {
        rows: averaged.rows,
        cols: averaged.cols,
        data: averaged.data.iter().map(|v| -v).collect(),
    };
    let path = dtw(&cost);
    // The path reaches each row once from above or the left; the first cell
    // of each row is where that token starts.
    let mut times = Vec::with_capacity(averaged.rows);
    for k in 0..path.text.len() {
        if k == 0 || path.text[k] != path.text[k - 1] {
            times.push(path.time[k] as f32 / FRAMES_PER_SECOND);
        }
    }
    debug_assert_eq!(times.len(), averaged.rows);
    Ok(times)
}

/// How a language is cut into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordSplit {
    /// At spaces and punctuation.
    Spaces,
    /// At every whole character: for languages written without spaces.
    Unicode,
}

impl WordSplit {
    /// Upstream's choice by language code: `zh`, `ja`, `th`, `lo`, `my` and
    /// `yue` are split by character, everything else at spaces.
    pub fn for_language(code: &str) -> Self {
        match code {
            "zh" | "ja" | "th" | "lo" | "my" | "yue" => Self::Unicode,
            _ => Self::Spaces,
        }
    }
}

/// A word: its text and the tokens it is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSpan {
    pub text: String,
    pub tokens: Range<usize>,
}

/// Group `tokens` into words. `bytes_of` gives a token's bytes (the
/// tokenizer's `token_bytes`). No token is dropped: the spans cover
/// `0..tokens.len()`.
pub fn group_words<'a>(
    tokens: &[u32],
    bytes_of: impl Fn(u32) -> Option<&'a [u8]>,
    split: WordSplit,
) -> Result<Vec<WordSpan>, TimingError> {
    let bytes: Vec<&[u8]> = tokens
        .iter()
        .map(|&t| bytes_of(t).ok_or(TimingError::UnknownToken(t)))
        .collect::<Result<_, _>>()?;
    let whole: Vec<u8> = bytes.concat();
    let full: Vec<char> = String::from_utf8_lossy(&whole).chars().collect();

    // Runs of tokens that end on a character boundary: a run is complete when
    // it decodes without a replacement character, or when its first one is
    // also in the decoding of everything (the bytes really are invalid).
    let mut runs: Vec<WordSpan> = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    let (mut start, mut offset) = (0, 0);
    for (i, b) in bytes.iter().enumerate() {
        current.extend_from_slice(b);
        let decoded = String::from_utf8_lossy(&current).into_owned();
        let complete = match decoded.chars().position(|c| c == '\u{FFFD}') {
            None => true,
            Some(p) => full.get(offset + p) == Some(&'\u{FFFD}'),
        };
        if complete {
            offset += decoded.chars().count();
            runs.push(WordSpan {
                text: decoded,
                tokens: start..i + 1,
            });
            current.clear();
            start = i + 1;
        }
    }
    if start < tokens.len() {
        // Cannot happen with a consistent decoding; never lose a token if it does.
        runs.push(WordSpan {
            text: String::from_utf8_lossy(&current).into_owned(),
            tokens: start..tokens.len(),
        });
    }
    if split == WordSplit::Unicode {
        return Ok(runs);
    }

    // At spaces: a run opens a word if it starts with a space or is
    // punctuation, otherwise it continues the word before. Python's
    // `x in string.punctuation` is a substring test, so a run of only
    // whitespace (trimmed to "") counts as punctuation, and it is kept here.
    const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
    let mut words: Vec<WordSpan> = Vec::new();
    for run in runs {
        match words.last_mut() {
            Some(word) if !run.text.starts_with(' ') && !PUNCTUATION.contains(run.text.trim()) => {
                word.text.push_str(&run.text);
                word.tokens.end = run.tokens.end;
            }
            _ => words.push(run),
        }
    }
    Ok(words)
}

/// A word with its time.
#[derive(Debug, Clone, PartialEq)]
pub struct WordTiming {
    pub text: String,
    pub tokens: Range<usize>,
    pub start_s: f32,
    pub end_s: f32,
    /// Mean of the model's probabilities of the word's tokens.
    pub probability: f32,
}

/// What [`align_words`] needs.
pub struct AlignmentInput<'a> {
    /// One matrix per alignment head, each `tokens.len() + 1` rows.
    pub heads: &'a [Matrix],
    /// Encoder frames that hold audio ([`encoder_frames`]).
    pub content_frames: usize,
    /// The sampled text tokens (no prompt, no end token, no timestamps).
    pub tokens: &'a [u32],
    /// The model's probability of each token (the decode loop's `token_probs`).
    pub token_probs: &'a [f32],
}

/// Words with times, in seconds from the start of the window.
pub fn align_words<'a>(
    input: &AlignmentInput<'_>,
    bytes_of: impl Fn(u32) -> Option<&'a [u8]>,
    split: WordSplit,
) -> Result<Vec<WordTiming>, TimingError> {
    let AlignmentInput {
        heads,
        content_frames,
        tokens,
        token_probs,
    } = *input;
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    if token_probs.len() != tokens.len() {
        return Err(TimingError::Shape(format!(
            "{} probabilities for {} tokens",
            token_probs.len(),
            tokens.len()
        )));
    }
    if heads.is_empty() {
        return Err(TimingError::NoHeads);
    }
    if let Some(h) = heads.iter().find(|h| h.rows != tokens.len() + 1) {
        return Err(TimingError::Shape(format!(
            "a head has {} rows, expected {} (one per token and one for the end)",
            h.rows,
            tokens.len() + 1
        )));
    }
    let times = token_start_times(heads, content_frames)?;
    let words = group_words(tokens, bytes_of, split)?;
    Ok(words
        .into_iter()
        .map(|w| {
            let probs = &token_probs[w.tokens.clone()];
            WordTiming {
                start_s: times[w.tokens.start],
                end_s: times[w.tokens.end],
                probability: (probs.iter().map(|&p| f64::from(p)).sum::<f64>() / probs.len() as f64)
                    as f32,
                text: w.text,
                tokens: w.tokens,
            }
        })
        .collect())
}

/// Attach punctuation to its neighbours as upstream's `merge_punctuations`
/// does: a word that is one of `prepended` (and starts with a space) joins the
/// word after, a word that is in `appended` joins the word before. The times
/// of the surviving word are not changed. Emptied words are removed.
pub fn merge_punctuations(
    mut words: Vec<WordTiming>,
    prepended: &str,
    appended: &str,
) -> Vec<WordTiming> {
    // Opening marks join the word after. Scan from the end; `j` is the word
    // they would join. Python's `in` on strings is a substring test, kept.
    if words.len() >= 2 {
        let mut j = words.len() - 1;
        for i in (0..words.len() - 1).rev() {
            if words[i].text.starts_with(' ') && prepended.contains(words[i].text.trim()) {
                let text = std::mem::take(&mut words[i].text);
                let range = words[i].tokens.clone();
                words[j].text.insert_str(0, &text);
                if words[j].tokens.is_empty() {
                    words[j].tokens = range.clone();
                } else {
                    words[j].tokens.start = range.start;
                }
                words[i].tokens = range.start..range.start;
            } else {
                j = i;
            }
        }
    }
    // Closing marks join the word before.
    let mut i = 0;
    for j in 1..words.len() {
        if !words[i].text.ends_with(' ') && appended.contains(words[j].text.as_str()) {
            let text = std::mem::take(&mut words[j].text);
            let range = words[j].tokens.clone();
            words[i].text.push_str(&text);
            if words[i].tokens.is_empty() {
                words[i].tokens = range.clone();
            } else if !range.is_empty() {
                words[i].tokens.end = range.end;
            }
            words[j].tokens = range.start..range.start;
        } else {
            i = j;
        }
    }
    words.retain(|w| !w.text.is_empty());
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const ATTN: &[u8] = include_bytes!("../tests/fixtures/timing_attn.f32");
    const MATRIX: &[u8] = include_bytes!("../tests/fixtures/timing_matrix.f32");
    const PATH: &[u8] = include_bytes!("../tests/fixtures/timing_path.i32");
    const TIMES: &[u8] = include_bytes!("../tests/fixtures/timing_times.f32");
    const DTW_COST: &[u8] = include_bytes!("../tests/fixtures/dtw_cost.f32");
    const DTW_PATH: &[u8] = include_bytes!("../tests/fixtures/dtw_path.i32");

    // The shape of the made-up weights in make_timing_fixture.py.
    const HEADS: usize = 3;
    const ROWS: usize = 12;
    const COLS: usize = 200;
    const CONTENT_FRAMES: usize = 160;

    /// How closely the Rust matrix must match PyTorch's. Measured: 4.8e-7.
    const GOLDEN_TOLERANCE: f32 = 1e-5;

    fn f32s(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    fn i32s(bytes: &[u8]) -> Vec<usize> {
        bytes
            .chunks_exact(4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .collect()
    }

    fn heads() -> Vec<Matrix> {
        let all = f32s(ATTN);
        assert_eq!(all.len(), HEADS * ROWS * COLS);
        all.chunks_exact(ROWS * COLS)
            .map(|h| Matrix::new(ROWS, COLS, h.to_vec()).unwrap())
            .collect()
    }

    fn matrix(rows: usize, cols: usize, values: &[f32]) -> Matrix {
        Matrix::new(rows, cols, values.to_vec()).unwrap()
    }

    fn negated(m: &Matrix) -> Matrix {
        Matrix::new(
            m.rows(),
            m.cols(),
            m.as_slice().iter().map(|v| -v).collect(),
        )
        .unwrap()
    }

    // ---- DTW -------------------------------------------------------------------

    #[test]
    fn dtw_matches_upstream_on_integer_costs_full_of_ties() {
        let cost = matrix(9, 14, &f32s(DTW_COST));
        let expected = i32s(DTW_PATH);
        let (text, time) = expected.split_at(expected.len() / 2);

        let path = dtw(&cost);

        assert_eq!(path.text, text);
        assert_eq!(path.time, time);
    }

    #[test]
    fn dtw_follows_a_diagonal() {
        // 0 on the diagonal, 1 elsewhere.
        let n = 5;
        let cost: Vec<f32> = (0..n * n)
            .map(|k| if k / n == k % n { 0.0 } else { 1.0 })
            .collect();
        let path = dtw(&matrix(n, n, &cost));
        assert_eq!(path.text, [0, 1, 2, 3, 4]);
        assert_eq!(path.time, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn dtw_ties_go_to_the_horizontal_step() {
        // Two rows, five columns, all costs equal. Every path ties; upstream
        // breaks ties towards the horizontal step, so the path drops to the
        // last row at the first column and then runs along it.
        let path = dtw(&matrix(2, 5, &[0.0; 10]));
        assert_eq!(path.text, [0, 1, 1, 1, 1, 1]);
        assert_eq!(path.time, [0, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_dtw_path_runs_corner_to_corner_in_unit_steps() {
        // A deterministic pseudo-random cost, several shapes.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 1000) as f32 / 100.0
        };
        for (rows, cols) in [(1, 1), (1, 6), (6, 1), (3, 3), (4, 11), (11, 4), (20, 37)] {
            let cost = Matrix::new(rows, cols, (0..rows * cols).map(|_| next()).collect()).unwrap();
            let path = dtw(&cost);
            assert_eq!((path.text[0], path.time[0]), (0, 0), "{rows}x{cols}");
            assert_eq!(
                (*path.text.last().unwrap(), *path.time.last().unwrap()),
                (rows - 1, cols - 1),
                "{rows}x{cols}"
            );
            for k in 1..path.text.len() {
                let (di, dj) = (
                    path.text[k] - path.text[k - 1],
                    path.time[k] - path.time[k - 1],
                );
                assert!(
                    di <= 1 && dj <= 1 && di + dj >= 1,
                    "{rows}x{cols} step {k}: ({di}, {dj})"
                );
            }
        }
    }

    // ---- the matrix -------------------------------------------------------------

    #[test]
    fn median_filter_takes_the_middle_value_with_reflected_edges() {
        // Reflected: 9 1 9 2 8 3 7 4 7 for 1 9 2 8 3 7 4 at width 3.
        let mut m = matrix(1, 7, &[1.0, 9.0, 2.0, 8.0, 3.0, 7.0, 4.0]);
        median_filter_rows(&mut m, 3).unwrap();
        assert_eq!(m.as_slice(), [9.0, 2.0, 8.0, 3.0, 7.0, 4.0, 7.0]);

        // Rows are filtered separately, and a spike narrower than half the
        // window disappears.
        let mut m = matrix(2, 5, &[1.0, 1.0, 9.0, 1.0, 1.0, 0.0, 0.0, 9.0, 0.0, 0.0]);
        median_filter_rows(&mut m, 5).unwrap();
        assert_eq!(m.row(0), [1.0; 5]);
        assert_eq!(m.row(1), [0.0; 5]);
    }

    #[test]
    fn median_filter_leaves_a_short_row_alone_and_refuses_an_even_width() {
        // Upstream returns the input unchanged when the row is no longer than
        // `width / 2` (the reflection needs a pad shorter than the row).
        let mut m = matrix(1, 3, &[3.0, 1.0, 2.0]);
        median_filter_rows(&mut m, 7).unwrap();
        assert_eq!(m.as_slice(), [3.0, 1.0, 2.0]);
        median_filter_rows(&mut m, 9).unwrap();
        assert_eq!(m.as_slice(), [3.0, 1.0, 2.0]);

        // One longer than that is filtered: 4 1 3 2 reflected by 3 each side is
        // 2 3 1 | 4 1 3 2 | 3 1 4, whose width-7 medians are 2 3 2 3.
        let mut m = matrix(1, 4, &[4.0, 1.0, 3.0, 2.0]);
        median_filter_rows(&mut m, 7).unwrap();
        assert_eq!(m.as_slice(), [2.0, 3.0, 2.0, 3.0]);

        assert!(matches!(
            median_filter_rows(&mut m, 4),
            Err(TimingError::EvenFilterWidth(4))
        ));
        assert!(matches!(
            median_filter_rows(&mut m, 0),
            Err(TimingError::EvenFilterWidth(0))
        ));
    }

    #[test]
    fn frames_are_z_scored_over_the_tokens_and_cropped_to_the_content() {
        // One head, two tokens, three content frames and a fourth that must be
        // ignored. Column 0 is [1, 3]: mean 2, std 1, so -1 and 1. Column 1 is
        // constant: 0 (not NaN). Column 2 is [3, 1]: 1 and -1.
        let head = matrix(2, 4, &[1.0, 2.0, 3.0, 100.0, 3.0, 2.0, 1.0, -100.0]);
        let m = averaged_matrix(&[head], 3, 1).unwrap();
        assert_eq!((m.rows(), m.cols()), (2, 3));
        assert_eq!(m.as_slice(), [-1.0, 0.0, 1.0, 1.0, 0.0, -1.0]);
    }

    #[test]
    fn heads_are_averaged() {
        let a = matrix(2, 2, &[1.0, 3.0, 3.0, 1.0]); // z-scored: -1 1 / 1 -1
        let b = matrix(2, 2, &[3.0, 1.0, 1.0, 3.0]); // z-scored: 1 -1 / -1 1
        let m = averaged_matrix(&[a, b], 2, 1).unwrap();
        assert_eq!(m.as_slice(), [0.0; 4]);
    }

    #[test]
    fn the_averaged_matrix_matches_upstream() {
        let expected = f32s(MATRIX);
        assert_eq!(expected.len(), ROWS * CONTENT_FRAMES);

        let got = averaged_matrix(&heads(), CONTENT_FRAMES, MEDIAN_FILTER_WIDTH).unwrap();

        assert_eq!((got.rows(), got.cols()), (ROWS, CONTENT_FRAMES));
        let worst = got
            .as_slice()
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < GOLDEN_TOLERANCE, "max abs error {worst}");
    }

    #[test]
    fn the_path_and_token_times_match_upstream() {
        let expected_path = i32s(PATH);
        let (text, time) = expected_path.split_at(expected_path.len() / 2);
        let averaged = averaged_matrix(&heads(), CONTENT_FRAMES, MEDIAN_FILTER_WIDTH).unwrap();
        let path = dtw(&negated(&averaged));
        assert_eq!(path.text, text);
        assert_eq!(path.time, time);

        let times = token_start_times(&heads(), CONTENT_FRAMES).unwrap();
        let expected = f32s(TIMES);
        assert_eq!(times.len(), ROWS);
        for (k, (got, want)) in times.iter().zip(&expected).enumerate() {
            assert!((got - want).abs() < 1e-5, "row {k}: {got} vs {want}");
        }
    }

    #[test]
    fn a_synthetic_diagonal_puts_each_token_between_its_neighbours() {
        // Row k attends around frame 10 + 20 k with a Gaussian, softmaxed
        // over 200 frames. Token k should start about halfway between its
        // ridge and the previous one: frame 20 k, that is 0.4 k seconds.
        let rows = 8;
        let heads: Vec<Matrix> = [4.0f32, 6.0]
            .iter()
            .map(|sigma| {
                let mut m = Matrix::new(
                    rows,
                    200,
                    (0..rows * 200)
                        .map(|k| {
                            let (r, f) = (k / 200, (k % 200) as f32);
                            -((f - (10.0 + 20.0 * r as f32)).powi(2)) / (2.0 * sigma * sigma)
                        })
                        .collect(),
                )
                .unwrap();
                m.softmax_rows();
                m
            })
            .collect();

        let times = token_start_times(&heads, 170).unwrap();

        assert_eq!(times.len(), rows);
        assert_eq!(times[0], 0.0);
        for (k, t) in times.iter().enumerate().skip(1) {
            let want = 0.4 * k as f32;
            assert!(
                (t - want).abs() <= 0.04,
                "token {k} starts at {t}, expected about {want}"
            );
        }
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn softmax_rows_makes_each_row_a_distribution() {
        let mut m = matrix(2, 3, &[0.0, 0.0, 0.0, 1000.0, 0.0, 0.0]);
        m.softmax_rows();
        for r in 0..2 {
            assert!((m.row(r).iter().sum::<f32>() - 1.0).abs() < 1e-6);
        }
        assert!((m.row(0)[1] - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(m.row(1), [1.0, 0.0, 0.0], "a huge score must not overflow");
    }

    // ---- words -----------------------------------------------------------------

    /// A token table for the word tests: id -> bytes.
    struct Table(HashMap<u32, Vec<u8>>);

    impl Table {
        fn new(entries: &[(u32, &[u8])]) -> Self {
            Self(entries.iter().map(|(id, b)| (*id, b.to_vec())).collect())
        }

        fn bytes<'a>(&'a self) -> impl Fn(u32) -> Option<&'a [u8]> + 'a {
            |id| self.0.get(&id).map(Vec::as_slice)
        }
    }

    fn text_table() -> Table {
        Table::new(&[
            (0, b" Hel"),
            (1, b"lo"),
            (2, b" wor"),
            (3, b"ld"),
            (4, b"."),
            (5, b" caf"),
            (6, b"\xc3"),
            (7, b"\xa9"),
            (8, b" "),
            (9, b"b"),
            (10, b"\xe6\x97"),
            (11, b"\xa5\xe6\x9c"),
            (12, b"\xac"),
            (13, b" a"),
            (14, b","),
            (15, b"\""),
        ])
    }

    fn spans(table: &Table, tokens: &[u32], split: WordSplit) -> Vec<(String, Range<usize>)> {
        group_words(tokens, table.bytes(), split)
            .unwrap()
            .into_iter()
            .map(|w| (w.text, w.tokens))
            .collect()
    }

    #[test]
    fn tokens_without_a_leading_space_join_the_word_before() {
        let table = text_table();
        assert_eq!(
            spans(&table, &[0, 1, 2, 3, 4], WordSplit::Spaces),
            [
                (" Hello".to_string(), 0..2),
                (" world".to_string(), 2..4),
                (".".to_string(), 4..5)
            ]
        );
    }

    #[test]
    fn a_character_split_across_tokens_stays_in_one_word() {
        let table = text_table();
        // " caf" + 0xC3 + 0xA9: the two bytes are one "é", glued to the word.
        assert_eq!(
            spans(&table, &[5, 6, 7], WordSplit::Spaces),
            [(" café".to_string(), 0..3)]
        );
    }

    #[test]
    fn languages_without_spaces_split_at_whole_characters() {
        let table = text_table();
        // 日 = E6 97 A5 and 本 = E6 9C AC, cut as [E6 97] [A5 E6 9C] [AC]: no
        // token boundary is a character boundary, so the first complete
        // group is all three tokens, "日本".
        assert_eq!(
            spans(&table, &[10, 11, 12], WordSplit::Unicode),
            [("日本".to_string(), 0..3)]
        );
        // Mixed with a clean token: " a" is its own group in Unicode mode and
        // the character groups follow.
        assert_eq!(
            spans(&table, &[13, 10, 11, 12], WordSplit::Unicode),
            [(" a".to_string(), 0..1), ("日本".to_string(), 1..4)]
        );
        // Spaces mode would glue the characters to " a".
        assert_eq!(
            spans(&table, &[13, 10, 11, 12], WordSplit::Spaces),
            [(" a日本".to_string(), 0..4)]
        );
    }

    #[test]
    fn a_lone_space_starts_a_word_as_in_upstream() {
        // Upstream tests `subword.strip() in string.punctuation`, a substring
        // test, so a whitespace-only subword (strip() is "") counts as
        // punctuation and starts a word; the token after it joins that word.
        let table = text_table();
        assert_eq!(
            spans(&table, &[13, 8, 9], WordSplit::Spaces),
            [(" a".to_string(), 0..1), (" b".to_string(), 1..3)]
        );
    }

    #[test]
    fn words_cover_every_token_and_reject_unknown_ones() {
        let table = text_table();
        assert!(spans(&table, &[], WordSplit::Spaces).is_empty());
        let all: Vec<u32> = (0..16).collect();
        for split in [WordSplit::Spaces, WordSplit::Unicode] {
            let words = spans(&table, &all, split);
            assert_eq!(words.first().unwrap().1.start, 0);
            assert_eq!(words.last().unwrap().1.end, 16);
            assert!(words.windows(2).all(|w| w[0].1.end == w[1].1.start));
        }
        assert!(matches!(
            group_words(&[0, 99], table.bytes(), WordSplit::Spaces),
            Err(TimingError::UnknownToken(99))
        ));
    }

    #[test]
    fn the_language_picks_the_split() {
        for code in ["zh", "ja", "th", "lo", "my", "yue"] {
            assert_eq!(WordSplit::for_language(code), WordSplit::Unicode, "{code}");
        }
        for code in ["en", "de", "fr", "ko", "ru", "ar", ""] {
            assert_eq!(WordSplit::for_language(code), WordSplit::Spaces, "{code}");
        }
    }

    /// Attention for `n` tokens plus the end: row k peaks at frame 10 + 20 k.
    fn ridge(n: usize) -> Vec<Matrix> {
        [4.0f32, 6.0]
            .iter()
            .map(|sigma| {
                let rows = n + 1;
                let mut m = Matrix::new(
                    rows,
                    300,
                    (0..rows * 300)
                        .map(|k| {
                            -(((k % 300) as f32 - (10.0 + 20.0 * (k / 300) as f32)).powi(2))
                                / (2.0 * sigma * sigma)
                        })
                        .collect(),
                )
                .unwrap();
                m.softmax_rows();
                m
            })
            .collect()
    }

    #[test]
    fn words_get_the_start_of_their_first_token_and_the_start_of_the_next() {
        let table = text_table();
        let heads = ridge(5);
        let tokens = [0, 1, 2, 3, 4];
        let probs = [0.9, 0.8, 0.7, 0.6, 0.5];
        let times = token_start_times(&heads, 120).unwrap();

        let words = align_words(
            &AlignmentInput {
                heads: &heads,
                content_frames: 120,
                tokens: &tokens,
                token_probs: &probs,
            },
            table.bytes(),
            WordSplit::Spaces,
        )
        .unwrap();

        assert_eq!(words.len(), 3);
        assert_eq!(words[0].text, " Hello");
        assert_eq!((words[0].start_s, words[0].end_s), (times[0], times[2]));
        assert!((words[0].probability - 0.85).abs() < 1e-6);
        assert_eq!(words[1].text, " world");
        assert_eq!((words[1].start_s, words[1].end_s), (times[2], times[4]));
        assert!((words[1].probability - 0.65).abs() < 1e-6);
        assert_eq!(words[2].text, ".");
        assert_eq!((words[2].start_s, words[2].end_s), (times[4], times[5]));
        assert_eq!(words[2].tokens, 4..5);
    }

    #[test]
    fn alignment_inputs_that_disagree_are_errors() {
        let table = text_table();
        let heads = ridge(5);
        let tokens = [0, 1, 2, 3, 4];
        let probs = [0.5; 5];
        let run = |heads: &[Matrix], content_frames, tokens: &[u32], probs: &[f32]| {
            align_words(
                &AlignmentInput {
                    heads,
                    content_frames,
                    tokens,
                    token_probs: probs,
                },
                table.bytes(),
                WordSplit::Spaces,
            )
        };
        assert!(run(&heads, 120, &tokens, &probs).is_ok());
        assert!(matches!(
            run(&[], 120, &tokens, &probs),
            Err(TimingError::NoHeads)
        ));
        // One row too few for five tokens.
        assert!(matches!(
            run(&heads, 120, &tokens[..4], &probs[..4]),
            Err(TimingError::Shape(_))
        ));
        assert!(matches!(
            run(&heads, 120, &tokens, &probs[..4]),
            Err(TimingError::Shape(_))
        ));
        assert!(matches!(
            run(&heads, 0, &tokens, &probs),
            Err(TimingError::Shape(_))
        ));
        assert!(matches!(
            run(&heads, 301, &tokens, &probs),
            Err(TimingError::Shape(_))
        ));
        // Heads of different widths.
        let odd = vec![
            heads[0].clone(),
            Matrix::new(6, 299, vec![0.0; 6 * 299]).unwrap(),
        ];
        assert!(matches!(
            run(&odd, 120, &tokens, &probs),
            Err(TimingError::Shape(_))
        ));
        // No tokens is no words, not an error.
        assert!(run(&heads, 120, &[], &[]).unwrap().is_empty());
    }

    // ---- punctuation -----------------------------------------------------------

    fn word(text: &str, tokens: Range<usize>, start_s: f32, end_s: f32) -> WordTiming {
        WordTiming {
            text: text.into(),
            tokens,
            start_s,
            end_s,
            probability: 0.5,
        }
    }

    #[test]
    fn closing_punctuation_joins_the_word_before_without_moving_its_times() {
        let merged = merge_punctuations(
            vec![
                word(" Hello", 0..1, 0.0, 0.5),
                word(",", 1..2, 0.5, 0.6),
                word(" world", 2..3, 0.6, 1.0),
                word(".", 3..4, 1.0, 1.1),
            ],
            PREPEND_PUNCTUATIONS,
            APPEND_PUNCTUATIONS,
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(
            (
                merged[0].text.as_str(),
                merged[0].tokens.clone(),
                merged[0].start_s,
                merged[0].end_s
            ),
            (" Hello,", 0..2, 0.0, 0.5)
        );
        assert_eq!(
            (
                merged[1].text.as_str(),
                merged[1].tokens.clone(),
                merged[1].start_s,
                merged[1].end_s
            ),
            (" world.", 2..4, 0.6, 1.0)
        );
    }

    #[test]
    fn opening_punctuation_joins_the_word_after() {
        let merged = merge_punctuations(
            vec![
                word(" Say", 0..1, 0.0, 0.3),
                word(" \"", 1..2, 0.3, 0.4),
                word("Hi", 2..3, 0.4, 0.8),
                word("\"", 3..4, 0.8, 0.9),
            ],
            PREPEND_PUNCTUATIONS,
            APPEND_PUNCTUATIONS,
        );
        let texts: Vec<_> = merged.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, [" Say", " \"Hi\""]);
        assert_eq!(merged[1].tokens, 1..4);
        assert_eq!((merged[1].start_s, merged[1].end_s), (0.4, 0.8));
    }

    #[test]
    fn several_marks_in_a_row_all_merge() {
        let merged = merge_punctuations(
            vec![
                word(" go", 0..1, 0.0, 0.2),
                word("!", 1..2, 0.2, 0.3),
                word("\"", 2..3, 0.3, 0.4),
                word(" (", 3..4, 0.4, 0.5),
                word("ok", 4..5, 0.5, 0.9),
            ],
            PREPEND_PUNCTUATIONS,
            APPEND_PUNCTUATIONS,
        );
        let texts: Vec<_> = merged.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, [" go!\"", " (ok"]);
        assert_eq!(
            (merged[0].tokens.clone(), merged[1].tokens.clone()),
            (0..3, 3..5)
        );
    }

    #[test]
    fn a_word_that_is_not_punctuation_is_left_alone() {
        let input = vec![word(" a", 0..1, 0.0, 0.1), word(" b", 1..2, 0.1, 0.2)];
        assert_eq!(
            merge_punctuations(input.clone(), PREPEND_PUNCTUATIONS, APPEND_PUNCTUATIONS),
            input
        );
        assert!(merge_punctuations(vec![], PREPEND_PUNCTUATIONS, APPEND_PUNCTUATIONS).is_empty());
    }

    #[test]
    fn the_encoder_has_a_frame_per_320_samples() {
        assert_eq!(encoder_frames(480_000), 1500);
        assert_eq!(encoder_frames(16_000), 50);
        assert_eq!(encoder_frames(319), 0);
        assert_eq!(SECONDS_PER_FRAME * encoder_frames(480_000) as f32, 30.0);
    }

    #[test]
    fn matrix_checks_its_size() {
        assert!(Matrix::new(2, 3, vec![0.0; 5]).is_err());
        assert!(Matrix::new(2, 3, vec![0.0; 6]).is_ok());
    }
}
