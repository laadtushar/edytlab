//! Multi-track render tests (M21).
//!
//! Acceptance criteria from `docs/superpowers/plans/2026-05-05-phase-2-mashup.md` §M21:
//! 1. Two source files mix per-sample-sum (with each track's gain applied).
//! 2. `muted` / `soloed` flags honored: solo overrides mute; if any track
//!    is soloed, non-soloed tracks are silent.
//! 3. Tracks at different sample rates auto-resample to the project rate.
//! 4. Multi-track render is byte-deterministic across runs.

use std::path::{Path, PathBuf};

use audio_engine::render_state_to_wav;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use realfft::num_complex::Complex;
use realfft::RealFftPlanner;
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

const PROJECT_RATE: u32 = 48_000;

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
        sends: Vec::new(),
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

fn read_f32_mono(path: &Path) -> (Vec<f32>, u32) {
    let mut reader = WavReader::open(path).expect("open out");
    let sr = reader.spec().sample_rate;
    let chans = reader.spec().channels as usize;
    let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
    // Downmix to mono if needed.
    let frames = samples.len() / chans;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0.0f32;
        for c in 0..chans {
            sum += samples[f * chans + c] as f32 / 32_768.0;
        }
        out.push(sum / chans as f32);
    }
    (out, sr)
}

/// Magnitude spectrum (single-sided) for `signal` at `sr`. Returns frequency
/// bins in Hz and corresponding magnitudes. Uses a power-of-two window of
/// length 2^k that fits in the signal.
fn spectrum(signal: &[f32], sr: u32) -> (Vec<f32>, Vec<f32>) {
    let n = {
        let mut p = 1;
        while p * 2 <= signal.len() {
            p *= 2;
        }
        p
    };
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let mut input = signal[..n].to_vec();
    let mut output: Vec<Complex<f32>> = vec![Complex { re: 0.0, im: 0.0 }; n / 2 + 1];
    fft.process(&mut input, &mut output).unwrap();

    let mags: Vec<f32> = output.iter().map(|c| c.norm()).collect();
    let freqs: Vec<f32> = (0..mags.len())
        .map(|k| k as f32 * sr as f32 / n as f32)
        .collect();
    (freqs, mags)
}

