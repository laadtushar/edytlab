//! `docs/tools-reference.md` is generated from the registry, and this
//! test is what keeps it that way.
//!
//! The file used to be written by hand. It documented 27 of 93 tools,
//! called itself "Complete reference for all 28", and got the parameter
//! names wrong in 26 of the 27 it did cover — every entry used
//! `track_id` and seconds, while every real tool takes `track` and
//! sample counts. `docs/contributing.md` declares the file canonical
//! and says a PR adding a tool without updating it is incomplete, so a
//! contributor following the process was being sent to a file that
//! answered "does this tool exist?" wrongly two times in three.
//!
//! Nothing connected it to anything: `website_tool_docs.rs` roots at
//! `../../website` and is website-only by construction, so the guarded,
//! correct copy was the marketing site and the unguarded, wrong one was
//! the file in the repo.
//!
//! ## Regenerating
//!
//! ```text
//! UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc
//! ```
//!
//! Adding a tool changes the file; commit the result. There is nothing
//! to keep in sync by hand, which is the point.

use std::path::PathBuf;

use serde_json::Value;
use tools::ToolDispatcher;

const ENV_UPDATE: &str = "UPDATE_TOOLS_REFERENCE";

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/tools-reference.md")
}

/// The committed file, with line endings normalised.
///
/// Git checks the file out with CRLF on Windows, and the generator
/// emits LF — so a byte comparison told a Windows contributor their
/// perfectly current file was out of date, and regenerating it would
/// not have helped. The document's content is what this test is about;
/// which newline the working tree uses is not.
fn read_doc() -> String {
    let path = doc_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// Every registered tool as `(name, description, input_schema)`,
/// alphabetical.
fn tools() -> Vec<(String, String, Value)> {
    let schemas = ToolDispatcher::default_dispatcher().tool_schemas();
    let mut out: Vec<(String, String, Value)> = schemas
        .as_array()
        .expect("tool_schemas returns an array")
        .iter()
        .map(|s| {
            let name = s["name"].as_str().unwrap_or_default().to_string();
            let desc = s["description"].as_str().unwrap_or_default().to_string();
            (name, desc, s["input_schema"].clone())
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Collapse to one line and escape what a table cell cannot hold.
///
/// Descriptions are prose written for the model, several sentences long
/// and full of backticked names — a raw `|` in one would silently split
/// the row into a wrong column.
fn cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// The first sentence of a description, for the index.
fn first_sentence(text: &str) -> String {
    let flat = cell(text);
    match flat.find(". ") {
        Some(i) => flat[..=i].trim_end().to_string(),
        None => flat,
    }
}

/// A JSON Schema property rendered as a type the reader can act on.
fn type_of(prop: &Value) -> String {
    if let Some(values) = prop.get("enum").and_then(Value::as_array) {
        let names: Vec<String> = values
            .iter()
            .map(|v| match v.as_str() {
                Some(s) => format!("`{s}`"),
                None => format!("`{v}`"),
            })
            .collect();
        return format!("one of {}", names.join(", "));
    }
    let base = match prop.get("type") {
        Some(Value::String(t)) => t.clone(),
        // `["string", "null"]` — an optional written as a union.
        Some(Value::Array(ts)) => ts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => {
            // `$ref`, `oneOf`, `allOf`: a shape too big for a table
            // cell. Say so rather than printing an empty column.
            return "object (see the tool's schema)".to_string();
        }
    };
    if base == "array" {
        if let Some(items) = prop.get("items") {
            return format!("array of {}", type_of(items));
        }
    }
    base
}

/// The generated markdown, in full.
fn render() -> String {
    let tools = tools();
    let mut out = String::new();

    out.push_str("# edytlab — Tools Reference\n\n");
    out.push_str(&format!(
        "> Every one of the {} tools the AI agent can call: parameters, types, \
         and what each one does.\n\n",
        tools.len()
    ));
    out.push_str(
        "<!-- GENERATED FILE — do not edit by hand.\n     \
         Regenerate with:\n       \
         UPDATE_TOOLS_REFERENCE=1 cargo test -p tools --test tools_reference_doc\n     \
         The source of truth is the tool registry itself\n     \
         (`ToolDispatcher::default_dispatcher()`), so this file cannot\n     \
         disagree with what the agent can actually call. -->\n\n",
    );

    out.push_str("## What a tool is\n\n");
    out.push_str(
        "Tools are deterministic functions the agent calls to manipulate the audio \
         session. Each one receives JSON validated against the schema below, reads \
         and writes `SessionState` through the `Store`, and appends a new DAG node \
         when it changes something — so every edit is non-destructive and \
         reversible.\n\n\
         You do not call tools directly. The agent picks them from what you ask for; \
         this page is for knowing what exists and what it takes.\n\n\
         Implementations live in `crates/tools/src/tool/`. A tool that is not \
         registered in `crates/tools/src/dispatcher.rs` is not on this page and the \
         agent cannot call it.\n\n",
    );

    out.push_str("## Index\n\n");
    for (name, desc, _) in &tools {
        out.push_str(&format!(
            "- [`{name}`](#{name}) — {}\n",
            first_sentence(desc)
        ));
    }
    out.push('\n');
    out.push_str("---\n\n");

    for (name, desc, schema) in &tools {
        out.push_str(&format!("## `{name}`\n\n"));
        out.push_str(&format!("{}\n\n", cell(desc)));

        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        match schema.get("properties").and_then(Value::as_object) {
            Some(props) if !props.is_empty() => {
                out.push_str("| Parameter | Type | Required | Notes |\n");
                out.push_str("|---|---|---|---|\n");
                // `properties` is a serde_json Map; iteration order is
                // insertion order for `preserve_order` and sorted
                // otherwise. Sort explicitly so the file is stable
                // whichever it is.
                let mut names: Vec<&String> = props.keys().collect();
                names.sort();
                for prop_name in names {
                    let prop = &props[prop_name];
                    let note = prop
                        .get("description")
                        .and_then(Value::as_str)
                        .map(cell)
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "| `{prop_name}` | {} | {} | {note} |\n",
                        type_of(prop),
                        if required.contains(&prop_name.as_str()) {
                            "yes"
                        } else {
                            "no"
                        },
                    ));
                }
            }
            _ => out.push_str("Takes no parameters.\n"),
        }

        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            out.push_str(
                "\nUnlisted parameters are rejected: the dispatcher validates \
                 against this schema before the tool runs.\n",
            );
        }
        out.push('\n');
    }

    out
}

#[test]
fn the_tools_reference_matches_the_registry() {
    let generated = render();
    let path = doc_path();

    if std::env::var(ENV_UPDATE).is_ok() {
        std::fs::write(&path, &generated)
            .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()));
        return;
    }

    let committed = read_doc();

    if committed == generated {
        return;
    }

    // Point at the first difference rather than dumping two documents.
    let (mut line, mut detail) = (0usize, String::from("the files differ in length"));
    for (i, (a, b)) in committed.lines().zip(generated.lines()).enumerate() {
        if a != b {
            line = i + 1;
            detail = format!("committed: {a}\n  generated: {b}");
            break;
        }
    }

    panic!(
        "docs/tools-reference.md is out of date with the tool registry \
         (first difference at line {line}).\n  {detail}\n\n\
         Regenerate it:\n  \
         {ENV_UPDATE}=1 cargo test -p tools --test tools_reference_doc\n"
    );
}

