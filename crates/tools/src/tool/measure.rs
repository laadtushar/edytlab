//! `measure` — the numbers a distributor checks, for the mix, a track or a
//! range of either (#763).
//!
//! `analyze_track` answers a musical question (tempo, key, beats) and gives
//! one loudness figure. Podcasters and audiobook narrators need the other
//! kind: the loudness and peaks a platform measures, how loud the room tone
//! is, whether anything clips. ACX rejects a file on exactly these. So this
//! is a tool of its own and not more fields on `analyze_track`: it measures
//! what plays (gain, pan, effects and the master chain included) where
//! `analyze_track` reads the track before them; it takes a range, which
//! `analyze_track` has no use for; and it streams, where `analyze_track`
//! decodes the whole file for a tempo search it does not need here.
//!
//! ## What is measured
//!
//! The head is rendered to a WAV in the OS temp folder, which is removed when
//! the call ends, and the render is read in chunks by
//! [`audio_analysis::qc::QcMeter`] (the definitions, and the references each
//! number is held to, are in that module). With `track` the render is that
//! track alone, soloed: its own gain, pan and effects apply, and so do the
//! sends and the master chain it goes through. Without it, the mix as it
//! plays. A range is in seconds of the session: the render is at the
//! session's rate whatever the rates of the files, so `start_sec` and
//! `end_sec` name the same moments on every track, and the clip frames of
//! #234 never come into it.
//!
//! Loudness is that of the rendered mix at its channel count. A mono session
//! is measured as one channel; a stereo one counts both, so the same voice in
//! a stereo session reads 3 LU louder than in a mono one (BS.1770).
//!
//! ## ACX
//!
//! `check: "acx"` adds a verdict per rule against ACX's published
//! requirements (<https://help.acx.com/s/article/acx-audio-submission-requirements>):
//! volume between -23 and -18 dB RMS, peaks no higher than -3 dB, a noise
//! floor no higher than -60 dB RMS. The sample peak is the peak, as in a
//! DAW's meter. Each limit is inclusive and each value is compared as it is
//! reported, rounded to 0.01 dB, so what the model reads and what passes can
//! never disagree.
//!
//! Read-only: it appends no node and writes nothing in the project. It does
//! not call `rederive::materialize` as `render_preview` does: that rebuilds
//! the audio of an old node a history sweep removed, and a sweep keeps the
//! audio the head names, which is the only node this reads. That is also what
//! lets it run without the store's lock, as `analyze_track` and
//! `plot_spectrum` do.

use std::io::BufReader;
use std::path::Path;

use audio_analysis::qc::{QcMeter, QcOptions, QcReport, DEFAULT_CLIP_TIMES, MAX_CLIP_TIMES};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::schema::anthropic_tool;
use crate::tool::util::{
    attach_render_warnings, check_optional_seconds_order, check_track_index, load_head_state,
};
use crate::{Tool, ToolContext, ToolResult};

/// ACX's requirements page, cited in every ACX verdict.
pub const ACX_SOURCE: &str = "https://help.acx.com/s/article/acx-audio-submission-requirements";

/// ACX: RMS between these, inclusive (dBFS).
const ACX_RMS_MIN: f64 = -23.0;
const ACX_RMS_MAX: f64 = -18.0;
/// ACX: peaks no higher than this (dBFS).
const ACX_PEAK_MAX: f64 = -3.0;
/// ACX: a noise floor no higher than this (dBFS RMS).
const ACX_NOISE_MAX: f64 = -60.0;

/// What the ACX verdict does not look at, said in the result so a pass is not
/// read as more than it is.
const ACX_NOT_CHECKED: &str =
    "file format (192 kbps or higher CBR, 44.1 kHz MP3), one channel format \
for all files, room tone of 0.5 to 5 s at each end, clicks, pops and outtakes";

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    track: Option<usize>,
    #[serde(default)]
    start_sec: Option<f64>,
    #[serde(default)]
    end_sec: Option<f64>,
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    max_clip_times: Option<usize>,
}

pub struct MeasureTool;

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn opt(x: Option<f64>) -> Value {
    x.map_or(Value::Null, |v| json!(r2(v)))
}

