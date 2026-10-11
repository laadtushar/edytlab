//! Hex decoding of node ids and content hashes must reject bad input with
//! an error, never panic (#523).
//!
//! `NodeId::from_hex` used to slice its input by byte pairs after checking
//! only the byte length. A 64-byte string holding a multi-byte character
//! that straddles a pair boundary is not sliceable there, so the slice
//! panicked. `Store::open` reads the `head` file through that function, so
//! a hand-edited or corrupted `head` could crash the app on open. The two
//! serde decoders (`NodeId`, `Clip::content_hash`) had the same slicing.

use std::fs;

use session::{Clip, Error, NodeId, Store, STORE_DIR};
use tempfile::TempDir;

/// 64 bytes, not 64 characters, and not sliceable at byte offset 2: an
/// ASCII byte, 31 two-byte characters, an ASCII byte.
fn straddling_multibyte() -> String {
    let s = format!("a{}a", "é".repeat(31));
    assert_eq!(s.len(), 64);
    assert!(!s.is_char_boundary(2));
    s
}

/// 64 bytes of three-byte characters plus one ASCII byte.
fn three_byte_chars() -> String {
    let s = format!("{}a", "€".repeat(21));
    assert_eq!(s.len(), 64);
    s
}

/// 64 bytes with a four-byte character in the middle of valid hex.
fn emoji_in_hex() -> String {
    let s = format!("{}😀{}", "ab".repeat(15), "cd".repeat(15));
    assert_eq!(s.len(), 64);
    s
}

fn non_ascii_inputs() -> Vec<(&'static str, String)> {
    vec![
        ("two-byte chars straddling a pair", straddling_multibyte()),
        ("three-byte chars", three_byte_chars()),
        ("four-byte char amid hex", emoji_in_hex()),
        ("fullwidth digits", format!("{}0", "０".repeat(21))),
    ]
}

#[test]
fn from_hex_rejects_non_ascii_input_of_length_64_without_panicking() {
    for (what, input) in non_ascii_inputs() {
        assert_eq!(input.len(), 64, "{what}: fixture must be 64 bytes");
        let got = std::panic::catch_unwind(|| NodeId::from_hex(&input));
        let got = got.unwrap_or_else(|_| panic!("{what}: from_hex panicked"));
        assert!(
            matches!(got, Err(Error::HexDecode(_))),
            "{what}: expected HexDecode, got {got:?}"
        );
    }
}

#[test]
fn from_hex_rejects_non_ascii_input_of_any_length() {
    // Shorter and longer inputs already fail the length check; they must
    // keep failing it, and must not reach the slicing.
    for input in ["é", "日本語", &"é".repeat(40)] {
        assert!(matches!(NodeId::from_hex(input), Err(Error::HexDecode(_))));
    }
}

#[test]
fn from_hex_rejects_signs_that_from_str_radix_would_accept() {
    // `u8::from_str_radix("+f", 16)` is Ok(15), so a pair-by-pair parse
    // took a 64-byte string of `+x` pairs for a valid id. Hex digits only.
    let input = "+1".repeat(32);
    assert_eq!(input.len(), 64);
    assert!(
        matches!(NodeId::from_hex(&input), Err(Error::HexDecode(_))),
        "a sign is not a hex digit"
    );
}

#[test]
fn from_hex_still_reads_valid_ids_in_either_case() {
    let id = NodeId(std::array::from_fn(|i| (i * 8 + 3) as u8));
    let lower = id.to_hex();
    assert_eq!(NodeId::from_hex(&lower).unwrap(), id);
    assert_eq!(NodeId::from_hex(&lower.to_uppercase()).unwrap(), id);
    assert_eq!(NodeId::from_hex(&"00".repeat(32)).unwrap(), NodeId([0; 32]));
    assert_eq!(
        NodeId::from_hex(&"ff".repeat(32)).unwrap(),
        NodeId([255; 32])
    );
}

#[test]
fn node_id_deserialize_rejects_non_ascii_input_without_panicking() {
    for (what, input) in non_ascii_inputs() {
        let json = serde_json::Value::String(input).to_string();
        let got = std::panic::catch_unwind(|| serde_json::from_str::<NodeId>(&json));
        let got = got.unwrap_or_else(|_| panic!("{what}: NodeId deserialize panicked"));
        assert!(got.is_err(), "{what}: expected an error");
    }
}

#[test]
fn clip_content_hash_deserialize_rejects_non_ascii_input_without_panicking() {
    for (what, input) in non_ascii_inputs() {
        let json = serde_json::json!({
            "source_path": "media/a.wav",
            "start_in_track": 0,
            "source_offset": 0,
            "length": 10,
            "content_hash": input,
        })
        .to_string();
        let got = std::panic::catch_unwind(|| serde_json::from_str::<Clip>(&json));
        let got = got.unwrap_or_else(|_| panic!("{what}: Clip deserialize panicked"));
        assert!(got.is_err(), "{what}: expected an error");
    }
}

/// A project directory whose `.audiograph/head` holds `contents`.
fn project_with_head(contents: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    let store_dir = dir.path().join(STORE_DIR);
    fs::create_dir_all(&store_dir).unwrap();
    fs::write(store_dir.join("head"), contents).unwrap();
    dir
}

#[test]
fn open_with_a_non_ascii_head_file_is_a_clear_error_not_a_panic() {
    for (what, input) in non_ascii_inputs() {
        let dir = project_with_head(&input);
        let got = std::panic::catch_unwind(|| Store::open(dir.path()));
        let got = got.unwrap_or_else(|_| panic!("{what}: Store::open panicked"));
        match got {
            Err(Error::InvalidHeadHex(shown)) => {
                assert_eq!(shown, input, "{what}: the error shows the bad head");
            }
            Err(other) => panic!("{what}: expected InvalidHeadHex, got {other:?}"),
            Ok(_) => panic!("{what}: a corrupt head must not open as if it were fine"),
        }
        // Opening is read-only for a head it cannot parse: the file is
        // left as it was for the user to inspect.
        let after = fs::read_to_string(dir.path().join(STORE_DIR).join("head")).unwrap();
        assert_eq!(after, input, "{what}: head file must be untouched");
    }
}

#[test]
fn open_with_a_non_ascii_head_file_that_has_trailing_whitespace() {
    // `open` trims the file before parsing, so the 64 bytes can be padded.
    let dir = project_with_head(&format!("{}\n", straddling_multibyte()));
    let got = std::panic::catch_unwind(|| Store::open(dir.path()));
    let got = got.expect("Store::open panicked");
    assert!(matches!(got, Err(Error::InvalidHeadHex(_))));
}

#[test]
fn open_treats_missing_and_empty_head_as_no_head() {
    // The existing fallbacks, pinned so the corrupt-head decision above is
    // a deliberate difference: absent or blank means "nothing recorded yet",
    // but a head that names something unreadable is not "nothing".
    let missing = TempDir::new().unwrap();
    assert_eq!(Store::open(missing.path()).unwrap().head(), None);

    let empty = project_with_head("  \n");
    assert_eq!(Store::open(empty.path()).unwrap().head(), None);
}

#[test]
fn open_with_an_ascii_non_hex_head_file_is_the_same_clear_error() {
    let dir = project_with_head(&"zz".repeat(32));
    assert!(matches!(
        Store::open(dir.path()),
        Err(Error::InvalidHeadHex(_))
    ));
}

#[test]
fn open_still_reads_a_valid_head_file() {
    let id = NodeId([0x5a; 32]);
    let dir = project_with_head(&id.to_hex());
    assert_eq!(Store::open(dir.path()).unwrap().head(), Some(id));
}
