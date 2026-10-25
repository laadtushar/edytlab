//! The merged decoder's key-value cache, and the attention it hands back.
//!
//! [`CachedDecoder`] is the [`DecoderStep`] of the real model. It does not
//! touch ONNX Runtime itself: each step it builds a [`Feed`], hands it to a
//! [`DecoderRun`] (in the app, [`crate::ort_io::OrtDecoder`], which runs the
//! session) and keeps what comes back. That split exists so the part that is
//! easy to get silently wrong, namely which tensor is fed back as which, is
//! tested in CI with a fake runner and no model.
//!
//! ## One sequence
//!
//! The merged export (`decoder_model_merged_int8.onnx`) holds two graphs behind
//! an `If` on `use_cache_branch`:
//!
//! * **`use_cache_branch = false`**, the first step of a sequence, takes the whole
//!   prompt and computes every layer's self-attention keys and values for it
//!   (`present.{i}.decoder.key|value`, `[1, heads, prompt, head_dim]`) and its
//!   cross-attention keys and values for the encoder's output
//!   (`present.{i}.encoder.key|value`, `[1, heads, 1500, head_dim]`). The
//!   `past_key_values.*` inputs still have to be supplied; they are not read, so
//!   they are empty (length 0 on the sequence axis).
//! * **`use_cache_branch = true`**, every later step, takes one token and the
//!   cache: `past_key_values.{i}.decoder.*` is everything fed so far, and
//!   `past_key_values.{i}.encoder.*` is the cross-attention cache **from the
//!   first step**. The graph's `present.{i}.decoder.*` outputs are the
//!   old cache plus the new position. Its `present.{i}.encoder.*` outputs are
//!   not read: the cross-attention cache is what the first step produced and
//!   does not change, so those tensors are not asked for and the encoder half of
//!   the cache stays as it was.
//!
//! The decode loop starts a sequence with `past_len == 0` (language detection
//! feeds `[SOT]` that way, and then the real prompt starts again from
//! nothing), and that resets everything here, the cache and the collected
//! attention, which belongs to the sequence it was collected in.
//!
//! ## Attention for word timing
//!
//! The `_timestamped` export also returns `cross_attentions.{i}`, `[1, heads, seq, 1500]`,
//! the cross-attention weights of layer `i` for the `seq` positions fed in
//! this step. For the alignment heads only (a few `(layer, head)` pairs), the
//! decoder appends those rows to a per-head list, in position order, so after
//! a window it holds one `positions x 1500` matrix per head. Cross-attention
//! of a position does not depend on later positions, so the rows collected one
//! step at a time are the rows a single pass over the whole sequence would give
//! (which is how `whisper.timing.find_alignment` gets them).
//!
//! [`CachedDecoder::attention`] cuts out the rows `timing::align_words` wants.
//! Row `k` is the step that predicts text token `k`, so the first is the **last
//! prompt position**, and there is one row more than there are tokens (the one
//! that predicts `<|endoftext|>`).
//!
//! The window is decoded in timestamp mode, whose prompt has no
//! `<|notimestamps|>` and whose sequence holds timestamp tokens, so its own
//! attention is not what the aligner was built on. [`CachedDecoder::alignment_attention`]
//! runs the text tokens again as a fresh sequence, in one pass, with
//! `<|notimestamps|>` in the prompt: what `find_alignment` in openai/whisper
//! does (#659).

use crate::decode::{DecodeError, DecoderStep, EncoderStates, Logits};
use crate::export::ExportLayout;
use crate::timing::{Matrix, TimingError};

/// One tensor of the cache, as its runner holds it.
///
/// The decoder never reads a cache tensor: it keeps what a step returned and
/// hands it to the next step. So the runner picks the type. The fake in the
/// tests holds `Vec<f32>`s it can compare; the ONNX Runtime one holds the
/// runtime's own values ([`crate::ort_io::OrtTensor`]), which are never copied
/// into Rust, because copying the self-attention cache out after every step
/// made a window's cost grow with the square of its length (#657). `Default`
/// is the empty tensor of the first step, whose `past_key_values.*` are not read.
pub(crate) trait CacheTensor: Default {
    /// How many `f32`s it holds, to check what a step returned against the layout.
    fn elements(&self) -> usize;
}

impl CacheTensor for Vec<f32> {
    fn elements(&self) -> usize {
        self.len()
    }
}

/// One layer's cache: the self-attention half grows by a position per step,
/// the cross-attention half is set by the first step and never changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LayerKv<T = Vec<f32>> {
    /// `[1, heads, decoder_len, head_dim]`.
    pub dec_key: T,
    pub dec_value: T,
    /// `[1, heads, encoder_len, head_dim]`.
    pub enc_key: T,
    pub enc_value: T,
}

/// What one decoder step is fed.
pub(crate) struct Feed<'a, T = Vec<f32>> {
    /// `[1, seq]`.
    pub input_ids: &'a [i64],
    /// `[1, frames, width]`.
    pub encoder: &'a EncoderStates,
    /// `use_cache_branch`.
    pub use_cache: bool,
    /// One entry per layer. Empty tensors (length 0) when `use_cache` is false.
    pub past: &'a [LayerKv<T>],
    /// Length of the sequence axis of each `past` decoder tensor.
    pub past_decoder_len: usize,
    /// Length of the sequence axis of each `past` encoder tensor.
    pub past_encoder_len: usize,
}

