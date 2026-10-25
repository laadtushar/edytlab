//! The loaded model, and transcribing with it.
//!
//! [`WhisperModel::load`] takes the folder of the pinned export
//! (`ml_pipeline::models::whisper_base()`: `onnx/encoder_model.onnx`,
//! `onnx/decoder_model_merged_int8.onnx`, `tokenizer.json`), builds both
//! sessions and checks them by name ([`crate::export`]). [`transcribe`] then
//! turns any audio buffer into timed words:
//!
//! 1. [`resample_to_16khz_mono`]: any rate, any channel count.
//! 2. [`crate::chunk::run_windows`]: 30 s windows and where each one starts.
//! 3. For each window: log-mel (`MelExtractor::log_mel_window`, the Hugging
//!    Face convention: pad the audio to 30 s, then transform), the encoder, the
//!    greedy decode loop ([`decode_window`]) over the cached decoder
//!    ([`CachedDecoder`]) **with timestamp tokens**, the segments those cut the
//!    tokens into ([`crate::segments::split_segments`]), one more pass of the
//!    text tokens for the cross-attention of the alignment heads
//!    ([`CachedDecoder::alignment_attention`]), [`align_words`], and the words
//!    placed in their segments ([`crate::segments::place_words`]) as
//!    `whisper.timing.add_word_timestamps` does (#659).
//!
//! ## Where things run
//!
//! CPU only. `ExecProvider::default_for_platform()` would pick CoreML on
//! macOS; the merged decoder's `If` node and its dynamic shapes are not
//! something CoreML is known to run, and this has been measured on Linux CPU
//! only (see the PR for #384), so the choice is `Cpu` until someone measures
//! the other.
//!
//! The two sessions live in a process-wide [`ModelRegistry`], keyed by the
//! file's canonical path, size and modification time, so loading the same
//! folder twice does not hold two copies of 136 MB of weights, and replacing
//! a file gets a new session.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ml_pipeline::{ExecProvider, ModelRegistry, SharedSession};

use crate::cached::CachedDecoder;
use crate::chunk::{run_windows_observed, WindowRequest, WindowResult, WindowTranscriber};
use crate::decode::{decode_window, DecodeConfig, StopReason, TokenIds};
use crate::export::{self, alignment_heads, ExportLayout};
use crate::mel::{MelExtractor, N_SAMPLES};
use crate::ort_io::{lock_session, run_encoder, specs, OrtDecoder};
use crate::refine::{is_silent, trim_to_sound};
use crate::segments::{place_words, split_segments, text_of_segments};
use crate::timing::{
    align_words, encoder_frames, AlignmentInput, WordSplit, APPEND_PUNCTUATIONS,
    PREPEND_PUNCTUATIONS,
};
use crate::tokenizer::{Piece, Tokenizer};
use crate::{resample_to_16khz_mono, Result, WhisperError, Word};

/// Where the encoder is, inside the model folder. These three paths are the
/// ones `crates/ml-pipeline/models.json` pins for `whisper-base`
/// (`the_file_names_are_the_pinned_ones` fails if they stop agreeing), so a
/// model the app downloaded is a folder `WhisperModel::load` reads.
pub const ENCODER_FILE: &str = "onnx/encoder_model.onnx";
/// Where the merged int8 decoder is, inside the model folder.
pub const DECODER_FILE: &str = "onnx/decoder_model_merged_int8.onnx";
/// Where the tokenizer is, inside the model folder.
pub const TOKENIZER_FILE: &str = "tokenizer.json";

/// The words of a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// In order, with times in seconds from the start of the audio.
    pub words: Vec<Word>,
    /// The language code (`"en"`), given or detected from the first window that
    /// had speech; `None` if no window had any.
    pub language: Option<String>,
    /// How many 30 s windows were decoded.
    pub windows: usize,
}

