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
    #[error("device sample format `{0:?}` is not supported")]
    UnsupportedSampleFormat(SampleFormat),
}

/// An open output stream. All operations are safe to call from any thread
/// other than the audio callback itself.
pub trait OutputStream: Send {
    fn play(&mut self) -> Result<()>;
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
    let channels_cb = channels as usize;

    // Dispatch on the device's native sample format. The f32 path keeps the
    // hot `pop_slice` memcpy callback. The integer paths use a per-sample
    // `FromSample` conversion. f64 / other formats are surfaced explicitly so
    // we don't paper over an unsupported device with garbled audio.
    let stream = match supported.sample_format() {
        SampleFormat::F32 => {
            build_stream_f32(&device, &config, consumer, channels_cb, Arc::clone(&played))?
        }
        SampleFormat::I16 => build_stream_convert::<i16, _>(
            &device,
            &config,
            consumer,
            channels_cb,
            Arc::clone(&played),
        )?,
        SampleFormat::U16 => build_stream_convert::<u16, _>(
            &device,
            &config,
            consumer,
            channels_cb,
            Arc::clone(&played),
        )?,
        SampleFormat::I32 => build_stream_convert::<i32, _>(
            &device,
            &config,
            consumer,
            channels_cb,
            Arc::clone(&played),
        )?,
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

/// Audio-thread callback body for f32 devices. Pulls frames from the ring;
/// writes silence on underrun. **No allocation, no locking, no panicking
/// paths.**
fn fill_output<C>(out: &mut [f32], consumer: &mut C, channels: usize, played: &AtomicU64)
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

/// Build the f32 fast-path stream. Preserves the existing `pop_slice` memcpy
/// callback used by the underrun property test.
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
    let stream = device.build_output_stream::<f32, _, _>(
        config,
        move |out: &mut [f32], _info| {
            fill_output(out, &mut consumer, channels, &played);
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

/// Build a stream for a non-f32 sample format. Each sample is pulled from the
/// f32 ring and converted via `cpal::FromSample`. The conversion is per-sample
/// rather than per-block because cpal already promises any cost there is
/// negligible against the audio-thread budget; the alternative (a scratch
/// f32 buffer + `Vec`-allocated convert) would allocate inside the callback,
/// which we forbid.
fn build_stream_convert<T, C>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: C,
    channels: usize,
    played: Arc<AtomicU64>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
    C: ringbuf::traits::Consumer<Item = f32> + Send + 'static,
{
    let err_fn = |e| tracing::error!(error = %e, "audio-io: cpal stream error");
    let stream = device.build_output_stream::<T, _, _>(
        config,
        move |out: &mut [T], _info| {
            for s in out.iter_mut() {
                let f = consumer.try_pop().unwrap_or(0.0);
                *s = T::from_sample(f);
            }
            let frames = (out.len() / channels) as u64;
            played.fetch_add(frames, Ordering::Relaxed);
        },
        err_fn,
        None,
    )?;
    Ok(stream)
}

struct ResamplerState {
    inner: FftFixedInOut<f32>,
    channels: usize,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    /// Caller frames not yet handed to the resampler.
    pending: Vec<Vec<f32>>,
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
        Ok(Self {
            inner,
            channels,
            in_buf,
            out_buf,
            pending,
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
// `cpal::Stream` is `!Send` because on Windows the WASAPI/COM apartment
// model requires the stream to be destroyed on the same thread that
// constructed it (the COM-init thread). On macOS the underlying
// CoreAudio handle has a similar (weaker) thread-affinity property.
//
// We assert `unsafe impl Send` here so callers can hold the boxed
// stream in `tauri::State` / `Arc<Mutex<...>>` for Phase 1. The
// soundness invariant is that the stream must be both constructed AND
// dropped on the same thread. All other operations (`play`, `pause`,
// `write_samples`, `frames_played`) are safe from any thread because
// they only touch the lock-free ringbuf and atomic counter.
//
// In practice for Phase 1: the engine creates this on the Tauri main
// thread and only the main thread tears it down at shutdown. Worker
// threads only call ringbuf-side methods, never `Drop`.
//
// FOLLOW-UP for Phase 2: wrap this in an `AudioCoordinator` that owns
// a dedicated thread holding the stream; other threads communicate via
// mpsc channels for play/pause and the already-Send ringbuf for
// samples. That removes the unsafe-Send contract from this crate's
// API surface.
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

        // Reinterleave on the stack-allocated path: a small scratch Vec is
        // fine here, this is the producer thread, not the audio callback.
        let mut interleaved = Vec::with_capacity(out_written * channels);
        for f in 0..out_written {
            for ch in 0..channels {
                interleaved.push(rs.out_buf[ch][f]);
            }
        }
        push_interleaved(producer, &interleaved);
    }
    Ok(())
}
