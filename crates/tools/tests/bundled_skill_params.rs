//! Bundled skills may only name parameters the tools actually accept.
//!
//! The skill bodies under `apps/desktop/src-tauri/resources/skills` are
//! injected verbatim into the system prompt, and they ship with the app
//! and reach every user's skills folder (installed on first launch, and
//! updated at each launch while the user has not edited the file,
//! `bundled_skills.rs`). So a wrong parameter name in a
//! skill is not a documentation slip — it is an instruction the model
//! follows, producing a call the dispatcher rejects with
//! `SchemaValidation` and a visible stumble mid-workflow.
//!
//! That is what happened with `normalize target_db=-1.0` in three of
//! the eight skills: `normalize`'s parameter is `target_dbfs`, and
//! `target_db` is the *neighbouring* tool `leveler`'s real parameter,
//! which is how it survived review.
//!
//! Nothing connected the two before this test: the skills are markdown
//! resources, the schemas are Rust, and no code read one against the
//! other.
//!
//! ## What is checked
//!
//! Every ``​`tool` param=value`` mention in a skill body:
//!
//! * the backticked name is a registered tool, and
//! * each `param=` that follows it exists in that tool's
//!   `input_schema.properties`.
//!
//! Prose is not the target — `parameters()` only collects `ident=`
//! tokens, which is how an argument is written and not how prose reads.
//! A skill that says "boost 2–4 kHz" names no parameter and is ignored.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;
use tools::dispatcher::ToolDispatcher;

fn skills_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src-tauri/resources/skills")
}

/// Every bundled skill as `(file name, body)`.
fn bundled_skills() -> Vec<(String, String)> {
    let dir = skills_dir();
    let mut out = Vec::new();
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.extension().is_some_and(|e| e == "md") {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            let body = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
            out.push((name, body));
        }
    }
    out.sort();
    out
}

/// The property names a tool's `input_schema` declares.
fn schema_properties(dispatcher: &ToolDispatcher, name: &str) -> Option<BTreeSet<String>> {
    let schemas = dispatcher.tool_schemas();
    let arr = schemas.as_array()?;
    let found = arr
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some(name))?;
    let props = found.get("input_schema")?.get("properties")?.as_object()?;
    Some(props.keys().cloned().collect())
}

/// A `` `tool` `` mention and the parameters written after it.
#[derive(Debug)]
struct Mention {
    line_no: usize,
    tool: String,
    params: Vec<String>,
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Identifiers immediately followed by `=` — how an argument is
/// written. `==`, `!=`, `>=` and `<=` are excluded so a comparison in
/// prose is not mistaken for an assignment.
fn parameters(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '=' {
            i += 1;
            continue;
        }
        // `==`, and the trailing half of `!=` / `>=` / `<=`.
        if chars.get(i + 1) == Some(&'=') || matches!(chars.get(i - 1), Some('!' | '>' | '<' | '='))
        {
            i += 1;
            continue;
        }
        let end = i;
        let mut start = i;
        while start > 0 && is_ident(chars[start - 1]) {
            start -= 1;
        }
        if start < end {
            out.push(chars[start..end].iter().collect());
        }
        i += 1;
    }
    out
}

/// Split a line into backticked and plain spans, and attribute each
/// `param=` to the backticked name that precedes it.
///
/// A backticked span holding a bare identifier opens a new mention —
/// that is how a tool is written. Any other span, backticked or not, is
/// scanned for parameters and credited to the mention already open, so
/// an argument written inside backticks (`` `preset="spotify"` ``) is
/// checked like any other and a line naming two tools does not credit
/// the first one's parameters to the second.
fn mentions(body: &str) -> Vec<Mention> {
    let mut out: Vec<Mention> = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let mut current: Option<Mention> = None;
        let mut rest = line;
        while let Some(open) = rest.find('`') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('`') else { break };
            let inner = &after[..close];
            // Plain text before this backtick belongs to whatever was
            // named before it.
            if let Some(m) = current.as_mut() {
                m.params.extend(parameters(&rest[..open]));
            }
            if !inner.is_empty() && inner.chars().all(is_ident) {
                if let Some(m) = current.take() {
                    out.push(m);
                }
                current = Some(Mention {
                    line_no: idx + 1,
                    tool: inner.to_string(),
                    params: Vec::new(),
                });
            } else if let Some(m) = current.as_mut() {
                m.params.extend(parameters(inner));
            }
            rest = &after[close + 1..];
        }
        if let Some(mut m) = current {
            m.params.extend(parameters(rest));
            out.push(m);
        }
    }
    out
}

