use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::args_and_head;
use crate::{Tool, ToolContext, ToolResult};
use session::annotation::{Annotation, AnnotationKind};

pub(crate) fn format_labels(annotations: &[Annotation]) -> String {
    annotations
        .iter()
        .map(|a| match &a.kind {
            AnnotationKind::Marker { time_sec } => {
                format!("{time_sec}\t{time_sec}\t{}\n", a.name)
            }
            AnnotationKind::Region { start_sec, end_sec } => {
                format!("{start_sec}\t{end_sec}\t{}\n", a.name)
            }
        })
        .collect()
}

/// Where a chapter starts, and its name, for every annotation with a finite
/// time, sorted by start (stable, so two at one time keep the session's
/// order). A region starts at its start. `session::check_marker_time` keeps
/// a time finite and not negative, but a stored state is not re-checked on
/// read, so a time that is not finite is left out and a negative one is
/// read as 0.
fn chapters(annotations: &[Annotation]) -> Vec<(f64, &str)> {
    let mut out: Vec<(f64, &str)> = annotations
        .iter()
        .map(|a| match a.kind {
            AnnotationKind::Marker { time_sec } => (time_sec, a.name.as_str()),
            AnnotationKind::Region { start_sec, .. } => (start_sec, a.name.as_str()),
        })
        .filter(|(t, _)| t.is_finite())
        .map(|(t, name)| (t.max(0.0), name))
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Normal Play Time as Podlove Simple Chapters writes it, `HH:MM:SS.mmm`,
/// to the nearest millisecond; the hours grow past two digits.
fn normal_play_time(sec: f64) -> String {
    let ms = (sec * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Podlove Simple Chapters as JSON: `[{"start": "HH:MM:SS.mmm", "title": ..}]`.
fn podlove_json(chapters: &[(f64, &str)]) -> String {
    let list: Vec<Value> = chapters
        .iter()
        .map(|(t, name)| json!({ "start": normal_play_time(*t), "title": name }))
        .collect();
    let mut text = serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into());
    text.push('\n');
    text
}

/// A CUE sheet holds at most 99 tracks.
const CUE_MAX_TRACKS: usize = 99;
/// CD-Text titles are at most 80 characters.
const CUE_MAX_TITLE_CHARS: usize = 80;

/// A CUE string has no escapes: a double quote becomes a single one and a
/// control character (a tab, a line break) a space, so the value stays on
/// its line inside its quotes.
fn cue_string(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '"' => '\'',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect()
}

/// `mm:ss:ff` at 75 frames a second, to the nearest frame; the minutes are
/// not capped at 99.
fn cue_time(sec: f64) -> String {
    let frames = (sec * 75.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        frames / 4500,
        frames / 75 % 60,
        frames % 75
    )
}

/// The sheet, and how many chapters past track 99 it leaves out.
fn cue_sheet(chapters: &[(f64, &str)], file_name: &str) -> (String, usize) {
    let file = cue_string(file_name);
    let (stem, kind) = match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            let kind = match ext.to_ascii_lowercase().as_str() {
                "mp3" => "MP3",
                "aif" | "aiff" | "aifc" => "AIFF",
                _ => "WAVE",
            };
            (stem, kind)
        }
        _ => (file.as_str(), "WAVE"),
    };
    let disc: String = stem.chars().take(CUE_MAX_TITLE_CHARS).collect();
    let mut text = format!("TITLE \"{disc}\"\nFILE \"{file}\" {kind}\n");
    for (i, (t, name)) in chapters.iter().take(CUE_MAX_TRACKS).enumerate() {
        let title: String = cue_string(name).chars().take(CUE_MAX_TITLE_CHARS).collect();
        text.push_str(&format!(
            "  TRACK {:02} AUDIO\n    TITLE \"{title}\"\n    INDEX 01 {}\n",
            i + 1,
            cue_time(*t)
        ));
    }
    (text, chapters.len().saturating_sub(CUE_MAX_TRACKS))
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Format {
    #[default]
    Audacity,
    PodloveJson,
    Cue,
}

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    format: Format,
    file_name: Option<String>,
}

pub struct ExportLabelsTool;