/// A loaded Whisper model: two ONNX sessions, the tokenizer and the settings
/// that go with this export.
///
/// `Send + Sync`: the sessions sit behind mutexes, so two threads transcribing
/// with one model take turns on each session.
pub struct WhisperModel {
    encoder: SharedSession,
    decoder: SharedSession,
    layout: ExportLayout,
    heads: &'static [(usize, usize)],
    tokenizer: Tokenizer,
    config: DecodeConfig,
    mel: MelExtractor,
}

// `ort::session::Session` doesn't implement `Debug`; provide a manual
// stub so callers can use `.unwrap()`/`.expect_err()` in tests.
impl std::fmt::Debug for WhisperModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WhisperModel")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

fn registry() -> &'static ModelRegistry {
    static REGISTRY: OnceLock<ModelRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ModelRegistry::new)
}

/// Where a session comes from: the role and the file's place. It names the
/// role, so the same file given to both roles (a wrong folder, or symlinks) is
/// two sessions, not one session locked twice.
fn session_family(role: &str, path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("whisper:{role}:{}", canonical.display())
}

/// What the file holds, as far as the registry can tell without reading it: its
/// size and modification time. A file that is replaced is a new version of the
/// same [`session_family`], which gets a new session.
fn file_version(path: &Path) -> String {
    let (len, modified) = std::fs::metadata(path)
        .map(|m| {
            let nanos = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            (m.len(), nanos)
        })
        .unwrap_or((0, 0));
    format!("{len}:{modified}")
}

/// The session for one of the model's files. The registry drops the sessions of
/// the file's earlier versions once this one is built (#658), so replacing the
/// model's files does not leave the old weights loaded for the rest of the
/// process.
fn load_session(
    registry: &ModelRegistry,
    role: &str,
    path: &Path,
) -> std::result::Result<SharedSession, ml_pipeline::Error> {
    registry.load_versioned(
        &session_family(role, path),
        &file_version(path),
        path,
        ExecProvider::Cpu,
    )
}

/// Turn the pipeline's error for loading `file` into ours.
fn load_error(e: ml_pipeline::Error, file: &Path) -> WhisperError {
    use ml_pipeline::Error as E;
    match e {
        E::MissingRuntime { .. } | E::RuntimeLoad { .. } | E::RuntimeUnsupported { .. } => {
            WhisperError::RuntimeUnavailable(e.to_string())
        }
        E::Io(io) if io.kind() == std::io::ErrorKind::NotFound => WhisperError::ModelMissing {
            path: file.display().to_string(),
        },
        other => WhisperError::ModelLoad(format!("{}: {other}", file.display())),
    }
}

