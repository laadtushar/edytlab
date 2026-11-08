//! Fixtures the render tests share: the WAVs they write, the one-track
//! sessions they build and the PCM they read back.
//!
//! Each render test used to carry its own copy of the same four things — a
//! tone written with `hound`, a `SessionState` spelled out field by field, a
//! render into a temp folder, and a read of the result. A test binary
//! includes this with `mod support;` and uses a subset. A test whose session
//! is shaped differently (several tracks, buses, sends) builds it from
//! [`plain_track`] and [`session_of`] and sets the fields it cares about.
#![allow(dead_code)]

use std::f32::consts::PI;
use std::path::{Path, PathBuf};

use audio_engine::{render_state_to_wav, RenderReport, TimeRange};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use session::{BusGraph, Clip, EffectInstance, SessionState, TempoMap, Track, TrackId};
use tempfile::TempDir;

// --- WAV files -------------------------------------------------------------

/// Write `frames` frames to `dir/name`, each `sample(n)` repeated on every
/// one of `channels` channels, as 16-bit PCM.
pub fn write_wav_i16(
    dir: &Path,
    name: &str,
    rate: u32,
    channels: u16,
    frames: usize,
    mut sample: impl FnMut(usize) -> i16,
) -> PathBuf {
    let path = dir.join(name);
    let spec = WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec).expect("wav writer");
    for n in 0..frames {
        let q = sample(n);
        for _ in 0..channels {
            writer.write_sample(q).unwrap();
        }
    }
    writer.finalize().unwrap();
    path
}

/// Sample `n` of a sine of `freq` Hz and peak `amp` at `rate`, as a float.
pub fn sine_at(n: usize, freq: f32, amp: f32, rate: u32) -> f32 {
    let t = n as f32 / rate as f32;
    (2.0 * PI * freq * t).sin() * amp
}

/// A float sample as 16-bit PCM, scaled by 32 767 and truncated.
pub fn pcm_trunc(s: f32) -> i16 {
    (s * 32_767.0) as i16
}

/// A float sample as 16-bit PCM, scaled by 32 768, rounded and clamped.
pub fn pcm_round(s: f32) -> i16 {
    (s * 32_768.0)
        .round()
        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

/// A mono sine, `frames` long, quantised by [`pcm_trunc`].
pub fn write_sine_trunc(
    dir: &Path,
    name: &str,
    rate: u32,
    frames: usize,
    freq: f32,
    amp: f32,
) -> PathBuf {
    write_wav_i16(dir, name, rate, 1, frames, |n| {
        pcm_trunc(sine_at(n, freq, amp, rate))
    })
}

/// A sine on `channels` channels, `frames` long, quantised by [`pcm_round`].
pub fn write_sine_round(
    dir: &Path,
    name: &str,
    rate: u32,
    channels: u16,
    frames: usize,
    freq: f32,
    amp: f32,
) -> PathBuf {
    write_wav_i16(dir, name, rate, channels, frames, |n| {
        pcm_round(sine_at(n, freq, amp, rate))
    })
}

/// A mono sine of `frames` frames at `rate`, quantised by [`pcm_round`].
pub fn write_sine_wav_frames(
    dir: &Path,
    name: &str,
    freq: f32,
    rate: u32,
    frames: usize,
    amp: f32,
) -> PathBuf {
    write_sine_round(dir, name, rate, 1, frames, freq, amp)
}

/// A mono sine of `secs` seconds at `rate`, quantised by [`pcm_round`].
pub fn write_sine_wav(
    dir: &Path,
    name: &str,
    freq: f32,
    rate: u32,
    secs: f32,
    amp: f32,
) -> PathBuf {
    write_sine_wav_frames(dir, name, freq, rate, (rate as f32 * secs) as usize, amp)
}

// --- sessions --------------------------------------------------------------

/// A clip of `length` frames of `source` placed at `start_in_track`, with
/// nothing else on it.
pub fn unity_clip(source: &Path, start_in_track: u64, source_offset: u64, length: u64) -> Clip {
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
        volume_envelope_frames: session::EnvelopeFrames::default(),
    }
}

/// A track holding `clips`: unity gain, centred, audible, no chain, no sends.
pub fn plain_track(name: &str, clips: Vec<Clip>) -> Track {
    Track {
        id: TrackId::new(),
        name: name.into(),
        clips,
        gain_db: 0.0,
        pan: 0.0,
        muted: false,
        soloed: false,
        effects: Vec::new(),
        sends: Vec::new(),
    }
}

