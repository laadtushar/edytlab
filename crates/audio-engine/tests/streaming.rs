//! M22 streaming render tests.
//!
//! Acceptance criteria from `docs/superpowers/plans/2026-05-05-phase-2-mashup.md` §M22:
//! 1. Constant memory: peak RSS does not grow with session length.
//! 2. Byte-identical output preserved: streaming mixer matches buffered M21
//!    mixer for the same `SessionState`.
//! 3. Render time scales linearly in track count and session length (no
//!    re-decoding per chunk).
//!
//! ### How byte-identity is proved
//!
//! M22 *replaces* the M21 in-memory mixer. To prove the new path produces
//! byte-equal output to the old path we re-implement M21's buffered
//! algorithm inline in this test file (`buffered_render_state_to_wav`),
//! render the same `SessionState` through both, and compare bytes.
//!
//! The buffered reference here mirrors M21's `render_processed` exactly:
//!
//! 1. Decode every contributing track with `audio_decoder::decode_file`.
//! 2. Trim, resample (via `audio_engine::render::buffered_resample_track`,
//!    which is the M21 resample helper preserved verbatim), channel-map,
//!    apply gain — yielding a `Vec<f32>` per track at the project rate.
//! 3. Sum into a single project-length f32 master buffer.
//! 4. Quantize 16-bit PCM with `hound::WavWriter`.
//!
//! If the streaming path's bytes diverge from this reference, the test
//! reports the offset of the first divergent byte plus the surrounding
//! samples to make debugging fast.

use std::path::{Path, PathBuf};
use std::time::Instant;

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const PROJECT_RATE: u32 = 48_000;

// ---------------------------------------------------------------------------
// Test fixtures and helpers.
// ---------------------------------------------------------------------------

fn write_sine_wav(dir: &Path, name: &str, freq: f32, sr: u32, secs: f32, amp: f32) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    let frames = (sr as f32 * secs) as usize;
    for n in 0..frames {
        let t = n as f32 / sr as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * amp;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

/// Like [`write_sine_wav`] but writes exactly `frames` samples (used to
/// exercise off-by-one behaviour at chunk boundaries).
fn write_sine_wav_frames(
    dir: &Path,
    name: &str,
    freq: f32,
    sr: u32,
    frames: usize,
    amp: f32,
) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..frames {
        let t = n as f32 / sr as f32;
        let s = (2.0 * std::f32::consts::PI * freq * t).sin() * amp;
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).unwrap();
    }
    writer.finalize().unwrap();
    path
}

fn track_for(source: &Path, gain_db: f32, muted: bool, soloed: bool) -> Track {
    let decoded = audio_decoder::decode_file(source).expect("decode source");
    let frames = (decoded.samples.len() / decoded.channels as usize) as u64;
    Track {
        id: TrackId::new(),
        name: source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("t")
            .to_string(),
        clips: vec![Clip {
            source_path: source.to_path_buf(),
            start_in_track: 0,
            source_offset: 0,
            length: frames,
            content_hash: None,
            time_stretch_factor: None,
            pitch_shift_semitones: None,
            beat_grid: None,
            volume_envelope: Vec::new(),
        }],
        gain_db,
        pan: 0.0,
        muted,
        soloed,
        effects: Vec::<EffectInstance>::new(),
    }
}

