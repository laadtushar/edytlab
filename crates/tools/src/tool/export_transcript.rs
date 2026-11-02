//! Export captions without changing the transcript, audio, or session history.
//!
//! WebVTT follows `website/lib/srt-to-vtt.ts`: retain cue numbers, prepend
//! `WEBVTT`, use dot milliseconds and LF endings. Direct exports additionally
//! escape cue text so words cannot become WebVTT markup or a timing arrow.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::transcript_cues::{cues, source_info, BoundaryMode, Grouping, TIME_NOTE};
use crate::{Tool, ToolContext, ToolResult};

const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Format {
    #[default]
    Srt,
    Vtt,
    Txt,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Vtt => "vtt",
            Self::Txt => "txt",
        }
    }
}

#[derive(Deserialize)]
struct Args {
    out_path: String,
    #[serde(default)]
    format: Format,
    pause_sec: Option<f64>,
    max_cue_sec: Option<f64>,
    max_chars_per_line: Option<usize>,
}

pub struct ExportTranscriptTool;

fn timestamp(milliseconds: u64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        milliseconds / 3_600_000,
        milliseconds / 60_000 % 60,
        milliseconds / 1000 % 60,
        milliseconds % 1000,
    )
}

fn append(output: &mut String, text: &str) -> Result<(), String> {
    if output
        .len()
        .checked_add(text.len())
        .is_none_or(|size| size > MAX_OUTPUT_BYTES)
    {
        return Err(format!(
            "export exceeds the {MAX_OUTPUT_BYTES} byte output limit"
        ));
    }
    output.push_str(text);
    Ok(())
}

fn append_vtt_text(output: &mut String, text: &str) -> Result<(), String> {
    for character in text.chars() {
        let mut buffer = [0; 4];
        append(
            output,
            match character {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                character => character.encode_utf8(&mut buffer),
            },
        )?;
    }
    Ok(())
}

impl Tool for ExportTranscriptTool {
    fn name(&self) -> &'static str {
        "export_transcript"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "export_transcript",
            "Export the stored transcript as SRT, WebVTT, or plain text. Times are session \
             seconds for a track and follow edits that move, cut or stretch it; align_to_beat \
             leaves them stale. Writes a file, never inside .audiograph; call transcribe first.",
            json!({
                "type": "object",
                "properties": {
                    "out_path": { "type": "string", "description": "Destination, relative to the project directory or absolute" },
                    "format": { "type": "string", "enum": ["srt", "vtt", "txt"], "default": "srt" },
                    "pause_sec": { "type": "number", "minimum": 0, "default": 0.75, "description": "Start a new segment after a pause of at least this many seconds" },
                    "max_cue_sec": { "type": "number", "exclusiveMinimum": 0, "default": 6, "description": "Maximum segment duration, unless one stored word alone exceeds it" },
                    "max_chars_per_line": { "type": "integer", "minimum": 1, "maximum": 200, "default": 42, "description": "Maximum Unicode characters per caption line; segments normally contain at most two lines" }
                },
                "required": ["out_path"],
                "additionalProperties": false
            }),
        )
    }

    // Default mutates=true retains plan approval for an arbitrary file write.
    // Default runs_off_the_lock=false prevents retrying that external write.
    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::Error(format!("invalid arguments: {error}"))),
        };
        let result = export(args, ctx);
        Ok(match result {
            Ok(result) => ToolResult::Ok(result),
            Err(error) => ToolResult::Error(error),
        })
    }
}

/// Where the export goes, refused if it would replace audio the project plays
/// or anything inside its own `.audiograph` folder. Errors name the caller's
/// `out_path` as written, never the project's location on disk.
fn destination(ctx: &ToolContext, out_path: &str) -> Result<PathBuf, String> {
    let raw_path = Path::new(out_path);
    let path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        ctx.store.project_dir().join(raw_path)
    };
    let refused = |what: &str| format!("not written: `{out_path}` {what}. Choose another path.");
    let name = path
        .file_name()
        .ok_or_else(|| format!("out_path `{out_path}` has no file name"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "out_path has no parent directory".to_string())?;
    // A path that does not exist yet is checked by its parent, which must exist.
    let parent = parent.canonicalize().map_err(|error| {
        format!(
            "out_path `{out_path}`: its folder cannot be used: {}",
            error.kind()
        )
    })?;
    let store_dir = ctx.store.project_dir().join(session::STORE_DIR);
    let store_dir = store_dir.canonicalize().unwrap_or_else(|_| {
        ctx.store
            .project_dir()
            .canonicalize()
            .map(|dir| dir.join(session::STORE_DIR))
            .unwrap_or(store_dir)
    });
    if parent.join(name).starts_with(&store_dir) {
        return Err(refused("is inside the project's own .audiograph folder"));
    }
    crate::destination::refuse_project_audio(ctx.store, [&path]).map_err(|_| {
        refused(
            "is audio the project plays, is inside a folder of the project's own audio \
             (`generated/`), or the project's history could not be read to check",
        )
    })?;
    Ok(path)
}

