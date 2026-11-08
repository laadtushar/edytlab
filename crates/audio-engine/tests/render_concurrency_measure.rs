//! Opt-in, process-level render concurrency measurements for #721.
//!
//! Run exactly one point per fresh process, for example:
//! `BENCH_WORKLOAD=mixed BENCH_SCOPE=same BENCH_WORKERS=2 cargo test -p audio-engine --test render_concurrency_measure --release -- --ignored --exact measure_render_concurrency --nocapture`
//! Workloads: `mixed` (30 tracks, 180 s), `huge` (200 tracks, 184.5 s),
//! `limiter` (one stereo track, 600 s); scopes: `same`, `different`;
//! workers: 1, 2, 4. Set `BENCH_SECONDS` to a positive whole number for a
//! smoke run ONLY. Run all 18 combinations as separate invocations. Apply any
//! memory limit externally, e.g. with the shell's `ulimit`, not in this test.
//!
//! The synthetic fixtures intentionally share one file per rate within a
//! project; they do not represent independent source-media caches. Different
//! projects are independently written files, not hardlinks. The huge fixture
//! mirrors the request-size test's routing and stored effects, but the
//! renderer currently skips compressor, EQ and reverb; warnings are printed.
//! VmHWM is the process high-water mark since launch, INCLUDING setup, and
//! /proc RSS is approximate; neither includes all cgroup file-cache effects.
#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Barrier;
use std::time::Instant;

use audio_engine::{Engine, RenderReport};
use session::{
    Annotation, AnnotationId, AnnotationKind, Bus, BusGraph, Clip, EffectInstance, Send,
    SessionState, TempoMap, Track, TrackId,
};
use tempfile::TempDir;

const RATE: u32 = 48_000;
const LIMIT_DB: f32 = -20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workload {
    Mixed,
    Huge,
    Limiter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Same,
    Different,
}

#[derive(Debug)]
struct Options {
    workload: Workload,
    scope: Scope,
    workers: usize,
    override_seconds: Option<u32>,
}

impl Options {
    fn parse(
        workload: &str,
        scope: &str,
        workers: &str,
        seconds: Option<&str>,
    ) -> Result<Self, String> {
        let workload = match workload {
            "mixed" => Workload::Mixed,
            "huge" => Workload::Huge,
            "limiter" => Workload::Limiter,
            _ => return Err("BENCH_WORKLOAD must be mixed, huge, or limiter".into()),
        };
        let scope = match scope {
            "same" => Scope::Same,
            "different" => Scope::Different,
            _ => return Err("BENCH_SCOPE must be same or different".into()),
        };
        let workers = match workers {
            "1" => 1,
            "2" => 2,
            "4" => 4,
            _ => return Err("BENCH_WORKERS must be 1, 2, or 4".into()),
        };
        let override_seconds = seconds
            .map(|s| {
                s.parse::<u32>()
                    .ok()
                    .filter(|n| (1..=600).contains(n))
                    .ok_or("BENCH_SECONDS must be a whole number from 1 to 600".to_string())
            })
            .transpose()?;
        Ok(Self {
            workload,
            scope,
            workers,
            override_seconds,
        })
    }

    fn frames(&self, rate: u32) -> u64 {
        match self.override_seconds {
            Some(seconds) => u64::from(rate) * u64::from(seconds),
            None => match self.workload {
                Workload::Mixed => u64::from(rate) * 180,
                Workload::Huge => u64::from(rate) * 369 / 2,
                Workload::Limiter => u64::from(rate) * 600,
            },
        }
    }

    fn project_count(&self) -> usize {
        if self.scope == Scope::Same {
            1
        } else {
            self.workers
        }
    }
}

