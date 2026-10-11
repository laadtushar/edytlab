//! The greedy decoding loop, behind a trait so it can be tested without a model.
//!
//! This is plan step 3 on #384. The loop is a port of the relevant parts of
//! `whisper/decoding.py` in <https://github.com/openai/whisper> (MIT), release
//! 20250625: `DecodingTask._main_loop`, `detect_language`, `SuppressBlank`,
//! `SuppressTokens` and `ApplyTimestampRules`, with `GreedyDecoder`'s
//! temperature-0 `argmax`. Everything that touches ONNX Runtime is behind
//! [`DecoderStep`], which takes token ids and the encoder's output and gives
//! back logits; the tests drive the loop with a scripted fake that emits
//! fixed logits.
//!
//! ## One window
//!
//! [`decode_window`] decodes one 30 s window whose encoder output is given:
//!
//! 1. **Language.** If none is given, [`detect_language`] runs one decoder
//!    step on `[SOT]` and takes the language token with the largest logit
//!    (probabilities are a softmax over the language tokens only).
//! 2. **Prompt.** `[SOT, language, transcribe, notimestamps]`, fed in one
//!    step. The row of logits at `SOT` gives `P(nospeech)` (the
//!    `<|nocaptions|>` token, id 50362); the last row gives the first token.
//! 3. **No speech.** If `P(nospeech)` is above the threshold (0.6), the window
//!    is skipped and decoding stops there, one decoder step in. OpenAI's
//!    `transcribe` decodes the window first and then lets a high average
//!    log-probability override the skip; the plan on #384 says to skip on the
//!    probability alone, so this does, and saves the whole decode of a silent
//!    window.
//! 4. **Sample.** Each step masks the logits ([`DecodeConfig::suppress_tokens`]
//!    always; [`DecodeConfig::begin_suppress_tokens`], which hold the blank
//!    and `<|endoftext|>`, on the first token only), then takes the largest.
//!    Ties go to the lowest id. The recorded probability is the model's own,
//!    from the unmasked logits, so a mask does not inflate a word's
//!    confidence.
//! 5. **Stop** at `<|endoftext|>`, at [`DecodeConfig::max_new_tokens`] (224,
//!    half of Whisper's 448-token context), or when the output starts to loop
//!    ([`RepetitionRule`]). A loop is cut back to its first occurrence.
//!
//! ## Timestamp tokens
//!
//! The plan prompts with `<|notimestamps|>` and times words from the
//! cross-attention (plan step 4), so by default timestamp tokens and
//! `<|notimestamps|>` are masked out and never sampled.
//! [`DecodeConfig::with_timestamps`] switches to Whisper's other mode, which
//! omits `<|notimestamps|>` from the prompt and applies `ApplyTimestampRules`:
//! timestamps come in pairs (except directly before the end), never go
//! backwards, the first is at most [`DecodeConfig::max_initial_timestamp_index`]
//! steps in, and a timestamp is forced when their total probability beats
//! every text token. Segment splitting on those tokens is the chunking step's.
//!
//! ## What is not here
//!
//! No beam search, temperature, fallback or `initial_prompt`, and no
//! `generation_config.json` parser: the two token lists the loop needs are
//! the constants [`MULTILINGUAL_SUPPRESS_TOKENS`] and
//! [`MULTILINGUAL_BEGIN_SUPPRESS_TOKENS`]. Cross-attention for word timing is
//! not read from the decoder yet.

use std::ops::Range;

/// `suppress_tokens` from the pinned export's `generation_config.json`
/// (`onnx-community/whisper-base_timestamped` at
/// `608c49e61301901684bc36cac8f74b95ff6b5a8e`, sha256
/// `61070cf8de25b1e9256e8e102ded49d8d24a8369ed36ef84fdf21549e68125a0`): the
/// symbols and non-speech sounds OpenAI's `-1` setting suppresses, plus
/// `<|startoftranscript|>`, `<|translate|>`, `<|transcribe|>`,
/// `<|startoflm|>`, `<|startofprev|>` and `<|nocaptions|>`. The test model's
/// file (`whisper-tiny_timestamped` at
/// `517244293732ee2d58139af5814231b7e6830a0d`) has the same list.
///
/// The file is not one of the three files the plan pins, so the list is kept
/// here; `the_suppress_lists_are_the_pinned_generation_config` fingerprints it.
pub const MULTILINGUAL_SUPPRESS_TOKENS: [u32; 88] = [
    1, 2, 7, 8, 9, 10, 14, 25, 26, 27, 28, 29, 31, 58, 59, 60, 61, 62, 63, 90, 91, 92, 93, 359,
    503, 522, 542, 873, 893, 902, 918, 922, 931, 1350, 1853, 1982, 2460, 2627, 3246, 3253, 3268,
    3536, 3846, 3961, 4183, 4667, 6585, 6647, 7273, 9061, 9383, 10428, 10929, 11938, 12033, 12331,
    12562, 13793, 14157, 14635, 15265, 15618, 16553, 16604, 18362, 18956, 20075, 21675, 22520,
    26130, 26161, 26435, 28279, 29464, 31650, 32302, 32470, 36865, 42863, 47425, 49870, 50254,
    50258, 50358, 50359, 50360, 50361, 50362,
];

/// `begin_suppress_tokens` from the same file: the blank (`" "`, id 220) and
/// `<|endoftext|>`, so the first token is never a space or an immediate stop.
pub const MULTILINGUAL_BEGIN_SUPPRESS_TOKENS: [u32; 2] = [220, 50_257];

/// Why decoding could not run or could not finish.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The decoder session failed. The message is the backend's.
    #[error("decoder failed: {0}")]
    Backend(String),

    /// The decoder returned logits of the wrong shape: a different number of
    /// rows than tokens fed, or a different vocabulary width than the config.
    #[error(
        "decoder returned {rows} x {vocab} logits, expected {expected_rows} x {expected_vocab}"
    )]
    LogitsShape {
        rows: usize,
        vocab: usize,
        expected_rows: usize,
        expected_vocab: usize,
    },

    /// A NaN in the logits.
    #[error("decoder returned NaN logits")]
    NonFiniteLogits,

    /// Every token was masked: nothing can be sampled.
    #[error("no token can be sampled: every logit is masked")]
    NoViableToken,

    /// The language is not one of the language tokens.
    #[error("token {0} is not a language token")]
    BadLanguage(u32),

    /// The configuration contradicts itself or the vocabulary.
    #[error("invalid decode configuration: {0}")]
    BadConfig(String),

    /// The encoder output does not have the size it says.
    #[error("encoder states: {0}")]
    BadEncoderStates(String),
}

