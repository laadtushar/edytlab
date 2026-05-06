//! Audio output abstraction for edytlab (Phase 1, M03).
//!
//! Wraps `cpal` with a small, blocking-friendly producer API on top of a
//! lock-free ring buffer that feeds the audio callback. The callback runs on
//! the OS audio thread and never allocates, locks, or blocks; on starvation
//! it writes silence and advances the played-frames counter so the rest of
//! the pipeline keeps a coherent timeline.
//!
//! ## Platform notes
//!
//! - **macOS / Core Audio**: default HAL output, sample format negotiated at
//!   open time.
//! - **Windows / WASAPI shared mode**: this is the only WASAPI mode Phase 1
//!   targets. Shared mode imposes a ~10–20 ms latency floor depending on the
//!   Windows audio engine period; that is acceptable for non-realtime preview
//!   playback. WASAPI exclusive mode and `IAudioClient3` low-latency paths
//!   are deliberately out of scope.
//! - **Linux / ALSA**: not a Phase 1 target. The crate will compile on Linux
//!   if `libasound2-dev` is installed, but is otherwise unsupported.
//!
//! ## Sample-rate handling
//!
//! Callers push samples at a *logical* rate (the rate of the audio they are
//! decoding/rendering). If the device's preferred sample rate differs, an
//! `FftFixedInOut` resampler from `rubato 0.16` is inserted transparently at
//! the engine→I/O boundary. Mismatches are logged at `debug` level — they
//! are not surfaced as errors.