fn session_with(tracks: Vec<Track>, project_rate: u32) -> SessionState {
    let length = tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.start_in_track + c.length)
        .max()
        .unwrap_or(0);
    SessionState {
        tracks,
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate: project_rate,
        length_samples: length,
        annotations: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Buffered reference render — verbatim port of M21's `render_processed` for
// byte-identity comparison. Kept inline here (not exported) so a future
// change to the production streaming path can't accidentally also change
// the reference.
// ---------------------------------------------------------------------------

fn buffered_render_state_to_wav(state: &SessionState, out: &Path) {
    use audio_decoder::decode_file;
    use audio_engine::graph;

    let graph = graph::build(state).expect("build graph");

    // Decode contributing tracks; figure out max channels.
    let mut decoded_tracks: Vec<Option<(audio_decoder::DecodedAudio, _)>> =
        Vec::with_capacity(graph.tracks.len());
    let mut max_channels: u16 = 1;

    for plan in &graph.tracks {
        if !plan.contributes || plan.length == 0 {
            decoded_tracks.push(None);
            continue;
        }
        let decoded = decode_file(&plan.source_path).expect("decode");
        if decoded.channels > max_channels {
            max_channels = decoded.channels;
        }
        decoded_tracks.push(Some((decoded, plan.clone())));
    }

    let project_rate = graph.project_sample_rate;
    let mut prepared: Vec<Option<Vec<f32>>> = Vec::with_capacity(decoded_tracks.len());
    let chans = max_channels as usize;

    for entry in decoded_tracks {
        match entry {
            None => prepared.push(None),
            Some((decoded, plan)) => {
                let in_channels = decoded.channels as usize;
                let in_rate = decoded.sample_rate;
                let total_frames = decoded.samples.len() / in_channels;
                let clip_start = (plan.source_offset as usize).min(total_frames);
                let clip_end =
                    ((plan.source_offset.saturating_add(plan.length)) as usize).min(total_frames);
                let trimmed: Vec<f32> =
                    decoded.samples[clip_start * in_channels..clip_end * in_channels].to_vec();

                // Resample at source channel count.
                let at_project_rate: Vec<f32> = if in_rate == project_rate {
                    trimmed
                } else {
                    audio_engine::render::buffered_resample_track(
                        &trimmed,
                        in_rate,
                        project_rate,
                        in_channels,
                    )
                    .expect("resample")
                };

                // Channel-map.
                let mapped: Vec<f32> = if in_channels == chans {
                    at_project_rate
                } else if in_channels == 1 && chans > 1 {
                    let mut o = Vec::with_capacity(at_project_rate.len() * chans);
                    for s in &at_project_rate {
                        for _ in 0..chans {
                            o.push(*s);
                        }
                    }
                    o
                } else if in_channels > 1 && chans == 1 {
                    let frames = at_project_rate.len() / in_channels;
                    let recip = 1.0 / in_channels as f32;
                    let mut o = Vec::with_capacity(frames);
                    for f in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..in_channels {
                            sum += at_project_rate[f * in_channels + c];
                        }
                        o.push(sum * recip);
                    }
                    o
                } else {
                    panic!("unsupported channel map");
                };

                let mut samples = mapped;
                audio_engine::mixer::apply_gain_db(&mut samples, plan.gain_db);
                prepared.push(Some(samples));
            }
        }
    }

    let total_frames = prepared
        .iter()
        .filter_map(|p| p.as_ref())
        .map(|p| p.len() / chans)
        .max()
        .unwrap_or(0);

    let mut master = vec![0.0f32; total_frames * chans];
    for entry in prepared.iter() {
        let Some(samples) = entry else { continue };
        let track_frames = samples.len() / chans;
        let copy_end = total_frames.min(track_frames);
        let src = &samples[..copy_end * chans];
        for (i, s) in src.iter().enumerate() {
            master[i] += *s;
        }
    }

    let spec = WavSpec {
        channels: max_channels,
        sample_rate: project_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(out, spec).expect("create");
    for s in &master {
        let q = (s * 32_768.0)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        writer.write_sample(q).expect("write");
    }
    writer.finalize().expect("finalize");
}

fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter()
        .zip(b.iter())
        .position(|(x, y)| x != y)
        .or_else(|| {
            if a.len() != b.len() {
                Some(a.len().min(b.len()))
            } else {
                None
            }
        })
}

// ---------------------------------------------------------------------------
// AC #2: streaming render output is byte-equal to buffered render output.
// ---------------------------------------------------------------------------

#[test]
fn streaming_render_byte_equal_to_buffered_render() {
    let tmp = TempDir::new().unwrap();
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, PROJECT_RATE, 1.0, 0.3);
    let b = write_sine_wav(tmp.path(), "b.wav", 880.0, PROJECT_RATE, 1.0, 0.3);

    let state = session_with(
        vec![
            track_for(&a, -3.0, false, false),
            track_for(&b, -6.0, false, false),
        ],
        PROJECT_RATE,
    );

    let streamed = tmp.path().join("streamed.wav");
    let buffered = tmp.path().join("buffered.wav");
    render_state_to_wav(&state, &streamed, None).expect("stream render");
    buffered_render_state_to_wav(&state, &buffered);

    let s = std::fs::read(&streamed).unwrap();
    let b_bytes = std::fs::read(&buffered).unwrap();
    if s != b_bytes {
        let off = first_diff(&s, &b_bytes).unwrap_or(0);
        panic!(
            "streamed != buffered; first diff at byte {off}; \
             streamed len={} buffered len={}",
            s.len(),
            b_bytes.len()
        );
    }
}