/// The ids of the special tokens the loop uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenIds {
    /// Width of the logits: 51 865 for the multilingual export.
    pub vocab_size: usize,
    /// `<|endoftext|>`.
    pub eot: u32,
    /// `<|startoftranscript|>`.
    pub sot: u32,
    /// `<|transcribe|>`.
    pub transcribe: u32,
    /// `<|nocaptions|>`, the token whose probability at SOT says "no speech".
    pub no_speech: u32,
    /// `<|notimestamps|>`.
    pub no_timestamps: u32,
    /// `<|0.00|>`: ids from here to the end of the vocabulary are timestamps.
    pub timestamp_begin: u32,
    /// The language tokens, `<|en|>` to `<|su|>`.
    pub languages: Range<u32>,
}

impl TokenIds {
    /// The multilingual export in the plan on #384: SOT 50258, transcribe
    /// 50359, notimestamps 50363, nospeech 50362, EOT 50257, languages
    /// 50259 to 50357, timestamps from 50364, 51 865 logits.
    pub fn multilingual() -> Self {
        Self {
            vocab_size: 51_865,
            eot: 50_257,
            sot: 50_258,
            transcribe: 50_359,
            no_speech: 50_362,
            no_timestamps: 50_363,
            timestamp_begin: 50_364,
            languages: 50_259..50_358,
        }
    }
}

/// When to give up on a window that is repeating itself.
///
/// The loop is cut when the last `p * r` sampled tokens are one block of `p`
/// tokens repeated `r` times, for some block length `p` up to `max_period`,
/// where `r` is at least `min_repeats` and `p * r` is at least `min_tokens`.
/// The second condition makes a single token need many repeats while a longer
/// phrase needs only a few.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepetitionRule {
    /// Longest block that is looked for.
    pub max_period: usize,
    /// Fewest repeats of any block.
    pub min_repeats: usize,
    /// Fewest tokens in the repeated stretch.
    pub min_tokens: usize,
}

impl Default for RepetitionRule {
    /// Blocks of up to 16 tokens, at least 4 repeats, at least 24 tokens: 24
    /// of the same token, 12 of a pair, 8 of three, 6 of four, down to 4 of
    /// any block of 6 to 16.
    fn default() -> Self {
        Self {
            max_period: 16,
            min_repeats: 4,
            min_tokens: 24,
        }
    }
}

/// How one window is decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeConfig {
    pub tokens: TokenIds,
    /// Masked on every step.
    pub suppress_tokens: Vec<u32>,
    /// Masked on the first sampled token only.
    pub begin_suppress_tokens: Vec<u32>,
    /// The most tokens sampled, `<|endoftext|>` included: 224.
    pub max_new_tokens: usize,
    /// Skip the window if `P(nospeech)` at SOT is above this: 0.6. `None`
    /// never skips.
    pub no_speech_threshold: Option<f32>,
    /// `None` never aborts on repetition.
    pub repetition: Option<RepetitionRule>,
    /// Sample timestamp tokens (Whisper's other mode) instead of masking them.
    pub with_timestamps: bool,
    /// With timestamps, the first one is at most this many 20 ms steps in:
    /// 50, one second, as the pinned `generation_config.json` has it.
    pub max_initial_timestamp_index: Option<u32>,
}

impl DecodeConfig {
    /// The plan's loop settings for `tokens`, with no token masked.
    pub fn new(tokens: TokenIds) -> Self {
        Self {
            tokens,
            suppress_tokens: Vec::new(),
            begin_suppress_tokens: Vec::new(),
            max_new_tokens: 224,
            no_speech_threshold: Some(0.6),
            repetition: Some(RepetitionRule::default()),
            with_timestamps: false,
            max_initial_timestamp_index: Some(50),
        }
    }

    /// The pinned multilingual export: [`TokenIds::multilingual`] and the two
    /// lists from its `generation_config.json`.
    pub fn multilingual() -> Self {
        Self {
            suppress_tokens: MULTILINGUAL_SUPPRESS_TOKENS.to_vec(),
            begin_suppress_tokens: MULTILINGUAL_BEGIN_SUPPRESS_TOKENS.to_vec(),
            ..Self::new(TokenIds::multilingual())
        }
    }

    /// Refuse a configuration that cannot work, before any decoder step.
    fn validate(&self) -> Result<(), DecodeError> {
        let bad = |why: String| Err(DecodeError::BadConfig(why));
        let ids = &self.tokens;
        let vocab = ids.vocab_size;
        for (name, id) in [
            ("eot", ids.eot),
            ("sot", ids.sot),
            ("transcribe", ids.transcribe),
            ("no_speech", ids.no_speech),
            ("no_timestamps", ids.no_timestamps),
        ] {
            if id as usize >= vocab {
                return bad(format!(
                    "{name} token {id} is outside the vocabulary of {vocab}"
                ));
            }
        }
        if ids.timestamp_begin as usize > vocab {
            return bad(format!(
                "timestamp_begin {} is past the vocabulary of {vocab}",
                ids.timestamp_begin
            ));
        }
        if ids.languages.is_empty() || ids.languages.end as usize > vocab {
            return bad(format!(
                "language tokens {:?} do not fit the vocabulary of {vocab}",
                ids.languages
            ));
        }
        for (name, list) in [
            ("suppress_tokens", &self.suppress_tokens),
            ("begin_suppress_tokens", &self.begin_suppress_tokens),
        ] {
            if let Some(id) = list.iter().find(|&&id| id as usize >= vocab) {
                return bad(format!(
                    "{name} has {id}, outside the vocabulary of {vocab}"
                ));
            }
        }
        if self.max_new_tokens == 0 {
            return bad("max_new_tokens is 0".into());
        }
        if let Some(rule) = &self.repetition {
            if rule.max_period == 0 || rule.min_repeats < 2 {
                return bad(format!(
                    "repetition rule {rule:?} needs a period of at least 1 and at least 2 repeats"
                ));
            }
        }
        Ok(())
    }
}

/// The encoder's output for one window: `[1, frames, width]` (`[1, 1500, 512]`
/// for `base`), row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct EncoderStates {
    data: Vec<f32>,
    frames: usize,
    width: usize,
}

