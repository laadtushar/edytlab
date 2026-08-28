//! AI-driven E2E smoke for the Phase 1 podcast-cleanup demo (M16).
//!
//! Spawns the `edytlab-cli` binary (built from this same crate),
//! prompts the agent to remove the head silence and normalize to -1
//! dBFS, then asserts the rendered output matches the same duration /
//! peak constraints as `deterministic_pipeline.rs`.
//!
//! Marked `#[ignore]` because it costs an Anthropic API call and ~10 s
//! of network latency. The deterministic test is the always-on quality
//! gate; this is the human-in-the-loop pre-release smoke. Run with:
//!
//! ```bash
//! ANTHROPIC_API_KEY=sk-ant-... cargo test -p edytlab-cli --test podcast_cleanup -- --ignored --nocapture
//! ```
//!
//! Without `ANTHROPIC_API_KEY` set, the test prints a notice and exits
//! 0 (success-as-skip) so unattended `--ignored` runs on a fresh box
//! don't fail mysteriously.

use std::path::{Path, PathBuf};

use assert_cmd::Command;

const SAMPLE_RATE: u32 = 44_100;
const TOTAL_SECONDS: u32 = 15;
const SILENCE_SECONDS: u32 = 2;
const SINE_HZ: f32 = 440.0;
const SINE_AMPLITUDE: f32 = 0.501_187_2; // -6 dBFS linear

fn write_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("raw_podcast_intro.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("create wav");
    let total_frames = SAMPLE_RATE * TOTAL_SECONDS;
    let silence_frames = SAMPLE_RATE * SILENCE_SECONDS;
    for i in 0..total_frames {
        let s = if i < silence_frames {
            0.0
        } else {
            let t = (i - silence_frames) as f32 / SAMPLE_RATE as f32;
            (t * SINE_HZ * std::f32::consts::TAU).sin() * SINE_AMPLITUDE
        };
        let q = (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32);
        writer.write_sample(q as i16).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
    path
}

#[test]
#[ignore = "live Anthropic API; requires ANTHROPIC_API_KEY"]
fn ai_driven_podcast_cleanup() {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!(
                "ANTHROPIC_API_KEY not set; skipping AI-driven E2E. \
                 The deterministic_pipeline test is the always-on gate."
            );
            return;
        }
    };

    let project = tempfile::tempdir().expect("tempdir");
    let input_path = write_fixture(project.path());

    // Prime the session by `load`-ing the fixture. The CLI runs ONE
    // turn, so we do the load up-front via a separate invocation; the
    // model only needs to pick `cut_range` and `normalize` (and any
    // render the user asks for, though here we drive the final render
    // ourselves so the assertions don't depend on the model picking the
    // right `out_path`).
    //
    // Two-pass invocation: first turn = load, second turn = the user
    // prompt. The CLI prints JSON-lines on stdout; we don't parse them
    // here beyond a "process exited 0" check.
    let mut load_cmd = Command::cargo_bin("edytlab-cli").expect("locate cli binary");
    load_cmd
        .arg("--project-dir")
        .arg(project.path())
        .arg("--api-key")
        .arg(&api_key)
        .arg("--message")
        .arg(format!(
            "Load the file at {} as the current track.",
            input_path.display()
        ));
    let load_out = load_cmd.output().expect("spawn cli for load");
    assert!(
        load_out.status.success(),
        "load turn failed: stderr={}\nstdout={}",
        String::from_utf8_lossy(&load_out.stderr),
        String::from_utf8_lossy(&load_out.stdout),
    );

    let mut edit_cmd = Command::cargo_bin("edytlab-cli").expect("locate cli binary");
    edit_cmd
        .arg("--project-dir")
        .arg(project.path())
        .arg("--api-key")
        .arg(&api_key)
        .arg("--message")
        .arg("Remove the silence at the start, then normalize to -1 dBFS.");
    let edit_out = edit_cmd
        .timeout(std::time::Duration::from_secs(60))
        .output()
        .expect("spawn cli for edit");
    let edit_stdout = String::from_utf8_lossy(&edit_out.stdout);
    let edit_stderr = String::from_utf8_lossy(&edit_out.stderr);
    assert!(
        edit_out.status.success(),
        "edit turn failed: stderr={edit_stderr}\nstdout={edit_stdout}",
    );

    // The model is non-deterministic about exact phrasing, but it
    // should have used `cut_range` and `normalize` at least once each.
    // We grep the JSON-lines stdout for those tool names; this is
    // looser than parsing each line as JSON but resilient to any small
    // shape changes in the CLI event format.
    assert!(
        edit_stdout.contains("\"cut_range\""),
        "expected a cut_range tool call in output:\n{edit_stdout}",
    );
    assert!(
        edit_stdout.contains("\"normalize\""),
        "expected a normalize tool call in output:\n{edit_stdout}",
    );

    // Drive the final render ourselves against the resulting head.
    // Going through the dispatcher directly here keeps the assertion
    // surface narrow: we're testing whether the AI picked the right
    // edits, not whether it remembered to render.
    let head = {
        let store = session::Store::open(project.path()).expect("reopen store");
        store.head().expect("agent should have produced a head")
    };

    let out_path = project.path().join("out.wav");
    {
        let dispatcher = tools::ToolDispatcher::default_dispatcher();
        let mut store = session::Store::open(project.path()).expect("reopen store for render");
        let mut engine = audio_engine::Engine::new();
        let mut clipboard: Option<Vec<f32>> = None;
        let mut ctx = tools::ToolContext {
            store: &mut store,
            engine: &mut engine,
            user_message: "",
            clipboard: &mut clipboard,
            allowed_tools: None,
        };
        let res = dispatcher
            .invoke(
                "render_final",
                serde_json::json!({
                    "node_id": head.to_hex(),
                    "format": "wav",
                    "out_path": out_path.to_string_lossy(),
                }),
                &mut ctx,
            )
            .expect("render_final dispatch");
        assert!(
            matches!(res, tools::ToolResult::Ok(_)),
            "render_final returned {res:?}",
        );
    }

    let out = audio_decoder::decode_file(&out_path).expect("decode render output");
    let actual_frames = out.samples.len() / out.channels as usize;
    let expected_frames = (SAMPLE_RATE * (TOTAL_SECONDS - SILENCE_SECONDS)) as usize;
    // Looser than the deterministic test — the model may pick a
    // slightly different cut boundary (e.g. trim 1.9 s instead of 2 s).
    // Plan acceptance: duration shorter by ≥ 1.8 s.
    let duration_secs = actual_frames as f32 / SAMPLE_RATE as f32;
    assert!(
        duration_secs <= (TOTAL_SECONDS as f32) - 1.8,
        "expected ≥1.8 s removed; got duration {duration_secs} s ({actual_frames} frames vs {expected_frames})",
    );

    let peak_db = {
        let peak = out
            .samples
            .iter()
            .copied()
            .map(f32::abs)
            .fold(0.0, f32::max);
        20.0 * peak.log10()
    };
    assert!(
        (peak_db - (-1.0)).abs() < 0.5,
        "expected output peak ≈ -1 dBFS (±0.5), got {peak_db}",
    );
}