/// `tempfile` creates 0600; an export should be readable like any file the
/// user saves, and an overwritten file keeps the mode it had.
#[cfg(unix)]
fn set_export_mode(file: &std::fs::File, path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .unwrap_or(0o644);
    file.set_permissions(std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_export_mode(_file: &std::fs::File, _path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn export(args: Args, ctx: &ToolContext) -> Result<Value, String> {
    if args.out_path.trim().is_empty() {
        return Err("out_path must not be empty".to_string());
    }
    let head = ctx
        .store
        .head()
        .ok_or_else(|| "no transcript available; call `transcribe` first".to_string())?;
    // Read the stored state as-is. Loading audio or normalizing metadata is not
    // part of a transcript export.
    let node = ctx
        .store
        .get(head)
        .map_err(|error| format!("failed to read transcript: {error}"))?;
    let state = &node.state;
    let transcript = state
        .transcript
        .as_ref()
        .ok_or_else(|| "no transcript available; call `transcribe` first".to_string())?;
    let grouped = cues(
        &transcript.words,
        Grouping::from_options(args.pause_sec, args.max_cue_sec, args.max_chars_per_line)?,
        BoundaryMode::Segment,
    )?;
    let mut output = String::new();
    let separator = match args.format {
        Format::Srt | Format::Txt => ',',
        Format::Vtt => {
            append(&mut output, "WEBVTT\n\n")?;
            '.'
        }
    };
    for (index, cue) in grouped.iter().enumerate() {
        if matches!(args.format, Format::Txt) {
            append(&mut output, &cue.text)?;
            append(&mut output, "\n")?;
            continue;
        }
        append(
            &mut output,
            &format!(
                "{}\n{} --> {}\n",
                index + 1,
                timestamp(cue.start_ms, separator),
                timestamp(cue.end_ms, separator),
            ),
        )?;
        match args.format {
            Format::Srt => append(&mut output, &cue.caption)?,
            Format::Vtt => append_vtt_text(&mut output, &cue.caption)?,
            Format::Txt => unreachable!(),
        }
        append(&mut output, "\n")?;
        if matches!(args.format, Format::Srt) || index + 1 < grouped.len() {
            append(&mut output, "\n")?;
        }
    }
    let path = destination(ctx, &args.out_path)?;
    let parent = path
        .parent()
        .ok_or_else(|| "out_path has no parent directory".to_string())?;
    // The io errors of `tempfile` name the full path, which must not reach the model.
    let mut temporary = tempfile::Builder::new()
        .prefix(".transcript-")
        .tempfile_in(parent)
        .map_err(|error| {
            format!(
                "failed to create transcript export for `{}`: {}",
                args.out_path,
                error.kind()
            )
        })?;
    temporary
        .write_all(output.as_bytes())
        .and_then(|()| temporary.flush())
        .map_err(|error| {
            format!(
                "failed to write transcript export for `{}`: {}",
                args.out_path,
                error.kind()
            )
        })?;
    set_export_mode(temporary.as_file(), &path).map_err(|error| {
        format!(
            "failed to set permissions for `{}`: {}",
            args.out_path,
            error.kind()
        )
    })?;
    temporary.persist(&path).map_err(|error| {
        format!(
            "failed to publish transcript export to `{}`: {}",
            args.out_path,
            error.error.kind()
        )
    })?;

    let source = source_info(state, transcript);
    let time_basis = source["time_basis"].clone();
    Ok(json!({
        "out_path": args.out_path,
        "format": args.format.name(),
        "word_count": transcript.words.len(),
        "cue_count": grouped.len(),
        "byte_count": output.len(),
        "time_basis": time_basis,
        "time_note": TIME_NOTE,
        "source": source,
        "summary": format!("Exported {} transcript cue(s) as {}", grouped.len(), args.format.name()),
    }))
}
