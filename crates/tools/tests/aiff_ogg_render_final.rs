//! `render_final` writes AIFF and OGG (#764): through the tool, not just the
//! encoders (`audio-engine/tests/aiff_ogg_export.rs` holds those).

use serde_json::json;

#[path = "support/harness.rs"]
mod harness;
use harness::{err, ok, Session};

const SAMPLE_RATE: u32 = 44_100;

#[test]
fn aiff_export_is_the_wav_export_sample_for_sample() {
    let mut s = Session::with_sine(SAMPLE_RATE, 1);
    let node = s.head();
    let wav = s.dir.path().join("ref.wav");
    let aiff = s.dir.path().join("out.aiff");
    for (format, path) in [("wav", &wav), ("aiff", &aiff)] {
        ok(s.call(
            "render_final",
            json!({ "node_id": node, "format": format, "out_path": path.to_string_lossy() }),
        ));
    }
    let (w, a) = (
        audio_decoder::decode_file(&wav).unwrap(),
        audio_decoder::decode_file(&aiff).unwrap(),
    );
    assert_eq!((a.sample_rate, a.channels), (w.sample_rate, w.channels));
    assert_eq!(a.samples, w.samples);
}

#[test]
fn ogg_export_decodes_at_48k_with_the_inputs_length_and_reports_its_bitrate() {
    let mut s = Session::with_sine(SAMPLE_RATE, 1);
    let node = s.head();
    let wav = s.dir.path().join("ref.wav");
    let ogg = s.dir.path().join("out.ogg");
    ok(s.call(
        "render_final",
        json!({ "node_id": node, "format": "wav", "out_path": wav.to_string_lossy() }),
    ));
    let result = ok(s.call(
        "render_final",
        json!({ "node_id": node, "format": "ogg", "out_path": ogg.to_string_lossy() }),
    ));
    assert_eq!(
        result["bitrate_kbps"],
        json!(64),
        "mono default is half of 128"
    );

    let (w, o) = (
        audio_decoder::decode_file(&wav).unwrap(),
        audio_decoder::decode_file(&ogg).unwrap(),
    );
    assert_eq!(o.sample_rate, 48_000);
    assert_eq!(o.channels, w.channels);
    let expected = (w.samples.len() as u64 * 48_000 + 22_050) / u64::from(SAMPLE_RATE);
    assert_eq!(o.samples.len() as u64, expected);
    assert!(std::fs::metadata(&ogg).unwrap().len() < std::fs::metadata(&wav).unwrap().len());
}

#[test]
fn ogg_carries_tags_aiff_refuses_them_and_a_bitrate_stays_lossy_only() {
    let mut s = Session::with_sine(SAMPLE_RATE, 1);
    let node = s.head();
    let ogg = s.dir.path().join("t.ogg");
    let aiff = s.dir.path().join("t.aiff");

    ok(s.call(
        "render_final",
        json!({
            "node_id": node, "format": "ogg", "out_path": ogg.to_string_lossy(),
            "bitrate_kbps": 48, "metadata": { "title": "Episode 12" },
        }),
    ));
    let bytes = std::fs::read(&ogg).unwrap();
    assert!(bytes.windows(15).any(|w| w == b"TITLE=Episode 1"));

    let msg = err(s.call(
        "render_final",
        json!({
            "node_id": node, "format": "aiff", "out_path": aiff.to_string_lossy(),
            "metadata": { "title": "x" },
        }),
    ));
    assert!(msg.contains("AIFF"), "{msg}");
    assert!(!aiff.exists());

    let msg = err(s.call(
        "render_final",
        json!({
            "node_id": node, "format": "aiff", "out_path": aiff.to_string_lossy(),
            "bitrate_kbps": 128,
        }),
    ));
    assert!(msg.contains("mp3 and ogg only"), "{msg}");
}