impl WhisperModel {
    /// Open the model in `dir`, the folder of the pinned export.
    ///
    /// Returns [`WhisperError::ModelMissing`] if `dir` does not exist (so
    /// callers can present a structured hint instead of an opaque ORT error),
    /// [`WhisperError::RuntimeUnavailable`] if there is no ONNX Runtime library
    /// to load, [`WhisperError::UnexpectedExport`] if the folder is not the
    /// export this build reads (a file is missing, or a session's inputs and
    /// outputs differ), and [`WhisperError::ModelLoad`] for any other failure of
    /// the runtime to open a file.
    ///
    /// Nothing is downloaded: the folder is the user's, from Settings, or a
    /// developer's (`WHISPER_MODEL_PATH`).
    pub fn load(dir: &Path) -> Result<Self> {
        if !dir.exists() {
            return Err(WhisperError::ModelMissing {
                path: dir.display().to_string(),
            });
        }
        // Before any `ort` call: `Session::builder()` panics, rather than
        // erroring, when the library is missing. `ModelRegistry::load` asks
        // too. Asking here, before the folder is looked at, makes the answer on
        // a machine without the runtime the same whatever the path holds
        // (`tests/runtime_missing.rs` and the tools' `ml_runtime_missing` point
        // it at a file that is not a model).
        ml_pipeline::runtime::ensure()
            .map_err(|e| WhisperError::RuntimeUnavailable(e.to_string()))?;

        let files = locate(dir)?;

        let tokenizer = Tokenizer::from_file(&files.tokenizer)?;
        let ids = token_ids(&tokenizer)?;

        let encoder = load_session(registry(), "encoder", &files.encoder)
            .map_err(|e| load_error(e, &files.encoder))?;
        let decoder = load_session(registry(), "decoder", &files.decoder)
            .map_err(|e| load_error(e, &files.decoder))?;

        // One lock at a time.
        let (enc_inputs, enc_outputs) = {
            let enc = lock_session(&encoder);
            (specs(enc.inputs()), specs(enc.outputs()))
        };
        let (dec_inputs, dec_outputs) = {
            let dec = lock_session(&decoder);
            (specs(dec.inputs()), specs(dec.outputs()))
        };
        export::check_encoder(&enc_inputs, &enc_outputs)?;
        let frames_dims = enc_outputs
            .iter()
            .find(|o| o.name == "last_hidden_state")
            .map(|o| o.dims.clone())
            .unwrap_or_default();
        let layout = export::check_decoder(&dec_inputs, &dec_outputs, &frames_dims)?;
        if layout.vocab != ids.vocab_size || tokenizer.vocab_size() != layout.vocab {
            return Err(WhisperError::UnexpectedExport(format!(
                "the decoder has {} logits but tokenizer.json has {} tokens",
                layout.vocab,
                tokenizer.vocab_size()
            )));
        }
        let heads = alignment_heads(layout.layers, layout.heads).ok_or_else(|| {
            WhisperError::UnexpectedExport(format!(
                "this build does not know the alignment heads of a decoder with {} layers of {} \
                 heads",
                layout.layers, layout.heads
            ))
        })?;

        // The no-speech rule (`no_speech_threshold` and `logprob_threshold`) is
        // the config's, upstream's, and lives in `decode.rs`.
        let config = DecodeConfig {
            tokens: ids,
            // Timestamp tokens (#659): the model says where each segment starts
            // and stops, and the window uses that for its first word and for
            // where the next window starts.
            with_timestamps: true,
            ..DecodeConfig::multilingual()
        };
        Ok(Self {
            encoder,
            decoder,
            layout,
            heads,
            tokenizer,
            config,
            mel: MelExtractor::new(),
        })
    }

    /// The decoder's geometry, as read from the files.
    pub fn layout(&self) -> &ExportLayout {
        &self.layout
    }

    /// The token for a language code (`"en"`), or an error if the model has
    /// no such language.
    fn language_token(&self, code: &str) -> Result<u32> {
        language_token(&self.tokenizer, &self.config.tokens.languages, code)
    }
}

/// The three files of the export, found.
struct Files {
    encoder: PathBuf,
    decoder: PathBuf,
    tokenizer: PathBuf,
}

/// Find the export's files in `dir`, or say which is missing.
fn locate(dir: &Path) -> Result<Files> {
    let file = |name: &str| -> Result<PathBuf> {
        let path = dir.join(name);
        if path.is_file() {
            Ok(path)
        } else {
            Err(WhisperError::UnexpectedExport(format!(
                "{} has no {name}; the folder has to hold the files of \
                 onnx-community/whisper-base_timestamped ({ENCODER_FILE}, {DECODER_FILE} and \
                 {TOKENIZER_FILE})",
                dir.display()
            )))
        }
    };
    Ok(Files {
        encoder: file(ENCODER_FILE)?,
        decoder: file(DECODER_FILE)?,
        tokenizer: file(TOKENIZER_FILE)?,
    })
}

/// The token for a language code (`"en"`) among the language tokens
/// `languages`, or [`WhisperError::UnknownLanguage`].
fn language_token(tokenizer: &Tokenizer, languages: &Range<u32>, code: &str) -> Result<u32> {
    tokenizer
        .special_id(&format!("<|{}|>", code.trim().to_ascii_lowercase()))
        .filter(|t| languages.contains(t))
        .ok_or_else(|| WhisperError::UnknownLanguage(code.to_string()))
}

