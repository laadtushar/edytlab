//! `batch_apply` — run one edit chain across a folder (#169).
//!
//! Podcast and interview work arrives as twelve files, not one. Each
//! was a separate session with the edits repeated by hand.
//!
//! A recipe (#162) is an edit chain with no audio in it, so running one
//! across a folder is the natural next step: for each file, a fresh
//! project with its own history, the chain replayed against it, and a
//! line in the report saying what happened.
//!
//! ## Each file gets its own session
//!
//! Not one giant session with twelve tracks. Twelve projects, each with
//! its own `.audiograph/`, because that is what they are: twelve pieces
//! of work that happen to share a chain. Undo on one has nothing to do
//! with the others.
//!
//! ## A failure on file 3 does not abandon files 4–12
//!
//! The whole point of a batch is that it is unattended. Every file is
//! attempted, and the report distinguishes what succeeded from what
//! refused and why — because in a batch of twelve, two will have a
//! different sample rate or a silent channel, and a run that hides
//! those is worse than one that stops.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::{recipe, Tool, ToolContext, ToolDispatcher, ToolResult};

/// Extensions the decoder can actually open. Anything else in the
/// folder is skipped without comment — a batch over a music directory
/// should not report every `.txt` in it as a failure.
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg"];

#[derive(Debug, Deserialize)]
struct Args {
    /// Recipe to run, as written by `export_recipe`.
    recipe_path: String,
    /// Folder of audio to run it over.
    input_dir: String,
    /// Where the per-file projects go. Defaults to `input_dir`.
    #[serde(default)]
    output_dir: Option<String>,
    /// Also render each result to this format beside its project.
    #[serde(default)]
    render_format: Option<String>,
}

pub struct BatchApplyTool;

impl Tool for BatchApplyTool {
    fn name(&self) -> &'static str {
        "batch_apply"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "batch_apply",
            "Run an exported edit chain across every audio file in a folder. Each file becomes \
             its own project with its own history — a batch is not one giant session. Every file \
             is attempted even if an earlier one fails, and the report says what succeeded, what \
             refused, and why. Optionally renders each result.",
            json!({
                "type": "object",
                "properties": {
                    "recipe_path": { "type": "string", "description": "Recipe JSON from export_recipe" },
                    "input_dir": { "type": "string", "description": "Folder of audio to process" },
                    "output_dir": {
                        "type": "string",
                        "description": "Where the per-file projects go. Defaults to input_dir.",
                    },
                    "render_format": {
                        "type": "string",
                        "enum": ["wav", "flac", "mp3"],
                        "description": "Also render each result to this format beside its project.",
                    },
                },
                "required": ["recipe_path", "input_dir"],
                "additionalProperties": false,
            }),
        )
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };

        let text = match std::fs::read_to_string(&args.recipe_path) {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "failed to read recipe {}: {e}",
                    args.recipe_path
                )))
            }
        };
        let template = match recipe::parse(&text) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        // Refused once, up front, rather than twelve times over. A
        // chain that cannot run is a property of the chain, not of the
        // files.
        let blockers = template.blockers();
        if !blockers.is_empty() {
            return Ok(ToolResult::Error(format!(
                "this recipe cannot be replayed, so it cannot be batched: {}",
                blockers.join("; ")
            )));
        }

        let input_dir = PathBuf::from(&args.input_dir);
        let files = match audio_files_in(&input_dir) {
            Ok(f) => f,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if files.is_empty() {
            return Ok(ToolResult::Error(format!(
                "no audio files in {} (looked for {})",
                input_dir.display(),
                AUDIO_EXTENSIONS.join(", ")
            )));
        }

        let output_dir = args
            .output_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| input_dir.clone());

        let dispatcher = ToolDispatcher::default_dispatcher();
        let mut results = Vec::new();
        let (mut done, mut refused) = (0usize, 0usize);

        // Clear any stale cancel before the first file. Doing it here
        // rather than at the cancel site means a stop that arrives just
        // after a run finishes cannot kill the *next* one.
        crate::progress::begin();
        let mut cancelled = false;

        for (index, file) in files.iter().enumerate() {
            // Between files, never inside one. A file is either edited
            // and rendered or not started — stopping midway would leave
            // a project whose history ends in the middle of a chain,
            // which is worse than one more file's wait.
            if crate::progress::cancelled() {
                cancelled = true;
                break;
            }

            crate::progress::report(json!({
                "kind": "batch_apply",
                "index": index,
                "total": files.len(),
                "file": file.display().to_string(),
                "succeeded": done,
                "refused": refused,
            }));

            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("track")
                .to_string();
            let project = output_dir.join(&stem);

            match run_one(
                &dispatcher,
                ctx,
                &template,
                file,
                &project,
                args.render_format.as_deref(),
            ) {
                Ok(entry) => {
                    done += 1;
                    results.push(entry);
                }
                Err(reason) => {
                    // Recorded and carried on. A batch that abandons
                    // files 4–12 because file 3 was odd is not a batch.
                    refused += 1;
                    results.push(json!({
                        "file": file.display().to_string(),
                        "ok": false,
                        "reason": reason,
                    }));
                }
            }
        }

        let attempted = done + refused;
        crate::progress::report(json!({
            "kind": "batch_apply",
            "done": true,
            "cancelled": cancelled,
            "total": files.len(),
            "succeeded": done,
            "refused": refused,
        }));

        Ok(ToolResult::Ok(json!({
            "files": files.len(),
            "attempted": attempted,
            "succeeded": done,
            "refused": refused,
            "cancelled": cancelled,
            "results": results,
            "summary": format!(
                "{} a {}-step chain over {} of {} file{}: {} succeeded, {} refused.{}",
                if cancelled { "Stopped partway through" } else { "Ran" },
                template.steps.len(),
                attempted,
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                done,
                refused,
                if cancelled {
                    " Cancelled between files, so every project either has the whole chain or was \
                     never started."
                } else if refused > 0 {
                    " The refusals are listed with their reasons rather than folded into a count."
                } else {
                    ""
                },
            ),
        })))
    }
}

