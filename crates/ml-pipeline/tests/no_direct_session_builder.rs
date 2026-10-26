//! Production ONNX sessions are built through ml-pipeline (#720).
//!
//! This std-only lexer follows the established no_unregistered_open guard.
//! It skips comments/literals and brace-matched test-only items, and catches
//! direct paths and simple import/type aliases. It does not resolve Rust names,
//! expand macros, or follow test-only out-of-line modules; those files are
//! scanned conservatively. Keep detector examples alongside the repository scan.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// =============================================================================
// A small Rust lexer: enough to tell code from comments and literals
// =============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Punct(char),
    /// A string, char or number literal. Its contents are not code.
    Literal,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `source` as tokens. Comments (nested block comments too) are dropped; string
/// literals (raw and byte included), char literals and numbers become
/// `Literal`; lifetimes become nothing.
fn lex(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    let at = |i: usize| chars.get(i).copied();
    while let Some(c) = at(i) {
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && at(i + 1) == Some('/') {
            while at(i).is_some_and(|c| c != '\n') {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == Some('*') {
            let mut depth = 1;
            i += 2;
            while depth > 0 && i < chars.len() {
                if at(i) == Some('/') && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if at(i) == Some('*') && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                } else {
                    if at(i) == Some('\n') {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if let Some(end) = string_end(&chars, i) {
            out.push(Token {
                tok: Tok::Literal,
                line,
            });
            line += chars[i..end].iter().filter(|c| **c == '\n').count();
            i = end;
        } else if c == '\'' {
            // A char literal ('x', '\n', '\u{..}') or a lifetime ('a).
            if at(i + 1) == Some('\\') {
                i += 3; // the tick, the backslash and the escaped character
                while at(i).is_some_and(|c| c != '\'') {
                    i += 1;
                }
                i += 1;
                out.push(Token {
                    tok: Tok::Literal,
                    line,
                });
            } else if at(i + 2) == Some('\'') {
                i += 3;
                out.push(Token {
                    tok: Tok::Literal,
                    line,
                });
            } else {
                i += 1; // the tick of a lifetime; the name lexes as an ident
            }
        } else if is_ident_start(c) {
            let start = i;
            while at(i).is_some_and(is_ident_char) {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Ident(chars[start..i].iter().collect()),
                line,
            });
        } else if c.is_ascii_digit() {
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                // `1..2` and `x.0.1` are not worth telling apart here; only
                // that a number is not an identifier.
                if at(i) == Some('.') && !at(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                    break;
                }
                i += 1;
            }
            out.push(Token {
                tok: Tok::Literal,
                line,
            });
        } else {
            out.push(Token {
                tok: Tok::Punct(c),
                line,
            });
            i += 1;
        }
    }
    out
}

/// If a string literal starts at `i` (`"..."`, `b"..."`, `r"..."`,
/// `r#"..."#`, `br#"..."#`), where it ends.
fn string_end(chars: &[char], i: usize) -> Option<usize> {
    // A `b` or `r` that ends an identifier is not the start of a literal.
    if i > 0 && is_ident_char(chars[i - 1]) {
        return None;
    }
    let mut j = i;
    if chars.get(j) == Some(&'b') {
        j += 1;
    }
    let raw = chars.get(j) == Some(&'r');
    if raw {
        j += 1;
    }
    let mut hashes = 0;
    while raw && chars.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    if chars.get(j) != Some(&'"') {
        return None;
    }
    j += 1;
    while j < chars.len() {
        match chars[j] {
            '\\' if !raw => j += 2,
            '"' => {
                let closing = &chars[j + 1..];
                if closing.len() >= hashes && closing[..hashes].iter().all(|c| *c == '#') {
                    return Some(j + 1 + hashes);
                }
                j += 1;
            }
            _ => j += 1,
        }
    }
    Some(chars.len())
}

// =============================================================================
// Removing test-only items
// =============================================================================

fn is_punct(t: &Token, c: char) -> bool {
    t.tok == Tok::Punct(c)
}

fn is_ident(t: &Token, name: &str) -> bool {
    matches!(&t.tok, Tok::Ident(n) if n == name)
}

/// The index just past the bracket group that opens at `at` (a `(`, `[` or
/// `{`).
fn past_group(tokens: &[Token], at: usize) -> usize {
    let mut depth = 0;
    for (i, t) in tokens.iter().enumerate().skip(at) {
        match t.tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// Whether the attribute whose `[` is at `open` is `cfg(test)` or
/// `cfg(all(test, ..))`: code that exists only in a test build.
/// `cfg(not(test))` and `cfg(any(test, ..))` are not.
fn is_test_only_attr(tokens: &[Token], open: usize) -> bool {
    let end = past_group(tokens, open);
    let inside: Vec<&Token> = tokens[open + 1..end.saturating_sub(1)].iter().collect();
    match inside.as_slice() {
        [cfg, paren, test, close] => {
            is_ident(cfg, "cfg")
                && is_punct(paren, '(')
                && is_ident(test, "test")
                && is_punct(close, ')')
        }
        [cfg, paren1, all, paren2, test, ..] => {
            is_ident(cfg, "cfg")
                && is_punct(paren1, '(')
                && is_ident(all, "all")
                && is_punct(paren2, '(')
                && is_ident(test, "test")
        }
        _ => false,
    }
}

/// Keywords that begin an item which a `,` does not end: a function, a type,
/// a module, a `use`, a constant. What a `,` does end is a field, a variant,
/// an argument, a match arm: things that begin with a name or a visibility.
const ITEM_KEYWORDS: [&str; 15] = [
    "fn",
    "mod",
    "impl",
    "struct",
    "enum",
    "trait",
    "union",
    "type",
    "use",
    "static",
    "const",
    "let",
    "extern",
    "macro_rules",
    "unsafe",
];

/// Whether the item at `at` begins with an item keyword, after any
/// visibility (`pub`, `pub(crate)`) and `async`.
fn begins_with_item_keyword(tokens: &[Token], mut at: usize) -> bool {
    if tokens.get(at).is_some_and(|t| is_ident(t, "pub")) {
        at += 1;
        if tokens.get(at).is_some_and(|t| is_punct(t, '(')) {
            at = past_group(tokens, at);
        }
    }
    while tokens.get(at).is_some_and(|t| is_ident(t, "async")) {
        at += 1;
    }
    tokens
        .get(at)
        .is_some_and(|t| ITEM_KEYWORDS.iter().any(|k| is_ident(t, k)))
}

/// The index just past the item that begins at `at`: through its first
/// top-level `;`, or `,` when it is a field, variant, argument or arm, or
/// through its body when it has one (and a `;` or `,` right after that body,
/// which `const X: S = S { .. };` has). A `,` inside `(..)`, `[..]` or
/// `<..>` does not count, and neither does one in a `where` clause or a
/// generic parameter list of an item that is not comma-terminated.
///
/// `<` and `>` are told from comparisons and arrows only roughly (`->` and
/// `=>` are not closers). A comparison that is never closed makes the item
/// run on to its next `;`, body or the end of the group it is in, which can
/// hide more than the item; that is the one way this errs towards not
/// seeing a raw open, and it takes a `<` in a field or discriminant to do it.
fn past_item(tokens: &[Token], mut at: usize) -> usize {
    // Attributes that follow belong to the same item.
    while at < tokens.len()
        && is_punct(&tokens[at], '#')
        && tokens.get(at + 1).is_some_and(|t| is_punct(t, '['))
    {
        at = past_group(tokens, at + 1);
    }
    let comma_ends = !begins_with_item_keyword(tokens, at);
    let mut angle = 0usize;
    let mut i = at;
    while i < tokens.len() {
        match tokens[i].tok {
            Tok::Punct(';') => return i + 1,
            Tok::Punct(',') if comma_ends && angle == 0 => return i + 1,
            Tok::Punct('<') => {
                angle += 1;
                i += 1;
            }
            Tok::Punct('>') => {
                let arrow =
                    i > 0 && (is_punct(&tokens[i - 1], '-') || is_punct(&tokens[i - 1], '='));
                if !arrow {
                    angle = angle.saturating_sub(1);
                }
                i += 1;
            }
            Tok::Punct('{') => {
                let after = past_group(tokens, i);
                return match tokens.get(after) {
                    Some(t) if is_punct(t, ';') || (comma_ends && is_punct(t, ',')) => after + 1,
                    _ => after,
                };
            }
            Tok::Punct('(' | '[') => i = past_group(tokens, i),
            // The end of the group the item is in: it was the last item.
            Tok::Punct('}' | ')' | ']') => return i,
            _ => i += 1,
        }
    }
    tokens.len()
}

/// `tokens` without the items that exist only in a test build. A file that
/// begins `#![cfg(test)]` is all test code.
fn without_test_items(tokens: &[Token]) -> Vec<Token> {
    if tokens.len() > 3
        && is_punct(&tokens[0], '#')
        && is_punct(&tokens[1], '!')
        && is_punct(&tokens[2], '[')
        && is_test_only_attr(tokens, 2)
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let test_attr = is_punct(&tokens[i], '#')
            && tokens.get(i + 1).is_some_and(|t| is_punct(t, '['))
            && is_test_only_attr(tokens, i + 1);
        if test_attr {
            i = past_item(tokens, past_group(tokens, i + 1));
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}

fn direct_builders(source: &str) -> Vec<usize> {
    let tokens = without_test_items(&lex(source));
    let ident = |i: usize| match tokens.get(i).map(|t| &t.tok) {
        Some(Tok::Ident(name)) => Some(name.as_str()),
        _ => None,
    };
    let punct = |i: usize, c| tokens.get(i).is_some_and(|t| is_punct(t, c));
    let separator = |i: usize| punct(i, ':') && punct(i + 1, ':');
    let mut names = HashSet::from(["Session".to_owned()]);
    for i in 0..tokens.len() {
        if ident(i) == Some("Session") && ident(i + 1) == Some("as") {
            if let Some(alias) = ident(i + 2) {
                names.insert(alias.to_owned());
            }
        }
        if ident(i) == Some("type") && punct(i + 2, '=') {
            let end = (i + 3..tokens.len())
                .find(|&j| punct(j, ';'))
                .unwrap_or(tokens.len());
            if (i + 3..end).any(|j| ident(j) == Some("Session")) {
                if let Some(alias) = ident(i + 1) {
                    names.insert(alias.to_owned());
                }
            }
        }
    }
    let mut lines = Vec::new();
    for i in 0..tokens.len() {
        if ident(i).is_some_and(|name| names.contains(name)) {
            let after = if punct(i + 1, '>') { i + 2 } else { i + 1 };
            let direct = separator(after) && ident(after + 2) == Some("builder");
            let grouped = separator(after)
                && punct(after + 2, '{')
                && (after + 3..past_group(&tokens, after + 2)).any(|j| ident(j) == Some("builder"));
            if direct || grouped {
                lines.push(tokens[i].line);
            }
        }
    }
    lines
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn only_ml_pipeline_builds_production_sessions() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let allowed = repo.join("crates/ml-pipeline/src/onnx_session.rs");
    let mut files = Vec::new();
    for group in ["crates", "apps"] {
        for entry in fs::read_dir(repo.join(group)).unwrap() {
            let member = entry.unwrap().path();
            for root in [member.join("src"), member.join("src-tauri/src")] {
                if root.is_dir() {
                    rust_files(&root, &mut files);
                }
            }
        }
    }
    assert!(files.len() > 50, "scan found too few Rust sources");
    assert!(
        files
            .iter()
            .any(|p| p.ends_with("apps/desktop/src-tauri/src/commands.rs")),
        "desktop backend was not scanned"
    );
    assert!(
        files
            .iter()
            .any(|p| p.ends_with("crates/ml-demucs/src/lib.rs")),
        "Demucs was not scanned"
    );
    let mut offenders = Vec::new();
    for file in files {
        if file == allowed {
            continue;
        }
        let source = fs::read_to_string(&file).unwrap();
        for line in direct_builders(&source) {
            offenders.push(format!(
                "{}:{line}: Session::builder",
                file.strip_prefix(&repo).unwrap().display()
            ));
        }
    }
    assert!(offenders.is_empty(),
        "build ONNX sessions through ml_pipeline::onnx_session::build, which ensures the runtime before ort (#720):\n{}",
        offenders.join("\n"));
}

#[test]
fn guard_finds_direct_qualified_spaced_and_aliased_builders() {
    for source in [
        "fn f() { Session::builder(); }",
        "fn f() { ort::session::Session :: builder (); }",
        "fn f() { <ort::session::Session>::builder(); }",
        "fn f() { let build = Session::builder; }",
        "use ort::session::Session as S; fn f() { S::builder(); }",
        "use ort::session::{Session as S}; fn f() { S::builder(); }",
        "type S = ort::session::Session; fn f() { S::builder(); }",
        "use ort::session::Session::builder;",
        "use ort::session::Session::{builder};",
    ] {
        assert_eq!(direct_builders(source), vec![1], "{source}");
    }
}

#[test]
fn guard_skips_comments_and_literals_but_preserves_line_numbers() {
    let source = r####"
// Session::builder();
/* Session::builder() /* nested Session::builder() */ */
const S: &str = "Session::builder()";
const RAW: &str = r###"Session::builder() \""###;
const BYTES: &[u8] = br#"Session::builder()"#;
const C: char = '"';
fn f<'a>() { Session::builder(); }
"####;
    assert_eq!(direct_builders(source), vec![8]);
    assert!(direct_builders("fn f() { ml_pipeline::onnx_session::build(p, opts); }").is_empty());
}

#[test]
fn guard_skips_only_the_test_item_and_keeps_later_production_code() {
    let source = r#"
#[cfg(test)]
mod tests { fn f() { Session::builder(); } }
#[cfg(all(test, unix))]
fn helper<T, U>() -> (T, U) where T: A, U: B { Session::builder(); }
struct S {
    #[cfg(test)]
    probe: HashMap<String, Vec<u8>>,
    real: u8,
}
#[cfg(test)]
use ort::session::Session as Hidden;
#[cfg_attr(test, derive(Debug))]
fn production() { Session::builder(); }
"#;
    assert_eq!(direct_builders(source), vec![14]);
    assert!(direct_builders("#![cfg(test)]\nfn f() { Session::builder(); }").is_empty());
}

#[test]
fn guard_does_not_exempt_not_test_or_any_test() {
    let source = "#[cfg(not(test))]\nfn a() { Session::builder(); }\n#[cfg(any(test, unix))]\nfn b() { Session::builder(); }";
    assert_eq!(direct_builders(source), vec![2, 4]);
}