impl EncoderStates {
    /// Wrap `data`, which must be `frames * width` long.
    pub fn new(data: Vec<f32>, frames: usize, width: usize) -> Result<Self, DecodeError> {
        if data.len() != frames * width {
            return Err(DecodeError::BadEncoderStates(format!(
                "{} values for {frames} frames of width {width}",
                data.len()
            )));
        }
        Ok(Self {
            data,
            frames,
            width,
        })
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }
}

/// Logits for each fed position: `rows` x `vocab_size`, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Logits {
    vocab: usize,
    data: Vec<f32>,
}

impl Logits {
    /// Wrap `data`, whose length must be a whole number of `vocab`-wide rows.
    pub fn new(vocab: usize, data: Vec<f32>) -> Result<Self, DecodeError> {
        if vocab == 0 || data.len() % vocab != 0 {
            return Err(DecodeError::LogitsShape {
                rows: 0,
                vocab: data.len(),
                expected_rows: 0,
                expected_vocab: vocab,
            });
        }
        Ok(Self { vocab, data })
    }

    pub fn rows(&self) -> usize {
        self.data.len().checked_div(self.vocab).unwrap_or(0)
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab
    }

    pub fn row(&self, i: usize) -> &[f32] {
        &self.data[i * self.vocab..(i + 1) * self.vocab]
    }
}

/// One step of the Whisper decoder, with whatever cache the implementation
/// keeps.
///
/// `new_tokens` are the tokens the decoder has **not** seen yet in this
/// sequence, and `past_len` is how many it has seen: the first call of a
/// sequence has `past_len == 0` and the whole prompt, every later call has
/// one token. A call with `past_len == 0` starts a new sequence, so the
/// implementation drops its cache then (for the merged ONNX decoder:
/// `use_cache_branch = past_len > 0`). The result has one row of logits per
/// entry of `new_tokens`.
pub trait DecoderStep {
    fn step(
        &mut self,
        new_tokens: &[u32],
        past_len: usize,
        encoder: &EncoderStates,
    ) -> Result<Logits, DecodeError>;
}

/// The language the decoder thinks is spoken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LanguageDetection {
    /// The language token (`<|en|>` is 50259).
    pub token: u32,
    /// Its probability among the language tokens.
    pub prob: f32,
}

/// Guess the language from one decoder step on `[SOT]`.
pub fn detect_language<D: DecoderStep>(
    decoder: &mut D,
    encoder: &EncoderStates,
    ids: &TokenIds,
) -> Result<LanguageDetection, DecodeError> {
    let logits = decoder.step(&[ids.sot], 0, encoder)?;
    check_logits(&logits, 1, ids.vocab_size)?;
    let languages = ids
        .languages
        .clone()
        .map(|t| (t, logits.row(0)[t as usize]))
        .collect::<Vec<_>>();
    // The softmax is over the language tokens only: everything else is
    // masked before the probabilities are taken, as `detect_language` does.
    let (token, best) = languages
        .iter()
        .copied()
        .reduce(|a, b| if b.1 > a.1 { b } else { a })
        .ok_or(DecodeError::NoViableToken)?;
    let total: f64 = languages
        .iter()
        .map(|&(_, l)| f64::from(l - best).exp())
        .sum();
    Ok(LanguageDetection {
        token,
        prob: (1.0 / total) as f32,
    })
}

/// Why a window stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model sampled `<|endoftext|>`.
    EndOfText,
    /// [`DecodeConfig::max_new_tokens`] were sampled.
    MaxLength,
    /// The output started to loop and was cut back ([`RepetitionRule`]).
    Repetition,
    /// `P(nospeech)` was above the threshold; no tokens were sampled.
    NoSpeech,
}

/// What came out of one window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowDecode {
    /// The language token used, given or detected.
    pub language: u32,
    /// The detected language's probability; `None` if one was given.
    pub language_prob: Option<f32>,
    /// The sampled tokens, without the prompt and without `<|endoftext|>`.
    /// With [`DecodeConfig::with_timestamps`] they include timestamp tokens.
    pub tokens: Vec<u32>,
    /// For each token, the model's probability of it (softmax of the unmasked
    /// logits), in `[0, 1]`.
    pub token_probs: Vec<f32>,
    /// `P(nospeech)` at the SOT position, whether or not it caused a skip.
    pub no_speech_prob: f32,
    pub stop: StopReason,
}

/// Decode one window. `language` is a language token, or `None` to detect it.
pub fn decode_window<D: DecoderStep>(
    decoder: &mut D,
    encoder: &EncoderStates,
    config: &DecodeConfig,
    language: Option<u32>,
) -> Result<WindowDecode, DecodeError> {
    config.validate()?;
    let ids = &config.tokens;
    let (language, language_prob) = match language {
        Some(token) if ids.languages.contains(&token) => (token, None),
        Some(token) => return Err(DecodeError::BadLanguage(token)),
        None => {
            let found = detect_language(decoder, encoder, ids)?;
            (found.token, Some(found.prob))
        }
    };

    let mut prompt = vec![ids.sot, language, ids.transcribe];
    if !config.with_timestamps {
        prompt.push(ids.no_timestamps);
    }
    let first = decoder.step(&prompt, 0, encoder)?;
    check_logits(&first, prompt.len(), ids.vocab_size)?;

    // `P(nospeech)` is read where the SOT token was fed, the first row.
    let no_speech_prob = softmax_prob(first.row(0), ids.no_speech);
    let result = |tokens, token_probs, stop| WindowDecode {
        language,
        language_prob,
        tokens,
        token_probs,
        no_speech_prob,
        stop,
    };
    if config
        .no_speech_threshold
        .is_some_and(|limit| no_speech_prob > limit)
    {
        return Ok(result(Vec::new(), Vec::new(), StopReason::NoSpeech));
    }

    let mut logits = first.row(first.rows() - 1).to_vec();
    let mut tokens: Vec<u32> = Vec::new();
    let mut token_probs: Vec<f32> = Vec::new();
    let stop = loop {
        let mut masked = logits.clone();
        apply_filters(&mut masked, &tokens, config);
        let next = argmax(&masked)?;
        if next == ids.eot {
            break StopReason::EndOfText;
        }
        tokens.push(next);
        token_probs.push(softmax_prob(&logits, next));

        if let Some(keep) = config
            .repetition
            .as_ref()
            .and_then(|rule| repeated_tail(&tokens, rule))
        {
            tokens.truncate(keep);
            token_probs.truncate(keep);
            break StopReason::Repetition;
        }
        if tokens.len() >= config.max_new_tokens {
            break StopReason::MaxLength;
        }

        // The decoder has seen the prompt and every sampled token but the one
        // just chosen.
        let next_logits = decoder.step(&[next], prompt.len() + tokens.len() - 1, encoder)?;
        check_logits(&next_logits, 1, ids.vocab_size)?;
        logits = next_logits.row(0).to_vec();
    };
    Ok(result(tokens, token_probs, stop))
}