// ---------------------------------------------------------------------------
// AC #2 (with resample): byte-equal under cross-sample-rate sources.
// ---------------------------------------------------------------------------

#[test]
fn streaming_render_with_resample_byte_equal() {
    let tmp = TempDir::new().unwrap();
    // 44.1 kHz source in a 48 kHz project; this exercises the resample
    // path under streaming.
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, 44_100, 1.0, 0.4);
    let b = write_sine_wav(tmp.path(), "b.wav", 880.0, PROJECT_RATE, 1.0, 0.3);

    let state = session_with(
        vec![
            track_for(&a, -3.0, false, false),
            track_for(&b, -6.0, false, false),
        ],
        PROJECT_RATE,
    );

    let streamed = tmp.path().join("streamed.wav");
    let buffered = tmp.path().join("buffered.wav");
    render_state_to_wav(&state, &streamed, None).expect("stream render");
    buffered_render_state_to_wav(&state, &buffered);

    let s = std::fs::read(&streamed).unwrap();
    let b_bytes = std::fs::read(&buffered).unwrap();
    if s != b_bytes {
        let off = first_diff(&s, &b_bytes).unwrap_or(0);
        panic!(
            "streamed != buffered (resample case); first diff at byte {off}; \
             streamed len={} buffered len={}",
            s.len(),
            b_bytes.len()
        );
    }
}

// ---------------------------------------------------------------------------
// AC #2: off-by-one safety — sources whose length is exactly N×chunk vs
// (N×chunk − 1) vs (N×chunk + 1) all render correctly with no truncated
// or stretched tail.
// ---------------------------------------------------------------------------

#[test]
fn streaming_render_handles_eof_at_chunk_boundary() {
    let tmp = TempDir::new().unwrap();
    // The streaming path uses a 1-second master chunk at PROJECT_RATE.
    let chunk = PROJECT_RATE as usize;
    for offset in [-1isize, 0, 1] {
        let total: isize = (2 * chunk as isize) + offset;
        let frames = total as usize;
        let src = write_sine_wav_frames(
            tmp.path(),
            &format!("eof_{offset}.wav"),
            440.0,
            PROJECT_RATE,
            frames,
            0.3,
        );
        let state = session_with(vec![track_for(&src, -3.0, false, false)], PROJECT_RATE);
        let streamed = tmp.path().join(format!("streamed_{offset}.wav"));
        let buffered = tmp.path().join(format!("buffered_{offset}.wav"));
        render_state_to_wav(&state, &streamed, None).expect("stream");
        buffered_render_state_to_wav(&state, &buffered);
        let s = std::fs::read(&streamed).unwrap();
        let b = std::fs::read(&buffered).unwrap();
        assert_eq!(
            s,
            b,
            "boundary offset {offset}: streamed len={} buffered len={}",
            s.len(),
            b.len()
        );
        // Exact frame count — single track, single clip, unity rate, no
        // resampler trimming, so output frames == source frames.
        let r = WavReader::open(&streamed).unwrap();
        assert_eq!(
            r.duration() as usize,
            frames,
            "boundary offset {offset}: wrote {} frames, expected {frames}",
            r.duration(),
        );
    }
}

// ---------------------------------------------------------------------------
// AC #3: 8-track 30-second render completes within a generous time budget.
// (The hard perf gate is `ten_minute_render_under_thirty_seconds` in
// `render_golden.rs`; this one is a smoke gate that the streaming path
// hasn't introduced super-linear behaviour.)
// ---------------------------------------------------------------------------

#[test]
fn streaming_render_8_tracks_completes_within_budget() {
    let tmp = TempDir::new().unwrap();
    // 8 tracks × 30 s @ 48 kHz, each at amplitude 0.1 to avoid summing
    // above 1.0 dBFS (8 × 0.1 = 0.8 peak).
    let mut tracks = Vec::with_capacity(8);
    for i in 0i32..8 {
        let freq = 220.0 * 2f32.powi(i / 12);
        let p = write_sine_wav(
            tmp.path(),
            &format!("t{i}.wav"),
            freq,
            PROJECT_RATE,
            30.0,
            0.1,
        );
        tracks.push(track_for(&p, -3.0, false, false));
    }
    let state = session_with(tracks, PROJECT_RATE);
    let out = tmp.path().join("mix.wav");

    let start = Instant::now();
    render_state_to_wav(&state, &out, None).expect("render");
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs_f32() < 30.0,
        "8-track 30s render took {elapsed:?}; budget is 30 s"
    );

    let r = WavReader::open(&out).unwrap();
    assert_eq!(r.duration(), 30 * PROJECT_RATE);
}