/// A session of `tracks` at `sample_rate`, `length_samples` long, with no
/// buses, no master chain and everything else at its default.
pub fn session_of(tracks: Vec<Track>, sample_rate: u32, length_samples: u64) -> SessionState {
    SessionState {
        tracks,
        bus_routing: BusGraph::default(),
        master_chain: Vec::new(),
        tempo_map: TempoMap::default(),
        key_map: None,
        transcript: None,
        sample_rate,
        length_samples,
        annotations: Vec::new(),
        sync_lock: false,
    }
}

/// [`session_of`] with the length the furthest clip end gives.
pub fn session_to_end(tracks: Vec<Track>, sample_rate: u32) -> SessionState {
    let length = tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.start_in_track + c.length)
        .max()
        .unwrap_or(0);
    session_of(tracks, sample_rate, length)
}

/// The frames and rate of `source` as the decoder reads it.
pub fn decoded_frames(source: &Path) -> (u64, u32) {
    let decoded = audio_decoder::decode_file(source).expect("decode source");
    let frames = (decoded.samples.len() / decoded.channels as usize) as u64;
    (frames, decoded.sample_rate)
}

/// One track `t1` with one clip over the whole of `source`, at the
/// source's own rate.
pub fn single_track_state(source: &Path) -> SessionState {
    let (frames, rate) = decoded_frames(source);
    session_of(
        vec![plain_track("t1", vec![unity_clip(source, 0, 0, frames)])],
        rate,
        frames,
    )
}

/// The track of [`single_track_state`], for a test that sets its fields.
pub fn only_track(state: &mut SessionState) -> &mut Track {
    &mut state.tracks[0]
}

/// A track for `source` named after its file, one clip over all of it.
pub fn track_for(source: &Path, gain_db: f32, muted: bool, soloed: bool) -> Track {
    let (frames, _) = decoded_frames(source);
    let name = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("t")
        .to_string();
    let mut track = plain_track(&name, vec![unity_clip(source, 0, 0, frames)]);
    track.gain_db = gain_db;
    track.muted = muted;
    track.soloed = soloed;
    track
}

// --- effects ---------------------------------------------------------------

pub fn effect(kind: &str, params: serde_json::Value) -> EffectInstance {
    EffectInstance {
        kind: kind.to_string(),
        params,
        bypassed: false,
    }
}

pub fn bypassed(kind: &str, params: serde_json::Value) -> EffectInstance {
    EffectInstance {
        bypassed: true,
        ..effect(kind, params)
    }
}

pub fn gain(db: f32) -> EffectInstance {
    effect("gain", serde_json::json!({ "db": db }))
}

pub fn low_pass(cutoff_hz: f32) -> EffectInstance {
    effect(
        "low_pass_filter",
        serde_json::json!({ "cutoff_hz": cutoff_hz }),
    )
}

// --- rendering and reading back -------------------------------------------

/// The 16-bit samples of the WAV at `path`, channels interleaved.
pub fn read_i16(path: &Path) -> Vec<i16> {
    WavReader::open(path)
        .expect("open wav")
        .samples::<i16>()
        .map(|s| s.expect("sample"))
        .collect()
}

/// Render `state` (all of it, or `range`) into a temp folder and return the
/// report with the PCM.
pub fn render_ranged(state: &SessionState, range: Option<TimeRange>) -> (RenderReport, Vec<i16>) {
    let tmp = TempDir::new().expect("tempdir");
    let out = tmp.path().join("out.wav");
    let report = render_state_to_wav(state, &out, range).expect("render");
    (report, read_i16(&out))
}

/// The PCM of a render of the whole of `state`.
pub fn render_i16(state: &SessionState) -> Vec<i16> {
    render_ranged(state, None).1
}

pub fn rms(x: &[i16]) -> f32 {
    (x.iter().map(|v| (*v as f32).powi(2)).sum::<f32>() / x.len() as f32).sqrt()
}

/// Largest magnitude as a fraction of full scale.
pub fn peak(x: &[i16]) -> f32 {
    x.iter().map(|v| (*v as f32).abs()).fold(0.0, f32::max) / 32_767.0
}

/// Largest magnitude in LSB.
pub fn peak_lsb(x: &[i16]) -> i32 {
    x.iter().map(|s| (*s as i32).abs()).max().unwrap_or(0)
}
