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
//! Digital silence (a gap between clips, a clip that starts late, a gated
//! pause: exact zeros) is not room tone and ACX rejects it. The meter leaves
//! it out when it looks for the noise floor, and the ACX check has a rule of
//! its own for it. The RMS still counts it, as ACX's checker does, so when
//! there is silence the RMS rule's `gain_to_centre_db` is worked out from
//! the RMS of the rest (`rms_without_silence_dbfs`): that is what the file
//! reads once room tone fills the gaps, and centring the whole file would
//! have the model turn speech up for silence it is about to replace.
//!
//! ## A range
//!
//! Only the range is rendered, from `RENDER_PRE_ROLL_S` before it so the bus
//! and master chains (which a range render starts cold) have settled when it
//! begins; the pre-roll is read past and never counted. The meter itself
//! starts at the range, as it always has. The user's Cancel is read after
//! the render and between the chunks of the pass over it.
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
for all files, room tone of 0.5 to 1 s at the start and 1 to 5 s at the end, clicks, pops and \
outtakes";

/// Seconds rendered before a range, so the bus and master chains are warm
/// when it starts. The renderer runs the track chains from the start anyway.
const RENDER_PRE_ROLL_S: f64 = 2.0;

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
             (quietest 0.5 s without digital silence), digital silence, clipping (count and first \
             times) and speech-to-background contrast. check \"acx\" adds pass or fail with the \
             measured value for ACX audiobook rules: RMS -23 to -18 dBFS, peak at most -3, noise \
             floor at most -60, no digital silence. Read-only.",
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

        // Clears a Cancel pressed before this call began, so it cannot stop
        // this one.
        crate::progress::begin();

        // Only the range is rendered, from a little before it. The frames are
        // the session's: the render is at its rate.
        let rate = state.sample_rate as f64;
        let to_frame = |s: f64| (s * rate).round() as u64;
        let range_start = args.start_sec.map_or(0, to_frame);
        let render_start = range_start.saturating_sub(to_frame(RENDER_PRE_ROLL_S));
        let range =
            (args.start_sec.is_some() || args.end_sec.is_some()).then(|| audio_engine::TimeRange {
                start_frame: render_start,
                end_frame: args.end_sec.map_or(i64::MAX as u64, to_frame),
            });
        let report = match audio_engine::render_state_to_wav(&state, &rendered, range) {
            Ok(r) => r,
            Err(audio_engine::Error::InvalidRange) => {
                return Ok(ToolResult::Error(format!(
                    "the range is outside the audio, which is {:.3} s long",
                    state.length_samples as f64 / rate
                )))
            }
            Err(e) => return Ok(ToolResult::Error(format!("render failed: {e}"))),
        };
        if crate::progress::cancelled() {
            return Ok(ToolResult::Error(CANCELLED.to_string()));
        }

        let measured = match measure_file(
            &rendered,
            render_start,
            args.start_sec,
            args.end_sec,
            options,
            &|| crate::progress::cancelled(),
        ) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::Error(e)),
        };

        let mut out = describe(&measured, args.track, args.check.is_some());
        attach_render_warnings(&mut out, &report.warnings);
        Ok(ToolResult::Ok(out))
    }
}

const CANCELLED: &str = "Measuring was cancelled by the user. Nothing was changed. Do not run \
it again unless asked.";

/// A measurement and the part of the render it covers.
struct Measured {
    report: QcReport,
    /// Seconds of the session where the measured audio starts and ends.
    start_s: f64,
    end_s: f64,
}

