//! Turning Whisper's token ids back into text.
//!
//! The decoder (plan step 3 on #384) produces ids; the transcript needs
//! words. The pinned export ships `tokenizer.json` in Hugging Face's format
//! (a GPT-2-style byte-level BPE, 51 865 ids), and this module reads it. It
//! only **decodes**: Whisper's decoder is fed a fixed prompt of special ids,
//! never user text, so there is no need for the merge table or for the
//! `tokenizers` crate (the plan chose to do without it; it would bring a
//! regex engine and a few dozen dependencies to read one 2.4 MB file).
//!
//! ## What the file says
//!
//! - `model.vocab` maps each token string to its id, ids `0..=50257`. The
//!   strings are *byte-level*: every byte is written as one printable
//!   character by GPT-2's `bytes_to_unicode` table (space is `Ġ`, newline
//!   `Ċ`, the byte `0xC3` is `Ã`), so any byte sequence, including half a
//!   UTF-8 character, is a valid token. `byte_for_char` is that table
//!   read backwards.
//! - `added_tokens` lists ids `50257..=51864`: the control tokens
//!   (`<|endoftext|>`, `<|startoftranscript|>`, one `<|en|>` per language,
//!   `<|translate|>`, `<|transcribe|>`, `<|nocaptions|>`,
//!   `<|notimestamps|>`, ...) and 1 501 timestamp tokens `<|0.00|>` to
//!   `<|30.00|>`. `<|endoftext|>` is in both lists.
//!
//! ## What decoding does
//!
//! - **Text tokens** (vocab only): their bytes are joined and then decoded as
//!   UTF-8 *once*, not token by token. Whisper regularly splits one character
//!   across tokens (an emoji is typically two); decoding each token alone
//!   would give a replacement character for each half. Bytes that are still
//!   not valid UTF-8 after joining become U+FFFD, as OpenAI's own
//!   `decode(errors="replace")` does.
//! - **Control tokens** carry no text and are dropped from [`Tokenizer::decode`].
//! - **Timestamp tokens** are time, not text. Token `<|x|>` is the
//!   `(id - id_of("<|0.00|>"))`-th 20 ms step, so in the pinned export id
//!   `50364 + n` is `n * 0.02` seconds. They are dropped from `decode` and
//!   reported by [`Tokenizer::pieces`] with their time. The tokenizer checks
//!   at load that the names and ids agree, so a differently numbered export
//!   is an error rather than a transcript with wrong times.
//! - **Leading spaces are kept.** Whisper writes a word with its space in
//!   front (`" Hello"`); the space is how the timing step (plan step 4)
//!   finds word boundaries, so `decode` does not trim.
//! - **An unknown id is an error**, not a silent gap: it means the model and
//!   the tokenizer file do not belong together.
//!
//! The tests run against a small hand-written file in the same format
//! (`tests/fixtures/tokenizer_tiny.json`, which Hugging Face's `tokenizers`
//! also loads) so CI needs no download. `tests/tokenizer_pinned_file.rs`
//! checks the real file, and is ignored unless it is pointed at one.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// Timestamp tokens are 20 ms apart: 50 per second.
pub const TIMESTAMPS_PER_SECOND: u32 = 50;

/// Why a tokenizer could not be loaded, or why ids could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum TokenizerError {
    #[error("cannot read the tokenizer file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },

    #[error("tokenizer.json is not the expected JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// A tokenizer that is not a byte-level BPE (Whisper's is).
    #[error("unsupported tokenizer: {0}")]
    Unsupported(String),

    /// A vocab string with a character outside GPT-2's byte table, so it is
    /// not a byte-level token.
    #[error(
        "vocab entry {token:?} (id {id}) is not byte-level: {ch:?} is not in GPT-2's byte table"
    )]
    NotByteLevel { token: String, id: u32, ch: char },

    /// The file contradicts itself: a shared id, or timestamp tokens whose
    /// names do not match their ids.
    #[error("inconsistent tokenizer.json: {0}")]
    Inconsistent(String),

    /// An id the file does not define.
    #[error("token id {0} is not in the tokenizer")]
    UnknownId(u32),
}

/// What kind of token an id is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Part of the text: its bytes are in [`Tokenizer::token_bytes`].
    Text,
    /// `<|endoftext|>`, `<|startoftranscript|>`, a language, a task, ...
    Control,
    /// `<|0.00|>` to `<|30.00|>`.
    Timestamp,
}