/// A generator that produced an empty or truncated document would
/// satisfy the comparison above forever, as long as the committed file
/// were equally empty.
#[test]
fn the_generator_actually_produces_a_reference() {
    let tools = tools();
    assert!(
        tools.len() > 50,
        "only {} tools came back from the registry — the schema shape changed",
        tools.len()
    );

    let out = render();
    for (name, _, _) in &tools {
        assert!(
            out.contains(&format!("## `{name}`\n")),
            "`{name}` is registered but has no section in the generated reference"
        );
    }
}

/// The parameter names are the part that was wrong 26 times out of 27,
/// so check them against the schemas directly rather than trusting the
/// renderer to have read the right field.
#[test]
fn every_documented_parameter_is_one_the_tool_accepts() {
    let doc = read_doc();

    for (name, _, schema) in tools() {
        let Some(props) = schema.get("properties").and_then(Value::as_object) else {
            continue;
        };
        let Some(start) = doc.find(&format!("## `{name}`\n")) else {
            panic!("`{name}` has no section in docs/tools-reference.md");
        };
        let section = &doc[start..];
        let section = match section[1..].find("\n## ") {
            Some(end) => &section[..end + 1],
            None => section,
        };

        for row in section.lines().filter(|l| l.starts_with("| `")) {
            let param = row
                .trim_start_matches("| `")
                .split('`')
                .next()
                .unwrap_or_default();
            assert!(
                props.contains_key(param),
                "docs/tools-reference.md gives `{name}` a parameter `{param}` \
                 that its schema does not declare"
            );
        }

        for param in props.keys() {
            assert!(
                section.contains(&format!("| `{param}` |")),
                "`{name}` accepts `{param}`, which the reference does not list"
            );
        }
    }
}
