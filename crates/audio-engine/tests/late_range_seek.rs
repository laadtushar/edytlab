//! A late range must not read or mix the preceding minutes of PCM when the
//! mix has no history-dependent processing. Other mixes keep their history.

use std::cell::Cell;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use audio_engine::{
    render_state_range_to_wav_cancellable, render_state_to_wav, RenderOutcome, TimeRange,
};
use hound::{SampleFormat, WavReader, WavSpec, WavSpecEx, WavWriter};
use tempfile::TempDir;

mod support;
use support::{effect, plain_track, session_of, unity_clip, write_wav_i16};

const RATE: u32 = 8_000;
const START: u64 = RATE as u64 * 599 + 17;
const OFFSET: u64 = 37;
const WINDOW: usize = 1_000;

fn sample(n: usize) -> i16 {
    ((n as i32 * 139 % 24_000) - 12_000) as i16
}

// A sparse ten-minute WAV, with the same audible window as the short WAV.
// Creating the long prefix takes no time or memory proportional to its size.
fn write_long_wav(path: &Path) {
    let frames = RATE * 600 + OFFSET as u32;
    let data_len = frames * 2;
    let mut file = File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + data_len).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&RATE.to_le_bytes()).unwrap();
    file.write_all(&(RATE * 2).to_le_bytes()).unwrap();
    file.write_all(&2_u16.to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_len.to_le_bytes()).unwrap();
    file.set_len(u64::from(data_len) + 44).unwrap();
    file.seek(SeekFrom::Start(44 + (OFFSET + START) * 2))
        .unwrap();
    for n in 0..WINDOW {
        file.write_all(&sample(n).to_le_bytes()).unwrap();
    }
}

#[cfg(target_os = "linux")]
fn thread_read_bytes() -> u64 {
    std::fs::read_to_string("/proc/thread-self/io")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("rchar: "))
        .unwrap()
        .parse()
        .unwrap()
}

fn render_late_and_compare() -> (usize, u64) {
    let dir = TempDir::new().unwrap();
    let long = dir.path().join("long.wav");
    write_long_wav(&long);
    let short = write_wav_i16(dir.path(), "short.wav", RATE, 1, WINDOW, sample);
    let mut track = plain_track(
        "voice",
        vec![unity_clip(&long, 0, OFFSET, u64::from(RATE) * 600)],
    );
    track.gain_db = -3.0;
    let late_state = session_of(vec![track], RATE, u64::from(RATE) * 600);
    let mut short_track = plain_track("voice", vec![unity_clip(&short, 0, 0, WINDOW as u64)]);
    short_track.gain_db = -3.0;
    let short_state = session_of(vec![short_track], RATE, WINDOW as u64);
    let expected_path = dir.path().join("expected.wav");
    let expected = render_state_to_wav(
        &short_state,
        &expected_path,
        Some(TimeRange {
            start_frame: 0,
            end_frame: WINDOW as u64,
        }),
    )
    .unwrap();

    let actual_path = dir.path().join("actual.wav");
    let checks = Cell::new(0);
    #[cfg(target_os = "linux")]
    let before = thread_read_bytes();
    let outcome = render_state_range_to_wav_cancellable(
        &late_state,
        &actual_path,
        TimeRange {
            start_frame: START,
            end_frame: START + WINDOW as u64,
        },
        &|| {
            checks.set(checks.get() + 1);
            false
        },
    )
    .unwrap();
    #[cfg(target_os = "linux")]
    let read_bytes = thread_read_bytes() - before;
    #[cfg(not(target_os = "linux"))]
    let read_bytes = 0;
    let RenderOutcome::Complete(actual) = outcome else {
        panic!("render cancelled")
    };
    assert_eq!(
        std::fs::read(actual_path).unwrap(),
        std::fs::read(expected_path).unwrap()
    );
    assert_eq!(actual.frames_written, expected.frames_written);
    assert_eq!(actual.sample_rate, expected.sample_rate);
    assert_eq!(actual.channels, expected.channels);
    assert_eq!(actual.peak_dbfs.to_bits(), expected.peak_dbfs.to_bits());
    assert_eq!(actual.warnings, expected.warnings);
    (checks.get(), read_bytes)
}