/// Find the dominant peak (frequency, magnitude) within `[lo, hi]` Hz.
fn peak_in(freqs: &[f32], mags: &[f32], lo: f32, hi: f32) -> (f32, f32) {
    let mut best = (0.0f32, 0.0f32);
    for (i, &f) in freqs.iter().enumerate() {
        if f >= lo && f <= hi && mags[i] > best.1 {
            best = (f, mags[i]);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Acceptance #1: 440 Hz + 880 Hz mix shows both peaks in the spectrum.
// ---------------------------------------------------------------------------

#[test]
fn two_tone_mixdown_has_both_spectral_peaks() {
    let tmp = TempDir::new().unwrap();
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, PROJECT_RATE, 1.0, 0.4);
    let b = write_sine_wav(tmp.path(), "b.wav", 880.0, PROJECT_RATE, 1.0, 0.4);

    let state = session_with(
        vec![
            track_for(&a, 0.0, false, false),
            track_for(&b, 0.0, false, false),
        ],
        PROJECT_RATE,
    );
    let out = tmp.path().join("mix.wav");
    let report = render_state_to_wav(&state, &out, None).expect("render");
    assert_eq!(report.sample_rate, PROJECT_RATE);
    assert_eq!(report.frames_written, PROJECT_RATE as u64);

    let (signal, sr) = read_f32_mono(&out);
    let (freqs, mags) = spectrum(&signal, sr);

    // Bin width @ 32k FFT, 48 kHz sr ≈ 1.46 Hz.
    let (f440, m440) = peak_in(&freqs, &mags, 430.0, 450.0);
    let (f880, m880) = peak_in(&freqs, &mags, 870.0, 890.0);
    let (_floor_f, m_floor) = peak_in(&freqs, &mags, 1500.0, 2000.0);

    assert!(
        (f440 - 440.0).abs() < 2.0,
        "440 Hz peak at {f440} Hz (mag {m440})"
    );
    assert!(
        (f880 - 880.0).abs() < 2.0,
        "880 Hz peak at {f880} Hz (mag {m880})"
    );
    // Both peaks should dwarf out-of-band noise floor.
    assert!(
        m440 > m_floor * 10.0 && m880 > m_floor * 10.0,
        "expected both peaks to dominate noise floor; m440={m440} m880={m880} floor={m_floor}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance #2a: solo silences other tracks.
// ---------------------------------------------------------------------------

#[test]
fn solo_silences_other_tracks() {
    let tmp = TempDir::new().unwrap();
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, PROJECT_RATE, 1.0, 0.4);
    let b = write_sine_wav(tmp.path(), "b.wav", 880.0, PROJECT_RATE, 1.0, 0.4);

    let state = session_with(
        vec![
            track_for(&a, 0.0, false, true), // solo'd
            track_for(&b, 0.0, false, false),
        ],
        PROJECT_RATE,
    );
    let out = tmp.path().join("mix.wav");
    render_state_to_wav(&state, &out, None).expect("render");

    let (signal, sr) = read_f32_mono(&out);
    let (freqs, mags) = spectrum(&signal, sr);
    let (_, m440) = peak_in(&freqs, &mags, 430.0, 450.0);
    let (_, m880) = peak_in(&freqs, &mags, 870.0, 890.0);
    assert!(m440 > 0.0, "solo'd 440 track must contribute");
    // The non-solo'd track must contribute zero — its peak shouldn't even
    // be visible above the rest of the spectrum.
    assert!(
        m880 < m440 * 0.01,
        "non-solo track leaked: m880={m880} m440={m440}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance #2b: mute silences only the muted track when no solo is set.
// ---------------------------------------------------------------------------

#[test]
fn mute_silences_only_muted() {
    let tmp = TempDir::new().unwrap();
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, PROJECT_RATE, 1.0, 0.4);
    let b = write_sine_wav(tmp.path(), "b.wav", 880.0, PROJECT_RATE, 1.0, 0.4);

    let state = session_with(
        vec![
            track_for(&a, 0.0, true, false), // muted
            track_for(&b, 0.0, false, false),
        ],
        PROJECT_RATE,
    );
    let out = tmp.path().join("mix.wav");
    render_state_to_wav(&state, &out, None).expect("render");

    let (signal, sr) = read_f32_mono(&out);
    let (freqs, mags) = spectrum(&signal, sr);
    let (_, m440) = peak_in(&freqs, &mags, 430.0, 450.0);
    let (_, m880) = peak_in(&freqs, &mags, 870.0, 890.0);
    assert!(
        m880 > 0.0,
        "non-muted 880 Hz track must contribute (m880={m880})"
    );
    assert!(
        m440 < m880 * 0.01,
        "muted track leaked: m440={m440} m880={m880}"
    );
}

// ---------------------------------------------------------------------------
// Acceptance #3: 44.1 kHz source rendered at 48 kHz project rate is
// resampled (no clipping or huge discontinuities).
// ---------------------------------------------------------------------------

#[test]
fn cross_sample_rate_render_resamples_to_project_rate() {
    let tmp = TempDir::new().unwrap();
    // Single 440 Hz sine at 44.1 kHz, project rate 48 kHz, length 1 s.
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, 44_100, 1.0, 0.5);

    // gain != 0 to force the f32 path; otherwise the unity-passthrough
    // gate would refuse anyway because rates differ, but an explicit
    // mismatch keeps the intent clear.
    let state = session_with(vec![track_for(&a, -3.0, false, false)], PROJECT_RATE);
    let out = tmp.path().join("up.wav");
    let report = render_state_to_wav(&state, &out, None).expect("render");

    assert_eq!(report.sample_rate, PROJECT_RATE);
    // Resampler may emit ±1 frame depending on chunk boundaries, but the
    // length must match the project-rate expectation to within 0.5 %.
    let expected = PROJECT_RATE as i64;
    let diff = (report.frames_written as i64 - expected).abs();
    assert!(
        diff < (expected / 200),
        "expected ~{expected} frames at 48 kHz, got {}",
        report.frames_written,
    );

    // RMS sanity: a sine at amplitude ≈ 0.5 with a -3 dB gain ≈ 0.354.
    // RMS = amp/sqrt(2) ≈ 0.25. Allow +/- 30 % tolerance for resampler
    // ringing and quantization.
    let (signal, _sr) = read_f32_mono(&out);
    let rms = (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt();
    assert!(
        rms > 0.18 && rms < 0.34,
        "expected RMS ~0.25 after resample+gain, got {rms}"
    );

    // No clipping: peak must not saturate.
    let peak = signal.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak < 0.99, "resampled output clipped at peak {peak}");
}

// ---------------------------------------------------------------------------
// Acceptance #4: multi-track render is byte-deterministic across runs.
// ---------------------------------------------------------------------------

#[test]
fn multitrack_render_is_byte_deterministic() {
    let tmp = TempDir::new().unwrap();
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, PROJECT_RATE, 1.0, 0.3);
    let b = write_sine_wav(tmp.path(), "b.wav", 880.0, PROJECT_RATE, 1.0, 0.3);
    let c = write_sine_wav(tmp.path(), "c.wav", 1320.0, 44_100, 1.0, 0.3);

    let state = session_with(
        vec![
            track_for(&a, -3.0, false, false),
            track_for(&b, -6.0, false, false),
            track_for(&c, 0.0, false, false), // exercises the resample path
        ],
        PROJECT_RATE,
    );

    let first = tmp.path().join("first.wav");
    render_state_to_wav(&state, &first, None).expect("render 1");
    let baseline = std::fs::read(&first).expect("read 1");

    for i in 0..10 {
        let p = tmp.path().join(format!("run_{i}.wav"));
        render_state_to_wav(&state, &p, None).expect("render n");
        let bytes = std::fs::read(&p).expect("read n");
        assert_eq!(bytes, baseline, "run {i} differs from baseline");
    }
}

// ---------------------------------------------------------------------------
// Per-sample-sum sanity: mixing two correlated sines (both 440 Hz, in
// phase) yields a doubled peak, while mixing 440 + 880 doesn't peak as
// strongly. Pins down "actually summing" rather than "max of the two".
// ---------------------------------------------------------------------------

#[test]
fn correlated_tracks_sum_to_double_amplitude() {
    let tmp = TempDir::new().unwrap();
    let a = write_sine_wav(tmp.path(), "a.wav", 440.0, PROJECT_RATE, 1.0, 0.4);
    let state_one = session_with(vec![track_for(&a, 0.0, false, false)], PROJECT_RATE);
    let state_two = session_with(
        vec![
            track_for(&a, 0.0, false, false),
            track_for(&a, 0.0, false, false),
        ],
        PROJECT_RATE,
    );

    let out_one = tmp.path().join("one.wav");
    let out_two = tmp.path().join("two.wav");
    render_state_to_wav(&state_one, &out_one, None).expect("render one");
    render_state_to_wav(&state_two, &out_two, None).expect("render two");

    let (s1, _) = read_f32_mono(&out_one);
    let (s2, _) = read_f32_mono(&out_two);

    let p1 = s1.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let p2 = s2.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

    // p2 should be ~2x p1 (modulo i16 saturation if amp > 0.5; we used 0.4
    // so 2x = 0.8, well below the i16 ceiling).
    let ratio = p2 / p1;
    assert!(
        (ratio - 2.0).abs() < 0.05,
        "expected p2/p1 ≈ 2.0, got {ratio} (p1={p1} p2={p2})"
    );
}
