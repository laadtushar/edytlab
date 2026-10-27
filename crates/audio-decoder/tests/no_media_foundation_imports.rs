//! `os_aac/media_foundation.rs` must not import Media Foundation (#764).
//!
//! Windows N and KN editions ship without `mfplat.dll` and `mfreadwrite.dll`
//! until the Media Feature Pack is installed, and a program whose import table
//! names them does not start at all. The `windows` crate's free functions
//! (`MFStartup`, `MFCreateMediaType`, ...) link their DLL at load time, so one
//! call to any of them puts the whole app out of reach of an N edition. The
//! four that are needed are found with `LoadLibraryExW` and `GetProcAddress`
//! instead, and everything else is a method on a COM interface, which imports
//! nothing.
//!
//! This test runs on every platform: it reads the source and fails on any
//! mention, in code, of a free function of the `Win32::Media::MediaFoundation`
//! module. Free functions there are `MF` followed by a capital and lower case
//! letters, with no underscore (`MFStartup`); constants (`MF_VERSION`,
//! `MFAudioFormat_Float`) and interfaces (`IMFSourceReader`) are not.

use std::path::Path;

/// `src` with comments, string and char literals blanked out (their length and
/// line breaks kept), so a name in a doc comment or a `c"MFStartup"` is not
/// code.
fn code_only(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let blank = |out: &mut String, c: u8| out.push(if c == b'\n' { '\n' } else { ' ' });
    while i < b.len() {
        let c = b[i];
        // Line comment.
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        // Block comment (nested).
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    blank(&mut out, b[i]);
                    i += 1;
                }
            }
            continue;
        }
        // Raw string: r"..", r#".."#, also br and cr.
        let raw_start = {
            let mut j = i;
            if matches!(b[j], b'b' | b'c') {
                j += 1;
            }
            if b.get(j) == Some(&b'r') {
                let mut k = j + 1;
                while b.get(k) == Some(&b'#') {
                    k += 1;
                }
                (b.get(k) == Some(&b'"')).then_some((j, k))
            } else {
                None
            }
        };
        if let Some((r, quote)) = raw_start {
            let hashes = quote - r - 1;
            let mut end = quote + 1;
            'find: while end < b.len() {
                if b[end] == b'"' && (0..hashes).all(|h| b.get(end + 1 + h) == Some(&b'#')) {
                    end += 1 + hashes;
                    break 'find;
                }
                end += 1;
            }
            for &x in &b[i..end.min(b.len())] {
                blank(&mut out, x);
            }
            i = end.min(b.len());
            continue;
        }
        // Ordinary string (also b"" and c"").
        if c == b'"' || (matches!(c, b'b' | b'c') && b.get(i + 1) == Some(&b'"')) {
            if c != b'"' {
                out.push(' ');
                i += 1;
            }
            out.push(' ');
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' && i + 1 < b.len() {
                    out.push(' ');
                    i += 1;
                }
                blank(&mut out, b[i]);
                i += 1;
            }
            out.push(' ');
            i += 1;
            continue;
        }
        // Char literal such as '"' or '\n' (a lifetime has no closing quote
        // two or three bytes on).
        if c == b'\'' {
            let close = if b.get(i + 1) == Some(&b'\\') {
                i + 3
            } else {
                i + 2
            };
            if b.get(close) == Some(&b'\'') {
                for _ in i..=close {
                    out.push(' ');
                }
                i = close + 1;
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

/// The identifiers in `code` that name a free function of the Media Foundation
/// module: `MF` + a capital + at least one lower case letter, letters and
/// digits only.
fn free_function_mentions(code: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (n, line) in code.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &line[start..i];
                let rest = &word[2.min(word.len())..];
                if word.starts_with("MF")
                    && rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && rest.chars().any(|c| c.is_ascii_lowercase())
                    && !word.contains('_')
                {
                    found.push((n + 1, word.to_string()));
                }
            } else {
                i += 1;
            }
        }
    }
    found
}

/// Glob imports of the module and `self` imports hide which names are used.
fn glob_imports(code: &str) -> Vec<usize> {
    code.lines()
        .enumerate()
        .filter(|(_, l)| {
            let l: String = l.split_whitespace().collect();
            l.contains("MediaFoundation::*")
        })
        .map(|(n, _)| n + 1)
        .collect()
}

#[test]
fn media_foundation_rs_calls_no_free_function_of_the_module() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/os_aac/media_foundation.rs");
    let src = std::fs::read_to_string(&path).expect("read media_foundation.rs");
    let code = code_only(&src);
    let calls = free_function_mentions(&code);
    assert!(
        calls.is_empty(),
        "media_foundation.rs names a free function of windows::Win32::Media::MediaFoundation {calls:?}. \
         It would link mfplat.dll or mfreadwrite.dll at load time, and the app would not start on a \
         Windows N edition without the Media Feature Pack. Find it with GetProcAddress like the four \
         in `load_api`, or call a COM interface method."
    );
    assert!(
        glob_imports(&code).is_empty(),
        "media_foundation.rs must name what it imports from MediaFoundation, not glob it"
    );
}

#[test]
fn the_four_functions_it_does_use_are_looked_up_not_linked() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/os_aac/media_foundation.rs");
    let src = std::fs::read_to_string(&path).expect("read media_foundation.rs");
    for name in [
        "MFStartup",
        "MFShutdown",
        "MFCreateMediaType",
        "MFCreateSourceReaderFromURL",
    ] {
        assert!(
            src.contains(&format!("c\"{name}\"")),
            "{name} is expected to be found with GetProcAddress"
        );
    }
}

// The lexer is what the guard stands on, so it is checked on small examples.

#[test]
fn the_guard_sees_a_call_and_ignores_comments_and_strings() {
    let bad = "fn f() { unsafe { MFStartup(1, 0).ok() } }";
    assert_eq!(
        free_function_mentions(&code_only(bad)),
        vec![(1, "MFStartup".to_string())]
    );
    let qualified = "let t = windows::Win32::Media::MediaFoundation::MFCreateMediaType();";
    assert_eq!(free_function_mentions(&code_only(qualified)).len(), 1);

    let fine = r##"
        // MFStartup in a comment
        /* MFShutdown in a /* nested */ block comment */
        /// MFCreateMediaType in a doc comment
        let a = c"MFStartup";
        let b = "MFCreateSourceReaderFromURL \" still a string";
        let c = r#"MFGetAttributeSize "quoted" "#;
        let d = '"';
        let e = MF_VERSION + MFSTARTUP_NOSOCKET;
        let f: IMFSourceReader = x; let g = MFAudioFormat_Float; let h = MfStartup;
    "##;
    assert_eq!(free_function_mentions(&code_only(fine)), vec![]);
}

#[test]
fn the_guard_sees_a_glob_import() {
    let bad = "use windows::Win32::Media::MediaFoundation::*;";
    assert_eq!(glob_imports(&code_only(bad)), vec![1]);
    let ok = "use windows::Win32::Media::MediaFoundation::{IMFSample, MF_VERSION};";
    assert!(glob_imports(&code_only(ok)).is_empty());
}