/// One run of a decoded sequence, in order. See [`Tokenizer::pieces`].
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    /// Consecutive text tokens, decoded together.
    Text(String),
    /// A control token, by its name (`"<|en|>"`).
    Control(String),
    /// A timestamp token, in seconds.
    Timestamp(f32),
}

/// A decode-only view of Whisper's `tokenizer.json`.
#[derive(Debug, Clone)]
pub struct Tokenizer {
    /// Indexed by id.
    entries: Vec<Entry>,
    /// Control and timestamp tokens by name.
    named: HashMap<String, u32>,
    /// The id of `<|0.00|>`, if the file has timestamp tokens.
    timestamp_begin: Option<u32>,
}

#[derive(Debug, Clone)]
enum Entry {
    /// No such id.
    Missing,
    Text(Box<[u8]>),
    Control(String),
    /// The step number: `n` is `n * 0.02` seconds.
    Timestamp(u32),
}

#[derive(Deserialize)]
struct File {
    added_tokens: Vec<AddedToken>,
    decoder: Option<Typed>,
    model: Model,
}

#[derive(Deserialize)]
struct AddedToken {
    id: u32,
    content: String,
}

#[derive(Deserialize)]
struct Typed {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Model {
    #[serde(rename = "type")]
    kind: String,
    vocab: HashMap<String, u32>,
}

impl Tokenizer {
    /// Read a `tokenizer.json` from its text.
    pub fn from_json(json: &str) -> Result<Self, TokenizerError> {
        let file: File = serde_json::from_str(json)?;

        if file.model.kind != "BPE" {
            return Err(TokenizerError::Unsupported(format!(
                "model type {:?}, expected \"BPE\"",
                file.model.kind
            )));
        }
        match file.decoder.as_ref().map(|d| d.kind.as_str()) {
            Some("ByteLevel") => {}
            other => {
                return Err(TokenizerError::Unsupported(format!(
                    "decoder {:?}, expected \"ByteLevel\"",
                    other.unwrap_or("none")
                )))
            }
        }

        let max_id = file
            .model
            .vocab
            .values()
            .copied()
            .chain(file.added_tokens.iter().map(|t| t.id))
            .max();
        let mut entries = vec![Entry::Missing; max_id.map_or(0, |m| m as usize + 1)];

        // Added tokens first: `<|endoftext|>` is in both lists, and the added
        // list is the one that says what it means.
        let mut named = HashMap::new();
        let mut timestamps = Vec::new();
        for token in &file.added_tokens {
            let slot = &mut entries[token.id as usize];
            if !matches!(slot, Entry::Missing) {
                return Err(TokenizerError::Inconsistent(format!(
                    "two added tokens have id {}",
                    token.id
                )));
            }
            *slot = match parse_timestamp(&token.content) {
                Some(step) => {
                    timestamps.push((step, token.id, token.content.as_str()));
                    Entry::Timestamp(step)
                }
                None => Entry::Control(token.content.clone()),
            };
            named.insert(token.content.clone(), token.id);
        }

        for (token, &id) in &file.model.vocab {
            let slot = &mut entries[id as usize];
            match slot {
                // The same token listed as added: already handled.
                Entry::Control(name) if name == token => continue,
                Entry::Missing => {}
                _ => {
                    return Err(TokenizerError::Inconsistent(format!(
                        "vocab entry {token:?} shares id {id} with another token"
                    )))
                }
            }
            let mut bytes = Vec::with_capacity(token.len());
            for ch in token.chars() {
                bytes.push(
                    byte_for_char(ch).ok_or_else(|| TokenizerError::NotByteLevel {
                        token: token.clone(),
                        id,
                        ch,
                    })?,
                );
            }
            *slot = Entry::Text(bytes.into_boxed_slice());
        }

        // `<|0.02|>` is step 1, so its id must be one more than `<|0.00|>`'s.
        // A file numbered differently would give every word the wrong time.
        let timestamp_begin = timestamps.iter().find(|(step, ..)| *step == 0).map(|t| t.1);
        if let Some(begin) = timestamp_begin {
            for (step, id, name) in &timestamps {
                if begin + step != *id {
                    return Err(TokenizerError::Inconsistent(format!(
                        "timestamp token {name} has id {id}, but step {step} after <|0.00|> (id {begin}) is id {}",
                        begin + step
                    )));
                }
            }
        } else if !timestamps.is_empty() {
            return Err(TokenizerError::Inconsistent(
                "timestamp tokens without <|0.00|>".into(),
            ));
        }

        Ok(Self {
            entries,
            named,
            timestamp_begin,
        })
    }