#[cfg(target_os = "macos")]
mod coreaudio;
#[cfg(target_os = "windows")]
mod wasapi;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, SampleFormat, SampleRate, SizedSample, Stream, StreamConfig,
};
use ringbuf::{traits::Split, HeapRb};
use rubato::{FftFixedInOut, Resampler};
use thiserror::Error;
use tracing::debug;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no default output device available")]
    NoDevice,
    #[error("no supported output config matches request (channels={channels}): {source}")]
    UnsupportedConfig {
        channels: u16,
        #[source]
        source: cpal::SupportedStreamConfigsError,
    },
    #[error("requested channel count {0} not offered by device")]
    UnsupportedChannelCount(u16),
    #[error("cpal build_output_stream failed: {0}")]
    BuildStream(#[from] cpal::BuildStreamError),
    #[error("cpal stream play failed: {0}")]
    Play(#[from] cpal::PlayStreamError),
    #[error("cpal stream pause failed: {0}")]
    Pause(#[from] cpal::PauseStreamError),
    #[error("rubato resampler construction failed: {0}")]
    ResamplerInit(#[from] rubato::ResamplerConstructionError),
    #[error("rubato resampler process failed: {0}")]
    ResamplerProcess(#[from] rubato::ResampleError),
    #[error(
        "interleaved sample buffer length {len} is not a multiple of channel count {channels}"
    )]
    UnalignedFrames { len: usize, channels: u16 },
    #[error("device sample format {0:?} is not supported by audio-io")]
    UnsupportedSampleFormat(SampleFormat),
}

/// An open output stream.
///
/// Threading rules:
/// - `play` and `pause` touch the underlying `cpal::Stream` and therefore
///   inherit its thread-affinity constraints (Windows COM apartment / macOS
///   HAL); they MUST be called on the same thread that constructed the
///   stream. The same is true for `Drop`.
/// - `write_samples`, `frames_played`, and `device_sample_rate` only touch
///   the lock-free ringbuf, the atomic counter, and a copy of the device
///   sample rate. They are safe to call from any thread.
///
/// The unsafe `Send` impl on the concrete type is sound only because the
/// engine upholds rule 1; M11 will wrap this in an `AudioCoordinator` that
/// owns the stream on a dedicated thread and exposes a fully Send+Sync
/// API via mpsc.
pub trait OutputStream: Send {
    /// Resume audio output. Must be called on the constructing thread.
    fn play(&mut self) -> Result<()>;
    /// Pause audio output. Must be called on the constructing thread.
    fn pause(&mut self) -> Result<()>;
    /// Push interleaved frames at the *logical* sample rate the stream was
    /// opened with. Resampling to the device rate (if any) is transparent.
    ///
    /// Samples are silently dropped when the internal ring buffer is full;
    /// callers must pace themselves using `frames_played` rather than relying
    /// on backpressure here.
    fn write_samples(&mut self, samples: &[f32]) -> Result<()>;
    /// Number of per-channel frames the audio callback has emitted to the
    /// device, including silence frames written on underrun.
    fn frames_played(&self) -> u64;
    /// Device sample rate the stream actually opened at. When this differs
    /// from the rate passed to `default_output`, an internal resampler is
    /// engaged on the producer side.
    fn device_sample_rate(&self) -> u32;
}

/// Logical handle to the host's default output device. Kept as a separate
/// trait so callers can swap in a fake in tests if needed.
pub trait OutputDevice {
    fn open(&self, sample_rate: u32, channels: u16) -> Result<Box<dyn OutputStream>>;
}

pub struct DefaultOutputDevice;

impl OutputDevice for DefaultOutputDevice {
    fn open(&self, sample_rate: u32, channels: u16) -> Result<Box<dyn OutputStream>> {
        default_output(sample_rate, channels)
    }
}

/// Open the host default output device. Sample-rate mismatch between the
/// caller and the device's preferred rate is handled transparently via
/// `rubato` resampling.
pub fn default_output(sample_rate: u32, channels: u16) -> Result<Box<dyn OutputStream>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(Error::NoDevice)?;

    let supported = pick_config(&device, channels)?;
    let device_sr = supported.sample_rate().0;
    let config: StreamConfig = StreamConfig {
        channels,
        sample_rate: SampleRate(device_sr),
        buffer_size: cpal::BufferSize::Default,
    };

    if device_sr != sample_rate {
        debug!(
            requested = sample_rate,
            device = device_sr,
            "audio-io: sample-rate mismatch, inserting rubato resampler"
        );
    }

    // Sized for ~250 ms of audio at the device rate; comfortably absorbs
    // typical decode/render jitter while staying small enough not to mask
    // genuine starvation in the underrun property test.
    let ring_capacity = (device_sr as usize) * channels as usize / 4;
    let rb = HeapRb::<f32>::new(ring_capacity.max(channels as usize * 1024));
    let (producer, consumer) = rb.split();

    let played = Arc::new(AtomicU64::new(0));
    let played_cb = Arc::clone(&played);
    let channels_cb = channels as usize;

    // cpal does NOT convert sample formats automatically; build_output_stream
    // requires T to match the device's native format, otherwise the backend
    // returns BuildStreamError. We dispatch on the format that pick_config
    // selected and convert from f32 (the ring buffer's element type) to the
    // device type inside the callback.
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream_f32(&device, &config, consumer, channels_cb, played_cb)?,
        SampleFormat::I16 => {
            build_stream_convert::<i16, _>(&device, &config, consumer, channels_cb, played_cb)?
        }
        SampleFormat::U16 => {
            build_stream_convert::<u16, _>(&device, &config, consumer, channels_cb, played_cb)?
        }
        SampleFormat::I32 => {
            build_stream_convert::<i32, _>(&device, &config, consumer, channels_cb, played_cb)?
        }
        other => return Err(Error::UnsupportedSampleFormat(other)),
    };

    let resampler = if device_sr != sample_rate {
        Some(ResamplerState::new(sample_rate, device_sr, channels)?)
    } else {
        None
    };

    Ok(Box::new(CpalOutputStream {
        stream,
        producer,
        played,
        channels,
        device_sample_rate: device_sr,
        resampler,
    }))
}

fn pick_config(device: &cpal::Device, channels: u16) -> Result<cpal::SupportedStreamConfig> {
    let configs = device
        .supported_output_configs()
        .map_err(|source| Error::UnsupportedConfig { channels, source })?;
    // Prefer an f32 config matching the requested channel count, falling
    // back to any config with the right channel count. cpal's
    // `with_max_sample_rate()` gives us the device's preferred (max) rate,
    // which matches what the OS audio engine is actually clocked at.
    let mut fallback = None;
    for cfg in configs {
        if cfg.channels() != channels {
            continue;
        }
        if cfg.sample_format() == SampleFormat::F32 {
            return Ok(cfg.with_max_sample_rate());
        }
        fallback.get_or_insert(cfg);
    }
    fallback
        .map(|c| c.with_max_sample_rate())
        .ok_or(Error::UnsupportedChannelCount(channels))
}