/// What a step is asked to bring back besides the logits and the new decoder
/// cache.
pub(crate) struct Want<'a> {
    /// The cross-attention cache (`present.*.encoder.*`): only the first step.
    pub encoder_present: bool,
    /// Layers whose `cross_attentions.{i}` are wanted.
    pub cross_layers: &'a [usize],
}

/// What a step returned: the logits and the attention rows copied out of the
/// session, and the cache tensors as the runner holds them.
pub(crate) struct Fetched<T = Vec<f32>> {
    /// `[seq, vocab]`.
    pub logits: Vec<f32>,
    /// Per layer, `present.{i}.decoder.key|value`, `[1, heads, decoder_len, head_dim]`.
    pub decoder: Vec<(T, T)>,
    /// The sequence axis of those tensors.
    pub decoder_len: usize,
    /// Per layer, `present.{i}.encoder.key|value`; `Some` when asked for.
    pub encoder: Option<Vec<(T, T)>>,
    /// The sequence axis of those tensors.
    pub encoder_len: usize,
    /// For each of [`Want::cross_layers`], `[heads, seq, frames]`.
    pub cross: Vec<Vec<f32>>,
}

/// Runs the decoder graph once. The ONNX Runtime implementation is
/// `ort_io::OrtDecoder`; the tests have a fake.
pub(crate) trait DecoderRun {
    /// The cache tensors, kept and handed back by [`CachedDecoder`] untouched.
    type Tensor: CacheTensor;

    fn run(
        &mut self,
        feed: &Feed<'_, Self::Tensor>,
        want: &Want<'_>,
    ) -> Result<Fetched<Self::Tensor>, DecodeError>;
}

/// The model's decoder, with its cache, as a [`DecoderStep`].
pub(crate) struct CachedDecoder<'c, R: DecoderRun> {
    run: R,
    layout: ExportLayout,
    /// The alignment heads: `(layer, head)`.
    heads: Vec<(usize, usize)>,
    /// The distinct layers of `heads`, ascending.
    cross_layers: Vec<usize>,
    /// For each of `heads`, where its layer is in `cross_layers`.
    head_slots: Vec<usize>,
    cancel: &'c dyn Fn() -> bool,
    cache: Vec<LayerKv<R::Tensor>>,
    decoder_len: usize,
    encoder_len: usize,
    /// Per alignment head, `rows x frames`, row-major, rows in position order.
    attention: Vec<Vec<f32>>,
    /// Positions fed in the current sequence.
    rows: usize,
}

impl<'c, R: DecoderRun> CachedDecoder<'c, R> {
    pub(crate) fn new(
        run: R,
        layout: ExportLayout,
        heads: &[(usize, usize)],
        cancel: &'c dyn Fn() -> bool,
    ) -> Self {
        let mut cross_layers: Vec<usize> = heads.iter().map(|&(l, _)| l).collect();
        cross_layers.sort_unstable();
        cross_layers.dedup();
        // Every head's layer is in `cross_layers`, which was made from them, so
        // this is its index: the first entry that is not under it.
        let head_slots = heads
            .iter()
            .map(|&(layer, _)| cross_layers.partition_point(|&l| l < layer))
            .collect();
        Self {
            run,
            layout,
            heads: heads.to_vec(),
            cross_layers,
            head_slots,
            cancel,
            cache: Vec::new(),
            decoder_len: 0,
            encoder_len: 0,
            attention: vec![Vec::new(); heads.len()],
            rows: 0,
        }
    }

    /// Positions fed in the current sequence (prompt included).
    #[cfg(test)]
    pub(crate) fn fed(&self) -> usize {
        self.rows
    }

    fn reset(&mut self) {
        self.cache = (0..self.layout.layers)
            .map(|_| LayerKv::default())
            .collect();
        self.decoder_len = 0;
        self.encoder_len = 0;
        for rows in &mut self.attention {
            rows.clear();
        }
        self.rows = 0;
    }

    /// The attention the aligner wants for `text`, the text tokens of a
    /// decoded window, and the probability of each: `prompt` and `text` are fed
    /// as **one new sequence** (the cache and the attention of what was decoded
    /// before are dropped), and the rows from the last prompt position on are
    /// returned, `text.len() + 1` of them, each rescaled over the first
    /// `content_frames` (see [`CachedDecoder::attention`]). `prompt` is `[SOT,
    /// language, transcribe, notimestamps]`.
    ///
    /// The probability of a text token is the softmax of the logits of the row
    /// that predicts it, over the tokens under `eot` (the text and the special
    /// tokens before `<|endoftext|>`), as `find_alignment` takes
    /// `logits[len(sot_sequence):, :tokenizer.eot]`. That is not the decode's
    /// own probability (a softmax over the whole vocabulary, in a sequence with
    /// timestamps in it).
    ///
    /// A causal decoder's row for a position does not depend on later
    /// positions, so one pass over the whole sequence gives the rows that
    /// feeding it a token at a time would.
    pub(crate) fn alignment_attention(
        &mut self,
        prompt: &[u32],
        text: &[u32],
        eot: u32,
        encoder: &EncoderStates,
        content_frames: usize,
    ) -> Result<(Vec<Matrix>, Vec<f32>), crate::WhisperError> {
        let mut sequence = prompt.to_vec();
        sequence.extend_from_slice(text);
        let logits = self.step(&sequence, 0, encoder)?;
        let probs = text
            .iter()
            .enumerate()
            .map(|(i, &token)| {
                let row = logits.row(prompt.len() - 1 + i);
                crate::decode::softmax_prob(&row[..eot as usize], token)
            })
            .collect();
        let heads = self.attention(prompt.len() - 1, prompt.len() + text.len(), content_frames)?;
        Ok((heads, probs))
    }