/// Check the shape the loop depends on, and that nothing is NaN.
fn check_logits(
    logits: &Logits,
    expected_rows: usize,
    expected_vocab: usize,
) -> Result<(), DecodeError> {
    if logits.rows() != expected_rows || logits.vocab_size() != expected_vocab {
        return Err(DecodeError::LogitsShape {
            rows: logits.rows(),
            vocab: logits.vocab_size(),
            expected_rows,
            expected_vocab,
        });
    }
    if logits.data.iter().any(|l| l.is_nan()) {
        return Err(DecodeError::NonFiniteLogits);
    }
    Ok(())
}

/// The softmax probability of `id` in `row`.
fn softmax_prob(row: &[f32], id: u32) -> f32 {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let total: f64 = row.iter().map(|&l| f64::from(l - max).exp()).sum();
    (f64::from(row[id as usize] - max).exp() / total) as f32
}

/// The id of the largest logit, the lowest on a tie; an error if every logit
/// is masked.
fn argmax(logits: &[f32]) -> Result<u32, DecodeError> {
    let mut best: Option<(usize, f32)> = None;
    for (i, &l) in logits.iter().enumerate() {
        if l > f32::NEG_INFINITY && best.is_none_or(|(_, b)| l > b) {
            best = Some((i, l));
        }
    }
    best.map(|(i, _)| i as u32)
        .ok_or(DecodeError::NoViableToken)
}

/// Mask `logits` for the next token, given the tokens sampled so far.
fn apply_filters(logits: &mut [f32], sampled: &[u32], config: &DecodeConfig) {
    let ids = &config.tokens;
    let mask = |logits: &mut [f32], range: Range<usize>| logits[range].fill(f32::NEG_INFINITY);

    // SuppressBlank: the blank and the end can't be the first token.
    if sampled.is_empty() {
        for &t in &config.begin_suppress_tokens {
            logits[t as usize] = f32::NEG_INFINITY;
        }
    }
    // SuppressTokens.
    for &t in &config.suppress_tokens {
        logits[t as usize] = f32::NEG_INFINITY;
    }
    // `<|notimestamps|>` belongs to the prompt, never to the output.
    logits[ids.no_timestamps as usize] = f32::NEG_INFINITY;

    let begin = ids.timestamp_begin as usize;
    if !config.with_timestamps {
        mask(logits, begin..logits.len());
        return;
    }

    // ApplyTimestampRules. Timestamps come in pairs, except directly before
    // the end; they never go back; the first is early; and one is forced if
    // together they are likelier than any text token.
    let is_timestamp = |t: u32| t >= ids.timestamp_begin;
    let last_was_timestamp = sampled.last().is_some_and(|&t| is_timestamp(t));
    let penultimate_was_timestamp = sampled.len() < 2 || is_timestamp(sampled[sampled.len() - 2]);
    if last_was_timestamp {
        if penultimate_was_timestamp {
            mask(logits, begin..logits.len()); // has to be text
        } else {
            mask(logits, 0..ids.eot as usize); // cannot be text
        }
    }
    if let Some(&last) = sampled.iter().rev().find(|&&t| is_timestamp(t)) {
        // Never earlier than the last timestamp, and a segment is at least
        // one step, unless the last token ended one.
        let earliest = if last_was_timestamp && !penultimate_was_timestamp {
            last
        } else {
            last + 1
        };
        mask(logits, begin..(earliest as usize).min(logits.len()));
    }
    if sampled.is_empty() {
        mask(logits, 0..begin);
        if let Some(steps) = config.max_initial_timestamp_index {
            let last_allowed = begin + steps as usize;
            mask(logits, (last_allowed + 1).min(logits.len())..logits.len());
        }
    }
    // The total probability of the timestamps against the best text token.
    // Both are log-probabilities under the same normaliser, so it cancels.
    let max_text = logits[..begin]
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let max_stamp = logits[begin..]
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    if max_stamp > f32::NEG_INFINITY {
        let log_sum_stamps = f64::from(max_stamp)
            + logits[begin..]
                .iter()
                .map(|&l| f64::from(l - max_stamp).exp())
                .sum::<f64>()
                .ln();
        if log_sum_stamps > f64::from(max_text) {
            mask(logits, 0..begin);
        }
    }
}

