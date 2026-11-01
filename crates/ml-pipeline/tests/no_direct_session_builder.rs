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

#[path = "../../../test-support/rust_source_scan.rs"]
mod rust_source_scan;
use rust_source_scan::{is_punct, lex, past_group, without_test_items, Tok};

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