#[test]
fn options_and_fixture_dimensions_are_explicit() {
    let opts = Options::parse("huge", "different", "4", None).unwrap();
    assert_eq!(opts.frames(RATE), 8_856_000);
    assert_eq!(opts.project_count(), 4);
    assert_eq!(
        Options::parse("mixed", "same", "2", None)
            .unwrap()
            .frames(44_100),
        7_938_000
    );
    assert_eq!(
        Options::parse("limiter", "same", "1", None)
            .unwrap()
            .frames(RATE),
        28_800_000
    );
    assert_eq!(
        Options::parse("huge", "same", "1", Some("1"))
            .unwrap()
            .frames(RATE),
        48_000
    );
    assert!(Options::parse("huge", "same", "3", None).is_err());
    assert!(Options::parse("huge", "same", "1", Some("0")).is_err());
    assert!(Options::parse("huge", "same", "1", Some("601")).is_err());
    let state = state_for(&opts, &[PathBuf::from("synthetic.wav")]);
    assert_eq!(state.tracks.len(), 200);
    assert_eq!(state.bus_routing.buses.len(), 50);
    assert_eq!(state.annotations.len(), 1000);
    assert!(state
        .tracks
        .iter()
        .all(|t| !t.muted && t.clips[0].length == 8_856_000));
    assert!(state.tracks[..150].iter().all(|t| t.sends.len() == 1));
    assert!(state.tracks[150..].iter().all(|t| t.sends.is_empty()));
    assert_eq!(state.master_chain[0].kind, "limiter");
}

fn effect(kind: &str, params: serde_json::Value) -> EffectInstance {
    EffectInstance {
        kind: kind.into(),
        params,
        bypassed: false,
    }
}

