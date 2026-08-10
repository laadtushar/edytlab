//! `flattened_track_wav` materialises a track's timeline as one file.
//!
//! The desktop app's `list_tracks` needs a path to hand the waveform
//! renderer. A single-clip track already is a file; a track split by an
//! interior cut is not, and returning nothing for it left the timeline
//! lane blank — the audio was there and the render was correct, but the
//! UI drew nothing at all.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::Clip;
use tempfile::TempDir;
use tools::flattened_track_wav;

const RATE: u32 = 8_000;
const SOURCE_FRAMES: usize = 800;

/// A ramp, so every frame names the source frame it came from.
fn write_ramp_wav(dir: &Path) -> PathBuf {
    let path = dir.join("in.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..SOURCE_FRAMES {
        let s = 0.5 * n as f32 / SOURCE_FRAMES as f32;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

fn clip(source: &Path, start_in_track: u64, source_offset: u64, length: u64) -> Clip {
    Clip {
        source_path: source.to_path_buf(),
        start_in_track,
        source_offset,
        length,
        content_hash: None,
        time_stretch_factor: None,
        pitch_shift_semitones: None,
        beat_grid: None,
        volume_envelope: Vec::new(),
    }
}

fn source_frame_of(sample: i16) -> f32 {
    (sample as f32 / 32_768.0) / 0.5 * SOURCE_FRAMES as f32
}

/// The written file is the timeline: both clips, in order, joined.
#[test]
fn writes_the_whole_timeline_not_the_first_clip() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path());

    // The shape an interior cut of [200,600) leaves behind.
    let clips = vec![clip(&src, 0, 0, 200), clip(&src, 200, 600, 200)];
    let path = flattened_track_wav(&clips).expect("flatten");

    let mut reader = WavReader::open(&path).expect("open flattened");
    assert_eq!(reader.spec().sample_rate, RATE);
    assert_eq!(reader.spec().channels, 1);

    let samples: Vec<i16> = reader.samples::<i16>().map(|r| r.unwrap()).collect();
    assert_eq!(samples.len(), 400, "both clips, not just the first");

    assert!(
        (source_frame_of(samples[0]) - 0.0).abs() < 5.0,
        "should open on the head of the ramp"
    );
    assert!(
        (source_frame_of(samples[199]) - 199.0).abs() < 5.0,
        "the head should run up to the cut"
    );
    assert!(
        (source_frame_of(samples[200]) - 600.0).abs() < 5.0,
        "the tail should resume where the cut ended, not continue the head"
    );
}

/// A gap between clips is silence, matching what the render engine does.
#[test]
fn a_gap_between_clips_is_silence() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path());

    let clips = vec![clip(&src, 0, 0, 100), clip(&src, 300, 0, 100)];
    let path = flattened_track_wav(&clips).expect("flatten");

    let mut reader = WavReader::open(&path).expect("open flattened");
    let samples: Vec<i16> = reader.samples::<i16>().map(|r| r.unwrap()).collect();
    assert_eq!(samples.len(), 400);

    let gap_peak = samples[100..300].iter().map(|s| s.abs()).max().unwrap();
    assert!(
        gap_peak < 40,
        "the gap should be silent, peak was {gap_peak}"
    );
}

/// The name is keyed on the clip list, so calling again is free.
///
/// This is what makes the helper safe to call from a listing: the second
/// call must find the file already written and return without touching
/// the sources. Deleting the source between calls proves it — a
/// re-decode would fail outright.
#[test]
fn a_repeat_call_does_not_re_read_the_sources() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path());
    let clips = vec![clip(&src, 0, 0, 200), clip(&src, 200, 600, 200)];

    let first = flattened_track_wav(&clips).expect("first call");
    std::fs::remove_file(&src).expect("remove source");

    let second = flattened_track_wav(&clips).expect("second call must not re-decode");
    assert_eq!(
        first, second,
        "the same clip list must map to the same file"
    );
    assert!(second.exists());
}

/// A different arrangement of the same source is a different file.
#[test]
fn a_different_clip_list_gets_a_different_file() {
    let tmp = TempDir::new().unwrap();
    let src = write_ramp_wav(tmp.path());

    let a = flattened_track_wav(&[clip(&src, 0, 0, 200), clip(&src, 200, 600, 200)]).unwrap();
    let b = flattened_track_wav(&[clip(&src, 0, 0, 300), clip(&src, 300, 600, 200)]).unwrap();
    assert_ne!(
        a, b,
        "two different cuts of one source must not share a waveform"
    );
}

/// An empty track has no timeline to write.
#[test]
fn an_empty_track_is_an_error_not_an_empty_file() {
    assert!(flattened_track_wav(&[]).is_err());
}