/// If the end of `tokens` is a loop by `rule`, how many tokens to keep: up to
/// and including the first occurrence of the repeated block.
fn repeated_tail(tokens: &[u32], rule: &RepetitionRule) -> Option<usize> {
    let n = tokens.len();
    for period in 1..=rule.max_period {
        let repeats = rule.min_repeats.max(rule.min_tokens.div_ceil(period));
        let span = period * repeats;
        if span > n {
            continue;
        }
        // The last `span` tokens are a block of `period` repeated `repeats`
        // times if each equals the one a period before it.
        if (n - span + period..n).all(|i| tokens[i] == tokens[i - period]) {
            return Some(n - period * (repeats - 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    // ---- a small vocabulary, laid out like the real one ----------------------
    //
    //  0..=9    text            10 eot          11 sot
    //  12..=14  languages       15 transcribe   16 nospeech   17 notimestamps
    //  18..=38  timestamps <|0.00|> to <|0.40|>
    const VOCAB: usize = 39;
    const EOT: u32 = 10;
    const SOT: u32 = 11;
    const EN: u32 = 12;
    const DE: u32 = 13;
    const FR: u32 = 14;
    const TRANSCRIBE: u32 = 15;
    const NO_SPEECH: u32 = 16;
    const NO_TIMESTAMPS: u32 = 17;
    const TS: u32 = 18; // <|0.00|>

    fn mini_ids() -> TokenIds {
        TokenIds {
            vocab_size: VOCAB,
            eot: EOT,
            sot: SOT,
            transcribe: TRANSCRIBE,
            no_speech: NO_SPEECH,
            no_timestamps: NO_TIMESTAMPS,
            timestamp_begin: TS,
            languages: EN..FR + 1,
        }
    }

    fn mini() -> DecodeConfig {
        let mut config = DecodeConfig::new(mini_ids());
        config.repetition = None;
        config
    }

    fn encoder() -> EncoderStates {
        EncoderStates::new(vec![0.0; 6], 3, 2).unwrap()
    }

    // ---- the scripted fake -------------------------------------------------

    /// One row of logits written sparsely: every id not listed has logit 0.
    type Row = Vec<(u32, f32)>;

    /// A decoder that answers each call from a script and records the calls.
    struct Scripted {
        vocab: usize,
        script: VecDeque<Result<Vec<Row>, String>>,
        calls: Vec<(Vec<u32>, usize)>,
    }

    impl Scripted {
        /// `script[i]` is the rows returned by call `i`.
        fn new(script: Vec<Vec<Row>>) -> Self {
            Self {
                vocab: VOCAB,
                script: script.into_iter().map(Ok).collect(),
                calls: Vec::new(),
            }
        }
    }

    impl DecoderStep for Scripted {
        fn step(
            &mut self,
            new_tokens: &[u32],
            past_len: usize,
            _: &EncoderStates,
        ) -> Result<Logits, DecodeError> {
            self.calls.push((new_tokens.to_vec(), past_len));
            let rows = self
                .script
                .pop_front()
                .unwrap_or_else(|| {
                    panic!(
                        "the loop made call {} but the script has no more",
                        self.calls.len()
                    )
                })
                .map_err(DecodeError::Backend)?;
            let mut data = vec![0.0; rows.len() * self.vocab];
            for (r, row) in rows.iter().enumerate() {
                for (id, logit) in row {
                    data[r * self.vocab + *id as usize] = *logit;
                }
            }
            Logits::new(self.vocab, data)
        }
    }

    /// A row that makes `id` the clear favourite (logit 10 against 0).
    fn favour(id: u32) -> Row {
        vec![(id, 10.0)]
    }

    /// The rows for a prompt of `n` tokens: all quiet except the last.
    fn prompt_rows(n: usize, last: Row) -> Vec<Row> {
        let mut rows = vec![Vec::new(); n];
        rows[n - 1] = last;
        rows
    }

    /// The softmax probability of a token whose logit is `logit` while the
    /// other `VOCAB - 1` are 0.
    fn prob_of(logit: f64) -> f32 {
        (logit.exp() / (logit.exp() + (VOCAB - 1) as f64)) as f32
    }

    /// The logit that gives `id` probability `p` against `VOCAB - 1` zeros.
    fn logit_for(p: f64) -> f32 {
        (p * (VOCAB - 1) as f64 / (1.0 - p)).ln() as f32
    }

    fn run(
        script: Vec<Vec<Row>>,
        config: &DecodeConfig,
        language: Option<u32>,
    ) -> (WindowDecode, Vec<(Vec<u32>, usize)>) {
        let mut fake = Scripted::new(script);
        let out = decode_window(&mut fake, &encoder(), config, language).unwrap();
        (out, fake.calls)
    }

    // ---- the real ids and lists ------------------------------------------------

    #[test]
    fn the_multilingual_ids_are_the_plans() {
        let ids = TokenIds::multilingual();
        assert_eq!(ids.vocab_size, 51_865);
        assert_eq!(ids.eot, 50_257);
        assert_eq!(ids.sot, 50_258);
        assert_eq!(ids.transcribe, 50_359);
        assert_eq!(ids.no_speech, 50_362);
        assert_eq!(ids.no_timestamps, 50_363);
        assert_eq!(ids.timestamp_begin, 50_364);
        assert_eq!(ids.languages, 50_259..50_358);
        // 99 languages, <|en|> to <|su|>; 1501 timestamps, <|0.00|> to <|30.00|>.
        assert_eq!(ids.languages.len(), 99);
        assert_eq!(ids.vocab_size - ids.timestamp_begin as usize, 1501);
    }

    #[test]
    fn the_suppress_lists_are_the_pinned_generation_config() {
        let list = MULTILINGUAL_SUPPRESS_TOKENS;
        assert_eq!(list.len(), 88);
        assert!(
            list.windows(2).all(|w| w[0] < w[1]),
            "the file lists them in ascending order"
        );
        assert_eq!(list.iter().map(|&t| u64::from(t)).sum::<u64>(), 1_128_152);
        // Never sampled: SOT, translate, transcribe, startoflm, startofprev, nocaptions.
        assert_eq!(
            list[list.len() - 6..],
            [50_258, 50_358, 50_359, 50_360, 50_361, 50_362]
        );
        assert_eq!(MULTILINGUAL_BEGIN_SUPPRESS_TOKENS, [220, 50_257]);

        let config = DecodeConfig::multilingual();
        assert_eq!(config.tokens, TokenIds::multilingual());
        assert_eq!(config.suppress_tokens, list);
        assert_eq!(
            config.begin_suppress_tokens,
            MULTILINGUAL_BEGIN_SUPPRESS_TOKENS
        );
        assert_eq!(config.max_new_tokens, 224);
        assert_eq!(config.no_speech_threshold, Some(0.6));
        assert_eq!(config.repetition, Some(RepetitionRule::default()));
        assert!(!config.with_timestamps);
        assert_eq!(config.max_initial_timestamp_index, Some(50));
        assert!(list
            .iter()
            .all(|&t| (t as usize) < config.tokens.vocab_size));
    }

    // ---- greedy sampling -----------------------------------------------------

    #[test]
    fn a_scripted_sequence_decodes_until_end_of_text() {
        let script = vec![
            prompt_rows(4, favour(3)),
            vec![favour(4)],
            vec![favour(5)],
            vec![favour(EOT)],
        ];
        let (out, calls) = run(script, &mini(), Some(DE));

        assert_eq!(out.tokens, [3, 4, 5]);
        assert_eq!(out.stop, StopReason::EndOfText);
        assert_eq!(out.language, DE);
        assert_eq!(out.language_prob, None);
        // The prompt in one step, then one token at a time with the count the
        // decoder has already seen.
        assert_eq!(
            calls,
            [
                (vec![SOT, DE, TRANSCRIBE, NO_TIMESTAMPS], 0),
                (vec![3], 4),
                (vec![4], 5),
                (vec![5], 6)
            ]
        );
        assert_eq!(out.token_probs.len(), 3);
        for p in &out.token_probs {
            assert!((p - prob_of(10.0)).abs() < 1e-6, "{p}");
        }
        // Nothing favoured nospeech at SOT: 1 / 39.
        assert!((out.no_speech_prob - 1.0 / VOCAB as f32).abs() < 1e-6);
    }

    #[test]
    fn the_probability_is_the_models_not_the_masked_ones() {
        // Token 3 has logit 10 and is masked; token 4 has logit 5 and is
        // picked. Its probability is 5 against all 39, token 3 included.
        let mut config = mini();
        config.suppress_tokens = vec![3];
        let row = vec![(3, 10.0), (4, 5.0)];
        let (out, _) = run(
            vec![prompt_rows(4, row), vec![favour(EOT)]],
            &config,
            Some(EN),
        );

        assert_eq!(out.tokens, [4]);
        let z = 10f64.exp() + 5f64.exp() + (VOCAB - 2) as f64;
        assert!((out.token_probs[0] - (5f64.exp() / z) as f32).abs() < 1e-6);
    }

    #[test]
    fn ties_go_to_the_lowest_id() {
        let (out, _) = run(
            vec![prompt_rows(4, vec![]), vec![favour(EOT)]],
            &mini(),
            Some(EN),
        );
        assert_eq!(out.tokens, [0]);
    }

    // ---- language ------------------------------------------------------------

    #[test]
    fn the_language_is_detected_from_one_step_on_sot_when_not_given() {
        // Text token 4 is the loudest at SOT, but only languages count: 13
        // wins among 12, 13 and 14.
        let at_sot = vec![(4, 9.0), (EN, 1.0), (DE, 3.0), (FR, 2.0)];
        let script = vec![vec![at_sot], prompt_rows(4, favour(EOT))];
        let (out, calls) = run(script, &mini(), None);

        assert_eq!(out.language, DE);
        let expected = 3f64.exp() / (1f64.exp() + 3f64.exp() + 2f64.exp());
        assert!((out.language_prob.unwrap() - expected as f32).abs() < 1e-6);
        assert_eq!(calls[0], (vec![SOT], 0));
        assert_eq!(calls[1], (vec![SOT, DE, TRANSCRIBE, NO_TIMESTAMPS], 0));
        assert_eq!(calls.len(), 2);
        assert!(out.tokens.is_empty());
    }

    #[test]
    fn detect_language_alone_makes_one_step() {
        let mut fake = Scripted::new(vec![vec![vec![(FR, 4.0)]]]);
        let found = detect_language(&mut fake, &encoder(), &mini_ids()).unwrap();
        assert_eq!(found.token, FR);
        let expected = 4f64.exp() / (4f64.exp() + 2.0);
        assert!((found.prob - expected as f32).abs() < 1e-6);
        assert_eq!(fake.calls, [(vec![SOT], 0)]);
    }

    #[test]
    fn a_language_that_is_not_a_language_token_is_an_error() {
        for bad in [3, EOT, TRANSCRIBE, 15, 99] {
            let mut fake = Scripted::new(vec![]);
            let err = decode_window(&mut fake, &encoder(), &mini(), Some(bad)).unwrap_err();
            assert!(
                matches!(err, DecodeError::BadLanguage(t) if t == bad),
                "{bad}: {err:?}"
            );
            assert!(fake.calls.is_empty());
        }
    }

    // ---- masks ---------------------------------------------------------------

    #[test]
    fn suppressed_tokens_are_never_picked() {
        let mut config = mini();
        config.suppress_tokens = vec![3, 7];
        let row = vec![(3, 10.0), (7, 9.0), (4, 5.0)];
        let (out, _) = run(
            vec![prompt_rows(4, row.clone()), vec![row], vec![favour(EOT)]],
            &config,
            Some(EN),
        );
        assert_eq!(out.tokens, [4, 4]);
    }

    #[test]
    fn begin_suppressed_tokens_are_masked_on_the_first_token_only() {
        let mut config = mini();
        config.begin_suppress_tokens = vec![9, EOT];
        // First token: 9 (10) and EOT (9) are masked, 4 (1) is picked. Second
        // token: 9 is allowed again. Third: EOT stops.
        let first = vec![(9, 10.0), (EOT, 9.0), (4, 1.0)];
        let script = vec![prompt_rows(4, first), vec![favour(9)], vec![favour(EOT)]];
        let (out, _) = run(script, &config, Some(EN));
        assert_eq!(out.tokens, [4, 9]);
        assert_eq!(out.stop, StopReason::EndOfText);
    }

    #[test]
    fn timestamp_tokens_and_notimestamps_are_never_sampled_by_default() {
        // The favourites are a timestamp, <|notimestamps|>, and the last
        // timestamp; text token 5 is the best that is left.
        let row = vec![
            (TS + 2, 10.0),
            (NO_TIMESTAMPS, 9.0),
            (VOCAB as u32 - 1, 8.0),
            (5, 1.0),
        ];
        let (out, _) = run(
            vec![prompt_rows(4, row), vec![favour(EOT)]],
            &mini(),
            Some(EN),
        );
        assert_eq!(out.tokens, [5]);
    }

    // ---- stopping ------------------------------------------------------------

    #[test]
    fn it_stops_at_the_cap_without_asking_for_another_step() {
        let mut config = mini();
        config.max_new_tokens = 3;
        // Exactly three scripted calls: the prompt and two more. A fourth
        // call would panic in the fake.
        let script = vec![prompt_rows(4, favour(1)), vec![favour(2)], vec![favour(3)]];
        let (out, calls) = run(script, &config, Some(EN));

        assert_eq!(out.tokens, [1, 2, 3]);
        assert_eq!(out.stop, StopReason::MaxLength);
        assert_eq!(calls.len(), 3);
    }

    #[test]
    fn the_last_slot_can_still_be_end_of_text() {
        let mut config = mini();
        config.max_new_tokens = 3;
        let script = vec![
            prompt_rows(4, favour(1)),
            vec![favour(2)],
            vec![favour(EOT)],
        ];
        let (out, _) = run(script, &config, Some(EN));
        assert_eq!(out.tokens, [1, 2]);
        assert_eq!(out.stop, StopReason::EndOfText);
    }

    // ---- no speech -----------------------------------------------------------

    #[test]
    fn a_window_that_is_probably_silence_is_skipped_after_one_step() {
        let loud = vec![(NO_SPEECH, logit_for(0.61))];
        let script = vec![vec![loud, vec![], vec![], favour(3)]];
        let (out, calls) = run(script, &mini(), Some(EN));

        assert_eq!(out.stop, StopReason::NoSpeech);
        assert!(out.tokens.is_empty() && out.token_probs.is_empty());
        assert!(
            (out.no_speech_prob - 0.61).abs() < 1e-5,
            "{}",
            out.no_speech_prob
        );
        assert_eq!(calls.len(), 1, "no step after the prompt");
    }

    #[test]
    fn a_window_just_under_the_threshold_is_decoded() {
        let quiet = vec![(NO_SPEECH, logit_for(0.59))];
        let script = vec![vec![quiet, vec![], vec![], favour(3)], vec![favour(EOT)]];
        let (out, _) = run(script, &mini(), Some(EN));
        assert_eq!(out.stop, StopReason::EndOfText);
        assert_eq!(out.tokens, [3]);
        assert!((out.no_speech_prob - 0.59).abs() < 1e-5);
    }

    #[test]
    fn nospeech_is_read_at_the_sot_row_not_the_last() {
        // The last row, where the first token comes from, is sure of
        // `nospeech`; the SOT row is not. Decoding goes on.
        let script = vec![prompt_rows(4, favour(NO_SPEECH)), vec![favour(EOT)]];
        let mut config = mini();
        config.suppress_tokens = vec![NO_SPEECH];
        let (out, _) = run(script, &config, Some(EN));
        assert_eq!(out.stop, StopReason::EndOfText);
        assert!(out.no_speech_prob < 0.1);
    }

    #[test]
    fn without_a_threshold_nothing_is_skipped() {
        let mut config = mini();
        config.no_speech_threshold = None;
        let script = vec![
            vec![favour(NO_SPEECH), vec![], vec![], vec![]],
            vec![favour(EOT)],
        ];
        let (out, _) = run(script, &config, Some(EN));
        assert_eq!(out.stop, StopReason::EndOfText);
        assert!(out.no_speech_prob > 0.99);
    }

    // ---- repetition ----------------------------------------------------------

    fn rule(max_period: usize, min_repeats: usize, min_tokens: usize) -> RepetitionRule {
        RepetitionRule {
            max_period,
            min_repeats,
            min_tokens,
        }
    }

    #[test]
    fn repeated_tail_finds_a_block_repeated_enough_times() {
        let r = rule(4, 3, 6);
        // Period 2, three repeats, 6 tokens: keep up to the first "3, 4".
        assert_eq!(repeated_tail(&[7, 3, 4, 3, 4, 3, 4], &r), Some(3));
        // Period 1 needs 6 of the same token.
        assert_eq!(repeated_tail(&[5, 5, 5, 5, 5], &r), None);
        assert_eq!(repeated_tail(&[1, 5, 5, 5, 5, 5, 5], &r), Some(2));
        // Period 3 with three repeats is 9 tokens.
        assert_eq!(repeated_tail(&[1, 2, 3, 1, 2, 3, 1, 2, 3], &r), Some(3));
        // Two repeats are not enough, and neither is a near miss.
        assert_eq!(repeated_tail(&[3, 4, 3, 4], &r), None);
        assert_eq!(repeated_tail(&[3, 4, 3, 4, 3, 5], &r), None);
        // A block longer than `max_period` is not looked for.
        assert_eq!(
            repeated_tail(&[1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5], &r),
            None
        );
        // Only the tail counts.
        assert_eq!(repeated_tail(&[3, 4, 3, 4, 3, 4, 9], &r), None);
        assert_eq!(repeated_tail(&[], &r), None);
    }

    #[test]
    fn the_default_rule_wants_24_tokens_of_it() {
        let r = RepetitionRule::default();
        assert_eq!(repeated_tail(&[8; 23], &r), None);
        assert_eq!(repeated_tail(&[8; 24], &r), Some(1));
        let pair: Vec<u32> = [1, 2].repeat(11);
        assert_eq!(repeated_tail(&pair, &r), None);
        assert_eq!(repeated_tail(&[1, 2].repeat(12), &r), Some(2));
        // Four repeats of a block of 6 are 24 tokens.
        assert_eq!(repeated_tail(&[1, 2, 3, 4, 5, 6].repeat(4), &r), Some(6));
        assert_eq!(repeated_tail(&[1, 2, 3, 4, 5, 6].repeat(3), &r), None);
        // Ordinary text, however long, is not a loop.
        let prose: Vec<u32> = (0..200).map(|i| (i * 7 + i / 3) % 97).collect();
        assert_eq!(repeated_tail(&prose, &r), None);
    }

    #[test]
    fn a_window_that_loops_is_cut_back_to_the_first_occurrence() {
        let mut config = mini();
        config.repetition = Some(rule(4, 3, 6));
        // 7, then "3 4" over and over. The loop is recognised once six tokens
        // of it are out (seven tokens sampled), and cut to 7, 3, 4.
        let loop_tokens = [7, 3, 4, 3, 4, 3, 4];
        let mut script = vec![prompt_rows(4, favour(loop_tokens[0]))];
        script.extend(loop_tokens[1..].iter().map(|&t| vec![favour(t)]));
        let (out, calls) = run(script, &config, Some(EN));

        assert_eq!(out.stop, StopReason::Repetition);
        assert_eq!(out.tokens, [7, 3, 4]);
        assert_eq!(out.token_probs.len(), 3);
        assert_eq!(
            calls.len(),
            7,
            "the prompt and six single-token steps; none after the abort"
        );
    }

    #[test]
    fn two_repeats_are_not_a_loop() {
        let mut config = mini();
        config.repetition = Some(rule(4, 3, 6));
        let tokens = [7, 3, 4, 3, 4, 8];
        let mut script = vec![prompt_rows(4, favour(tokens[0]))];
        script.extend(tokens[1..].iter().map(|&t| vec![favour(t)]));
        script.push(vec![favour(EOT)]);
        let (out, _) = run(script, &config, Some(EN));
        assert_eq!(out.stop, StopReason::EndOfText);
        assert_eq!(out.tokens, tokens);
    }

    // ---- timestamp mode ------------------------------------------------------

    fn timestamped() -> DecodeConfig {
        let mut config = mini();
        config.with_timestamps = true;
        config.max_initial_timestamp_index = Some(5);
        config
    }

    #[test]
    fn timestamp_mode_prompts_without_notimestamps_and_starts_on_a_timestamp() {
        // Text token 3 is the favourite but the first token must be a
        // timestamp, and at most 5 steps in: index 4 beats index 15.
        let first = vec![(3, 10.0), (TS + 15, 5.0), (TS + 4, 2.0)];
        let script = vec![prompt_rows(3, first), vec![favour(EOT)]];
        let (out, calls) = run(script, &timestamped(), Some(EN));

        assert_eq!(calls[0], (vec![SOT, EN, TRANSCRIBE], 0));
        assert_eq!(out.tokens, [TS + 4]);
        assert_eq!(out.stop, StopReason::EndOfText);
    }

    #[test]
    fn timestamps_come_in_pairs_and_never_go_back() {
        // One segment, start ts4 / text 3 / end ts7, then a restart at ts7
        // (allowed straight after an end) and the end of the text.
        let script = vec![
            // First token: only a timestamp, at most 5 steps in. Text 3 is the
            // favourite but is masked; ts4 beats the other timestamps.
            prompt_rows(3, vec![(3, 10.0), (TS + 4, 2.0)]),
            // After a lone start timestamp the next token is not a timestamp:
            // ts9 is the favourite and is masked, text 3 (2) beats the rest.
            vec![vec![(TS + 9, 10.0), (3, 2.0)]],
            // After text, a timestamp must be later than the start: ts4 is
            // masked, ts7 (3) is the best of the rest.
            vec![vec![(TS + 4, 10.0), (TS + 7, 3.0)]],
            // After an end timestamp, text is masked, and so is an earlier
            // timestamp (ts5); the same one is allowed. ts7 (1) beats EOT (0.5).
            vec![vec![(5, 10.0), (TS + 5, 9.0), (TS + 7, 1.0), (EOT, 0.5)]],
            // After two timestamps in a row the next is not one: the favourite
            // ts9 is masked and EOT (2) beats text (1).
            vec![vec![(TS + 9, 10.0), (EOT, 2.0), (5, 1.0)]],
        ];
        let (out, _) = run(script, &timestamped(), Some(EN));

        assert_eq!(out.tokens, [TS + 4, 3, TS + 7, TS + 7]);
        assert_eq!(out.stop, StopReason::EndOfText);
    }

    #[test]
    fn a_timestamp_is_forced_when_together_they_outweigh_every_text_token() {
        // After "ts0, 3" text is allowed. Text 4 has logit 1.0; each of the 21
        // timestamps has 0.5, a total of e^0.5 * 21 = 34.6 against e^1 = 2.7.
        // So text is banned. ts0 is banned too (a segment is at least one
        // step), and the lowest of the rest is ts1.
        let many: Row = (0..21).map(|i| (TS + i, 0.5)).chain([(4, 1.0)]).collect();
        let script = vec![
            prompt_rows(3, vec![(3, 5.0), (TS, 2.0)]),
            vec![vec![(3, 10.0), (TS + 3, 2.0)]],
            vec![many],
            vec![favour(EOT)],
        ];
        let mut config = timestamped();
        config.max_initial_timestamp_index = None;
        let (out, _) = run(script, &config, Some(EN));
        assert_eq!(out.tokens, [TS, 3, TS + 1]);
    }

    // ---- errors --------------------------------------------------------------

    #[test]
    fn a_backend_error_is_passed_up() {
        let mut fake = Scripted::new(vec![]);
        fake.script.push_back(Err("session lost".into()));
        let err = decode_window(&mut fake, &encoder(), &mini(), Some(EN)).unwrap_err();
        assert!(matches!(err, DecodeError::Backend(m) if m == "session lost"));
    }

    #[test]
    fn logits_of_the_wrong_shape_are_an_error() {
        // Three rows for a four-token prompt.
        let mut fake = Scripted::new(vec![vec![vec![]; 3]]);
        let err = decode_window(&mut fake, &encoder(), &mini(), Some(EN)).unwrap_err();
        assert!(
            matches!(
                err,
                DecodeError::LogitsShape {
                    rows: 3,
                    expected_rows: 4,
                    ..
                }
            ),
            "{err:?}"
        );

        // A vocabulary one wider than the config says.
        let mut fake = Scripted::new(vec![prompt_rows(4, vec![])]);
        fake.vocab = VOCAB + 1;
        let err = decode_window(&mut fake, &encoder(), &mini(), Some(EN)).unwrap_err();
        assert!(
            matches!(
                err,
                DecodeError::LogitsShape {
                    vocab: 40,
                    expected_vocab: 39,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn nan_logits_are_an_error() {
        let mut fake = Scripted::new(vec![prompt_rows(4, vec![(3, f32::NAN)])]);
        let err = decode_window(&mut fake, &encoder(), &mini(), Some(EN)).unwrap_err();
        assert!(matches!(err, DecodeError::NonFiniteLogits), "{err:?}");
    }

    #[test]
    fn masking_everything_is_an_error_not_an_arbitrary_token() {
        let mut config = mini();
        config.suppress_tokens = (0..VOCAB as u32).collect();
        let mut fake = Scripted::new(vec![prompt_rows(4, vec![])]);
        let err = decode_window(&mut fake, &encoder(), &config, Some(EN)).unwrap_err();
        assert!(matches!(err, DecodeError::NoViableToken), "{err:?}");
    }

    #[test]
    fn a_config_that_does_not_fit_the_vocabulary_is_an_error() {
        let mut fake = Scripted::new(vec![]);
        let mut config = mini();
        config.suppress_tokens = vec![VOCAB as u32];
        assert!(matches!(
            decode_window(&mut fake, &encoder(), &config, Some(EN)),
            Err(DecodeError::BadConfig(_))
        ));

        let mut config = mini();
        config.max_new_tokens = 0;
        assert!(matches!(
            decode_window(&mut fake, &encoder(), &config, Some(EN)),
            Err(DecodeError::BadConfig(_))
        ));

        let mut config = mini();
        config.tokens.timestamp_begin = VOCAB as u32 + 1;
        assert!(matches!(
            decode_window(&mut fake, &encoder(), &config, Some(EN)),
            Err(DecodeError::BadConfig(_))
        ));

        let mut config = mini();
        config.repetition = Some(rule(0, 3, 6));
        assert!(matches!(
            decode_window(&mut fake, &encoder(), &config, Some(EN)),
            Err(DecodeError::BadConfig(_))
        ));
        assert!(
            fake.calls.is_empty(),
            "a bad config is refused before any decoder step"
        );
    }

    #[test]
    fn encoder_states_and_logits_check_their_sizes() {
        assert!(EncoderStates::new(vec![0.0; 5], 3, 2).is_err());
        let ok = EncoderStates::new(vec![0.0; 6], 3, 2).unwrap();
        assert_eq!((ok.frames(), ok.width(), ok.as_slice().len()), (3, 2, 6));
        assert!(Logits::new(0, vec![]).is_err());
        assert!(Logits::new(4, vec![0.0; 6]).is_err());
        let two = Logits::new(3, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(
            (two.rows(), two.vocab_size(), two.row(1)),
            (2, 3, &[3.0, 4.0, 5.0][..])
        );
    }
}
