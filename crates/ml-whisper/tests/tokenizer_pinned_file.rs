//! The tokenizer against the real `tokenizer.json` of the pinned export
//! (`onnx-community/whisper-base_timestamped` at
//! `608c49e61301901684bc36cac8f74b95ff6b5a8e`, 2 480 466 bytes, sha256
//! `27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566`, plan
//! on #384).
//!
//! Ignored by default, because CI does not download models; the unit tests in
//! `src/tokenizer.rs` cover the logic on a hand-made file in the same format.
//! Run it with the file at hand:
//!
//! ```text
//! WHISPER_TOKENIZER_JSON=/path/to/tokenizer.json \
//!     cargo test -p ml-whisper --test tokenizer_pinned_file -- --ignored
//! ```
//!
//! A skip shows up as `ignored` with the reason below (#263), not as a pass.
//!
//! The expected texts are Hugging Face `tokenizers` 0.23.3's own output: each
//! sentence was encoded with it (`add_special_tokens=False`) and the ids
//! decoded back with `skip_special_tokens=True`.

use std::path::PathBuf;

use ml_whisper::tokenizer::{Piece, TokenKind, Tokenizer};

fn load() -> Tokenizer {
    let path = std::env::var_os("WHISPER_TOKENIZER_JSON")
        .map(PathBuf::from)
        .expect("WHISPER_TOKENIZER_JSON must point at the pinned tokenizer.json");
    Tokenizer::from_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// (ids from `tokenizers`' encoder, the text `tokenizers` decodes them to).
const SENTENCES: &[(&[u32], &str)] = &[
    (&[2425, 11, 1002, 13], " Hello, world."),
    (
        &[440, 1702, 6292, 21026, 16704, 670, 264, 14847, 3000, 13],
        " The quick brown fox jumps over the lazy dog.",
    ),
    (
        &[
            467, 311, 805, 25, 8465, 280, 13, 76, 7933, 293, 286, 16655, 291, 1848, 4762, 13, 2803,
            0,
        ],
        " It's 3:45 p.m., and I owe you $12.50!",
    ),
    (
        &[46701, 526, 12027, 9732, 11, 1667, 15487, 303, 22188, 13],
        " Café déjà vu, naïve señor.",
    ),
    // "日本語" is three characters in four tokens' worth of bytes; the emoji
    // below is split across two tokens.
    (
        &[220, 27311, 31348, 2972, 22985, 15535, 40498, 4767, 1543],
        " 日本語のテキストです。",
    ),
    (&[38932, 11, 20536, 0], " Привет, мир!"),
    (&[19289, 20732, 222, 31595], " 안녕하세요 😀 emoji"),
    (&[198, 18278, 1622, 197, 474, 4421], "\nNew line\tand tab"),
];

#[test]
#[ignore = "needs the pinned tokenizer.json; set WHISPER_TOKENIZER_JSON and run with --ignored"]
fn the_pinned_file_has_the_ids_the_plan_says() {
    let t = load();
    assert_eq!(t.vocab_size(), 51_865);
    for (name, id) in [
        ("<|endoftext|>", 50_257),
        ("<|startoftranscript|>", 50_258),
        ("<|en|>", 50_259),
        ("<|zh|>", 50_260),
        ("<|su|>", 50_357),
        ("<|translate|>", 50_358),
        ("<|transcribe|>", 50_359),
        ("<|startoflm|>", 50_360),
        ("<|startofprev|>", 50_361),
        ("<|nocaptions|>", 50_362),
        ("<|notimestamps|>", 50_363),
        ("<|0.00|>", 50_364),
        ("<|30.00|>", 51_864),
    ] {
        assert_eq!(t.special_id(name), Some(id), "{name}");
    }
    assert_eq!(t.timestamp_begin(), Some(50_364));
    assert_eq!(t.timestamp_seconds(50_364), Some(0.0));
    assert_eq!(t.timestamp_seconds(50_364 + 50), Some(1.0));
    assert_eq!(t.timestamp_seconds(51_864), Some(30.0));
    assert_eq!(t.kind(50_256), Some(TokenKind::Text));
    assert_eq!(t.kind(50_257), Some(TokenKind::Control));
    assert_eq!(t.kind(51_865), None);
}

#[test]
#[ignore = "needs the pinned tokenizer.json; set WHISPER_TOKENIZER_JSON and run with --ignored"]
fn the_pinned_file_decodes_like_hugging_faces_tokenizers() {
    let t = load();
    for (ids, expected) in SENTENCES {
        assert_eq!(&t.decode(ids).unwrap(), expected, "ids {ids:?}");
    }
}

#[test]
#[ignore = "needs the pinned tokenizer.json; set WHISPER_TOKENIZER_JSON and run with --ignored"]
fn a_prompt_a_reply_and_a_timestamp_decode_to_the_reply() {
    let t = load();
    // <|startoftranscript|> <|en|> <|transcribe|> <|notimestamps|>, " Hello
    // there." and <|1.00|>, <|endoftext|>: what tokenizers decodes with
    // skip_special_tokens=True is " Hello there." plus the timestamp's text,
    // which is not special in the file; ours drops it, as OpenAI's `decode`
    // does.
    let ids = [
        50_258, 50_259, 50_359, 50_363, 2425, 456, 13, 50_414, 50_257,
    ];
    assert_eq!(t.decode(&ids).unwrap(), " Hello there.");
    assert_eq!(
        t.pieces(&ids).unwrap(),
        vec![
            Piece::Control("<|startoftranscript|>".into()),
            Piece::Control("<|en|>".into()),
            Piece::Control("<|transcribe|>".into()),
            Piece::Control("<|notimestamps|>".into()),
            Piece::Text(" Hello there.".into()),
            Piece::Timestamp(1.0),
            Piece::Control("<|endoftext|>".into()),
        ]
    );
}
