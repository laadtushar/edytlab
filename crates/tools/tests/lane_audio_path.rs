//! `lane_audio_path` gives the timeline lane a file on the session's axis.
//!
//! Every lane draws its audio against the one ruler, so second *t* of
//! the file it is handed has to be second *t* of the session (#348). A
//! single clip's source is that file only when the clip is the whole
//! source placed at zero; a moved or trimmed clip handed over its
//! source anyway, and the lane drew the wrong audio under the ruler.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::Clip;
use tempfile::TempDir;
use tools::lane_audio_path;

const FRAMES: u64 = 400;

/// A mono ramp that never touches zero, so silence is unmistakable.
fn source(dir: &Path) -> PathBuf {
    let path = dir.join("take.wav");
    let spec = WavSpec {
        channels: 1,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w = WavWriter::create(&path, spec).unwrap();
    for n in 0..FRAMES as i16 {
        w.write_sample(1_000 + n).unwrap();
    }
    w.finalize().unwrap();
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

fn samples(path: &Path) -> Vec<i16> {
    WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(|s| s.unwrap())
        .collect()
}

fn derived_files(project: &Path) -> usize {
    std::fs::read_dir(tools::provenance::derived_dir(project))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[test]
fn a_track_with_no_clips_has_nothing_to_draw() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(lane_audio_path(tmp.path(), &[]), None);
}

/// A file just loaded: the source is the lane, and nothing is written.
#[test]
fn a_whole_source_at_zero_is_handed_over_untouched() {
    let tmp = TempDir::new().unwrap();
    let src = source(tmp.path());
    assert_eq!(
        lane_audio_path(tmp.path(), &[clip(&src, 0, 0, FRAMES)]),
        Some(src)
    );
    assert_eq!(
        derived_files(tmp.path()),
        0,
        "no flatten for the common case"
    );
}

/// Moved along the timeline: silence until the clip starts.
#[test]
fn a_moved_clip_starts_where_it_was_placed() {
    let tmp = TempDir::new().unwrap();
    let src = source(tmp.path());
    let path = lane_audio_path(tmp.path(), &[clip(&src, 100, 0, FRAMES)]).unwrap();
    assert_ne!(path, src);

    let s = samples(&path);
    assert_eq!(s.len() as u64, 100 + FRAMES);
    assert!(s[..100].iter().all(|&v| v == 0), "silence before the clip");
    assert_eq!(s[100], 1_000, "then the source, from its first frame");
}

/// Trimmed at the head: the lane starts on the first frame kept.
#[test]
fn a_clip_trimmed_at_the_head_starts_on_its_first_kept_frame() {
    let tmp = TempDir::new().unwrap();
    let src = source(tmp.path());
    let path = lane_audio_path(tmp.path(), &[clip(&src, 0, 50, FRAMES - 50)]).unwrap();

    let s = samples(&path);
    assert_eq!(s.len() as u64, FRAMES - 50);
    assert_eq!(s[0], 1_050);
}

/// Trimmed at the tail: the lane ends where the clip does, not where
/// the source does.
#[test]
fn a_clip_trimmed_at_the_tail_ends_where_the_clip_does() {
    let tmp = TempDir::new().unwrap();
    let src = source(tmp.path());
    let path = lane_audio_path(tmp.path(), &[clip(&src, 0, 0, 120)]).unwrap();
    assert_eq!(samples(&path).len(), 120);
}

#[test]
fn several_clips_are_flattened() {
    let tmp = TempDir::new().unwrap();
    let src = source(tmp.path());
    let path = lane_audio_path(
        tmp.path(),
        &[clip(&src, 0, 0, 100), clip(&src, 100, 300, 100)],
    )
    .unwrap();
    let s = samples(&path);
    assert_eq!((s[99], s[100]), (1_099, 1_300));
}

/// A source that cannot be read is handed over as it is, so the lane
/// can say what is wrong with it rather than draw nothing.
#[test]
fn an_unreadable_source_at_zero_is_handed_over_for_the_lane_to_report() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("gone.wav");
    assert_eq!(
        lane_audio_path(tmp.path(), &[clip(&missing, 0, 0, FRAMES)]),
        Some(missing)
    );
}
