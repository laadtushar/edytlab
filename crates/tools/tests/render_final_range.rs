//! A final export may contain only a requested part of the session.
use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{read_pcm, Session};

#[test]
fn final_wav_contains_only_the_requested_range() {
    let mut s = Session::with_sine(48_000, 2);
    let full = s.dir.path().join("full.wav");
    let part = s.dir.path().join("part.wav");
    let node = s.head();
    s.ok(
        "render_final",
        json!({ "node_id": node, "format": "wav", "out_path": full }),
    );
    let result = s.ok(
        "render_final",
        json!({
            "node_id": node, "format": "wav", "out_path": part,
            "range": [0.25, 0.75]
        }),
    );
    let full = read_pcm(&full);
    let part = read_pcm(&part);
    let channels = result["channels"].as_u64().unwrap() as usize;
    assert_eq!(result["frames_written"], 24_000);
    assert_eq!(part, full[12_000 * channels..36_000 * channels]);
}

#[test]
fn lossless_formats_contain_the_same_requested_audio() {
    let mut s = Session::with_sine(48_000, 2);
    let node = s.head();
    let mut samples = Vec::new();
    for format in ["wav", "aiff", "flac"] {
        let path = s.dir.path().join(format!("part.{format}"));
        let result = s.ok(
            "render_final",
            json!({
                "node_id": node, "format": format, "out_path": path,
                "range": [0.25, 0.75]
            }),
        );
        assert_eq!(result["frames_written"], 24_000, "{format}");
        samples.push(audio_decoder::decode_file(&path).unwrap().samples);
    }
    assert_eq!(samples[0], samples[1]);
    assert_eq!(samples[0], samples[2]);
}

#[test]
fn incomplete_or_reversed_ranges_do_not_write_the_output() {
    let mut s = Session::with_sine(48_000, 2);
    let node = s.head();
    for (index, range) in [
        json!([0.5]),
        json!([1.0, 0.5]),
        json!([0.5, 0.5]),
        json!([0.5, 0.500001]),
    ]
    .iter()
    .enumerate()
    {
        let path = s.dir.path().join(format!("invalid-{index}.wav"));
        let args = json!({"node_id": node, "format": "wav", "out_path": path, "range": range});
        let answer = s.try_call("render_final", args);
        assert!(answer.is_err() || matches!(answer, Ok(tools::ToolResult::Error(_))));
        assert!(!path.exists());
    }
}

#[test]
fn range_chapters_are_trimmed_and_relative_to_the_export() {
    let mut s = Session::with_sine(48_000, 2);
    for (time, name) in [(0.1, "before"), (0.5, "inside"), (1.5, "after")] {
        s.ok("label", json!({"time": time, "name": name}));
    }
    let path = s.dir.path().join("part.flac");
    let node = s.head();
    let result = s.ok(
        "render_final",
        json!({
            "node_id": node, "format": "flac", "out_path": path,
            "range": [0.25, 1.0], "markers_as_chapters": true
        }),
    );
    assert_eq!(result["chapters"], 1);
    let tags = audio_engine::read_flac_tags(&path).unwrap();
    assert!(
        tags.iter()
            .any(|(k, v)| k == "CHAPTER001" && v == "00:00:00.250"),
        "{tags:?}"
    );
    assert!(
        tags.iter()
            .any(|(k, v)| k == "CHAPTER001NAME" && v == "inside"),
        "{tags:?}"
    );
}

#[test]
fn lossy_formats_encode_only_the_requested_range() {
    let mut s = Session::with_sine(48_000, 2);
    let node = s.head();
    for format in ["mp3", "ogg"] {
        let path = s.dir.path().join(format!("part.{format}"));
        let result = s.ok(
            "render_final",
            json!({
                "node_id": node, "format": format, "out_path": path,
                "range": [0.25, 0.75]
            }),
        );
        assert_eq!(result["frames_written"], 24_000, "{format}");
        let decoded = audio_decoder::decode_file(&path).unwrap();
        let duration = decoded.samples.len() as f64
            / f64::from(decoded.sample_rate)
            / f64::from(decoded.channels);
        assert!((0.45..0.6).contains(&duration), "{format}: {duration}");
    }
}

#[test]
fn range_export_cannot_overwrite_project_audio() {
    let mut s = Session::with_sine(48_000, 2);
    let node = s.head();
    let source = s.dir.path().join("in.wav");
    let original = std::fs::read(&source).unwrap();
    assert!(matches!(
        s.call(
            "render_final",
            json!({
                "node_id": node, "format": "wav", "out_path": source,
                "range": [0.25, 0.75]
            })
        ),
        tools::ToolResult::Error(_)
    ));
    assert_eq!(std::fs::read(source).unwrap(), original);
}
