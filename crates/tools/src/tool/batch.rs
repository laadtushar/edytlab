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
    /// Where the per-file projects and renders go. Defaults to `input_dir`.
    #[serde(default)]
    output_dir: Option<String>,
    /// Also render each result to this format, named for its project and
    /// beside it (`render_path_for`).
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
             refused, and why. Optionally renders each result. output_dir (default: input_dir, \
             beside the originals) holds the projects and renders; a render that would land on an \
             input file refuses the batch up front, so set output_dir to another folder.",
            json!({
                "type": "object",
                "properties": {
                    "recipe_path": { "type": "string", "description": "Recipe JSON from export_recipe" },
                    "input_dir": { "type": "string", "description": "Folder of audio to process" },
                    "output_dir": {
                        "type": "string",
                        "description": "Where the per-file projects and renders go. Defaults to input_dir, beside the originals.",
                    },
                    "render_format": {
                        "type": "string",
                        "enum": ["wav", "flac", "mp3"],
                        "description": "Also render each result to this format, in output_dir under the input's name (take1.wav to take1.mp3).",
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

        // Refused once, up front, as a chain that cannot run is: a per-file
        // project that is a project somebody has open (the window's, or the
        // one this very turn is working in) would get a second writer and a
        // second history appended behind its back (#566). With the default
        // `output_dir` a folder holding `podcast.wav` next to a project
        // folder named `podcast/` is enough. Nothing is started, so it is
        // not left half done either.
        let open: Vec<PathBuf> = files
            .iter()
            .map(|file| project_for(&output_dir, file))
            .filter(|project| session::Store::is_open(project))
            .collect();
        if !open.is_empty() {
            return Ok(ToolResult::Error(project_is_open(&open)));
        }

        // Refused once, up front, before any file is started: a render that
        // is the same file as an input is written onto the user's own audio
        // (#653). The default `output_dir` is the input folder, so a
        // `render_format` equal to an input's extension is enough, and the
        // clip reads the original while the render truncates it. Compared
        // as files, not as spellings. The render tool refuses the same
        // thing again per file, from what each project's history names.
        if let Some(format) = args.render_format.as_deref() {
            let planned: Vec<PathBuf> = files
                .iter()
                .map(|file| render_path_for(&project_for(&output_dir, file), format))
                .collect();
            let collisions = crate::destination::overlaps(&planned, &files);
            if let Some(&(render, input)) = collisions.first() {
                return Ok(ToolResult::Error(render_is_an_input(
                    &files[render],
                    &planned[render],
                    &files[input],
                    collisions.len(),
                )));
            }
        }

        // Refused once, up front, after the check above so that a render
        // landing on an original is named as that: two inputs whose names differ only in
        // their extension (or only in case) would be one project folder, one
        // history and one render, the second file's written over the
        // first's and both reported as succeeded (#667).
        if let Some((first, second)) = shared_project(&output_dir, &files) {
            return Ok(ToolResult::Error(format!(
                "batch_apply refused before starting: {} and {} would both be edited in the project \
                 folder {}, so one would replace the other. Rename one of them, or move it to a \
                 folder of its own, and run the batch again.",
                first.display(),
                second.display(),
                project_for(&output_dir, &first).display()
            )));
        }

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

            let project = project_for(&output_dir, file);

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

/// Where `file`'s project goes: its stem, in `output_dir`.
fn project_for(output_dir: &Path, file: &Path) -> PathBuf {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
    output_dir.join(stem)
}

/// Where `project`'s render goes when the batch is asked for `format`: beside
/// the project folder, named for it. The format is appended to the whole
/// name, not put in place of its last dotted part: `take.1` renders to
/// `take.1.wav`, where `with_extension` gave `take.wav`, the same file as
/// `take.2`'s render (#667).
fn render_path_for(project: &Path, format: &str) -> PathBuf {
    let mut name = project.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(format);
    project.with_file_name(name)
}

/// The first two inputs that would share a project folder, and so a history
/// and a render: `take.wav` and `take.mp3`, or, on a disk that folds case,
/// `Take.wav` and `take.wav`. Compared without case on every platform, so a
/// batch refused on one disk is refused on all of them.
fn shared_project(output_dir: &Path, files: &[PathBuf]) -> Option<(PathBuf, PathBuf)> {
    let mut seen: std::collections::HashMap<String, &PathBuf> = std::collections::HashMap::new();
    for file in files {
        let key = project_for(output_dir, file)
            .to_string_lossy()
            .to_lowercase();
        if let Some(first) = seen.get(&key) {
            return Some(((*first).clone(), file.clone()));
        }
        seen.insert(key, file);
    }
    None
}

/// The refusal for a batch whose render would be written onto an input. Names
/// the first collision, says what decides where renders go, and what to
/// change, because the model has to choose another `output_dir`.
fn render_is_an_input(file: &Path, render: &Path, input: &Path, collisions: usize) -> String {
    let which = if render == input {
        "one of the input files".to_string()
    } else {
        format!("the same file as the input {}", input.display())
    };
    let more = if collisions > 1 {
        format!(" ({collisions} of the renders collide with inputs in this way.)")
    } else {
        String::new()
    };
    format!(
        "not run, and nothing was started: the render of {} would be written to {}, which is {} \
         and would replace it.{more} A batch renders each file to `<output_dir>/<file name \
         without its extension>.<render_format>`, and `output_dir` defaults to `input_dir`, so a \
         render lands among the originals. Set `output_dir` to another folder, one that holds \
         none of the input files.",
        file.display(),
        render.display(),
        which,
    )
}

/// The refusal for projects that are open. Names each folder and says what to
/// change, because the model has to choose another `output_dir` and the user
/// has to be able to follow why.
fn project_is_open(projects: &[PathBuf]) -> String {
    let named: Vec<String> = projects.iter().map(|p| p.display().to_string()).collect();
    format!(
        "not run: {} {} open right now, and the batch would write its own history into {}. Each \
         file's project goes in `<output_dir>/<file name without its extension>`; choose another \
         `output_dir` so none of them is a project that is open.",
        named.join(", "),
        if named.len() == 1 {
            "is a project that is"
        } else {
            "are projects that are"
        },
        if named.len() == 1 { "it" } else { "them" },
    )
}

/// The refusal for a per-file project whose folder was put there in place of
/// another that is still in use: a project open at that path (this turn's own
/// or the window's) has been moved or replaced, and it still writes through
/// the path. Not the "wait for the turn" the app's own open says: the caller
/// here is the turn, and waiting for itself is no answer.
fn project_was_replaced(project: &Path) -> String {
    format!(
        "not run: {} is a different folder from the project that was open there, and that \
         project is still in use and writes through this path, so the batch would write into it \
         as well. Each file's project goes in `<output_dir>/<file name without its extension>`; \
         choose another `output_dir`.",
        project.display()
    )
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
    // A store of its own, refused if the folder has a live one: it can have
    // become one since the check at the top of the run, and two writers on a
    // project is what this must never be.
    let mut store = session::Store::open_private(project).map_err(|e| match e {
        session::Error::AlreadyOpen { .. } => project_is_open(&[project.to_path_buf()]),
        session::Error::FolderReplaced { .. } => project_was_replaced(project),
        e => format!("could not open a session at {}: {e}", project.display()),
    })?;
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
            let out = render_path_for(project, format);
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

#[cfg(test)]
mod tests {

    #[test]
    fn a_render_keeps_every_dotted_part_of_the_name() {
        let out = Path::new("/out");
        assert_eq!(
            render_path_for(&out.join("take.1"), "wav"),
            out.join("take.1.wav")
        );
        assert_eq!(
            render_path_for(&out.join("take"), "mp3"),
            out.join("take.mp3")
        );
    }

    #[test]
    fn inputs_that_share_a_project_folder_are_found() {
        let out = Path::new("/out");
        let files = |names: &[&str]| {
            names
                .iter()
                .map(|n| PathBuf::from(format!("/in/{n}")))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shared_project(out, &files(&["take.1.wav", "take.2.wav"])),
            None
        );
        assert_eq!(
            shared_project(out, &files(&["take.flac", "take.wav"])),
            Some((
                PathBuf::from("/in/take.flac"),
                PathBuf::from("/in/take.wav")
            ))
        );
        assert!(
            shared_project(out, &files(&["Take.wav", "take.mp3"])).is_some(),
            "case folded"
        );
    }

    use super::*;
    use crate::{Clipboard, ToolContext};
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::TempDir;

    fn write_sine(path: &Path) {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut w = WavWriter::create(path, spec).unwrap();
        for n in 0..8_000 {
            let s = (n as f32 * 0.2).sin() * 0.25;
            w.write_sample((s * 32_767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    /// The up-front refusal is the one callers meet. The render tool's own
    /// refusal is the second line of it: a project whose history names a file
    /// that the render is then asked to write is refused there, and the file
    /// is reported as refused, untouched.
    #[test]
    fn a_render_onto_a_file_the_projects_history_names_is_refused_by_the_render_itself() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("take.wav");
        write_sine(&input);
        let before = std::fs::read(&input).unwrap();
        let recipe = recipe::Recipe {
            format_version: recipe::RECIPE_FORMAT_VERSION,
            engine_version: "0".into(),
            name: None,
            steps: vec![
                recipe::RecipeStep {
                    tool: "load".into(),
                    params: json!({ "path": "placeholder.wav" }),
                    inputs: Value::Null,
                    replayable: true,
                    label: None,
                },
                recipe::RecipeStep {
                    tool: "gain".into(),
                    params: json!({ "track": 0, "db": -6.0 }),
                    inputs: Value::Null,
                    replayable: true,
                    label: None,
                },
            ],
        };
        std::fs::create_dir_all(dir.path().join("host")).unwrap();
        let mut host = session::Store::open(&dir.path().join("host")).unwrap();
        let mut engine = audio_engine::Engine::new();
        let mut clipboard: Option<Clipboard> = None;
        let mut ctx = ToolContext {
            store: &mut host,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };

        // `run_one` renders to `render_path_for(project, format)`. The project
        // beside the input, as the default `output_dir` puts it, renders onto
        // the input: the case the up-front check refuses before `run_one` is
        // reached, here asked of the render's own refusal.
        let project = dir.path().join("take");
        assert_eq!(render_path_for(&project, "wav"), input);

        let outcome = run_one(
            &ToolDispatcher::default_dispatcher(),
            &mut ctx,
            &recipe,
            &input,
            &project,
            Some("wav"),
        );

        let reason = outcome.expect_err("the render has to be refused");
        assert!(reason.contains("render failed"), "{reason}");
        assert!(reason.contains("another"), "{reason}");
        assert_eq!(std::fs::read(&input).unwrap(), before, "the input changed");
    }
}