/// Audio-thread callback body for the fast path: device is f32 native, so we
/// `pop_slice` directly into the device buffer. No per-sample conversion.
/// **No allocation, no locking, no panicking paths.**
fn fill_output_f32<C>(out: &mut [f32], consumer: &mut C, channels: usize, played: &AtomicU64)
where
    C: ringbuf::traits::Consumer<Item = f32>,
{
    let written = consumer.pop_slice(out);
    if written < out.len() {
        for s in &mut out[written..] {
            *s = 0.0;
        }
    }
    let frames = (out.len() / channels) as u64;
    played.fetch_add(frames, Ordering::Relaxed);
}

/// Audio-thread callback body for non-f32 devices: pop one f32 sample at a
/// time and convert via cpal's `FromSample`. Slower than the f32 fast path
/// but the only correct answer for i16 / u16 / i32 outputs.
///
/// On underrun (first `try_pop` returning `None`) we stop polling the ring
/// buffer and fill the rest with the type's silent value — each `try_pop`
/// is an atomic op on the consumer, so doing one per remaining sample after
/// the producer has run dry is pure waste.
fn fill_output_convert<T, C>(out: &mut [T], consumer: &mut C, channels: usize, played: &AtomicU64)
where
    T: SizedSample + FromSample<f32>,
    C: ringbuf::traits::Consumer<Item = f32>,
{
    let silence = T::from_sample(0.0_f32);
    let mut iter = out.iter_mut();
    while let Some(slot) = iter.next() {
        match consumer.try_pop() {
            Some(f) => *slot = T::from_sample(f),
            None => {
                *slot = silence;
                for rest in iter.by_ref() {
                    *rest = silence;
                }
            }
        }
    }
    let frames = (out.len() / channels) as u64;
    played.fetch_add(frames, Ordering::Relaxed);
}

fn build_stream_f32<C>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: C,
    channels: usize,
    played: Arc<AtomicU64>,
) -> Result<Stream>
where
    C: ringbuf::traits::Consumer<Item = f32> + Send + 'static,
{
    let err_fn = |e| tracing::error!(error = %e, "audio-io: cpal stream error");
    Ok(device.build_output_stream::<f32, _, _>(
        config,
        move |out: &mut [f32], _info| {
            fill_output_f32(out, &mut consumer, channels, &played);
        },
        err_fn,
        None,
    )?)
}

fn build_stream_convert<T, C>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: C,
    channels: usize,
    played: Arc<AtomicU64>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
    C: ringbuf::traits::Consumer<Item = f32> + Send + 'static,
{
    let err_fn = |e| tracing::error!(error = %e, "audio-io: cpal stream error");
    Ok(device.build_output_stream::<T, _, _>(
        config,
        move |out: &mut [T], _info| {
            fill_output_convert(out, &mut consumer, channels, &played);
        },
        err_fn,
        None,
    )?)
}

struct ResamplerState {
    inner: FftFixedInOut<f32>,
    channels: usize,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    /// Caller frames not yet handed to the resampler.
    pending: Vec<Vec<f32>>,
    /// Reused scratch for reinterleaving resampler output before pushing to
    /// the ring; clear()-ed each chunk to avoid per-call heap allocs.
    interleaved_scratch: Vec<f32>,
}

impl ResamplerState {
    fn new(in_sr: u32, out_sr: u32, channels: u16) -> Result<Self> {
        let channels = channels as usize;
        // 1024-frame chunk is a good middle-ground: small enough to keep
        // resampling latency in the tens of ms, large enough to amortize
        // FFT cost.
        let inner = FftFixedInOut::<f32>::new(in_sr as usize, out_sr as usize, 1024, channels)?;
        let in_frames = inner.input_frames_max();
        let out_frames = inner.output_frames_max();
        let in_buf = vec![vec![0.0; in_frames]; channels];
        let out_buf = vec![vec![0.0; out_frames]; channels];
        let pending = vec![Vec::with_capacity(in_frames * 2); channels];
        let interleaved_scratch = Vec::with_capacity(out_frames * channels);
        Ok(Self {
            inner,
            channels,
            in_buf,
            out_buf,
            pending,
            interleaved_scratch,
        })
    }
}