    /// The collected attention as one matrix per alignment head: rows
    /// `from..to` of the sequence (`to - from` of them, each of the encoder's
    /// frames), each row rescaled to sum to 1 over its first `content_frames`.
    ///
    /// The weights come out of the export already softmaxed over all 1500
    /// frames, the padding of a short window included. `find_alignment` in
    /// openai/whisper crops to the frames that hold audio **before** its
    /// softmax, so the weights it uses sum to 1 over those frames. A softmax
    /// over a subset of values is the full softmax divided by the subset's
    /// total, so dividing here gives the same weights.
    pub(crate) fn attention(
        &self,
        from: usize,
        to: usize,
        content_frames: usize,
    ) -> Result<Vec<Matrix>, TimingError> {
        let frames = self.layout.frames;
        if from >= to || to > self.rows || content_frames == 0 || content_frames > frames {
            return Err(TimingError::Shape(format!(
                "rows {from}..{to} of {} fed, {content_frames} content frames of {frames}",
                self.rows
            )));
        }
        self.attention
            .iter()
            .map(|all| {
                let mut data = all[from * frames..to * frames].to_vec();
                for row in data.chunks_exact_mut(frames) {
                    let total: f32 = row[..content_frames].iter().sum();
                    if total > 0.0 {
                        for v in row.iter_mut() {
                            *v /= total;
                        }
                    }
                }
                Matrix::new(to - from, frames, data)
            })
            .collect()
    }
}