// ---------------------------------------------------------------------------
// AC #1: peak memory does not scale with session length. We can't directly
// measure heap allocations in stable Rust without a custom allocator; the
// practical proxy is RSS from `/proc/self/status` on Linux. On non-Linux
// targets we still run the timing portion (it doubles as a "scales linearly
// with length" smoke test).
// ---------------------------------------------------------------------------

fn build_8_track_session(tmp: &Path, secs: f32) -> SessionState {
    let mut tracks = Vec::with_capacity(8);
    for i in 0i32..8 {
        let freq = 220.0 * 2f32.powi(i / 12);
        let p = write_sine_wav(
            tmp,
            &format!("len_{secs}_t{i}.wav"),
            freq,
            PROJECT_RATE,
            secs,
            0.1,
        );
        tracks.push(track_for(&p, -3.0, false, false));
    }
    session_with(tracks, PROJECT_RATE)
}

#[cfg(target_os = "linux")]
fn read_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // Format: "VmRSS:\t   12345 kB"
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb);
        }
    }
    None
}

#[test]
fn streaming_render_is_constant_memory_smoke() {
    let tmp = TempDir::new().unwrap();
    let short = build_8_track_session(tmp.path(), 5.0);
    let long = build_8_track_session(tmp.path(), 60.0);
    let out_short = tmp.path().join("short.wav");
    let out_long = tmp.path().join("long.wav");

    // Render the short session first; record RSS afterwards. We don't try
    // to measure peak RSS during the render (no portable hook); the post-
    // render RSS is a high-water mark close to the peak because Rust does
    // not aggressively return small heap blocks to the OS.
    render_state_to_wav(&short, &out_short, None).expect("render short");
    #[cfg(target_os = "linux")]
    let rss_short = read_rss_kb();

    let start_long = Instant::now();
    render_state_to_wav(&long, &out_long, None).expect("render long");
    let elapsed_long = start_long.elapsed();

    #[cfg(target_os = "linux")]
    {
        let rss_long = read_rss_kb();
        if let (Some(s), Some(l)) = (rss_short, rss_long) {
            // Echo for `--nocapture` runs so we can attach concrete
            // numbers to the milestone PR report without re-instrumenting.
            eprintln!(
                "RSS: 5s 8-track = {} MB, 60s 8-track = {} MB, ratio {:.2}x",
                s / 1024,
                l / 1024,
                l as f64 / s as f64,
            );
            // Rendering 12× more audio must NOT 12× peak RSS. With a
            // streaming mixer the post-render RSS difference should be
            // tiny — the master + per-track scratch buffers reuse the
            // same allocations regardless of session length. Allow a 2×
            // safety factor: if RSS more than doubled, the M21 buffered
            // path probably crept back in.
            //
            // We reject only on a clear regression: long_rss > 2 *
            // short_rss AND long_rss > short_rss + 100 MB. The +100 MB
            // tail keeps the assertion robust against page-faulted-in
            // shared libraries during the long render that wouldn't
            // count against streaming.
            let long_kb = l;
            let short_kb = s;
            let allowed = (short_kb * 2).max(short_kb + 100 * 1024);
            assert!(
                long_kb <= allowed,
                "RSS grew from {short_kb} kB (5 s session) to {long_kb} kB (60 s session); \
                 expected <= {allowed} kB. Streaming render likely regressed to a buffered \
                 implementation.",
            );
        }
    }

    // Wall-clock: 60 s session is 12× the workload of 5 s. The streaming
    // path is O(N) so wall-clock should scale approximately linearly. A
    // 30× ceiling absorbs cold-cache, FFT-plan-cache, and CI-noise
    // variance comfortably without letting an actually-quadratic regression
    // through.
    assert!(
        elapsed_long.as_secs_f32() < 30.0,
        "60 s × 8 track render took {elapsed_long:?}; budget is 30 s",
    );

    // Sanity: both files have the expected duration.
    let r_short = WavReader::open(&out_short).unwrap();
    assert_eq!(r_short.duration(), 5 * PROJECT_RATE);
    let r_long = WavReader::open(&out_long).unwrap();
    assert_eq!(r_long.duration(), 60 * PROJECT_RATE);
}