/// Audio files in `dir`, sorted, so a batch runs in an order a person
/// can predict and a report reads in the same order twice.
fn audio_files_in(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Err(format!("not a folder: {}", dir.display()));
    }
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();
    out.sort();
    Ok(out)
}

/// One file: its own project, its own history, the chain replayed
/// against it.
fn run_one(
    dispatcher: &ToolDispatcher,
    ctx: &mut ToolContext,
    template: &recipe::Recipe,
    file: &Path,
    project: &Path,
    render_format: Option<&str>,
) -> Result<Value, String> {
    let mut chain = template.clone();
    recipe::retarget(&mut chain, &file.to_string_lossy());

    std::fs::create_dir_all(project)
        .map_err(|e| format!("could not make {}: {e}", project.display()))?;
    let mut store = session::Store::open(project)
        .map_err(|e| format!("could not open a session at {}: {e}", project.display()))?;
    let mut clipboard: Option<crate::Clipboard> = None;

    let mut applied = 0usize;
    let mut rendered: Option<String> = None;
    {
        let mut sub = ToolContext {
            store: &mut store,
            engine: &mut *ctx.engine,
            user_message: "",
            clipboard: &mut clipboard,
            // Inherit, never reset: this sub-context is exactly where
            // the restriction used to be lost (#238).
            allowed_tools: ctx.allowed_tools,
        };

        for (i, step) in chain.steps.iter().enumerate() {
            match dispatcher.invoke(&step.tool, step.params.clone(), &mut sub) {
                Ok(ToolResult::Ok(_)) => applied += 1,
                Ok(ToolResult::Error(msg)) => {
                    return Err(format!("step {} (`{}`) failed: {msg}", i + 1, step.tool))
                }
                Err(e) => {
                    return Err(format!(
                        "step {} (`{}`) could not run: {e}",
                        i + 1,
                        step.tool
                    ))
                }
            }
        }

        if let Some(format) = render_format {
            let head = sub
                .store
                .head()
                .ok_or_else(|| "the chain produced no session to render".to_string())?;
            let out = project.with_extension(format);
            match dispatcher.invoke(
                "render_final",
                json!({
                    "node_id": head.to_hex(),
                    "format": format,
                    "out_path": out.to_string_lossy(),
                }),
                &mut sub,
            ) {
                Ok(ToolResult::Ok(_)) => rendered = Some(out.display().to_string()),
                Ok(ToolResult::Error(msg)) => return Err(format!("render failed: {msg}")),
                Err(e) => return Err(format!("render could not run: {e}")),
            }
        }
    }

    Ok(json!({
        "file": file.display().to_string(),
        "ok": true,
        "project": project.display().to_string(),
        "steps_applied": applied,
        "rendered": rendered,
    }))
}
