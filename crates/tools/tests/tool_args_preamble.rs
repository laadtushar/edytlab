//! No tool parses its arguments by hand (#934).
//!
//! About ninety tools used to open with the same lines: `serde_json::from_value(args)`
//! matched into a `ToolResult::Error(format!("invalid arguments: {e}"))`, often
//! followed by `load_head_state(ctx)` matched the same way. SonarCloud counted
//! every copy as duplicated code. They now call `util::parse_args`, or
//! `util::args_and_head` when the head is read next, and the text is written
//! once, in `util::parse_args_message`. This test reads the crate's source and
//! fails a file that spells the preamble out again.

use std::path::{Path, PathBuf};

/// The one file that may parse a call's `Value` and write the error text.
const HOME: &str = "src/tool/util.rs";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read the source folder") {
        let path = entry.expect("read a folder entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).expect("read a source file");
            let name = path
                .strip_prefix(root)
                .expect("under the crate")
                .to_string_lossy()
                .replace('\\', "/");
            (name, text)
        })
        .collect()
}

#[test]
fn no_tool_open_codes_the_argument_preamble() {
    let mut offenders = Vec::new();
    for (name, text) in sources() {
        if name == HOME {
            continue;
        }
        for (n, line) in text.lines().enumerate() {
            if line.contains("from_value(args)") || line.contains("\"invalid arguments") {
                offenders.push(format!("{name}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "parse a tool's arguments with `util::parse_args` (or `util::args_and_head` when the \
         head is read next, `util::parse_args_message` in a `run` returning `Result<_, String>`) \
         instead of open-coding it:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_error_text_is_written_once_and_the_helpers_are_used() {
    let sources = sources();
    let home = &sources
        .iter()
        .find(|(name, _)| name == HOME)
        .expect("util.rs is in the crate")
        .1;
    assert_eq!(
        home.matches("\"invalid arguments: {e}\"").count(),
        1,
        "{HOME} writes the invalid-arguments text exactly once"
    );
    // A guard that matched nothing would pass however the tools were written:
    // the helpers have to be what the tools call.
    let callers = sources
        .iter()
        .filter(|(name, text)| {
            name != HOME
                && (text.contains("parse_args(")
                    || text.contains("args_and_head::<")
                    || text.contains("parse_args_message("))
        })
        .count();
    assert!(
        callers >= 90,
        "only {callers} files call the argument helpers"
    );
}