struct CpalOutputStream<P>
where
    P: ringbuf::traits::Producer<Item = f32> + Send + 'static,
{
    // Field order is significant: `stream` must drop before `producer` so the
    // audio callback is torn down while the ringbuf split is still valid (the
    // consumer half lives inside the callback closure owned by `stream`).
    stream: Stream,
    producer: P,
    played: Arc<AtomicU64>,
    channels: u16,
    device_sample_rate: u32,
    resampler: Option<ResamplerState>,
}

// SAFETY contract — read this before relying on `OutputStream: Send`.
//
// `cpal::Stream` is `!Send` because on Windows the WASAPI/COM apartment model
// requires the stream to be destroyed on the same thread that constructed it
// (the COM-init thread). On macOS the underlying CoreAudio handle has a
// similar (weaker) thread-affinity property.
//
// We assert `unsafe impl Send` here so callers can hold the boxed stream in
// `tauri::State` / `Arc<Mutex<…>>`, which is the natural integration shape
// for Phase 1. **The soundness invariant is that the stream must be both
// constructed AND dropped on the same thread.** All other operations
// (`play`, `pause`, `write_samples`, `frames_played`) are safe from any
// thread because they only touch the lock-free ringbuf and atomic counter,
// not the OS handle.
//
// In practice for Phase 1: the engine creates this on the Tauri main thread
// and only the main thread tears it down at shutdown. Worker threads only
// call the ringbuf-side methods, never `Drop`.
//
// FOLLOW-UP for M11 (Tauri commands wiring): wrap this in an
// `AudioCoordinator` that owns a dedicated thread which holds the stream;
// other threads communicate via mpsc channels for play/pause and the
// already-Send ringbuf for samples. That removes the unsafe-Send contract
// from this crate's API surface.
unsafe impl<P> Send for CpalOutputStream<P> where
    P: ringbuf::traits::Producer<Item = f32> + Send + 'static
{
}

impl<P> OutputStream for CpalOutputStream<P>
where
    P: ringbuf::traits::Producer<Item = f32> + Send + 'static,
{
    fn play(&mut self) -> Result<()> {
        self.stream.play()?;
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        self.stream.pause()?;
        Ok(())
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        let channels = self.channels as usize;
        if samples.len() % channels != 0 {
            return Err(Error::UnalignedFrames {
                len: samples.len(),
                channels: self.channels,
            });
        }

        match self.resampler.as_mut() {
            None => {
                push_interleaved(&mut self.producer, samples);
                Ok(())
            }
            Some(rs) => resample_and_push(&mut self.producer, rs, samples),
        }
    }

    fn frames_played(&self) -> u64 {
        self.played.load(Ordering::Relaxed)
    }

    fn device_sample_rate(&self) -> u32 {
        self.device_sample_rate
    }
}

fn push_interleaved<P>(producer: &mut P, samples: &[f32])
where
    P: ringbuf::traits::Producer<Item = f32>,
{
    // Drop frames that don't fit rather than block: the producer side is
    // expected to track ring fill via `frames_played` and pace itself.
    let _ = producer.push_slice(samples);
}

fn resample_and_push<P>(producer: &mut P, rs: &mut ResamplerState, samples: &[f32]) -> Result<()>
where
    P: ringbuf::traits::Producer<Item = f32>,
{
    let channels = rs.channels;
    let frames = samples.len() / channels;
    for ch in 0..channels {
        rs.pending[ch].reserve(frames);
        for f in 0..frames {
            rs.pending[ch].push(samples[f * channels + ch]);
        }
    }

    let chunk = rs.inner.input_frames_next();
    while rs.pending[0].len() >= chunk {
        for ch in 0..channels {
            rs.in_buf[ch].clear();
            rs.in_buf[ch].extend(rs.pending[ch].drain(..chunk));
        }
        let (_in_used, out_written) =
            rs.inner
                .process_into_buffer(&rs.in_buf, &mut rs.out_buf, None)?;

        // Reinterleave into a reused scratch buffer; the producer thread is
        // not real-time but a per-chunk heap alloc is still wasteful.
        rs.interleaved_scratch.clear();
        for f in 0..out_written {
            for ch in 0..channels {
                rs.interleaved_scratch.push(rs.out_buf[ch][f]);
            }
        }
        push_interleaved(producer, &rs.interleaved_scratch);
    }
    Ok(())
}