#[test]
fn late_range_does_not_mix_the_prefix() {
    let (checks, _) = render_late_and_compare();
    assert!(
        checks < 20,
        "a short range required {checks} cancellation checks; the prefix was mixed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn late_range_does_not_read_the_prefix_pcm() {
    let (_, bytes) = render_late_and_compare();
    assert!(
        bytes < 1_000_000,
        "a short range read {bytes} bytes from a ten-minute source"
    );
}

fn samples(path: &Path) -> Vec<i16> {
    WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn filtered_range_retains_history_before_its_start() {
    let dir = TempDir::new().unwrap();
    let start = RATE as usize * 2 + 17;
    let total = RATE as usize * 4;
    let source = write_wav_i16(dir.path(), "filtered.wav", RATE, 1, total, |n| {
        if n == start - 1 {
            30_000
        } else if n >= start {
            4_000
        } else {
            0
        }
    });
    for (index, fx) in [
        effect("low_pass_filter", serde_json::json!({"cutoff_hz": 300.0})),
        effect(
            "limiter",
            serde_json::json!({"ceiling_db": -12.0, "release_ms": 500.0}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut track = plain_track("voice", vec![unity_clip(&source, 0, 0, total as u64)]);
        track.effects.push(fx);
        let state = session_of(vec![track], RATE, total as u64);
        let full = dir.path().join(format!("full-{index}.wav"));
        let late = dir.path().join(format!("late-{index}.wav"));
        render_state_to_wav(
            &state,
            &full,
            Some(TimeRange {
                start_frame: 0,
                end_frame: total as u64,
            }),
        )
        .unwrap();
        render_state_to_wav(
            &state,
            &late,
            Some(TimeRange {
                start_frame: start as u64,
                end_frame: (start + WINDOW) as u64,
            }),
        )
        .unwrap();
        assert_eq!(
            samples(&late),
            samples(&full)[start..start + WINDOW],
            "effect {index} lost its prior state"
        );
    }
}

fn assert_effect_range_processes_prefix(state: &session::SessionState, out: &Path) {
    let checks = Cell::new(0);
    let start = u64::from(RATE) * 10;
    let result = render_state_range_to_wav_cancellable(
        state,
        out,
        TimeRange {
            start_frame: start,
            end_frame: start + WINDOW as u64,
        },
        &|| {
            checks.set(checks.get() + 1);
            false
        },
    )
    .unwrap();
    assert!(matches!(result, RenderOutcome::Complete(_)));
    assert!(
        checks.get() >= 10,
        "the active effect skipped prefix processing ({} cancellation checks)",
        checks.get()
    );
}

#[test]
fn active_master_effect_keeps_prefix_processing() {
    let dir = TempDir::new().unwrap();
    let frames = RATE as usize * 12;
    let source = write_wav_i16(dir.path(), "master-source.wav", RATE, 1, frames, sample);
    let track = plain_track("voice", vec![unity_clip(&source, 0, 0, frames as u64)]);
    let mut state = session_of(vec![track], RATE, frames as u64);
    state.master_chain.push(effect(
        "low_pass_filter",
        serde_json::json!({"cutoff_hz": 300.0}),
    ));
    assert_effect_range_processes_prefix(&state, &dir.path().join("master-out.wav"));
}

#[test]
fn active_bus_effect_keeps_prefix_processing() {
    let dir = TempDir::new().unwrap();
    let frames = RATE as usize * 12;
    let source = write_wav_i16(dir.path(), "bus-source.wav", RATE, 1, frames, sample);
    let bus_id = uuid::Uuid::new_v4();
    let mut track = plain_track("voice", vec![unity_clip(&source, 0, 0, frames as u64)]);
    track.sends.push(session::Send {
        bus_id,
        level_db: 0.0,
    });
    let mut state = session_of(vec![track], RATE, frames as u64);
    state.bus_routing.buses.push(session::Bus {
        id: bus_id,
        name: "filter".into(),
        effects: vec![effect(
            "low_pass_filter",
            serde_json::json!({"cutoff_hz": 300.0}),
        )],
    });
    assert_effect_range_processes_prefix(&state, &dir.path().join("bus-out.wav"));
}

#[test]
fn bypassed_master_and_bus_effects_allow_seeking() {
    let dir = TempDir::new().unwrap();
    let frames = RATE as usize * 12;
    let source = write_wav_i16(dir.path(), "bypassed-source.wav", RATE, 1, frames, sample);
    let bus_id = uuid::Uuid::new_v4();
    let mut track = plain_track("voice", vec![unity_clip(&source, 0, 0, frames as u64)]);
    track.sends.push(session::Send {
        bus_id,
        level_db: 0.0,
    });
    let mut state = session_of(vec![track], RATE, frames as u64);
    let mut bypassed = effect("low_pass_filter", serde_json::json!({"cutoff_hz": 300.0}));
    bypassed.bypassed = true;
    state.master_chain.push(bypassed.clone());
    state.bus_routing.buses.push(session::Bus {
        id: bus_id,
        name: "filter".into(),
        effects: vec![bypassed],
    });
    let checks = Cell::new(0);
    let start = u64::from(RATE) * 10;
    let result = render_state_range_to_wav_cancellable(
        &state,
        &dir.path().join("bypassed-out.wav"),
        TimeRange {
            start_frame: start,
            end_frame: start + WINDOW as u64,
        },
        &|| {
            checks.set(checks.get() + 1);
            false
        },
    )
    .unwrap();
    assert!(matches!(result, RenderOutcome::Complete(_)));
    assert!(
        checks.get() < 10,
        "bypassed effects prevented seeking ({} cancellation checks)",
        checks.get()
    );
}

#[test]
fn resampled_range_matches_full_render_at_unaligned_frame() {
    let dir = TempDir::new().unwrap();
    let source_rate = 11_025;
    let source_frames = source_rate as usize * 4;
    let source = write_wav_i16(
        dir.path(),
        "resampled.wav",
        source_rate,
        1,
        source_frames,
        sample,
    );
    let track = plain_track(
        "voice",
        vec![unity_clip(&source, 0, 0, source_frames as u64)],
    );
    let state = session_of(vec![track], RATE, u64::from(RATE) * 4);
    let full = dir.path().join("full.wav");
    let late = dir.path().join("late.wav");
    let start = RATE as usize * 2 + 17;
    render_state_to_wav(
        &state,
        &full,
        Some(TimeRange {
            start_frame: 0,
            end_frame: u64::from(RATE) * 4,
        }),
    )
    .unwrap();
    render_state_to_wav(
        &state,
        &late,
        Some(TimeRange {
            start_frame: start as u64,
            end_frame: (start + WINDOW) as u64,
        }),
    )
    .unwrap();
    assert_eq!(samples(&late), samples(&full)[start..start + WINDOW]);
}

#[test]
fn padded_24_bit_wav_with_source_offset_retains_sample_alignment() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("padded.wav");
    let spec = WavSpecEx {
        spec: WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        },
        bytes_per_sample: 4,
    };
    let mut writer = WavWriter::new_with_spec_ex(File::create(&source).unwrap(), spec).unwrap();
    for n in 0..4_000 {
        writer.write_sample(i32::from(sample(n)) * 256).unwrap();
    }
    writer.finalize().unwrap();
    let mut track = plain_track("voice", vec![unity_clip(&source, 0, 37, 3_900)]);
    track.gain_db = -3.0;
    let state = session_of(vec![track], RATE, 3_900);
    let full = dir.path().join("full.wav");
    let late = dir.path().join("late.wav");
    render_state_to_wav(
        &state,
        &full,
        Some(TimeRange {
            start_frame: 0,
            end_frame: 3_900,
        }),
    )
    .unwrap();
    render_state_to_wav(
        &state,
        &late,
        Some(TimeRange {
            start_frame: 1_007,
            end_frame: 2_007,
        }),
    )
    .unwrap();
    assert_eq!(samples(&late), samples(&full)[1_007..2_007]);
}

#[test]
fn truncated_source_is_not_hidden_by_a_late_seek() {
    let dir = TempDir::new().unwrap();
    let source = write_wav_i16(dir.path(), "truncated.wav", RATE, 1, 4_000, sample);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&source)
        .unwrap();
    let length = file.metadata().unwrap().len();
    file.set_len(length - 4_000).unwrap();
    let track = plain_track("voice", vec![unity_clip(&source, 0, 0, 4_000)]);
    let state = session_of(vec![track], RATE, 4_000);
    let result = render_state_to_wav(
        &state,
        &dir.path().join("out.wav"),
        Some(TimeRange {
            start_frame: 3_000,
            end_frame: 3_100,
        }),
    );
    assert!(
        matches!(result, Err(audio_engine::Error::Decode(_))),
        "truncated source: {result:?}"
    );
}

#[test]
fn positioned_overlapping_clips_keep_gaps_offsets_and_envelopes() {
    use session::EnvelopePoint;
    let dir = TempDir::new().unwrap();
    let source = write_wav_i16(dir.path(), "clips.wav", RATE, 1, 12_000, sample);
    let mut first = unity_clip(&source, 0, 200, 7_000);
    first.set_envelope(vec![
        EnvelopePoint {
            time_samples: 0,
            gain_db: -8.0,
        },
        EnvelopePoint {
            time_samples: 7_000,
            gain_db: 0.0,
        },
    ]);
    let second = unity_clip(&source, 5_000, 3_000, 4_000);
    let third = unity_clip(&source, 10_000, 100, 1_000);
    let mut track = plain_track("clips", vec![first, second, third]);
    track.pan = 0.3;
    let state = session_of(vec![track], RATE, 11_000);
    let full = dir.path().join("full.wav");
    let late = dir.path().join("late.wav");
    render_state_to_wav(
        &state,
        &full,
        Some(TimeRange {
            start_frame: 0,
            end_frame: 11_000,
        }),
    )
    .unwrap();
    render_state_to_wav(
        &state,
        &late,
        Some(TimeRange {
            start_frame: 6_003,
            end_frame: 10_500,
        }),
    )
    .unwrap();
    // Pan widens this mono mix to stereo.
    assert_eq!(samples(&late), samples(&full)[6_003 * 2..10_500 * 2]);
}

#[test]
fn ended_clip_with_unsupported_channel_map_still_reports_the_error() {
    let dir = TempDir::new().unwrap();
    let stereo = write_wav_i16(dir.path(), "stereo.wav", RATE, 2, 800, sample);
    let surround = write_wav_i16(dir.path(), "surround.wav", RATE, 3, 24_000, sample);
    let first = plain_track("short stereo", vec![unity_clip(&stereo, 0, 0, 800)]);
    let second = plain_track("surround", vec![unity_clip(&surround, 0, 0, 24_000)]);
    let state = session_of(vec![first, second], RATE, 24_000);
    let result = render_state_to_wav(
        &state,
        &dir.path().join("late.wav"),
        Some(TimeRange {
            start_frame: 16_000,
            end_frame: 17_000,
        }),
    );
    assert!(
        matches!(
            result,
            Err(audio_engine::Error::UnsupportedChannelMap { from: 2, to: 3 })
        ),
        "expected the earlier clip's channel-map error, got {result:?}"
    );
}

#[test]
fn source_alias_uses_the_original_range_path() {
    let dir = TempDir::new().unwrap();
    for hard_link in [false, true] {
        let source = write_wav_i16(
            dir.path(),
            if hard_link {
                "linked-source.wav"
            } else {
                "source.wav"
            },
            RATE,
            1,
            16_000,
            sample,
        );
        let out = if hard_link {
            let link = dir.path().join("linked-output.wav");
            if std::fs::hard_link(&source, &link).is_err() {
                continue;
            }
            link
        } else {
            source.clone()
        };
        // The source begins after the whole requested range. The legacy
        // renderer processes ten seconds of silence before writing it, so
        // the cancellation callback reveals whether that path was kept.
        // Both paths touch only the private TempDir, including the alias.
        let track = plain_track(
            "later clip",
            vec![unity_clip(&source, u64::from(RATE) * 20, 0, 16_000)],
        );
        let state = session_of(vec![track], RATE, u64::from(RATE) * 22);
        let checks = Cell::new(0);
        let result = render_state_range_to_wav_cancellable(
            &state,
            &out,
            TimeRange {
                start_frame: u64::from(RATE) * 10,
                end_frame: u64::from(RATE) * 10 + 1_000,
            },
            &|| {
                checks.set(checks.get() + 1);
                false
            },
        )
        .unwrap();
        assert!(matches!(result, RenderOutcome::Complete(_)));
        assert!(
            checks.get() >= 10,
            "aliased output skipped the original prefix path (hard link: {hard_link}, checks: {})",
            checks.get()
        );
    }
}

#[cfg(unix)]
#[test]
fn fifo_source_probe_preserves_the_producer_for_fallback() {
    const CHILD_ENV: &str = "EDYTLAB_FIFO_SOURCE_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let dir = TempDir::new().unwrap();
        let fixture = write_wav_i16(dir.path(), "source.wav", RATE, 1, 100, sample);
        let wav = std::fs::read(fixture).unwrap();
        let fifo = dir.path().join("source.fifo");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let producer_path = fifo.clone();
        // A producer serving the fallback reader once must not be consumed
        // by an eligibility probe before that reader opens the FIFO.
        let producer = std::thread::spawn(move || {
            let mut fifo = File::create(producer_path).unwrap();
            fifo.write_all(&wav).unwrap();
        });
        assert!(audio_decoder::WavStreamReader::open_seekable(&fifo).is_none());
        let reader = audio_decoder::WavStreamReader::open(&fifo).unwrap();
        assert_eq!(reader.total_frames(), 100);
        producer.join().unwrap();
        return;
    }

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "fifo_source_probe_preserves_the_producer_for_fallback",
        ])
        .env(CHILD_ENV, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "FIFO source child failed: {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("the seek probe consumed the FIFO producer before fallback");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn fifo_output_does_not_block_before_cancellation() {
    const CHILD_ENV: &str = "EDYTLAB_FIFO_RENDER_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let dir = TempDir::new().unwrap();
        let source = write_wav_i16(dir.path(), "source.wav", RATE, 1, 24_000, sample);
        let fifo = dir.path().join("output.fifo");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let track = plain_track("voice", vec![unity_clip(&source, 0, 0, 24_000)]);
        let state = session_of(vec![track], RATE, 24_000);
        let checks = Cell::new(0);
        let result = render_state_range_to_wav_cancellable(
            &state,
            &fifo,
            TimeRange {
                start_frame: 16_000,
                end_frame: 17_000,
            },
            &|| {
                checks.set(checks.get() + 1);
                checks.get() >= 2
            },
        )
        .unwrap();
        assert!(matches!(result, RenderOutcome::Cancelled));
        return;
    }

    // The old eligibility check opens a FIFO for reading and would block the
    // test indefinitely. A child process makes that failure bounded and can
    // be killed without leaving a blocked thread in the test runner.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fifo_output_does_not_block_before_cancellation"])
        .env(CHILD_ENV, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "FIFO cancellation child failed: {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("opening the FIFO blocked before the cancellation check");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn seekable_range_checks_cancellation_between_and_after_output_blocks() {
    let dir = TempDir::new().unwrap();
    let source = write_wav_i16(dir.path(), "source.wav", RATE, 1, 40_000, sample);
    let track = plain_track("voice", vec![unity_clip(&source, 0, 0, 40_000)]);
    let state = session_of(vec![track], RATE, 40_000);
    for (end, stop, written) in [(32_000, 2, RATE), (24_017, 3, RATE + 17)] {
        let out = dir.path().join(format!("cancel-{stop}.wav"));
        let checks = Cell::new(0);
        let result = render_state_range_to_wav_cancellable(
            &state,
            &out,
            TimeRange {
                start_frame: 16_000,
                end_frame: end,
            },
            &|| {
                if !out.exists() {
                    return false;
                }
                checks.set(checks.get() + 1);
                checks.get() >= stop
            },
        )
        .unwrap();
        assert!(matches!(result, RenderOutcome::Cancelled));
        assert_eq!(WavReader::open(out).unwrap().duration(), written);
    }
}