#[test]
fn every_parameter_a_bundled_skill_names_exists_in_that_tools_schema() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut problems = Vec::new();
    let mut checked = 0;

    for (file, body) in bundled_skills() {
        for m in mentions(&body) {
            let Some(props) = schema_properties(&dispatcher, &m.tool) else {
                // Not a registered tool — a prose backtick like
                // `achieved_lufs`. Only tool mentions carry parameters,
                // and an unregistered name with parameters is caught by
                // the sibling test below.
                continue;
            };
            for param in &m.params {
                checked += 1;
                if !props.contains(param) {
                    problems.push(format!(
                        "{file}:{} — `{}` has no parameter `{param}` (it accepts: {})",
                        m.line_no,
                        m.tool,
                        props.iter().cloned().collect::<Vec<_>>().join(", "),
                    ));
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "bundled skills instruct the model to pass parameters that do not exist, \
         so the call is rejected by the dispatcher:\n  {}",
        problems.join("\n  ")
    );
    // A parser that silently matched nothing would pass this test
    // forever.
    assert!(
        checked >= 20,
        "only {checked} parameter mentions found across the bundled skills — \
         the parser is probably no longer matching them"
    );
}

#[test]
fn a_bundled_skill_that_passes_parameters_names_a_registered_tool() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let mut problems = Vec::new();

    for (file, body) in bundled_skills() {
        for m in mentions(&body) {
            if m.params.is_empty() {
                continue;
            }
            if schema_properties(&dispatcher, &m.tool).is_none() {
                problems.push(format!(
                    "{file}:{} — `{}` is not a registered tool",
                    m.line_no, m.tool
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "bundled skills tell the model to call tools that do not exist:\n  {}",
        problems.join("\n  ")
    );
}

/// The LUFS skill has to name the LUFS tool.
///
/// `loudness-master` is triggered by `lufs`, `spotify` and `broadcast`,
/// and its whole subject is integrated-loudness targets — yet it only
/// reached for `leveler`, whose own note admits it uses RMS windowing
/// rather than true LUFS. `normalize_loudness` measures EBU R128 and
/// self-describes as the delivery tool, so leaving it unmentioned sent
/// the model to the approximate one for the exact job.
#[test]
fn the_loudness_skill_names_the_loudness_tool() {
    let body = bundled_skills()
        .into_iter()
        .find(|(name, _)| name == "loudness-master.md")
        .expect("loudness-master.md is bundled")
        .1;

    assert!(
        body.contains("normalize_loudness"),
        "loudness-master never mentions `normalize_loudness`, the only tool \
         that measures integrated LUFS"
    );
}

/// An `add_effect` call written in a skill: its line, the `kind=` it
/// names and the keys of its `params={...}` object.
#[derive(Debug)]
struct EffectCall {
    line_no: usize,
    kind: Option<String>,
    keys: Vec<String>,
}

/// Every `` `add_effect` `` call in a body. The call runs to the next
/// backtick, as an argument list does in [`mentions`].
fn add_effect_calls(body: &str) -> Vec<EffectCall> {
    const NAME: &str = "`add_effect`";
    let mut out = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find(NAME) {
            let after = &rest[at + NAME.len()..];
            let call = after.find('`').map_or(after, |end| &after[..end]);
            let kind = call.split_whitespace().find_map(|word| {
                word.strip_prefix("kind=")
                    .map(|k| k.trim_end_matches(|c: char| !is_ident(c)).to_string())
            });
            // The object's keys: each quoted string followed by a colon.
            let keys: Vec<String> = call
                .find("params={")
                .map(|at| {
                    let object = &call[at + "params={".len()..];
                    let object = object.find('}').map_or(object, |end| &object[..end]);
                    let parts: Vec<&str> = object.split('"').collect();
                    (1..parts.len())
                        .step_by(2)
                        .filter(|&i| {
                            parts
                                .get(i + 1)
                                .is_some_and(|next| next.trim_start().starts_with(':'))
                        })
                        .map(|i| parts[i].to_string())
                        .collect()
                })
                .unwrap_or_default();
            if kind.is_some() || !keys.is_empty() {
                out.push(EffectCall {
                    line_no: idx + 1,
                    kind,
                    keys,
                });
            }
            rest = after;
        }
    }
    out
}

/// `add_effect` takes any string as `kind`, so the schema check above
/// cannot see a skill that asks for an effect the renderer cannot run in a
/// chain (`add_effect` refuses it mid-workflow) or names a parameter the
/// kind does not read. Both lists come from the renderer's own registry.
#[test]
fn every_effect_a_bundled_skill_adds_is_one_the_chain_can_run() {
    let mut problems = Vec::new();
    let mut checked = 0;
    for (file, body) in bundled_skills() {
        for call in add_effect_calls(&body) {
            checked += 1;
            let Some(kind) = call.kind.as_deref() else {
                problems.push(format!(
                    "{file}:{} — `add_effect` with params but no kind=",
                    call.line_no
                ));
                continue;
            };
            let Some(params) = audio_engine::effect_chain::params_of(kind) else {
                problems.push(format!(
                    "{file}:{} — kind={kind} cannot run in a chain (it can: {})",
                    call.line_no,
                    audio_engine::effect_chain::streamable_kinds()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                continue;
            };
            for key in &call.keys {
                if !params.iter().any(|p| p.name == key.as_str()) {
                    problems.push(format!(
                        "{file}:{} — kind={kind} has no parameter {key:?}",
                        call.line_no
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "\n  {}", problems.join("\n  "));
    assert!(
        checked >= 10,
        "only {checked} add_effect calls found across the bundled skills — \
         the parser is probably no longer matching them"
    );
}

/// The mastering recipes (#930): one per target, each stating its loudness
/// target and true-peak ceiling with the source it took them from, calling
/// only registered tools, and holding the ceiling the one way that works:
/// the limiter effect added last, then `measure`'s true peak read back,
/// because the limiter is a sample-peak limiter.
const MASTERING_RECIPES: &[&str] = &["genre-master.md", "live-master.md", "lofi-master.md"];

#[test]
fn each_mastering_recipe_states_its_target_and_checks_the_true_peak() {
    let dispatcher = ToolDispatcher::default_dispatcher();
    let skills = bundled_skills();
    let mut problems = Vec::new();
    for recipe in MASTERING_RECIPES {
        let Some((_, body)) = skills.iter().find(|(name, _)| name == recipe) else {
            problems.push(format!("{recipe} is not bundled"));
            continue;
        };
        let calls: Vec<Mention> = mentions(body)
            .into_iter()
            .filter(|m| !m.params.is_empty())
            .collect();
        if calls.len() < 8 {
            problems.push(format!(
                "{recipe}: only {} tool calls found; a recipe is a chain of calls",
                calls.len()
            ));
        }
        for m in &calls {
            if schema_properties(&dispatcher, &m.tool).is_none() {
                problems.push(format!(
                    "{recipe}:{} — `{}` is not a registered tool",
                    m.line_no, m.tool
                ));
            }
        }
        for needle in [
            "LUFS",
            "dBTP",
            "https://",
            "`measure`",
            "`peak.true_dbtp`",
            "kind=limiter",
        ] {
            if !body.contains(needle) {
                problems.push(format!("{recipe}: never mentions {needle:?}"));
            }
        }
    }
    assert!(problems.is_empty(), "\n  {}", problems.join("\n  "));
}

#[cfg(test)]
mod parser_tests {
    use super::{add_effect_calls, mentions, parameters};

    #[test]
    fn collects_assignments_and_ignores_prose() {
        assert_eq!(parameters("cutoff_hz=120 — remove rumble"), ["cutoff_hz"]);
        assert_eq!(parameters("boost 2–4 kHz for presence"), [] as [&str; 0]);
        assert_eq!(
            parameters("threshold_db=-20, ratio=3.0"),
            ["threshold_db", "ratio"]
        );
    }

    #[test]
    fn ignores_comparisons() {
        assert_eq!(parameters("if count == 0 or n != 1"), [] as [&str; 0]);
    }

    #[test]
    fn attributes_parameters_to_the_preceding_tool() {
        let found = mentions("1. **X**: `noise_gate` threshold_db=-50 — silences gaps");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tool, "noise_gate");
        assert_eq!(found[0].params, ["threshold_db"]);
    }

    /// The reason attribution is per-span and not per-line: crediting a
    /// line's parameters to its last tool would blame the wrong schema.
    #[test]
    fn does_not_credit_one_tools_parameters_to_another() {
        let found = mentions("`normalize` target_dbfs=-1 then `limiter` ceiling_db=-0.3");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].tool, "normalize");
        assert_eq!(found[0].params, ["target_dbfs"]);
        assert_eq!(found[1].tool, "limiter");
        assert_eq!(found[1].params, ["ceiling_db"]);
    }

    /// A bare parameter name in prose has no tool to attribute it to,
    /// and must not be attached to whatever was named on a line above.
    #[test]
    fn a_line_with_no_tool_contributes_nothing() {
        assert!(mentions("Adjust threshold_db=-50 based on the noise floor").is_empty());
    }

    /// An argument is often written inside backticks. Skipping those
    /// spans would leave the most argument-shaped text in the file
    /// unchecked.
    #[test]
    fn reads_an_add_effect_calls_kind_and_parameter_keys() {
        let found = add_effect_calls(
            "`add_effect` scope=master kind=gain params={\"db\": <target − measured>} position=0, \
             then the `add_effect` result's `position`",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind.as_deref(), Some("gain"));
        assert_eq!(found[0].keys, ["db"]);
    }

    #[test]
    fn reads_arguments_written_inside_backticks() {
        let found = mentions("`normalize_loudness` — or `preset=\"spotify\"` for a platform");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tool, "normalize_loudness");
        assert_eq!(found[0].params, ["preset"]);
    }
}
