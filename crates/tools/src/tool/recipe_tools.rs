//! `export_recipe` and `apply_recipe` — the edit chain as a file (#162).
//!
//! Export walks the chain and writes JSON. Apply reads it, checks every
//! step *before* running any of them, and replays them onto the current
//! session.
//!
//! ## Why apply builds its own dispatcher
//!
//! A tool has no reference to the dispatcher that invoked it, and
//! threading one through `ToolContext` would put a self-reference in
//! every tool's context to serve one caller. Building a fresh default
//! dispatcher here is cheap and keeps the context unchanged — at the
//! cost of one thing that must be handled explicitly: a recipe that
//! contains `apply_recipe` would recurse. That is refused by name in
//! the pre-flight check rather than left to blow the stack.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::recipe;
use crate::schema::anthropic_tool;
use crate::{Tool, ToolContext, ToolResult};

pub struct ExportRecipeTool;

impl Tool for ExportRecipeTool {
    fn name(&self) -> &'static str {
        "export_recipe"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "export_recipe",
            "Export this session's edit chain — every tool and its parameters, in order — to a \
             JSON file with no audio in it. The result can be reviewed by eye and replayed \
             against the same source to reproduce it exactly, or against different audio. Steps \
             that cannot be replayed (ML models) are marked, and the file says so before anyone \
             runs it.",
            json!({
                "type": "object",
                "properties": {
                    "out_path": { "type": "string", "description": "Where to write the recipe JSON" },
                    "name": { "type": "string", "description": "Optional human name for the chain" },
                },
                "required": ["out_path"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            out_path: String,
            #[serde(default)]
            name: Option<String>,
        }
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let Some(head) = ctx.store.head() else {
            return Ok(ToolResult::Error(
                "no session loaded; call `load` first".to_string(),
            ));
        };

        let mut r = match recipe::export(ctx.store, head) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        r.name = args.name;

        let text = match serde_json::to_string_pretty(&r) {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::Error(format!("failed to serialise: {e}"))),
        };
        let path = PathBuf::from(&args.out_path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Ok(ToolResult::Error(format!(
                        "failed to create {}: {e}",
                        parent.display()
                    )));
                }
            }
        }
        if let Err(e) = std::fs::write(&path, &text) {
            return Ok(ToolResult::Error(format!(
                "failed to write {}: {e}",
                path.display()
            )));
        }

        let blockers = r.blockers();
        Ok(ToolResult::Ok(json!({
            "path": path.display().to_string(),
            "steps": r.steps.len(),
            "bytes": text.len(),
            "blockers": blockers,
            "summary": if blockers.is_empty() {
                format!(
                    "Wrote a {}-step recipe ({} bytes, no audio) to {}. It can be replayed as it stands.",
                    r.steps.len(), text.len(), path.display(),
                )
            } else {
                format!(
                    "Wrote a {}-step recipe ({} bytes, no audio) to {}. {} step(s) cannot be \
                     replayed: {}",
                    r.steps.len(), text.len(), path.display(), blockers.len(), blockers.join("; "),
                )
            },
        })))
    }
}

pub struct ApplyRecipeTool;

impl Tool for ApplyRecipeTool {
    fn name(&self) -> &'static str {
        "apply_recipe"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "apply_recipe",
            "Replay an exported edit chain. Every step is checked before any of them runs: if a \
             step cannot be replayed the whole recipe is refused, naming the step, rather than \
             half-applying it. Pass `source` to run the chain against different audio instead of \
             the file it was recorded from. Pass `dry_run` to see what would happen without \
             touching the session.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Recipe JSON to read" },
                    "source": {
                        "type": "string",
                        "description": "Optional audio file to run the chain against instead of the recorded one",
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Report the steps and any blockers without running them",
                    },
                },
                "required": ["path"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            path: String,
            #[serde(default)]
            source: Option<String>,
            #[serde(default)]
            dry_run: bool,
        }
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let text = match std::fs::read_to_string(&args.path) {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "failed to read recipe {}: {e}",
                    args.path
                )))
            }
        };
        let mut r = match recipe::parse(&text) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        let retargeted = match args.source.as_deref() {
            Some(src) => {
                if !std::path::Path::new(src).exists() {
                    return Ok(ToolResult::Error(format!("source file not found: {src}")));
                }
                recipe::retarget(&mut r, src)
            }
            None => 0,
        };

        // Everything is checked before anything runs. A chain that
        // stops halfway leaves a session in a state nobody chose, and
        // "it worked until step 6" is not a result anyone can use.
        let mut blockers = r.blockers();
        if r.steps.iter().any(|s| s.tool == "apply_recipe") {
            blockers.push(
                "a recipe cannot contain `apply_recipe` — replaying it would recurse".to_string(),
            );
        }
        if !blockers.is_empty() {
            return Ok(ToolResult::Error(format!(
                "this recipe cannot be replayed as it stands: {}",
                blockers.join("; ")
            )));
        }

        if args.dry_run {
            return Ok(ToolResult::Ok(json!({
                "dry_run": true,
                "steps": r.steps.len(),
                "retargeted_loads": retargeted,
                "plan": r.steps.iter().map(|s| json!({
                    "tool": s.tool,
                    "params": s.params,
                })).collect::<Vec<_>>(),
                "summary": format!(
                    "{} step(s) would run, in order: {}. Nothing was changed.",
                    r.steps.len(),
                    r.steps.iter().map(|s| s.tool.as_str()).collect::<Vec<_>>().join(" → "),
                ),
            })));
        }

        let dispatcher = crate::ToolDispatcher::default_dispatcher();
        let mut notes = Vec::new();
        for (i, step) in r.steps.iter().enumerate() {
            match dispatcher.invoke(&step.tool, step.params.clone(), ctx) {
                Ok(ToolResult::Ok(v)) => {
                    notes.push(format!(
                        "{}. {} — {}",
                        i + 1,
                        step.tool,
                        v.get("summary").and_then(Value::as_str).unwrap_or("ok"),
                    ));
                }
                // A step that fails stops the replay and says which one.
                // The session keeps whatever the earlier steps produced,
                // and every one of those is an ordinary undoable node.
                Ok(ToolResult::Error(msg)) => {
                    return Ok(ToolResult::Error(format!(
                        "step {} (`{}`) failed: {msg}. {} step(s) before it were applied and can \
                         be undone.",
                        i + 1,
                        step.tool,
                        i
                    )))
                }
                Err(e) => {
                    return Ok(ToolResult::Error(format!(
                        "step {} (`{}`) could not run: {e}. {} step(s) before it were applied.",
                        i + 1,
                        step.tool,
                        i
                    )))
                }
            }
        }

        let head = ctx.store.head().map(|h| h.to_hex());
        Ok(ToolResult::Ok(json!({
            "steps_applied": r.steps.len(),
            "retargeted_loads": retargeted,
            "head": head,
            "notes": notes,
            "summary": format!(
                "Replayed {} step(s){}. New head {}.",
                r.steps.len(),
                if retargeted > 0 {
                    format!(" against {}", args.source.unwrap_or_default())
                } else {
                    String::new()
                },
                head.clone().unwrap_or_else(|| "unknown".into()),
            ),
        })))
    }
}