fn write_source(path: &Path, rate: u32, frames: u64) {
    // Stream bounded PCM blocks, not an in-memory copy of the recording.
    // A triangle wave peaks above the -20 dBFS limiter ceiling.
    let mut writer = hound::WavWriter::new(
        BufWriter::new(File::create(path).unwrap()),
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for frame in 0..frames {
        let step = (frame % 512) as i32;
        let triangle = if step < 256 { step } else { 511 - step };
        let sample = ((triangle - 128) * 120) as i16;
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

fn state_for(options: &Options, paths: &[PathBuf]) -> SessionState {
    let count = match options.workload {
        Workload::Mixed => 30,
        Workload::Huge => 200,
        Workload::Limiter => 1,
    };
    let buses: Vec<Bus> = if options.workload == Workload::Huge {
        (0..50)
            .map(|n| Bus {
                id: uuid::Uuid::from_u128(n as u128 + 1),
                name: format!("Bus {n}"),
                effects: vec![effect("reverb", serde_json::json!({}))],
            })
            .collect()
    } else {
        vec![]
    };
    let tracks = (0..count)
        .map(|n| {
            let rate = if options.workload == Workload::Mixed && n % 2 == 0 {
                44_100
            } else {
                RATE
            };
            let path = if rate == 44_100 { &paths[1] } else { &paths[0] };
            Track {
                id: TrackId::new(),
                name: format!("track {n}"),
                clips: vec![Clip {
                    source_path: path.clone(),
                    start_in_track: 0,
                    source_offset: 0,
                    length: options.frames(rate),
                    content_hash: None,
                    time_stretch_factor: None,
                    pitch_shift_semitones: None,
                    beat_grid: None,
                    volume_envelope: Vec::new(),
                    volume_envelope_frames: session::EnvelopeFrames::default(),
                }],
                gain_db: if options.workload == Workload::Huge {
                    -2.5
                } else {
                    0.0
                },
                pan: 0.0,
                muted: false,
                soloed: false,
                effects: if options.workload == Workload::Huge {
                    vec![
                        effect("compressor", serde_json::json!({})),
                        effect("eq", serde_json::json!({})),
                    ]
                } else {
                    vec![]
                },
                sends: if options.workload == Workload::Huge && n < 150 {
                    vec![Send {
                        bus_id: buses[n / 3].id,
                        level_db: -6.0,
                    }]
                } else {
                    vec![]
                },
            }
        })
        .collect();
    let annotations = if options.workload == Workload::Huge {
        (0..1000)
            .map(|n| Annotation {
                id: AnnotationId::new(),
                name: format!("cue {n}"),
                kind: if n % 5 == 4 {
                    AnnotationKind::Region {
                        start_sec: n as f64 * 0.18,
                        end_sec: n as f64 * 0.18 + 2.0,
                    }
                } else {
                    AnnotationKind::Marker {
                        time_sec: n as f64 * 0.18,
                    }
                },
            })
            .collect()
    } else {
        vec![]
    };
    SessionState {
        tracks,
        bus_routing: BusGraph { buses },
        master_chain: vec![effect(
            "limiter",
            serde_json::json!({ "ceiling_db": LIMIT_DB }),
        )],
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: RATE,
        length_samples: options.frames(RATE),
        annotations,
        sync_lock: false,
    }
}

fn preflight_disk(dir: &Path, options: &Options) {
    let source_bytes = options.frames(RATE) * 4
        + if options.workload == Workload::Mixed {
            options.frames(44_100) * 4
        } else {
            0
        };
    let output_bytes = options.frames(RATE) * 4;
    // Reserve 1 GiB beyond source/output estimates, and allow file headers.
    let required = (source_bytes + 1024 * 1024) * options.project_count() as u64
        + (output_bytes + 1024 * 1024) * options.workers as u64
        + 1024 * 1024 * 1024;
    let output = Command::new("df")
        .arg("-Pk")
        .arg(dir)
        .output()
        .expect("disk preflight requires df");
    assert!(
        output.status.success(),
        "df failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("df output must be UTF-8");
    let available = text
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|k| k.checked_mul(1024))
        .expect("cannot determine available disk space from df -Pk");
    assert!(available >= required, "insufficient disk: need {required} bytes including 1 GiB reserve; available {available} bytes");
    println!("disk_available_bytes={available} disk_required_with_reserve_bytes={required}");
}

fn proc_memory() -> (u64, u64) {
    let status = fs::read_to_string("/proc/self/status").expect("Linux /proc/self/status required");
    let kb = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("missing {name} in /proc/self/status"))
    };
    (kb("VmRSS:"), kb("VmHWM:"))
}

fn verify(path: &Path, report: &RenderReport, frames: u64) -> u64 {
    assert_eq!(report.frames_written, frames);
    assert_eq!(report.sample_rate, RATE);
    assert_eq!(report.channels, 2);
    let mut reader = hound::WavReader::open(path).expect("open render WAV");
    let spec = reader.spec();
    assert_eq!(
        (
            spec.sample_rate,
            spec.channels,
            spec.bits_per_sample,
            spec.sample_format
        ),
        (RATE, 2, 16, hound::SampleFormat::Int)
    );
    assert_eq!(u64::from(reader.duration()), frames);
    let peak_in_first_second = reader
        .samples::<i16>()
        .take(RATE as usize * 2)
        .map(|sample| i32::from(sample.expect("read PCM sample")).abs())
        .max()
        .expect("render must contain samples");
    assert!(
        (500..=3281).contains(&peak_in_first_second),
        "rendered PCM does not show the active limiter: first-second peak {peak_in_first_second}"
    );
    let bytes = fs::metadata(path).unwrap().len();
    assert!(
        (frames * 4 + 44..=frames * 4 + 4096).contains(&bytes),
        "unexpected WAV size {bytes}"
    );
    assert!(
        report.peak_dbfs.is_finite()
            && report.peak_dbfs <= LIMIT_DB + 0.1
            && report.peak_dbfs > -60.0,
        "limiter not demonstrably active or output silent: peak {}",
        report.peak_dbfs
    );
    let mut file = File::open(path).unwrap();
    let mut buffer = [0u8; 64 * 1024];
    let mut hash = 0xcbf29ce484222325u64;
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        for byte in &buffer[..n] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    println!("output_bytes={bytes} frames={frames} rate={RATE} channels=2 bits=16 peak_dbfs={} fnv1a64={hash:016x}", report.peak_dbfs);
    hash
}

#[test]
#[ignore = "resource-intensive diagnostic; requires explicit BENCH_* configuration"]
fn measure_render_concurrency() {
    const BENCH_SECONDS: &str = "BENCH_SECONDS";
    let required = |name: &str| {
        std::env::var(name)
            .unwrap_or_else(|_| panic!("{name} is required; see test file documentation"))
    };
    let seconds = std::env::var(BENCH_SECONDS).ok();
    let options = Options::parse(
        &required("BENCH_WORKLOAD"),
        &required("BENCH_SCOPE"),
        &required("BENCH_WORKERS"),
        seconds.as_deref(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    println!("configuration={options:?} pid={} full_duration={} -- fresh process required for each point", std::process::id(), options.override_seconds.is_none());
    println!("Synthetic shared-per-project sources; not an independent-media cache model. VmHWM includes setup; /proc RSS is approximate and excludes cgroup-wide cache accounting.");
    let dir = TempDir::new().expect("private benchmark tempdir");
    preflight_disk(dir.path(), &options);
    let mut states = Vec::new();
    let mut inodes = Vec::new();
    for project in 0..options.project_count() {
        let path = dir.path().join(format!("project-{project}-48000.wav"));
        write_source(&path, RATE, options.frames(RATE));
        let mut paths = vec![path];
        if options.workload == Workload::Mixed {
            let path = dir.path().join(format!("project-{project}-44100.wav"));
            write_source(&path, 44_100, options.frames(44_100));
            paths.push(path);
        }
        inodes.push(
            paths
                .iter()
                .map(|p| {
                    let metadata = fs::metadata(p).unwrap();
                    (metadata.dev(), metadata.ino())
                })
                .collect::<Vec<_>>(),
        );
        states.push(state_for(&options, &paths));
    }
    for a in 0..inodes.len() {
        for b in 0..a {
            for inode in &inodes[a] {
                assert!(
                    !inodes[b].contains(inode),
                    "different projects must not use hardlinks"
                );
            }
        }
    }
    println!("project_count={} source_inode_pairs={inodes:?} tracks={} buses={} sends={} annotations={} master_limiter_ceiling_db={LIMIT_DB}", states.len(), states[0].tracks.len(), states[0].bus_routing.buses.len(), states[0].tracks.iter().map(|t| t.sends.len()).sum::<usize>(), states[0].annotations.len());
    let (rss_before, hwm_before) = proc_memory();
    println!("after_setup_vm_rss_kib={rss_before} after_setup_vm_hwm_kib={hwm_before}");
    let barrier = Barrier::new(options.workers + 1);
    let paths: Vec<_> = (0..options.workers)
        .map(|n| dir.path().join(format!("output-{n}.wav")))
        .collect();
    let (results, elapsed) = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..options.workers)
            .map(|n| {
                let state = &states[if options.scope == Scope::Same { 0 } else { n }];
                let path = &paths[n];
                let barrier = &barrier;
                scope.spawn(move || {
                    let engine = Engine::new();
                    barrier.wait();
                    let start = Instant::now();
                    let report = engine
                        .render_to_wav(state, path, None)
                        .expect("render failed");
                    (start.elapsed(), report)
                })
            })
            .collect();
        let start = Instant::now();
        barrier.wait();
        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("worker panicked"))
            .collect();
        (results, start.elapsed())
    });
    let (rss_after, hwm_after) = proc_memory();
    println!("batch_wall_seconds={:.6} renders_per_second={:.6} after_render_vm_rss_kib={rss_after} process_vm_hwm_kib={hwm_after}", elapsed.as_secs_f64(), options.workers as f64 / elapsed.as_secs_f64());
    let mut expected_hash = None;
    for (n, (duration, report)) in results.iter().enumerate() {
        println!(
            "worker={n} wall_seconds={:.6} warning_count={}",
            duration.as_secs_f64(),
            report.warnings.len()
        );
        if n == 0 {
            for warning in &report.warnings {
                println!("render_warning: {warning}");
            }
        }
        assert_eq!(report.warnings, results[0].1.warnings);
        let hash = verify(&paths[n], report, options.frames(RATE));
        if let Some(expected) = expected_hash {
            assert_eq!(hash, expected, "different worker outputs");
        } else {
            expected_hash = Some(hash);
        }
    }
    std::io::stdout().flush().unwrap();
}