/// Measure `[start_sec, end_sec)` (seconds of the session) of the WAV at
/// `path`, which holds the session from frame `offset` on (the whole render
/// starts at 0), reading it in chunks. `cancel` is asked between chunks.
fn measure_file(
    path: &Path,
    offset: u64,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    options: QcOptions,
    cancel: &dyn Fn() -> bool,
) -> Result<Measured, String> {
    let mut reader = hound::WavReader::new(BufReader::new(
        std::fs::File::open(path).map_err(|e| format!("could not read the render: {e}"))?,
    ))
    .map_err(|e| format!("could not read the render: {e}"))?;
    let spec = reader.spec();
    let rate = spec.sample_rate;
    let channels = spec.channels as usize;
    let total = reader.duration() as u64;

    // In the file's frames: the file starts `offset` frames into the session.
    let to_frame = |s: f64| ((s * rate as f64).round() as u64).saturating_sub(offset);
    let start = start_sec.map_or(0, to_frame).min(total);
    let end = end_sec.map_or(total, to_frame).min(total);
    if start >= end {
        return Err(format!(
            "the range is outside the audio, which is {:.3} s long",
            (total + offset) as f64 / rate as f64
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
        if cancel() {
            return Err(CANCELLED.to_string());
        }
        meter.push(buf).map_err(|e| e.to_string())?;
        buf.clear();
        Ok(())
    };
    match (spec.sample_format, spec.bits_per_sample) {
        // 16 and up: the renderer writes 16-bit, and 8-bit's top code
        // (127/128) is below the meter's clip threshold, so it would never
        // read as clipped.
        (hound::SampleFormat::Int, bits) if (16..=32).contains(&bits) => {
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
        start_s: (start + offset) as f64 / rate as f64,
        end_s: (end + offset) as f64 / rate as f64,
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
        "noise_floor": if r.noise_floor_found {
            json!({
                "dbfs": r2(r.noise_floor_dbfs),
                "at_sec": r3(m.start_s + r.noise_floor_at_s),
            })
        } else {
            // Every 0.5 s holds digital silence: there is no room tone.
            json!({ "dbfs": null, "at_sec": null, "found": false })
        },
        "digital_silence": {
            "seconds": r3(r.digital_silence_s),
            "rms_without_silence_dbfs": opt(r.rms_without_silence_dbfs),
        },
        "clipping": {
            "events": r.clipping.events,
            "samples": r.clipping.samples,
            "first_times_sec": r.clipping.first_times_s.iter()
                .map(|t| json!(r3(m.start_s + t))).collect::<Vec<_>>(),
        },
        "contrast": if r.noise_floor_found {
            json!({
                "speech_dbfs": r2(r.contrast.speech_dbfs),
                "background_dbfs": r2(r.contrast.background_dbfs),
                "difference_db": r2(r.contrast.difference_db),
                "wcag_20db_pass": r.contrast.wcag_pass,
            })
        } else {
            json!({
                "speech_dbfs": r2(r.contrast.speech_dbfs),
                "background_dbfs": null,
                "difference_db": null,
                "wcag_20db_pass": false,
            })
        },
    });
    if let Some(track) = track {
        out["track"] = json!(track);
    }

    let mut summary = format!(
        "{} {:.2}-{:.2} s: integrated {} LUFS, true peak {:.1} dBTP, sample peak {:.1} dBFS, RMS {:.1} dBFS, \
         noise floor {}, {} clip(s).",
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
        if r.noise_floor_found {
            format!("{:.1} dBFS", r.noise_floor_dbfs)
        } else {
            "not found (digital silence throughout)".to_string()
        },
        r.clipping.events,
    );
    if r.digital_silence_s > 0.0 {
        summary.push_str(&format!(
            " {:.2} s of it is digital silence (exact zeros), which is not room tone.",
            r.digital_silence_s
        ));
    }
    if acx {
        let (verdict, text) = acx_verdict(r);
        out["acx"] = verdict;
        summary.push(' ');
        summary.push_str(&text);
    }
    out["summary"] = Value::String(summary);
    out
}

/// ACX's rules (three levels and digital silence) against `r`, and a sentence for the summary.
fn acx_verdict(r: &QcReport) -> (Value, String) {
    // Compared as reported, to 0.01 dB.
    let rms = r2(r.rms_dbfs);
    let peak = r2(r.sample_peak_dbfs);
    let noise = r2(r.noise_floor_dbfs);
    let silence = r3(r.digital_silence_s);
    let rms_pass = (ACX_RMS_MIN..=ACX_RMS_MAX).contains(&rms);
    let peak_pass = peak <= ACX_PEAK_MAX;
    // No room tone to measure is not a pass: -120 dBFS is exact zeros.
    let noise_pass = r.noise_floor_found && noise <= ACX_NOISE_MAX;
    let silence_pass = silence == 0.0;

    let mut rms_rule = json!({
        "rule": "rms",
        "requirement": "-23 to -18 dBFS RMS",
        "measured": rms,
        "pass": rms_pass,
    });
    if !rms_pass {
        // The gain that puts the RMS in the middle of the band. ACX measures
        // the whole file, silence included, but the silence has to be
        // replaced with room tone, and then the file reads about as loud as
        // the rest does: so with silence in it, centre that.
        let (basis, why) = match r.rms_without_silence_dbfs {
            Some(active) if !silence_pass => (
                r2(active),
                Some(
                    "worked out from the RMS without the digital silence, which room tone \
                     will replace; the whole file reads lower only because of it",
                ),
            ),
            _ => (rms, None),
        };
        rms_rule["gain_to_centre_db"] = json!(r2((ACX_RMS_MIN + ACX_RMS_MAX) / 2.0 - basis));
        if let Some(why) = why {
            rms_rule["gain_note"] = json!(why);
            rms_rule["rms_without_silence_dbfs"] = json!(basis);
        }
    }
    let mut noise_rule = json!({
        "rule": "noise_floor",
        "requirement": "at or below -60 dBFS RMS noise floor",
        "measured": if r.noise_floor_found { json!(noise) } else { Value::Null },
        "pass": noise_pass,
    });
    if !r.noise_floor_found {
        noise_rule["note"] = json!(
            "no 0.5 s stretch is free of digital silence, so there is no room tone to measure; \
             add room tone"
        );
    }
    let mut silence_rule = json!({
        "rule": "digital_silence",
        "requirement": "no digital silence (exact zeros): room tone, not silence, in every pause \
                        and at both ends",
        "measured": silence,
        "pass": silence_pass,
    });
    if !silence_pass {
        silence_rule["note"] = json!(
            "ACX rejects digital silence. Fill the gaps with room tone (a quiet stretch of the \
             recording's own, or noise at about -65 dBFS RMS), not silence"
        );
    }
    let pass = rms_pass && peak_pass && noise_pass && silence_pass;
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
            noise_rule,
            silence_rule,
        ],
        "source": ACX_SOURCE,
        "not_checked": ACX_NOT_CHECKED,
    });
    let word = |ok: bool| if ok { "pass" } else { "FAIL" };
    let noise_text = if r.noise_floor_found {
        format!("{noise:.2}")
    } else {
        "not found".to_string()
    };
    let mut text = format!(
        "ACX {}: RMS {rms:.2} {}, peak {peak:.2} {}, noise floor {noise_text} {}, digital silence \
         {silence:.2} s {}.",
        if pass { "pass" } else { "FAIL" },
        word(rms_pass),
        word(peak_pass),
        word(noise_pass),
        word(silence_pass),
    );
    if !silence_pass {
        text.push_str(" Digital silence fails ACX: put room tone there, not silence.");
    }
    (verdict, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pass over the render asks to stop between chunks, and a stop is
    /// the user's answer, not an analysis failure.
    #[test]
    fn a_cancel_stops_the_pass_over_the_render() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..8_000 * 3 {
            w.write_sample(((i % 100) as i16) * 100).unwrap();
        }
        w.finalize().unwrap();

        let calls = std::cell::Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() >= 2
        };
        let e = match measure_file(&path, 0, None, None, QcOptions::default(), &cancel) {
            Ok(_) => panic!("a cancelled pass answered"),
            Err(e) => e,
        };
        assert_eq!(e, CANCELLED);
        assert_eq!(calls.get(), 2, "stops at the first chunk after the request");

        let ok = measure_file(&path, 0, None, None, QcOptions::default(), &|| false).unwrap();
        assert!((ok.end_s - 3.0).abs() < 1e-9);
    }
}