/// `"en"` for the token `<|en|>`.
fn language_code(tokenizer: &Tokenizer, token: u32) -> Option<String> {
    match tokenizer.pieces(&[token]).ok()?.pop()? {
        Piece::Control(name) => name
            .strip_prefix("<|")
            .and_then(|n| n.strip_suffix("|>"))
            .map(str::to_string),
        _ => None,
    }
}

/// The special-token ids of this tokenizer, which have to be the ones the
/// decode loop's defaults and the export's prompt use.
fn token_ids(tokenizer: &Tokenizer) -> Result<TokenIds> {
    let expected = TokenIds::multilingual();
    let named = |name: &str| tokenizer.special_id(name);
    let checks = [
        ("<|endoftext|>", expected.eot),
        ("<|startoftranscript|>", expected.sot),
        ("<|transcribe|>", expected.transcribe),
        ("<|nocaptions|>", expected.no_speech),
        ("<|notimestamps|>", expected.no_timestamps),
        ("<|0.00|>", expected.timestamp_begin),
        ("<|en|>", expected.languages.start),
    ];
    for (name, id) in checks {
        if named(name) != Some(id) {
            return Err(WhisperError::UnexpectedExport(format!(
                "tokenizer.json numbers {name} as {:?}, this build expects {id}: it is not the \
                 multilingual tokenizer of the pinned export",
                named(name)
            )));
        }
    }
    if tokenizer.vocab_size() != expected.vocab_size {
        return Err(WhisperError::UnexpectedExport(format!(
            "tokenizer.json has {} tokens, this build expects {}",
            tokenizer.vocab_size(),
            expected.vocab_size
        )));
    }
    Ok(expected)
}

/// The windows of one run: the model and the cancel flag.
struct ModelWindows<'a> {
    model: &'a WhisperModel,
    cancel: &'a dyn Fn() -> bool,
}

impl WindowTranscriber for ModelWindows<'_> {
    fn window(&mut self, audio: &[f32], request: WindowRequest) -> Result<WindowResult> {
        let m = self.model;
        let language = request.language;
        if is_silent(audio) {
            // Nothing to hear, so nothing to ask the model, and no language to learn.
            return Ok(WindowResult {
                words: Vec::new(),
                language: language.unwrap_or(m.config.tokens.languages.start),
                speech: false,
                next: None,
                last_speech_s: None,
            });
        }
        let mel = m.mel.log_mel_window(audio).into_vec();
        let encoder = run_encoder(&m.encoder, mel, &m.layout)?;

        let run = OrtDecoder::lock(&m.decoder, m.layout);
        let mut decoder = CachedDecoder::new(run, m.layout, m.heads, self.cancel);
        let decoded = decode_window(&mut decoder, &encoder, &m.config, language)
            .map_err(WhisperError::from)?;
        let no_words = |language| WindowResult {
            words: Vec::new(),
            language,
            speech: false,
            next: None,
            last_speech_s: None,
        };

        if decoded.stop == StopReason::NoSpeech || decoded.tokens.is_empty() {
            return Ok(no_words(decoded.language));
        }

        let ids = &m.config.tokens;
        let window_s = audio.len() as f32 / crate::mel::SAMPLE_RATE as f32;
        let split = split_segments(
            &decoded.tokens,
            ids.timestamp_begin,
            window_s,
            request.to_end,
        );
        // The text of the segments that are kept, which is what is aligned:
        // upstream gives `find_alignment` the text tokens of the segments.
        let (text_tokens, segments) = text_of_segments(&split.segments, &decoded.tokens, ids.eot);
        if text_tokens.is_empty() {
            // Timestamps and no text: nothing was said that can be written down.
            let mut result = no_words(decoded.language);
            result.next = split.seek_s;
            return Ok(result);
        }

        // The alignment pass: the text again, as one sequence with
        // `<|notimestamps|>` in its prompt, so the attention is the one the
        // aligner was built on (`find_alignment` feeds `sot_sequence +
        // no_timestamps + text`). One forward pass, not one per token.
        let content_frames = encoder_frames(audio.len().min(N_SAMPLES)).clamp(1, m.layout.frames);
        let (heads, text_probs) = decoder.alignment_attention(
            &[ids.sot, decoded.language, ids.transcribe, ids.no_timestamps],
            &text_tokens,
            ids.eot,
            &encoder,
            content_frames,
        )?;

        let code = language_code(&m.tokenizer, decoded.language).unwrap_or_default();
        let alignment = align_words(
            &AlignmentInput {
                heads: &heads,
                content_frames,
                tokens: &text_tokens,
                token_probs: &text_probs,
            },
            |t| m.tokenizer.token_bytes(t),
            WordSplit::for_language(&code),
        )?;
        let placed = place_words(
            alignment,
            &segments,
            request.last_speech_s,
            window_s,
            PREPEND_PUNCTUATIONS,
            APPEND_PUNCTUATIONS,
        );
        // Upstream's `get_end`: the speech so far ends where the last word
        // does, before the sound pulls its edge in.
        let last_word_end = placed.last_word_end;
        let mut words = placed.words;
        // The timestamps do not say where speech starts after a pause or a
        // lead-in (the model's segments run end to end), so the sound does.
        trim_to_sound(&mut words, audio);

        // Where the next window starts: upstream seeks to the end of the last
        // word of the finished segments, or, with no words, to the last
        // timestamp; a window the model closed moves on whole.
        let next = if split.single_timestamp_ending {
            None
        } else {
            last_word_end.filter(|&e| e > 0.0).or(split.seek_s)
        };
        Ok(WindowResult {
            last_speech_s: last_word_end,
            words,
            language: decoded.language,
            speech: true,
            next,
        })
    }
}