impl Tool for MeasureTool {
    fn name(&self) -> &'static str {
        "measure"
    }

    fn schema(&self) -> Value {
        anthropic_tool(
            "measure",
            "Measure the mix, or one track as it plays (gain and effects included), over the whole \
             thing or start_sec to end_sec. Reports integrated, momentary-max and short-term-max \
             loudness (LUFS, EBU R128), loudness range (LU), sample and true peak, RMS, noise floor \
             (quietest 0.5 s), clipping (count and first times) and speech-to-background contrast. \
             check \"acx\" adds pass or fail with the measured value for ACX audiobook rules: RMS \
             -23 to -18 dBFS, peak at most -3, noise floor at most -60. Read-only.",
            json!({
                "type": "object",
                "properties": {
                    "track": { "type": "integer", "description": "Track index; omit for the mix." },
                    "start_sec": { "type": "number", "minimum": 0 },
                    "end_sec": { "type": "number", "minimum": 0 },
                    "check": { "type": "string", "enum": ["acx"] },
                    "max_clip_times": {
                        "type": "integer", "minimum": 0, "maximum": MAX_CLIP_TIMES,
                        "description": format!("Clip times to list. Default {DEFAULT_CLIP_TIMES}.")
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    // Read-only: it renders to a temporary file outside the project, reads
    // it and removes it.
    fn mutates(&self) -> bool {
        false
    }

    // Reads the head and appends nothing, so it is never run again (#421). A
    // render and a pass over it can take as long as the audio is long.
    fn runs_off_the_lock(&self) -> bool {
        true
    }

    fn invoke(&self, args: Value, ctx: &mut ToolContext) -> crate::Result<ToolResult> {
        let args: Args = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::Error(format!("invalid arguments: {e}"))),
        };
        if let Some(check) = args.check.as_deref() {
            if check != "acx" {
                return Ok(ToolResult::Error(format!(
                    "unknown check {check:?}; the only one is \"acx\""
                )));
            }
        }
        if let Err(e) = check_optional_seconds_order(args.start_sec, args.end_sec) {
            return Ok(ToolResult::Error(e));
        }
        let options = QcOptions {
            max_clip_times: args
                .max_clip_times
                .unwrap_or(DEFAULT_CLIP_TIMES)
                .min(MAX_CLIP_TIMES),
        };

        let mut state = match load_head_state(ctx) {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::Error(e)),
        };
        if state.tracks.is_empty() {
            return Ok(ToolResult::Error("the session has no tracks".to_string()));
        }
        if let Some(track) = args.track {
            if let Err(e) = check_track_index(&state.tracks, track) {
                return Ok(ToolResult::Error(e));
            }
            if state.tracks[track].clips.is_empty() {
                return Ok(ToolResult::Error(format!("track {track} has no audio")));
            }
            // That track alone, whatever the user has muted or soloed: they
            // asked about it.
            for (i, t) in state.tracks.iter_mut().enumerate() {
                t.muted = false;
                t.soloed = i == track;
            }
        }

        let scratch = match tempfile::Builder::new()
            .prefix("edytlab-measure-")
            .tempdir()
        {
            Ok(d) => d,
            Err(e) => {
                return Ok(ToolResult::Error(format!(
                    "could not make a temporary folder: {e}"
                )))
            }
        };
        let rendered = scratch.path().join("measured.wav");
        let report = match audio_engine::render_state_to_wav(&state, &rendered, None) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
        };

        let measured = match measure_file(&rendered, args.start_sec, args.end_sec, options) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        let mut out = describe(&measured, args.track, args.check.is_some());
        attach_render_warnings(&mut out, &report.warnings);
        Ok(ToolResult::Ok(out))
    }
}

/// A measurement and the part of the render it covers.
struct Measured {
    report: QcReport,
    /// Seconds of the session where the measured audio starts and ends.
    start_s: f64,
    end_s: f64,
}

/// Measure `[start_sec, end_sec)` of the WAV at `path`, reading it in chunks.
fn measure_file(
    path: &Path,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    options: QcOptions,
) -> Result<Measured, String> {
    let mut reader = hound::WavReader::new(BufReader::new(
        std::fs::File::open(path).map_err(|e| format!("could not read the render: {e}"))?,
    ))
    .map_err(|e| format!("could not read the render: {e}"))?;
    let spec = reader.spec();
    let rate = spec.sample_rate;
    let channels = spec.channels as usize;
    let total = reader.duration() as u64;

    let to_frame = |s: f64| (s * rate as f64).round() as u64;
    let start = start_sec.map_or(0, to_frame).min(total);
    let end = end_sec.map_or(total, to_frame).min(total);
    if start >= end {
        return Err(format!(
            "the range is outside the audio, which is {:.3} s long",
            total as f64 / rate as f64
        ));
    }

    let mut meter = QcMeter::new(rate, spec.channels, options).map_err(|e| e.to_string())?;
    reader
        .seek(start as u32)
        .map_err(|e| format!("could not seek in the render: {e}"))?;

    let want = (end - start) as usize * channels;
    // A second of audio at a time: memory does not grow with the length.
    let chunk = rate as usize * channels;
    let mut buf: Vec<f32> = Vec::with_capacity(chunk);
    let push = |buf: &mut Vec<f32>, meter: &mut QcMeter| -> Result<(), String> {
        meter.push(buf).map_err(|e| e.to_string())?;
        buf.clear();
        Ok(())
    };
    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, bits) if (8..=32).contains(&bits) => {
            let scale = 1.0 / (1u64 << (bits - 1)) as f32;
            for s in reader.samples::<i32>().take(want) {
                buf.push(s.map_err(|e| e.to_string())? as f32 * scale);
                if buf.len() == chunk {
                    push(&mut buf, &mut meter)?;
                }
            }
        }
        (hound::SampleFormat::Float, 32) => {
            for s in reader.samples::<f32>().take(want) {
                buf.push(s.map_err(|e| e.to_string())?);
                if buf.len() == chunk {
                    push(&mut buf, &mut meter)?;
                }
            }
        }
        (format, bits) => {
            return Err(format!(
                "the render is {bits}-bit {format:?} WAV, which the meter does not read"
            ))
        }
    }
    if !buf.is_empty() {
        push(&mut buf, &mut meter)?;
    }
    let report = meter.finish().map_err(|e| e.to_string())?;
    Ok(Measured {
        report,
        start_s: start as f64 / rate as f64,
        end_s: end as f64 / rate as f64,
    })
}

