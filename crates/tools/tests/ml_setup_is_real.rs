//! The app must not send people to a setup that does not exist (#233).
//!
//! Transcription and stem separation are headline features on the README
//! and the marketing site. Neither works, and every route out of that
//! was a dead end:
//!
//! * Six production strings — two error messages, two tool schemas, a
//!   module doc and the in-app card — told the user to run
//!   `scripts/fetch-models.sh`. That file has never been in this
//!   repository. Two tests pinned the dangling name in place.
//! * The automatic download the docs promised was
//!   `fetched_model_path`, which unconditionally errors and has no
//!   callers.
//! * `EDYTLAB_MODEL_DIR`, documented as the cache override, was read by
//!   no code in the tree.
//! * Supplying a correct model file anyway produced `Ok(vec![])` from
//!   `transcribe` — **success, with an empty transcript**, which reads
//!   as "this recording has no speech".
//!
//! Three of those are prose and would drift back silently. So this file
//! checks the two properties that make the prose true: nothing names a
//! setup step that does not exist, and the stubs fail rather than
//! succeeding emptily.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every authored source file under `dir`, as `(display path, contents)`.
///
/// Walked rather than listed: an instruction on a page nobody thought
/// to name is exactly what this is trying to prevent. Paths are joined
/// with `/` on every platform so the exemptions below match on Windows
/// too — a lesson from #298, where a `\` separator made a guard pass
/// on one CI leg and fail on another.
fn sources(dir: &Path, exts: &[&str]) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, exts: &[&str], out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "node_modules" || name == "target" || name == ".git" || name == ".next" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, exts, out);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
            {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("/");
                    out.push((rel, text));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &repo_root(), exts, &mut out);
    out
}

/// Whole files that are historical records, allowed to describe what
/// was believed at the time. Everything else is a live instruction.
///
/// The development guide used to be listed here, which exempted the
/// *entire* document — so a future reintroduction of
/// `scripts/fetch-models.sh` or `EDYTLAB_MODEL_DIR` anywhere in a live
/// setup guide would have passed. Raised in review on #317. It now
/// marks its retrospective paragraph instead, and the rest of the file
/// is scanned like any other.
fn is_historical(path: &str) -> bool {
    path.contains("superpowers/plans")
        || path.contains("/specs/")
        || path.ends_with("HANDOVER.md")
        || path.ends_with("CHANGELOG.md")
        // This file, which has to quote the strings to forbid them.
        || path.ends_with("ml_setup_is_real.rs")
}

