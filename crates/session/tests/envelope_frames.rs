//! The unit of a clip's volume envelope is stored only when it is not the
//! default, so a session saved before it existed loads, and hashes,
//! unchanged (#677).

use std::path::PathBuf;

use session::{Clip, EnvelopeFrames, EnvelopePoint};

fn clip() -> Clip {
    Clip {
        source_path: PathBuf::from("a.wav"),
        start_in_track: 0,
        source_offset: 0,
        length: 100,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
        volume_envelope_frames: EnvelopeFrames::default(),
    }
}

fn point(time_samples: u64) -> EnvelopePoint {
    EnvelopePoint {
        time_samples,
        gain_db: -6.0,
    }
}

#[test]
fn a_clip_stored_before_the_unit_existed_reads_back_byte_for_byte() {
    // The bytes a build before #677 wrote for a clip with an envelope.
    let old = r#"{"source_path":"a.wav","start_in_track":0,"source_offset":0,"length":100,"content_hash":null,"volume_envelope":[{"time_samples":10,"gain_db":-6.0}]}"#;

    let read: Clip = serde_json::from_str(old).unwrap();
    assert_eq!(read.volume_envelope_frames, EnvelopeFrames::Project);
    assert_eq!(serde_json::to_string(&read).unwrap(), old);
}

#[test]
fn a_clip_without_an_envelope_serialises_as_it_always_did() {
    let v = serde_json::to_value(clip()).unwrap();
    assert!(v.get("volume_envelope").is_none());
    assert!(v.get("volume_envelope_frames").is_none());
}

#[test]
fn set_envelope_marks_the_unit_and_clearing_it_unmarks() {
    let mut c = clip();
    c.set_envelope(vec![point(5)]);
    assert_eq!(c.volume_envelope_frames, EnvelopeFrames::Source);
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["volume_envelope_frames"], "source");
    let back: Clip = serde_json::from_value(v).unwrap();
    assert_eq!(back, c);

    c.set_envelope(Vec::new());
    assert_eq!(c.volume_envelope_frames, EnvelopeFrames::Project);
    assert!(serde_json::to_value(&c)
        .unwrap()
        .get("volume_envelope_frames")
        .is_none());
}