/// Transcribe `samples`, interleaved at `sample_rate` Hz with `channels`
/// channels, into words with times.
///
/// `language` is a language code (`"en"`), or `None` to detect it from the
/// first window that has speech. `cancel` is asked before every window and
/// before every decoder step; when it answers `true` the call ends with
/// [`WhisperError::Cancelled`] and returns nothing.
///
/// Slow work: a 30 s window costs one encoder pass and up to 224 decoder
/// steps. Call it off any lock the app holds.
///
/// ## What the words are
///
/// `text` has its leading space trimmed and its punctuation attached
/// (`"Hello,"`). `start_s`/`end_s` are seconds from the start of `samples`,
/// estimated from the decoder's cross-attention (a DTW over a few attention
/// heads, as `whisper.timing.find_alignment` does), not measured, and placed in
/// the segments Whisper's timestamp tokens give as `add_word_timestamps` does:
/// a word that ran on across a pause is cut back to about twice the median
/// word, and a window's last words are not decoded again by the next. The
/// aligner lays words end to end, so a word ends where the next starts; the
/// sound in the audio then pulls an edge in (`refine::trim_to_sound`): a word
/// that is followed or preceded by a pause of a quarter of a second or more ends
/// or starts where its sound does, not across the pause. Between words spoken
/// with no pause the boundary stays the aligner's, which on the pinned model is
/// about 0.14 s early, so a start is typically within 0.2 s of the word's and an
/// end up to a tenth of a second or so before the word has finished.
/// `confidence` is the mean probability the model gave the word's tokens.
pub fn transcribe(
    model: &WhisperModel,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    language: Option<&str>,
    cancel: impl Fn() -> bool,
) -> Result<Transcript> {
    transcribe_observed(
        model,
        samples,
        sample_rate,
        channels,
        language,
        cancel,
        |_, _| {},
    )
}