/// Rust source with `//`-style comments (doc comments included)
/// removed, so a guard reads what the code does rather than what the
/// prose says about it.
///
/// Line comments only: every case here is a `//!` header or a `///`
/// doc comment quoting the old behaviour. Block comments are left
/// alone rather than half-handled, which would be worse than not
/// trying.
fn strip_rust_comments(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Regions a live document marks as an account of what it used to say.
///
/// A file explaining its own past mistake has to quote the mistake.
/// Bounding that quotation keeps the rest of the file under the guard,
/// which exempting the whole file did not.
///
/// Unbalanced markers fail closed: an unterminated `begin` would
/// otherwise silence everything after it, so the region runs to the
/// marker or not at all.
fn strip_historical_regions(text: &str) -> String {
    const BEGIN: &str = "<!-- historical:begin -->";
    const END: &str = "<!-- historical:end -->";

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + BEGIN.len()..];
        match after.find(END) {
            Some(stop) => rest = &after[stop + END.len()..],
            // No closing marker: keep the remainder rather than
            // dropping it, so a typo cannot blind the guard.
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Nothing tells anyone to run a script that is not in the repository.
///
/// The check is general rather than a denylist of one name: any
/// `scripts/*.sh` mentioned in live prose or code has to exist. That
/// way adding a real installer passes, and adding a second imaginary
/// one fails.
#[test]
fn no_one_is_told_to_run_a_script_that_does_not_exist() {
    let root = repo_root();
    let mut files = sources(&root.join("crates"), &["rs"]);
    files.extend(sources(&root.join("apps"), &["ts", "tsx", "rs"]));
    files.extend(sources(&root.join("website"), &["ts", "tsx"]));
    files.extend(sources(&root.join("docs"), &["md"]));
    files.push((
        "README.md".to_string(),
        std::fs::read_to_string(root.join("README.md")).unwrap_or_default(),
    ));

    // Assert the walk found something first. A walker that silently
    // reads nothing passes every check below, which is the failure mode
    // that makes a guard worse than none.
    let live: Vec<_> = files
        .into_iter()
        .filter(|(p, _)| !is_historical(p))
        .map(|(p, t)| {
            let stripped = strip_historical_regions(&t);
            (p, stripped)
        })
        .collect();
    assert!(
        live.len() > 100,
        "only walked {} files — the walk is broken and everything below is vacuous",
        live.len()
    );

    let mut dangling = Vec::new();
    for (path, text) in &live {
        for (idx, _) in text.match_indices("scripts/") {
            let rest = &text[idx..];
            let end = rest
                .find(|c: char| {
                    !(c.is_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.')
                })
                .unwrap_or(rest.len());
            let script = &rest[..end];
            if !script.ends_with(".sh") {
                continue;
            }
            // `is_file`, not `exists`: a *directory* called
            // `scripts/foo.sh` would satisfy `exists` while there is
            // still nothing to run. Raised in review on #317.
            if !root.join(script).is_file() {
                dangling.push(format!(
                    "{path} names `{script}`, which is not in the repository"
                ));
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    assert!(
        dangling.is_empty(),
        "these tell someone to run a script that does not exist:\n  {}",
        dangling.join("\n  ")
    );
}

/// `EDYTLAB_MODEL_DIR` is documented only if something reads it.
///
/// It was documented in two places and read nowhere, so a developer
/// who set it got no error and no effect — the worst of the three
/// possible outcomes.
#[test]
fn no_env_var_is_documented_that_nothing_reads() {
    let root = repo_root();
    const VAR: &str = "EDYTLAB_MODEL_DIR";

    // A *read*, not a mention. `t.contains(VAR)` was satisfied by a
    // comment, a schema description or an error string naming the
    // variable, so documenting a dead setting stayed legal as long as
    // something talked about it. Raised in review on #317, and the
    // same class of bug as the one found in the frontend guard (#319).
    let reads_var = |t: &str| {
        [
            format!("var(\"{VAR}\")"),
            format!("var_os(\"{VAR}\")"),
            format!("env!(\"{VAR}\")"),
            format!("option_env!(\"{VAR}\")"),
        ]
        .iter()
        .any(|form| t.contains(form.as_str()))
    };
    let code = sources(&root.join("crates"), &["rs"])
        .into_iter()
        .chain(sources(&root.join("apps"), &["rs", "ts", "tsx"]))
        .any(|(p, t)| !is_historical(&p) && reads_var(&t));

    let documented: Vec<_> = sources(&root.join("docs"), &["md"])
        .into_iter()
        .filter(|(p, t)| !is_historical(p) && strip_historical_regions(t).contains(VAR))
        .map(|(p, _)| p)
        .collect();

    assert!(
        code || documented.is_empty(),
        "{VAR} is documented in {documented:?} but no code reads it. Either wire it up or drop \
         it — a setting that silently does nothing is worse than an undocumented one."
    );
}

/// A stub must fail, not succeed with nothing.
///
/// This is the criterion that is about correctness rather than prose.
/// `Ok(vec![])` from `transcribe` was indistinguishable from "no speech
/// in this audio", so a user who had done everything right was told,
/// in effect, that their recording was silent.
#[test]
fn transcribe_does_not_report_success_with_an_empty_transcript() {
    let err = ml_whisper::WhisperModel::load(Path::new("/nonexistent-whisper-model.onnx"))
        .expect_err("a missing model must not load");
    let msg = err.to_string();
    assert!(
        msg.contains("not implemented in this build"),
        "the missing-model error should say the feature is unavailable rather than imply a \
         model would fix it; got: {msg}"
    );

    let unimplemented = ml_whisper::WhisperError::NotImplemented.to_string();
    assert!(
        unimplemented.contains("not implemented in this build"),
        "NotImplemented must say so plainly; got: {unimplemented}"
    );

    // And the decoder itself must not answer with an empty success.
    //
    // This half used to be the assertion above and nothing more, which
    // had no teeth at all: a regression restoring `Ok(Vec::new())`
    // inside `transcribe` leaves the enum variant and its message
    // exactly as they are, so the test stayed green through the very
    // bug it was named for. Raised in review on #317.
    //
    // CI has no model, so `transcribe` cannot be called here. The
    // check is therefore on the source — crude, and honest about being
    // crude, in the same spirit as the rest of this file. It targets
    // the one regression that matters rather than proving a property.
    // Comments are stripped first. Both files that explain this fix
    // have to *quote* the old return value to explain it, and a guard
    // that cannot tell a warning from the thing it warns about fires on
    // its own documentation — which is exactly the failure this whole
    // test file was written to catch elsewhere.
    let body = strip_rust_comments(
        &std::fs::read_to_string(repo_root().join("crates/ml-whisper/src/lib.rs"))
            .expect("ml-whisper lib.rs is readable"),
    );
    for empty in ["Ok(Vec::new())", "Ok(vec![])"] {
        assert!(
            !body.contains(empty),
            "`ml-whisper` returns `{empty}` again. An empty transcript reported as success is \
             indistinguishable from \"this recording has no speech\", which is the #233 bug: the \
             caller who did everything right gets the most misleading answer available. Return \
             WhisperError::NotImplemented instead."
        );
    }
}

/// The tool schemas the *model* reads must say the feature is
/// unavailable.
///
/// This is the one an agent acts on. While the schema described a
/// working transcriber with a setup step, the agent would confidently
/// tell the user to install a model — repeating the dead end rather
/// than reporting it.
#[test]
fn the_schemas_tell_the_agent_the_features_are_unavailable() {
    let dispatcher = tools::ToolDispatcher::default_dispatcher();
    let schemas = dispatcher.tool_schemas();
    let schemas = schemas.as_array().expect("tool_schemas returns an array");

    for name in ["transcribe", "separate_stems"] {
        let schema = schemas
            .iter()
            .find(|s| s.get("name").and_then(|v| v.as_str()) == Some(name))
            .unwrap_or_else(|| panic!("{name} is not registered"));
        let desc = schema
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_lowercase();
        assert!(
            desc.contains("not implemented"),
            "`{name}`'s schema does not tell the agent the feature is unimplemented, so it will \
             offer it as though it works: {desc}"
        );
        // "Not implemented" alone is not enough. A description reading
        // "not implemented; install the model with ..." passes that
        // check and sends the agent straight back to the dead end, so
        // the two properties the fix actually relies on are asserted
        // separately. Raised in review on #317.
        assert!(
            desc.contains("no setup that changes that"),
            "`{name}`'s schema does not say the state is unfixable, so the agent may still \
             invent a setup step: {desc}"
        );
        assert!(
            desc.contains("do not suggest installing"),
            "`{name}`'s schema does not tell the agent to stop recommending an install: {desc}"
        );
    }
}