/// The result the model reads. Times are seconds of the session.
fn describe(m: &Measured, track: Option<usize>, acx: bool) -> Value {
    let r = &m.report;
    let mut out = json!({
        "source": if track.is_some() { "track" } else { "mix" },
        "start_sec": r3(m.start_s),
        "end_sec": r3(m.end_s),
        "duration_sec": r3(r.duration_s),
        "sample_rate": r.sample_rate,
        "channels": r.channels,
        "loudness": {
            "integrated_lufs": opt(r.integrated_lufs),
            "momentary_max_lufs": opt(r.momentary_max_lufs),
            "short_term_max_lufs": opt(r.short_term_max_lufs),
            "range_lu": opt(r.loudness_range_lu),
        },
        "peak": {
            "sample_dbfs": r2(r.sample_peak_dbfs),
            "true_dbtp": r2(r.true_peak_dbtp),
        },
        "rms_dbfs": r2(r.rms_dbfs),
        "noise_floor": {
            "dbfs": r2(r.noise_floor_dbfs),
            "at_sec": r3(m.start_s + r.noise_floor_at_s),
        },
        "clipping": {
            "events": r.clipping.events,
            "samples": r.clipping.samples,
            "first_times_sec": r.clipping.first_times_s.iter()
                .map(|t| json!(r3(m.start_s + t))).collect::<Vec<_>>(),
        },
        "contrast": {
            "speech_dbfs": r2(r.contrast.speech_dbfs),
            "background_dbfs": r2(r.contrast.background_dbfs),
            "difference_db": r2(r.contrast.difference_db),
            "wcag_20db_pass": r.contrast.wcag_pass,
        },
    });
    if let Some(track) = track {
        out["track"] = json!(track);
    }

    let mut summary = format!(
        "{} {:.2}-{:.2} s: integrated {} LUFS, true peak {:.1} dBTP, sample peak {:.1} dBFS, RMS {:.1} dBFS, \
         noise floor {:.1} dBFS, {} clip(s).",
        match track {
            Some(t) => format!("Track {t}"),
            None => "Mix".to_string(),
        },
        m.start_s,
        m.end_s,
        r.integrated_lufs.map_or("n/a".to_string(), |v| format!("{v:.1}")),
        r.true_peak_dbtp,
        r.sample_peak_dbfs,
        r.rms_dbfs,
        r.noise_floor_dbfs,
        r.clipping.events,
    );
    if acx {
        let (verdict, text) = acx_verdict(r);
        out["acx"] = verdict;
        summary.push(' ');
        summary.push_str(&text);
    }
    out["summary"] = Value::String(summary);
    out
}

/// ACX's three rules against `r`, and a sentence for the summary.
fn acx_verdict(r: &QcReport) -> (Value, String) {
    // Compared as reported, to 0.01 dB.
    let rms = r2(r.rms_dbfs);
    let peak = r2(r.sample_peak_dbfs);
    let noise = r2(r.noise_floor_dbfs);
    let rms_pass = (ACX_RMS_MIN..=ACX_RMS_MAX).contains(&rms);
    let peak_pass = peak <= ACX_PEAK_MAX;
    let noise_pass = noise <= ACX_NOISE_MAX;

    let mut rms_rule = json!({
        "rule": "rms",
        "requirement": "-23 to -18 dBFS RMS",
        "measured": rms,
        "pass": rms_pass,
    });
    if !rms_pass {
        // The gain that puts the RMS in the middle of the band.
        rms_rule["gain_to_centre_db"] = json!(r2((ACX_RMS_MIN + ACX_RMS_MAX) / 2.0 - rms));
    }
    let pass = rms_pass && peak_pass && noise_pass;
    let verdict = json!({
        "pass": pass,
        "rules": [
            rms_rule,
            {
                "rule": "peak",
                "requirement": "at or below -3 dBFS peak",
                "measured": peak,
                "pass": peak_pass,
            },
            {
                "rule": "noise_floor",
                "requirement": "at or below -60 dBFS RMS noise floor",
                "measured": noise,
                "pass": noise_pass,
            },
        ],
        "source": ACX_SOURCE,
        "not_checked": ACX_NOT_CHECKED,
    });
    let word = |ok: bool| if ok { "pass" } else { "FAIL" };
    let text = format!(
        "ACX {}: RMS {rms:.2} {}, peak {peak:.2} {}, noise floor {noise:.2} {}.",
        if pass { "pass" } else { "FAIL" },
        word(rms_pass),
        word(peak_pass),
        word(noise_pass),
    );
    (verdict, text)
}