/// [`transcribe`] that says which window it is on.
///
/// `on_window(index, total)` is called before each 30 s window is decoded
/// (and after `cancel` has answered no for it): `index` counts from 0 and is
/// the number of windows already done, `total` is the number the audio will
/// take as far as is known now. It is an estimate that can grow, never shrink,
/// because a window starts the next one where its last finished word ended,
/// earlier than 30 s on; `total` is always more than `index`. It is called on
/// the thread that called this and must not block.
pub fn transcribe_observed(
    model: &WhisperModel,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    language: Option<&str>,
    cancel: impl Fn() -> bool,
    on_window: impl Fn(usize, usize),
) -> Result<Transcript> {
    if !samples.iter().all(|s| s.is_finite()) {
        return Err(WhisperError::InvalidAudio(
            "audio buffer contains non-finite samples".into(),
        ));
    }
    let language = language
        .map(|code| model.language_token(code))
        .transpose()?;
    let mono = resample_to_16khz_mono(samples, sample_rate, channels)?;
    let mut windows = ModelWindows {
        model,
        cancel: &cancel,
    };
    let chunked = run_windows_observed(&mut windows, &mono, language, &cancel, &on_window)?;
    Ok(Transcript {
        words: chunked.words,
        language: chunked
            .language
            .and_then(|t| language_code(&model.tokenizer, t)),
        windows: chunked.windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_tokenizer() -> Tokenizer {
        Tokenizer::from_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tokenizer_tiny.json"),
        )
        .expect("the tiny tokenizer fixture loads")
    }

    // ---- replacing the files of a loaded model, on the real model -----------------

    /// Copy the three files of a model folder into `to`, as the downloader lays them out.
    fn copy_model(from: &Path, to: &Path) {
        for file in [ENCODER_FILE, DECODER_FILE, TOKENIZER_FILE] {
            let target = to.join(file);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(from.join(file), &target).unwrap();
        }
    }

    /// Give the model files in `dir` a later modification time, which is what
    /// a model removed and downloaded again looks like to `file_version`.
    fn touch_model(dir: &Path, later_by: std::time::Duration) {
        for file in [ENCODER_FILE, DECODER_FILE] {
            let f = std::fs::File::options()
                .write(true)
                .open(dir.join(file))
                .unwrap();
            f.set_modified(std::time::SystemTime::now() + later_by)
                .unwrap();
        }
    }

    #[test]
    #[ignore = "needs ORT_DYLIB_PATH and WHISPER_MODEL_PATH (the folder of the pinned whisper-base)"]
    fn replacing_the_model_files_replaces_their_sessions() {
        use std::sync::Arc;
        use std::time::Duration;

        let source =
            PathBuf::from(std::env::var_os("WHISPER_MODEL_PATH").expect(
                "WHISPER_MODEL_PATH is not set: a skipped --ignored test is a vacuous pass",
            ));
        assert!(
            std::env::var_os("ORT_DYLIB_PATH").is_some(),
            "ORT_DYLIB_PATH is not set: a skipped --ignored test is a vacuous pass"
        );
        let dir = tempfile::tempdir().unwrap();
        copy_model(&source, dir.path());

        // The registry is the process's, so count against where it started.
        let start = registry().len();

        let first = WhisperModel::load(dir.path()).expect("the model loads");
        assert_eq!(registry().len(), start + 2, "an encoder and a decoder");
        let first_encoder = Arc::downgrade(&first.encoder);
        let first_decoder = Arc::downgrade(&first.decoder);
        drop(first);
        assert!(
            first_encoder.upgrade().is_some(),
            "the registry keeps the sessions of a model that is not in use, so the next load is quick"
        );

        // The files are replaced (the model removed and downloaded again).
        touch_model(dir.path(), Duration::from_secs(3600));
        let second = WhisperModel::load(dir.path()).expect("the replaced model loads");
        assert_eq!(
            registry().len(),
            start + 2,
            "the sessions of the files that were replaced are still held: about 136 MB each time"
        );
        assert!(
            first_encoder.upgrade().is_none() && first_decoder.upgrade().is_none(),
            "nothing uses the first sessions and the registry still holds them"
        );

        // A model that is in use when its files are replaced keeps working.
        touch_model(dir.path(), Duration::from_secs(7200));
        let third = WhisperModel::load(dir.path()).expect("loads again");
        assert_eq!(registry().len(), start + 2);
        assert!(!lock_session(&second.encoder).inputs().is_empty());
        assert!(!lock_session(&second.decoder).inputs().is_empty());
        drop((second, third));
    }

    // ---- the cost of a step, on the real model ----------------------------------

    /// `[SOT, language, transcribe, notimestamps]`.
    const PROMPT_LEN: usize = 4;

    /// What a forced decode of `steps` tokens moved out of the runtime, and how
    /// long it took, step by step.
    struct Steps {
        /// Bytes copied out of ONNX Runtime by step `i` alone (the prompt is step 0).
        copied: Vec<u64>,
        /// Wall time of step `i`.
        took: Vec<std::time::Duration>,
    }

    /// Run the real decoder for the prompt and then `new_tokens` forced tokens,
    /// which is the shape of a window that runs to the length cap, without
    /// needing 30 s of dense speech: the cost of a step does not depend on which
    /// token it is fed.
    fn forced_decode(model: &WhisperModel, new_tokens: usize) -> Steps {
        use crate::decode::DecoderStep;
        let silence = vec![0.0f32; crate::mel::N_MELS * crate::mel::N_FRAMES];
        let encoder =
            run_encoder(&model.encoder, silence, &model.layout).expect("the encoder runs");
        let cancel = || false;
        let run = OrtDecoder::lock(&model.decoder, model.layout);
        let mut decoder = CachedDecoder::new(run, model.layout, model.heads, &cancel);
        let ids = &model.config.tokens;
        let prompt = [
            ids.sot,
            ids.languages.start,
            ids.transcribe,
            ids.no_timestamps,
        ];
        assert_eq!(prompt.len(), PROMPT_LEN);

        let (mut copied, mut took) = (Vec::new(), Vec::new());
        let mut step = |tokens: &[u32], past: usize| {
            let (bytes, clock) = (crate::ort_io::copied::total(), std::time::Instant::now());
            decoder.step(tokens, past, &encoder).expect("a step runs");
            took.push(clock.elapsed());
            copied.push(crate::ort_io::copied::total() - bytes);
        };
        step(&prompt, 0);
        for i in 0..new_tokens {
            step(&[440], PROMPT_LEN + i);
        }
        Steps { copied, took }
    }

    #[test]
    #[ignore = "needs ORT_DYLIB_PATH and WHISPER_MODEL_PATH (the folder of the pinned whisper-base)"]
    fn a_decoder_step_copies_a_cost_that_does_not_grow() {
        let dir =
            PathBuf::from(std::env::var_os("WHISPER_MODEL_PATH").expect(
                "WHISPER_MODEL_PATH is not set: a skipped --ignored test is a vacuous pass",
            ));
        assert!(
            std::env::var_os("ORT_DYLIB_PATH").is_some(),
            "ORT_DYLIB_PATH is not set: a skipped --ignored test is a vacuous pass"
        );
        let model = WhisperModel::load(&dir).expect("the model loads");
        let cap = model.config.max_new_tokens;
        let s = forced_decode(&model, cap);

        let sum = |a: usize, b: usize| {
            s.took[a..b]
                .iter()
                .map(|d| d.as_secs_f64() * 1e3)
                .sum::<f64>()
        };
        eprintln!(
            "steps: {}; copied out of the runtime: prompt {} B, step 20 {} B, step 100 {} B, \
             step {cap} {} B, all {} B",
            s.copied.len(),
            s.copied[0],
            s.copied[20],
            s.copied[100],
            s.copied[cap],
            s.copied.iter().sum::<u64>()
        );
        eprintln!(
            "wall: steps 1..=20 {:.1} ms, steps {}..={cap} {:.1} ms, all {:.1} ms",
            sum(1, 21),
            cap - 19,
            sum(cap - 19, cap + 1),
            sum(0, s.took.len())
        );

        // A later step moves what an earlier one does: its logits row and the
        // alignment heads' attention row, whatever the cache has grown to.
        assert_eq!(
            s.copied[cap], s.copied[20],
            "step {cap} copied {} B out of the runtime, step 20 copied {} B: the cache is being \
             copied on every step",
            s.copied[cap], s.copied[20]
        );
        assert!(
            s.copied[20] < 1 << 20,
            "a step copies {} B, which is more than a logits row and the attention rows",
            s.copied[20]
        );
    }

    #[test]
    fn the_file_names_are_the_pinned_ones() {
        // `ml_pipeline::models::whisper_base()` is the one place the downloaded
        // files' paths are written down (`models.json`). These constants say
        // where `load` looks, so a pin that moves a file must move them too, or
        // a model the app downloaded is a folder `load` refuses.
        let pinned: std::collections::BTreeSet<&str> = ml_pipeline::models::whisper_base()
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        let read: std::collections::BTreeSet<&str> =
            [ENCODER_FILE, DECODER_FILE, TOKENIZER_FILE].into();
        assert_eq!(read, pinned, "load reads {read:?}, the pin has {pinned:?}");
    }

    #[test]
    fn the_same_file_in_two_roles_is_two_sessions() {
        // Given one file for both, the registry used to return one session for
        // both and `load` locked it twice. (A symlink resolves to the same
        // canonical path.)
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("m.onnx");
        std::fs::write(&file, b"x").unwrap();
        assert_ne!(
            session_family("encoder", &file),
            session_family("decoder", &file)
        );
        assert_eq!(
            session_family("encoder", &file),
            session_family("encoder", &file)
        );
        // And a changed file is a new version of the same place, which is a
        // new session (and the registry drops the old one: #658).
        let before = (session_family("encoder", &file), file_version(&file));
        std::fs::write(&file, b"xy").unwrap();
        assert_eq!(before.0, session_family("encoder", &file));
        assert_ne!(before.1, file_version(&file));
    }

    #[test]
    fn a_folder_with_a_file_missing_says_which() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("onnx")).unwrap();
        std::fs::write(dir.path().join(ENCODER_FILE), b"x").unwrap();
        std::fs::write(dir.path().join(TOKENIZER_FILE), b"{}").unwrap();
        let err = locate(dir.path()).err().expect("the decoder is missing");
        let WhisperError::UnexpectedExport(why) = &err else {
            panic!("{err:?}");
        };
        assert!(why.contains(DECODER_FILE), "{why}");
        assert!(why.contains("whisper-base_timestamped"), "{why}");

        std::fs::write(dir.path().join(DECODER_FILE), b"x").unwrap();
        assert!(locate(dir.path()).is_ok());
    }

    #[test]
    fn a_path_that_is_a_file_is_not_a_model_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.onnx");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(
            locate(&file).err(),
            Some(WhisperError::UnexpectedExport(_))
        ));
    }

    #[test]
    fn a_tokenizer_numbered_differently_is_refused_by_name() {
        // The tiny fixture numbers <|endoftext|> as 11, not 50257.
        let err = token_ids(&tiny_tokenizer()).unwrap_err();
        let WhisperError::UnexpectedExport(why) = &err else {
            panic!("{err:?}");
        };
        assert!(
            why.contains("<|endoftext|>") && why.contains("50257"),
            "{why}"
        );
    }

    #[test]
    fn language_codes_map_to_tokens_and_back() {
        let tokenizer = tiny_tokenizer();
        let languages = 13..15; // <|en|>, <|de|>
        assert_eq!(language_token(&tokenizer, &languages, "en").unwrap(), 13);
        assert_eq!(language_token(&tokenizer, &languages, " DE ").unwrap(), 14);
        for bad in ["xx", "", "transcribe", "0.00"] {
            let err = language_token(&tokenizer, &languages, bad).unwrap_err();
            assert!(
                matches!(err, WhisperError::UnknownLanguage(_)),
                "{bad}: {err:?}"
            );
        }
        assert_eq!(language_code(&tokenizer, 13).as_deref(), Some("en"));
        assert_eq!(language_code(&tokenizer, 14).as_deref(), Some("de"));
        assert_eq!(
            language_code(&tokenizer, 0),
            None,
            "a text token is no language"
        );
    }
}