impl Tool for ExportLabelsTool {
    fn name(&self) -> &'static str {
        "export_labels"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "export_labels",
            "Export markers and regions as Audacity label text, Podlove chapters JSON or a CUE sheet \
             (at most 99 tracks). Chapters are sorted, a region at its start. Writes no file; returns the text.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["audacity", "podlove_json", "cue"], "default": "audacity" },
                    "file_name": { "type": "string", "default": "audio.wav", "description": "cue only: the audio file it names" }
                },
                "required": []
            }),
        )
    }

    // Read-only: it returns the text and writes no file, despite the name.
    fn mutates(&self) -> bool {
        false
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let (args, state) = match args_and_head::<Args>(args, ctx) {
            Ok(v) => v,
            Err(r) => return Ok(r),
        };
        if args.file_name.is_some() && !matches!(args.format, Format::Cue) {
            return Ok(ToolResult::Error(
                "file_name is used only by format \"cue\"; leave it out or set format to \"cue\""
                    .into(),
            ));
        }
        Ok(ToolResult::Ok(match args.format {
            Format::Audacity => {
                let count = state.annotations.len();
                json!({
                    "labels": format_labels(&state.annotations),
                    "count": count,
                    "format": "audacity",
                    "summary": format!("Exported {count} label(s) in Audacity format")
                })
            }
            Format::PodloveJson => {
                let chapters = chapters(&state.annotations);
                let count = chapters.len();
                json!({
                    "labels": podlove_json(&chapters),
                    "count": count,
                    "format": "podlove_json",
                    "summary": format!("Exported {count} chapter(s) as Podlove Simple Chapters JSON")
                })
            }
            Format::Cue => {
                let file_name = args.file_name.as_deref().unwrap_or("audio.wav");
                if cue_string(file_name).trim().is_empty() {
                    return Ok(ToolResult::Error(
                        "file_name is blank; give the audio file the cue sheet points to, such as \"episode.wav\""
                            .into(),
                    ));
                }
                let chapters = chapters(&state.annotations);
                let (text, omitted) = cue_sheet(&chapters, file_name);
                let count = chapters.len() - omitted;
                let mut summary = format!("Exported {count} chapter(s) as a CUE sheet");
                if omitted > 0 {
                    summary.push_str(&format!(
                        "; left out the last {omitted}, as a CUE sheet holds at most {CUE_MAX_TRACKS} tracks"
                    ));
                }
                let mut v = json!({
                    "labels": text,
                    "count": count,
                    "format": "cue",
                    "summary": summary
                });
                if omitted > 0 {
                    v["omitted"] = json!(omitted);
                }
                v
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::format_labels;
    use session::annotation::{Annotation, AnnotationId, AnnotationKind};
    use uuid::Uuid;

    fn make_region(start: f64, end: f64, name: &str) -> Annotation {
        Annotation {
            id: AnnotationId(Uuid::nil()),
            name: name.to_string(),
            kind: AnnotationKind::Region {
                start_sec: start,
                end_sec: end,
            },
        }
    }

    fn make_marker(time: f64, name: &str) -> Annotation {
        Annotation {
            id: AnnotationId(Uuid::nil()),
            name: name.to_string(),
            kind: AnnotationKind::Marker { time_sec: time },
        }
    }

    #[test]
    fn formats_region_correctly() {
        let ann = make_region(1.5, 3.0, "verse");
        let line = format_labels(&[ann]);
        assert!(
            line.contains("1.5") && line.contains("verse"),
            "format: {line:?}"
        );
    }

    #[test]
    fn formats_marker_as_point() {
        let ann = make_marker(2.5, "cue");
        let line = format_labels(&[ann]);
        assert!(
            line.contains("2.5") && line.contains("cue"),
            "marker format: {line:?}"
        );
    }

    #[test]
    fn empty_gives_empty_string() {
        assert_eq!(format_labels(&[]), "");
    }

    #[test]
    fn chapters_leave_out_a_time_that_is_not_finite_and_read_a_negative_one_as_zero() {
        let list = [
            make_marker(f64::NAN, "nan"),
            make_marker(f64::INFINITY, "inf"),
            make_region(f64::NEG_INFINITY, 1.0, "region"),
            make_marker(2.0, "two"),
            make_marker(-3.0, "negative"),
        ];
        assert_eq!(
            super::chapters(&list),
            vec![(0.0, "negative"), (2.0, "two")]
        );
    }

    #[test]
    fn times_round_to_the_nearest_unit() {
        assert_eq!(super::normal_play_time(0.0), "00:00:00.000");
        assert_eq!(super::normal_play_time(360_000.0), "100:00:00.000");
        assert_eq!(super::cue_time(1.0 / 75.0 * 74.4), "00:00:74");
        assert_eq!(super::cue_time(59.999), "01:00:00");
    }
}