impl<R: DecoderRun> DecoderStep for CachedDecoder<'_, R> {
    fn step(
        &mut self,
        new_tokens: &[u32],
        past_len: usize,
        encoder: &EncoderStates,
    ) -> Result<Logits, DecodeError> {
        if (self.cancel)() {
            return Err(DecodeError::Cancelled);
        }
        let layout = self.layout;
        if new_tokens.is_empty() {
            return Err(DecodeError::Backend(
                "a step needs at least one token".into(),
            ));
        }
        if encoder.frames() != layout.frames || encoder.width() != layout.width {
            return Err(DecodeError::BadEncoderStates(format!(
                "{} frames of width {}, the decoder takes {} of {}",
                encoder.frames(),
                encoder.width(),
                layout.frames,
                layout.width
            )));
        }
        let use_cache = past_len > 0;
        if use_cache {
            if past_len != self.decoder_len || past_len != self.rows {
                return Err(DecodeError::Backend(format!(
                    "asked to continue from {past_len} positions but the decoder cache holds {}",
                    self.decoder_len
                )));
            }
        } else {
            self.reset();
        }

        let ids: Vec<i64> = new_tokens.iter().map(|&t| i64::from(t)).collect();
        let seq = ids.len();
        let feed = Feed {
            input_ids: &ids,
            encoder,
            use_cache,
            past: &self.cache,
            past_decoder_len: self.decoder_len,
            past_encoder_len: self.encoder_len,
        };
        let want = Want {
            encoder_present: !use_cache,
            cross_layers: &self.cross_layers,
        };
        let fetched = self.run.run(&feed, &want)?;

        // What came back has to be what this layout says, or the cache would
        // silently be garbage on the next step.
        let bad = |what: String| Err(DecodeError::Backend(format!("decoder output: {what}")));
        if fetched.logits.len() != seq * layout.vocab {
            return bad(format!(
                "{} logits for {seq} positions of {}",
                fetched.logits.len(),
                layout.vocab
            ));
        }
        let new_decoder_len = past_len + seq;
        let kv_len = |len: usize| layout.heads * len * layout.head_dim;
        if fetched.decoder_len != new_decoder_len || fetched.decoder.len() != layout.layers {
            return bad(format!(
                "{} layers with a cache of {} positions, expected {} with {new_decoder_len}",
                fetched.decoder.len(),
                fetched.decoder_len,
                layout.layers
            ));
        }
        if fetched.decoder.iter().any(|(k, v)| {
            k.elements() != kv_len(new_decoder_len) || v.elements() != kv_len(new_decoder_len)
        }) {
            return bad(format!(
                "a self-attention cache is not [1, {}, {new_decoder_len}, {}]",
                layout.heads, layout.head_dim
            ));
        }
        if !use_cache {
            match &fetched.encoder {
                Some(enc)
                    if enc.len() == layout.layers
                        && fetched.encoder_len == layout.frames
                        && enc.iter().all(|(k, v)| {
                            k.elements() == kv_len(layout.frames)
                                && v.elements() == kv_len(layout.frames)
                        }) => {}
                _ => {
                    return bad(format!(
                        "the first step did not return a cross-attention cache of [1, {}, {}, {}] \
                         for each of {} layers",
                        layout.heads, layout.frames, layout.head_dim, layout.layers
                    ))
                }
            }
        }
        if fetched.cross.len() != self.cross_layers.len()
            || fetched
                .cross
                .iter()
                .any(|c| c.len() != layout.heads * seq * layout.frames)
        {
            return bad(format!(
                "cross_attentions are not [1, {}, {seq}, {}] for {} layers",
                layout.heads,
                layout.frames,
                self.cross_layers.len()
            ));
        }

        // Commit. Nothing above changed state, so a refused step leaves the
        // cache as it was.
        let Fetched {
            logits,
            decoder,
            encoder: first_encoder,
            encoder_len,
            cross,
            ..
        } = fetched;
        for (layer, (key, value)) in self.cache.iter_mut().zip(decoder) {
            layer.dec_key = key;
            layer.dec_value = value;
        }
        // Only the first step's cross-attention cache is kept. Whatever a
        // cache-branch step offers under `present.*.encoder.*` is not asked
        // for and, if it comes anyway, not used.
        if let (false, Some(enc)) = (use_cache, first_encoder) {
            for (layer, (key, value)) in self.cache.iter_mut().zip(enc) {
                layer.enc_key = key;
                layer.enc_value = value;
            }
            self.encoder_len = encoder_len;
        }
        self.decoder_len = new_decoder_len;
        for (a, (&(_, head), &at)) in self.heads.iter().zip(&self.head_slots).enumerate() {
            let per_head = seq * layout.frames;
            self.attention[a].extend_from_slice(&cross[at][head * per_head..(head + 1) * per_head]);
        }
        self.rows += seq;
        Logits::new(layout.vocab, logits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{decode_window, DecodeConfig, StopReason, TokenIds};

    // A small decoder: 2 layers of 2 heads of 2, a width of 4, 6 frames.
    const LAYOUT: ExportLayout = ExportLayout {
        layers: 2,
        heads: 2,
        head_dim: 2,
        width: 4,
        frames: 6,
        vocab: VOCAB,
    };

    // The vocabulary of decode.rs's tests: text 0..=9, eot 10, sot 11,
    // languages 12..=14, transcribe 15, nospeech 16, notimestamps 17,
    // timestamps 18..=38.
    const VOCAB: usize = 39;
    const EOT: u32 = 10;
    const SOT: u32 = 11;
    const EN: u32 = 12;
    const TRANSCRIBE: u32 = 15;
    const NO_TIMESTAMPS: u32 = 17;
    /// Alignment heads: layer 1 heads 0 and 1, layer 0 head 1. Two layers,
    /// out of layer order on purpose.
    const HEADS: [(usize, usize); 3] = [(1, 0), (1, 1), (0, 1)];

    fn ids() -> TokenIds {
        TokenIds {
            vocab_size: VOCAB,
            eot: EOT,
            sot: SOT,
            transcribe: TRANSCRIBE,
            no_speech: 16,
            no_timestamps: NO_TIMESTAMPS,
            timestamp_begin: 18,
            languages: EN..15,
        }
    }

    fn encoder() -> EncoderStates {
        let data = (0..LAYOUT.frames * LAYOUT.width)
            .map(|i| i as f32)
            .collect();
        EncoderStates::new(data, LAYOUT.frames, LAYOUT.width).unwrap()
    }

    /// What the fake saw in one call.
    #[derive(Debug, Clone, PartialEq)]
    struct Seen {
        input_ids: Vec<i64>,
        use_cache: bool,
        past_decoder_len: usize,
        past_encoder_len: usize,
        /// `(dec_key, dec_value, enc_key, enc_value)` of each layer.
        past: Vec<LayerKv>,
        encoder_present_wanted: bool,
        cross_layers: Vec<usize>,
        encoder_data: Vec<f32>,
    }

    /// A runner that records its feeds and answers from a formula, so the
    /// test can say where every number in the cache came from.
    ///
    /// * the self-attention cache of layer `l` after the call `c` (counted
    ///   from 0 within the sequence) is filled with `1000 * c + 10 * l + 1`
    ///   (key) and `... + 2` (value) at every position, so the old positions
    ///   are told apart by the call that wrote them;
    /// * the cross-attention cache is filled with `7000 + 10 * l + 1|2` on the
    ///   first call, and with junk (`-1`) if a later call is asked for it;
    /// * `cross_attentions.l` at head `h`, position `p` of the call, frame `f`
    ///   is `100 * l + 10 * h + p + f / 100`... see `cross_value`;
    /// * the logits make the model say whatever `script` says about the next
    ///   absolute position.
    struct Fake {
        seen: Vec<Seen>,
        calls_in_sequence: usize,
        /// The token predicted after absolute position `p`.
        script: Box<dyn Fn(usize) -> u32>,
        /// Add a cross-attention cache of junk to cache-branch answers, which
        /// the decoder must ignore.
        offer_junk_encoder: bool,
    }

    fn cross_value(layer: usize, head: usize, position: usize, frame: usize) -> f32 {
        // Unique per cell: position and frame are small.
        (layer * 1000 + head * 100 + position * 10 + frame) as f32
    }

    impl Fake {
        fn new(script: impl Fn(usize) -> u32 + 'static) -> Self {
            Self {
                seen: Vec::new(),
                calls_in_sequence: 0,
                script: Box::new(script),
                offer_junk_encoder: false,
            }
        }
    }

    impl DecoderRun for Fake {
        type Tensor = Vec<f32>;

        fn run(&mut self, feed: &Feed<'_>, want: &Want<'_>) -> Result<Fetched, DecodeError> {
            if !feed.use_cache {
                self.calls_in_sequence = 0;
            }
            self.seen.push(Seen {
                input_ids: feed.input_ids.to_vec(),
                use_cache: feed.use_cache,
                past_decoder_len: feed.past_decoder_len,
                past_encoder_len: feed.past_encoder_len,
                past: feed.past.to_vec(),
                encoder_present_wanted: want.encoder_present,
                cross_layers: want.cross_layers.to_vec(),
                encoder_data: feed.encoder.as_slice().to_vec(),
            });
            let call = self.calls_in_sequence;
            self.calls_in_sequence += 1;
            let seq = feed.input_ids.len();
            let decoder_len = feed.past_decoder_len + seq;
            let l = &LAYOUT;
            let kv = l.heads * decoder_len * l.head_dim;

            // Position of this call's first token in the sequence.
            let first = feed.past_decoder_len;
            let mut logits = vec![0.0f32; seq * l.vocab];
            for r in 0..seq {
                logits[r * l.vocab + (self.script)(first + r) as usize] = 10.0;
            }
            let decoder = (0..l.layers)
                .map(|layer| {
                    let base = (1000 * call + 10 * layer) as f32;
                    (vec![base + 1.0; kv], vec![base + 2.0; kv])
                })
                .collect();
            let enc_kv = l.heads * l.frames * l.head_dim;
            let encoder = if want.encoder_present {
                Some(
                    (0..l.layers)
                        .map(|layer| {
                            let base = 7000.0 + 10.0 * layer as f32;
                            (vec![base + 1.0; enc_kv], vec![base + 2.0; enc_kv])
                        })
                        .collect(),
                )
            } else if self.offer_junk_encoder {
                Some(vec![(vec![-1.0; enc_kv], vec![-1.0; enc_kv]); l.layers])
            } else {
                None
            };
            let cross = want
                .cross_layers
                .iter()
                .map(|&layer| {
                    let mut data = Vec::with_capacity(l.heads * seq * l.frames);
                    for head in 0..l.heads {
                        for p in 0..seq {
                            for f in 0..l.frames {
                                data.push(cross_value(layer, head, first + p, f));
                            }
                        }
                    }
                    data
                })
                .collect();
            Ok(Fetched {
                logits,
                decoder,
                decoder_len,
                encoder,
                encoder_len: if want.encoder_present { l.frames } else { 0 },
                cross,
            })
        }
    }

    fn no_cancel() -> bool {
        false
    }

    fn decoder<'c>(fake: Fake, cancel: &'c dyn Fn() -> bool) -> CachedDecoder<'c, Fake> {
        CachedDecoder::new(fake, LAYOUT, &HEADS, cancel)
    }

    #[test]
    fn the_first_step_runs_the_no_cache_branch_with_empty_pasts() {
        let mut dec = decoder(Fake::new(|_| 0), &no_cancel);
        let enc = encoder();
        let logits = dec
            .step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        assert_eq!((logits.rows(), logits.vocab_size()), (4, VOCAB));

        let seen = &dec.run.seen[0];
        assert_eq!(seen.input_ids, vec![11, 12, 15, 17]);
        assert!(
            !seen.use_cache,
            "use_cache_branch is false on the first step"
        );
        assert_eq!((seen.past_decoder_len, seen.past_encoder_len), (0, 0));
        assert!(
            seen.past.iter().all(|l| *l == LayerKv::default()),
            "the past tensors are empty on the first step"
        );
        assert!(
            seen.encoder_present_wanted,
            "the first step brings the cross-attention cache"
        );
        assert_eq!(seen.cross_layers, vec![0, 1], "distinct layers, ascending");
        assert_eq!(seen.encoder_data, enc.as_slice());
    }

    #[test]
    fn later_steps_feed_back_the_present_decoder_cache_and_the_first_steps_encoder_cache() {
        let mut fake = Fake::new(|_| 0);
        // A cache-branch answer that carries a cross-attention cache of junk
        // must not replace the first step's.
        fake.offer_junk_encoder = true;
        let mut dec = decoder(fake, &no_cancel);
        let enc = encoder();
        dec.step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        dec.step(&[3], 4, &enc).unwrap();
        dec.step(&[4], 5, &enc).unwrap();

        let seen = &dec.run.seen;
        // Second call: the cache branch, the prompt's cache, one token.
        assert_eq!(seen[1].input_ids, vec![3]);
        assert!(seen[1].use_cache);
        assert!(
            !seen[1].encoder_present_wanted,
            "not asked for on the cache branch"
        );
        assert_eq!((seen[1].past_decoder_len, seen[1].past_encoder_len), (4, 6));
        let kv = |len: usize| LAYOUT.heads * len * LAYOUT.head_dim;
        for (layer, past) in seen[1].past.iter().enumerate() {
            let base = (10 * layer) as f32; // written by call 0
            assert_eq!(
                past.dec_key,
                vec![base + 1.0; kv(4)],
                "layer {layer} decoder key"
            );
            assert_eq!(
                past.dec_value,
                vec![base + 2.0; kv(4)],
                "layer {layer} decoder value"
            );
            assert_eq!(
                past.enc_key,
                vec![7000.0 + base + 1.0; kv(6)],
                "layer {layer} encoder key"
            );
            assert_eq!(
                past.enc_value,
                vec![7000.0 + base + 2.0; kv(6)],
                "layer {layer} encoder value"
            );
        }
        // Third call: the decoder cache is what call 1 wrote (5 positions),
        // the encoder cache is still the first step's.
        assert_eq!((seen[2].past_decoder_len, seen[2].past_encoder_len), (5, 6));
        for (layer, past) in seen[2].past.iter().enumerate() {
            let base = (1000 + 10 * layer) as f32;
            assert_eq!(past.dec_key, vec![base + 1.0; kv(5)], "layer {layer}");
            assert_eq!(
                past.enc_key,
                vec![7000.0 + (10 * layer) as f32 + 1.0; kv(6)],
                "layer {layer}"
            );
        }
        assert_eq!(dec.fed(), 6);
    }

    /// A cache tensor the decoder cannot read or copy: it has a serial number
    /// and a size, and is neither `Clone` nor `Copy`, as the runtime's values
    /// are not copied into Rust either (#657). Serial 0 is the empty tensor.
    #[derive(Debug, Default, PartialEq)]
    struct Tagged {
        serial: u32,
        elements: usize,
    }

    impl CacheTensor for Tagged {
        fn elements(&self) -> usize {
            self.elements
        }
    }

    /// Hands out tensors with fresh serial numbers and records which it was fed.
    #[derive(Default)]
    struct Tagging {
        next: u32,
        /// Per call, per layer: the serials of `dec_key, dec_value, enc_key, enc_value` fed.
        fed: Vec<Vec<[u32; 4]>>,
        /// The same for what the call returned (`0` for what it did not).
        returned: Vec<Vec<[u32; 4]>>,
    }

    impl Tagging {
        fn tagged(&mut self, elements: usize) -> Tagged {
            self.next += 1;
            Tagged {
                serial: self.next,
                elements,
            }
        }
    }

    impl DecoderRun for Tagging {
        type Tensor = Tagged;

        fn run(
            &mut self,
            feed: &Feed<'_, Tagged>,
            want: &Want<'_>,
        ) -> Result<Fetched<Tagged>, DecodeError> {
            let l = &LAYOUT;
            let seq = feed.input_ids.len();
            let decoder_len = feed.past_decoder_len + seq;
            self.fed.push(
                feed.past
                    .iter()
                    .map(|k| {
                        [
                            k.dec_key.serial,
                            k.dec_value.serial,
                            k.enc_key.serial,
                            k.enc_value.serial,
                        ]
                    })
                    .collect(),
            );
            let kv = |len: usize| l.heads * len * l.head_dim;
            let mut returned = vec![[0; 4]; l.layers];
            let decoder = (0..l.layers)
                .map(|layer| {
                    let (k, v) = (self.tagged(kv(decoder_len)), self.tagged(kv(decoder_len)));
                    (returned[layer][0], returned[layer][1]) = (k.serial, v.serial);
                    (k, v)
                })
                .collect();
            let encoder = want.encoder_present.then(|| {
                (0..l.layers)
                    .map(|layer| {
                        let (k, v) = (self.tagged(kv(l.frames)), self.tagged(kv(l.frames)));
                        (returned[layer][2], returned[layer][3]) = (k.serial, v.serial);
                        (k, v)
                    })
                    .collect()
            });
            self.returned.push(returned);
            Ok(Fetched {
                logits: vec![0.0; seq * l.vocab],
                decoder,
                decoder_len,
                encoder,
                encoder_len: if want.encoder_present { l.frames } else { 0 },
                cross: want
                    .cross_layers
                    .iter()
                    .map(|_| vec![0.0; l.heads * seq * l.frames])
                    .collect(),
            })
        }
    }

    #[test]
    fn the_cache_tensors_are_handed_back_untouched_and_never_copied() {
        let mut dec = CachedDecoder::new(Tagging::default(), LAYOUT, &HEADS, &no_cancel);
        let enc = encoder();
        dec.step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        dec.step(&[3], 4, &enc).unwrap();
        dec.step(&[4], 5, &enc).unwrap();
        // A new sequence starts from empty tensors again.
        dec.step(&[SOT], 0, &enc).unwrap();

        let run = &dec.run;
        assert_eq!(
            run.fed[0],
            vec![[0; 4]; LAYOUT.layers],
            "empty on the first step"
        );
        // Step 1 is fed what step 0 returned, all four tensors of each layer.
        assert_eq!(run.fed[1], run.returned[0]);
        // Step 2: the self-attention cache of step 1, the cross-attention cache of step 0.
        for layer in 0..LAYOUT.layers {
            let [dk, dv, ..] = run.returned[1][layer];
            let [_, _, ek, ev] = run.returned[0][layer];
            assert_ne!([dk, dv, ek, ev], [0; 4]);
            assert_eq!(run.fed[2][layer], [dk, dv, ek, ev], "layer {layer}");
            assert_eq!(run.returned[1][layer][2..], [0, 0], "not asked for");
        }
        assert_eq!(run.fed[3], vec![[0; 4]; LAYOUT.layers], "a new sequence");
    }

    #[test]
    fn a_step_from_nothing_starts_a_new_sequence() {
        // Language detection feeds [SOT] and the prompt then starts over.
        let mut dec = decoder(Fake::new(|_| 0), &no_cancel);
        let enc = encoder();
        dec.step(&[SOT], 0, &enc).unwrap();
        assert_eq!(dec.fed(), 1);
        dec.step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        assert_eq!(dec.fed(), 4, "the detection step's row is gone");
        let seen = &dec.run.seen;
        assert!(!seen[1].use_cache);
        assert_eq!(seen[1].past_decoder_len, 0);
        assert!(seen[1].past.iter().all(|l| *l == LayerKv::default()));
        assert!(seen[1].encoder_present_wanted);
        // The attention is the new sequence's alone: 4 rows.
        let m = dec.attention(0, 4, 6).unwrap();
        assert_eq!(m[0].rows(), 4);
    }

    #[test]
    fn a_step_that_does_not_continue_the_sequence_is_refused() {
        let mut dec = decoder(Fake::new(|_| 0), &no_cancel);
        let enc = encoder();
        dec.step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        // Continuing from 3 when 4 were fed.
        let err = dec.step(&[3], 3, &enc).unwrap_err();
        assert!(matches!(err, DecodeError::Backend(m) if m.contains("holds 4")));
        // And before any step.
        let mut fresh = decoder(Fake::new(|_| 0), &no_cancel);
        assert!(fresh.step(&[3], 2, &enc).is_err());
        assert_eq!(fresh.run.seen.len(), 0, "the runner was not called");
    }

    #[test]
    fn the_attention_is_collected_per_head_in_position_order() {
        let mut dec = decoder(Fake::new(|_| 0), &no_cancel);
        let enc = encoder();
        dec.step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        dec.step(&[3], 4, &enc).unwrap();
        dec.step(&[4], 5, &enc).unwrap();
        assert_eq!(dec.fed(), 6);

        // Rows 3..6 of each head, not renormalised (content = all frames, but
        // the fake's values are large, so look at the ratios).
        let m = dec.attention(3, 6, 6).unwrap();
        assert_eq!(m.len(), HEADS.len());
        for (matrix, &(layer, head)) in m.iter().zip(&HEADS) {
            assert_eq!((matrix.rows(), matrix.cols()), (3, 6));
            for (r, position) in (3..6).enumerate() {
                // After renormalising, a row is the fake's row over its total.
                let raw: Vec<f32> = (0..6)
                    .map(|f| cross_value(layer, head, position, f))
                    .collect();
                let total: f32 = raw.iter().sum();
                for (f, v) in raw.iter().enumerate() {
                    let got = matrix.row(r)[f];
                    assert!(
                        (got - v / total).abs() < 1e-6,
                        "head ({layer},{head}) position {position} frame {f}: {got} vs {}",
                        v / total
                    );
                }
            }
        }
    }

    #[test]
    fn attention_is_rescaled_to_sum_to_one_over_the_content_frames() {
        let mut dec = decoder(Fake::new(|_| 0), &no_cancel);
        let enc = encoder();
        dec.step(&[SOT, EN, TRANSCRIBE, NO_TIMESTAMPS], 0, &enc)
            .unwrap();
        let m = dec.attention(3, 4, 4).unwrap();
        for matrix in &m {
            let row = matrix.row(0);
            let content: f32 = row[..4].iter().sum();
            assert!((content - 1.0).abs() < 1e-6, "{row:?}");
            // The frames past the content are scaled by the same factor, not cleared.
            assert!(row[4] > 0.0);
        }
        assert!(dec.attention(3, 5, 4).is_err(), "more rows than were fed");
        assert!(
            dec.attention(3, 4, 7).is_err(),
            "more content frames than frames"
        );
        assert!(dec.attention(4, 4, 4).is_err(), "no rows");
    }

    #[test]
    fn a_refused_cancel_never_reaches_the_runner() {
        let cancel = || true;
        let mut dec = decoder(Fake::new(|_| 0), &cancel);
        let err = dec.step(&[SOT], 0, &encoder()).unwrap_err();
        assert!(matches!(err, DecodeError::Cancelled));
        assert_eq!(dec.run.seen.len(), 0);
    }

    #[test]
    fn a_runner_that_returns_the_wrong_shapes_is_caught_and_leaves_the_cache_alone() {
        struct Short;
        impl DecoderRun for Short {
            type Tensor = Vec<f32>;

            fn run(&mut self, feed: &Feed<'_>, _: &Want<'_>) -> Result<Fetched, DecodeError> {
                let seq = feed.input_ids.len();
                let l = &LAYOUT;
                let kv = l.heads * seq * l.head_dim;
                Ok(Fetched {
                    logits: vec![0.0; seq * l.vocab - 1], // one short
                    decoder: vec![(vec![0.0; kv], vec![0.0; kv]); l.layers],
                    decoder_len: seq,
                    encoder: None,
                    encoder_len: 0,
                    cross: Vec::new(),
                })
            }
        }
        let mut dec = CachedDecoder::new(Short, LAYOUT, &HEADS, &no_cancel);
        let err = dec.step(&[SOT], 0, &encoder()).unwrap_err();
        assert!(
            matches!(&err, DecodeError::Backend(m) if m.contains("logits")),
            "{err}"
        );
        assert_eq!(dec.fed(), 0, "a refused step records nothing");
    }

    #[test]
    fn an_encoder_output_of_the_wrong_size_is_refused() {
        let mut dec = decoder(Fake::new(|_| 0), &no_cancel);
        let wrong = EncoderStates::new(vec![0.0; 5 * 4], 5, 4).unwrap();
        let err = dec.step(&[SOT], 0, &wrong).unwrap_err();
        assert!(matches!(err, DecodeError::BadEncoderStates(_)), "{err}");
    }

    // ---- driven by the real loop ---------------------------------------------

    /// Tokens predicted at each absolute position: the prompt predicts
    /// nothing in particular, position 3 predicts the first text token, and
    /// the last position predicts EOT.
    fn say(text: Vec<u32>) -> impl Fn(usize) -> u32 {
        move |p| {
            if p < 3 {
                0
            } else {
                text.get(p - 3).copied().unwrap_or(EOT)
            }
        }
    }

    fn config() -> DecodeConfig {
        let mut config = DecodeConfig::new(ids());
        config.repetition = None;
        config
    }

    #[test]
    fn a_window_that_ends_on_eot_has_exactly_the_rows_alignment_needs() {
        let mut dec = decoder(Fake::new(say(vec![3, 4, 5])), &no_cancel);
        let enc = encoder();
        let out = decode_window(&mut dec, &enc, &config(), Some(EN)).unwrap();
        assert_eq!(out.tokens, vec![3, 4, 5]);
        assert_eq!(out.stop, StopReason::EndOfText);
        // prompt (4) + the three tokens fed back; EOT is never fed.
        assert_eq!(dec.fed(), 4 + 3);
        // Rows 3.. = the position before the first token up to the last token's:
        // tokens + 1 of them.
        let m = dec.attention(3, 3 + 3 + 1, 6).unwrap();
        assert_eq!(m[0].rows(), 4);
        // The calls: language was given, so prompt then one token at a time.
        let calls: Vec<(Vec<i64>, bool)> = dec
            .run
            .seen
            .iter()
            .map(|s| (s.input_ids.clone(), s.use_cache))
            .collect();
        assert_eq!(
            calls,
            vec![
                (vec![11, 12, 15, 17], false),
                (vec![3], true),
                (vec![4], true),
                (vec![5], true),
            ]
        );
    }

    #[test]
    fn the_alignment_pass_is_one_new_sequence_with_its_own_prompt() {
        let mut dec = decoder(Fake::new(say(vec![3, 4, 5])), &no_cancel);
        let enc = encoder();
        // A decode that left a sequence and its attention behind, cut by its
        // length as a window with timestamp tokens in it may be.
        let mut config = config();
        config.max_new_tokens = 5;
        decode_window(&mut dec, &enc, &config, Some(EN)).unwrap();
        let before = dec.run.seen.len();

        let prompt = [SOT, EN, TRANSCRIBE, NO_TIMESTAMPS];
        let (m, probs) = dec
            .alignment_attention(&prompt, &[3, 4], EOT, &enc, 6)
            .unwrap();
        // One call, no cache, the whole sequence.
        let calls = &dec.run.seen[before..];
        assert_eq!(calls.len(), 1, "one forward pass, not one step per token");
        assert_eq!(calls[0].input_ids, vec![11, 12, 15, 17, 3, 4]);
        assert!(!calls[0].use_cache);
        // Rows from the last prompt position: two tokens and the end.
        assert_eq!(dec.fed(), 6);
        assert_eq!(m.len(), dec.heads.len(), "one matrix per alignment head");
        assert!(m.iter().all(|h| h.rows() == 3));
        // And the attention is that of the new sequence, not the earlier one.
        let again = dec.attention(3, 3 + 2 + 1, 6).unwrap();
        assert_eq!(again[0].as_slice(), m[0].as_slice());
        // The probabilities are the softmax over the tokens under EOT of the
        // row that predicts each: the favourite has logit 10 against nine
        // zeros, whatever the 29 tokens past EOT hold.
        let want = (10f64.exp() / (10f64.exp() + 9.0)) as f32;
        assert_eq!(probs.len(), 2);
        assert!(probs.iter().all(|p| (p - want).abs() < 1e-6), "{probs:?}");
    }

    #[test]
    fn language_detection_is_a_separate_sequence_from_the_prompt() {
        let mut dec = decoder(Fake::new(say(vec![3])), &no_cancel);
        let enc = encoder();
        let out = decode_window(&mut dec, &enc, &config(), None).unwrap();
        assert_eq!(out.tokens, vec![3]);
        let calls: Vec<(Vec<i64>, bool, usize)> = dec
            .run
            .seen
            .iter()
            .map(|s| (s.input_ids.clone(), s.use_cache, s.past_decoder_len))
            .collect();
        assert_eq!(
            calls[0],
            (vec![11], false, 0),
            "detection feeds [SOT] alone"
        );
        assert_eq!(
            calls[1],
            (vec![11, 12, 15, 17], false, 0),
            "then the prompt, from nothing"
        );
        assert_eq!(calls[2], (vec![3], true, 4));
        assert_eq!(dec.fed(), 5, "the detection step's position is not counted");
    }
}