    /// Read a `tokenizer.json` from disk.
    pub fn from_file(path: &Path) -> Result<Self, TokenizerError> {
        let json = std::fs::read_to_string(path).map_err(|source| TokenizerError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&json)
    }

    /// One more than the largest id: the width of the decoder's logits
    /// (51 865 for the pinned export).
    pub fn vocab_size(&self) -> usize {
        self.entries.len()
    }

    /// The id of a control or timestamp token by its name, e.g.
    /// `"<|notimestamps|>"`. Text tokens are not listed here.
    pub fn special_id(&self, name: &str) -> Option<u32> {
        self.named.get(name).copied()
    }

    /// The id of `<|0.00|>`: ids from here up are timestamps.
    pub fn timestamp_begin(&self) -> Option<u32> {
        self.timestamp_begin
    }

    /// What `id` is, or `None` if the file does not define it.
    pub fn kind(&self, id: u32) -> Option<TokenKind> {
        match self.entry(id)? {
            Entry::Missing => None,
            Entry::Text(_) => Some(TokenKind::Text),
            Entry::Control(_) => Some(TokenKind::Control),
            Entry::Timestamp(_) => Some(TokenKind::Timestamp),
        }
    }

    /// The bytes of a text token (`None` for a control or timestamp token or
    /// an unknown id). The timing step uses these to group tokens into words
    /// without cutting a UTF-8 character in half.
    pub fn token_bytes(&self, id: u32) -> Option<&[u8]> {
        match self.entry(id)? {
            Entry::Text(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// The time of a timestamp token in seconds, or `None` if `id` is not
    /// one.
    pub fn timestamp_seconds(&self, id: u32) -> Option<f32> {
        match self.entry(id)? {
            Entry::Timestamp(step) => Some(*step as f32 / TIMESTAMPS_PER_SECOND as f32),
            _ => None,
        }
    }

    /// The text of `ids`: text tokens only, joined and decoded as UTF-8.
    /// Control and timestamp tokens are dropped.
    pub fn decode(&self, ids: &[u32]) -> Result<String, TokenizerError> {
        let mut bytes = Vec::new();
        for &id in ids {
            match self.entry(id) {
                None | Some(Entry::Missing) => return Err(TokenizerError::UnknownId(id)),
                Some(Entry::Text(b)) => bytes.extend_from_slice(b),
                Some(Entry::Control(_) | Entry::Timestamp(_)) => {}
            }
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `ids` as runs of text, control tokens and timestamps, in order: the
    /// form for a sequence decoded with timestamps (`<|0.00|> Hello<|1.20|>`).
    /// Consecutive text tokens are one [`Piece::Text`], decoded together.
    pub fn pieces(&self, ids: &[u32]) -> Result<Vec<Piece>, TokenizerError> {
        let mut pieces = Vec::new();
        let mut run: Vec<u8> = Vec::new();
        let flush = |run: &mut Vec<u8>, pieces: &mut Vec<Piece>| {
            if !run.is_empty() {
                pieces.push(Piece::Text(String::from_utf8_lossy(run).into_owned()));
                run.clear();
            }
        };
        for &id in ids {
            match self.entry(id) {
                None | Some(Entry::Missing) => return Err(TokenizerError::UnknownId(id)),
                Some(Entry::Text(b)) => run.extend_from_slice(b),
                Some(Entry::Control(name)) => {
                    flush(&mut run, &mut pieces);
                    pieces.push(Piece::Control(name.clone()));
                }
                Some(Entry::Timestamp(step)) => {
                    flush(&mut run, &mut pieces);
                    pieces.push(Piece::Timestamp(
                        *step as f32 / TIMESTAMPS_PER_SECOND as f32,
                    ));
                }
            }
        }
        flush(&mut run, &mut pieces);
        Ok(pieces)
    }

    fn entry(&self, id: u32) -> Option<&Entry> {
        self.entries.get(id as usize)
    }
}

/// The step number of a timestamp token's name: `<|1.20|>` is step 60.
/// Whisper writes seconds with two decimals and steps are 20 ms, so the
/// hundredths must be even; anything else is not a timestamp token.
fn parse_timestamp(name: &str) -> Option<u32> {
    let inner = name.strip_prefix("<|")?.strip_suffix("|>")?;
    let (whole, hundredths) = inner.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || hundredths.len() != 2 || !digits(hundredths) {
        return None;
    }
    let centiseconds =
        whole.parse::<u32>().ok()?.checked_mul(100)? + hundredths.parse::<u32>().ok()?;
    (centiseconds % 2 == 0).then_some(centiseconds / 2)
}

/// GPT-2's `bytes_to_unicode`, backwards: the byte a vocab character stands
/// for. The printable Latin-1 bytes (`!` to `~`, `¡` to `¬`, `®` to `ÿ`)
/// stand for themselves; the other 68 bytes (`0x00` to `0x20`, `0x7F` to
/// `0xA0`, `0xAD`) were given the characters `U+0100` onwards, in byte order.
fn byte_for_char(c: char) -> Option<u8> {
    match c as u32 {
        cp @ (0x21..=0x7E | 0xA1..=0xAC | 0xAE..=0xFF) => Some(cp as u8),
        cp @ 0x100..=0x143 => {
            // The n-th byte that is not printable, counting from zero: the 33
            // from 0x00 to 0x20, the 34 from 0x7F to 0xA0, then 0xAD.
            let n = cp - 0x100;
            Some(match n {
                0..=32 => n as u8,
                33..=66 => (0x7F + (n - 33)) as u8,
                _ => 0xAD,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = include_str!("../tests/fixtures/tokenizer_tiny.json");

    // The ids in tokenizer_tiny.json.
    const H: u32 = 0;
    const ELLO: u32 = 1;
    const SP_WORLD: u32 = 2;
    const DOT: u32 = 3;
    const SPACE: u32 = 4;
    const A_TILDE: u32 = 5; // byte 0xC3
    const COPYRIGHT: u32 = 6; // byte 0xA9; with 0xC3 it is "é"
    const SP_CAF: u32 = 7;
    const NEWLINE: u32 = 8;
    const RI_BEN_LEAD: u32 = 9; // bytes E6 97: the first two thirds of "日"
    const RI_BEN_TAIL: u32 = 10; // byte A5: the last third
    const EOT: u32 = 11;
    const SOT: u32 = 12;
    const EN: u32 = 13;
    const DE: u32 = 14;
    const TRANSCRIBE: u32 = 16;
    const NO_TIMESTAMPS: u32 = 20;
    const TS_0_00: u32 = 21;
    const TS_0_04: u32 = 23;
    const TS_0_06: u32 = 24;
    const TS_0_10: u32 = 26;

    fn tiny() -> Tokenizer {
        Tokenizer::from_json(TINY).expect("the fixture is a valid tokenizer.json")
    }

    /// GPT-2's `bytes_to_unicode`, forwards, written out as upstream does, so
    /// the table in the code under test is checked against an independent copy.
    fn byte_to_char(byte: u8) -> char {
        let printable = |b: u32| {
            (33..=126).contains(&b) || (161..=172).contains(&b) || (174..=255).contains(&b)
        };
        let b = byte as u32;
        if printable(b) {
            return char::from_u32(b).unwrap();
        }
        let n = (0..b).filter(|x| !printable(*x)).count() as u32;
        char::from_u32(256 + n).unwrap()
    }

    // ---- the byte table ----------------------------------------------------

    #[test]
    fn the_byte_table_inverts_gpt2s_bytes_to_unicode_for_all_256_bytes() {
        for byte in 0..=255u8 {
            let c = byte_to_char(byte);
            assert_eq!(
                byte_for_char(c),
                Some(byte),
                "byte {byte:#04x} is written {c:?}"
            );
        }
    }

    #[test]
    fn the_byte_table_has_the_characters_people_know() {
        for (c, byte) in [
            ('!', 0x21),
            ('~', 0x7E),
            ('Ġ', 0x20), // space
            ('Ċ', 0x0A), // newline
            ('ĉ', 0x09), // tab
            ('Ã', 0xC3),
            ('©', 0xA9),
            ('Ĺ', 0x97),
            ('Ń', 0xAD), // the soft hyphen, the one non-printable above 0xA0
        ] {
            assert_eq!(byte_for_char(c), Some(byte), "{c:?}");
        }
    }

    #[test]
    fn characters_outside_the_byte_table_are_not_bytes() {
        // A real character, a code point past the table (U+0100 + 68), and
        // the one Latin-1 byte value the table skips (0xAD itself).
        for c in ['日', '\u{0144}', '\u{00AD}', '\0', ' ', '\n'] {
            assert_eq!(byte_for_char(c), None, "{c:?}");
        }
    }

    // ---- decoding ----------------------------------------------------------

    #[test]
    fn words_decode_with_their_leading_space_kept() {
        let t = tiny();
        assert_eq!(t.decode(&[H, ELLO, SP_WORLD, DOT]).unwrap(), "Hello world.");
        assert_eq!(t.decode(&[SPACE, H]).unwrap(), " H");
        assert_eq!(t.decode(&[H, NEWLINE, ELLO]).unwrap(), "H\nello");
        assert_eq!(t.decode(&[]).unwrap(), "");
    }

    #[test]
    fn a_character_split_across_tokens_is_decoded_whole() {
        let t = tiny();
        // "é" is two tokens, one byte each; "日" is two tokens, 2 + 1 bytes.
        assert_eq!(t.decode(&[SP_CAF, A_TILDE, COPYRIGHT]).unwrap(), " café");
        assert_eq!(t.decode(&[RI_BEN_LEAD, RI_BEN_TAIL]).unwrap(), "日");
        // Half of it on its own is not valid UTF-8: replaced, not an error.
        assert_eq!(t.decode(&[RI_BEN_LEAD]).unwrap(), "\u{FFFD}");
        // And the bytes join across a control token that is dropped between.
        assert_eq!(
            t.decode(&[RI_BEN_LEAD, TS_0_04, RI_BEN_TAIL]).unwrap(),
            "日"
        );
    }

    #[test]
    fn control_and_timestamp_tokens_are_not_text() {
        let t = tiny();
        let prompt_and_reply = [
            SOT,
            EN,
            TRANSCRIBE,
            NO_TIMESTAMPS,
            H,
            ELLO,
            TS_0_00,
            TS_0_04,
            EOT,
        ];
        assert_eq!(t.decode(&prompt_and_reply).unwrap(), "Hello");
        assert_eq!(t.decode(&[SOT, DE, EOT]).unwrap(), "");
    }

    #[test]
    fn an_unknown_id_is_an_error_not_a_gap() {
        let t = tiny();
        assert_eq!(t.vocab_size(), 27);
        assert!(matches!(
            t.decode(&[H, 27]),
            Err(TokenizerError::UnknownId(27))
        ));
        assert!(matches!(
            t.pieces(&[99]),
            Err(TokenizerError::UnknownId(99))
        ));
        assert_eq!(t.kind(27), None);
        assert_eq!(t.token_bytes(27), None);
    }

    // ---- kinds, names and times --------------------------------------------

    #[test]
    fn ids_have_a_kind_and_text_tokens_expose_their_bytes() {
        let t = tiny();
        assert_eq!(t.kind(SP_WORLD), Some(TokenKind::Text));
        assert_eq!(t.kind(EOT), Some(TokenKind::Control));
        assert_eq!(t.kind(NO_TIMESTAMPS), Some(TokenKind::Control));
        assert_eq!(t.kind(TS_0_00), Some(TokenKind::Timestamp));
        assert_eq!(t.kind(TS_0_10), Some(TokenKind::Timestamp));

        assert_eq!(t.token_bytes(SP_WORLD), Some(&b" world"[..]));
        assert_eq!(t.token_bytes(RI_BEN_LEAD), Some(&[0xE6, 0x97][..]));
        // `<|endoftext|>` is in the vocab as well as the added tokens, and is
        // control: no bytes.
        assert_eq!(t.token_bytes(EOT), None);
        assert_eq!(t.token_bytes(TS_0_00), None);
    }

    #[test]
    fn special_tokens_are_found_by_name() {
        let t = tiny();
        assert_eq!(t.special_id("<|endoftext|>"), Some(EOT));
        assert_eq!(t.special_id("<|startoftranscript|>"), Some(SOT));
        assert_eq!(t.special_id("<|notimestamps|>"), Some(NO_TIMESTAMPS));
        assert_eq!(t.special_id("<|nocaptions|>"), Some(19));
        assert_eq!(t.special_id("<|0.06|>"), Some(TS_0_06));
        // A text token is not a special token, and an unknown name is none.
        assert_eq!(t.special_id(" world"), None);
        assert_eq!(t.special_id("<|fr|>"), None);
    }

    #[test]
    fn timestamp_tokens_are_20_ms_steps_from_the_first() {
        let t = tiny();
        assert_eq!(t.timestamp_begin(), Some(TS_0_00));
        for (id, seconds) in [
            (TS_0_00, 0.0),
            (22, 0.02),
            (TS_0_04, 0.04),
            (TS_0_06, 0.06),
            (25, 0.08),
            (TS_0_10, 0.10),
        ] {
            assert_eq!(t.timestamp_seconds(id), Some(seconds), "id {id}");
        }
        assert_eq!(t.timestamp_seconds(H), None);
        assert_eq!(t.timestamp_seconds(EOT), None);
        assert_eq!(t.timestamp_seconds(99), None);
    }

    #[test]
    fn pieces_keep_order_and_separate_text_from_time_and_control() {
        let t = tiny();
        let ids = [
            SOT, EN, TS_0_00, H, ELLO, TS_0_06, TS_0_06, SP_WORLD, DOT, TS_0_10, EOT,
        ];
        assert_eq!(
            t.pieces(&ids).unwrap(),
            vec![
                Piece::Control("<|startoftranscript|>".into()),
                Piece::Control("<|en|>".into()),
                Piece::Timestamp(0.0),
                Piece::Text("Hello".into()),
                Piece::Timestamp(0.06),
                Piece::Timestamp(0.06),
                Piece::Text(" world.".into()),
                Piece::Timestamp(0.10),
                Piece::Control("<|endoftext|>".into()),
            ]
        );
        assert_eq!(t.pieces(&[]).unwrap(), vec![]);
    }

    // ---- loading -----------------------------------------------------------

    /// The fixture with one substring replaced, to make a broken file.
    fn tiny_with(from: &str, to: &str) -> String {
        assert!(
            TINY.contains(from),
            "the fixture no longer contains {from:?}"
        );
        TINY.replacen(from, to, 1)
    }

    #[test]
    fn a_file_that_is_not_json_or_lacks_the_vocab_is_an_error() {
        assert!(matches!(
            Tokenizer::from_json("not json"),
            Err(TokenizerError::Json(_))
        ));
        assert!(matches!(
            Tokenizer::from_json("{}"),
            Err(TokenizerError::Json(_))
        ));
    }

    #[test]
    fn only_a_byte_level_bpe_is_accepted() {
        let wordpiece = tiny_with("\"type\": \"BPE\"", "\"type\": \"WordPiece\"");
        assert!(
            matches!(Tokenizer::from_json(&wordpiece), Err(TokenizerError::Unsupported(m)) if m.contains("WordPiece"))
        );
        let not_bytelevel = tiny_with(
            "\"decoder\": {\"type\": \"ByteLevel\"",
            "\"decoder\": {\"type\": \"Metaspace\"",
        );
        assert!(
            matches!(Tokenizer::from_json(&not_bytelevel), Err(TokenizerError::Unsupported(m)) if m.contains("Metaspace"))
        );
    }

    #[test]
    fn a_vocab_string_that_is_not_byte_level_is_an_error() {
        // "日" written as itself, not as the three characters that stand for
        // its bytes.
        let bad = tiny_with("\"Ġworld\": 2", "\"日\": 2");
        match Tokenizer::from_json(&bad) {
            Err(TokenizerError::NotByteLevel {
                id: 2, ch: '日', ..
            }) => {}
            other => panic!("expected NotByteLevel, got {other:?}"),
        }
    }

    #[test]
    fn a_shared_id_is_an_error() {
        let bad = tiny_with("\"ello\": 1", "\"ello\": 0");
        assert!(matches!(
            Tokenizer::from_json(&bad),
            Err(TokenizerError::Inconsistent(_))
        ));
        let bad = tiny_with("\"id\": 13,", "\"id\": 12,");
        assert!(matches!(
            Tokenizer::from_json(&bad),
            Err(TokenizerError::Inconsistent(_))
        ));
    }

    #[test]
    fn timestamp_names_that_disagree_with_their_ids_are_an_error() {
        // <|0.04|> moved to id 27, past the end, leaving a gap at 23.
        let bad = tiny_with(
            "\"id\": 23, \"content\": \"<|0.04|>\"",
            "\"id\": 27, \"content\": \"<|0.04|>\"",
        );
        match Tokenizer::from_json(&bad) {
            Err(TokenizerError::Inconsistent(m)) => assert!(m.contains("<|0.04|>"), "{m}"),
            other => panic!("expected Inconsistent, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let err =
            Tokenizer::from_file(Path::new("/definitely/not/here/tokenizer.json")).unwrap_err();
        match err {
            TokenizerError::Io { path, .. } => assert!(path.ends_with("tokenizer.json")),
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn from_file_reads_the_same_tokenizer_as_from_json() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tokenizer_tiny.json");
        let t = Tokenizer::from_file(&path).unwrap();
        assert_eq!(t.decode(&[H, ELLO]).unwrap(), "Hello");
    }
}
