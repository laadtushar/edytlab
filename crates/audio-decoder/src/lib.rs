//! Full-file audio decoder built on `symphonia`.
//!
//! Phase 1 needed only synchronous, full-file decoding into interleaved `f32`
//! PCM in [-1.0, 1.0] — that's still the API contract for [`decode_file`] /
//! [`decode_bytes`] and is what every analysis crate uses.
//!
//! Phase 2 M22 added a streaming WAV reader ([`stream::WavStreamReader`])
//! alongside the buffered API for the constant-memory render path. The
//! buffered API is unchanged — callers that already use [`decode_file`] do
//! NOT need to migrate.

mod stream;

pub use stream::WavStreamReader;

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded PCM audio, always interleaved (`L, R, L, R, ...` for stereo).
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Errors returned by [`decode_file`] / [`decode_bytes`].
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("corrupt or unrecognised audio data")]
    Corrupt,

    #[error("unsupported codec")]
    UnsupportedCodec,

    #[error("no audio track found in container")]
    NoAudioTrack,
}

pub type Result<T> = std::result::Result<T, DecodeError>;

/// Decode a full audio file from disk.
pub fn decode_file(path: &Path) -> Result<DecodedAudio> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    decode_with_hint(bytes, hint)
}

/// Decode an in-memory audio buffer. Used by the corrupt-input property test
/// and by callers that already have bytes in hand.
pub fn decode_bytes(bytes: &[u8]) -> Result<DecodedAudio> {
    // MediaSource requires Send+Sync+'static, so we own the buffer.
    decode_with_hint(bytes.to_vec(), Hint::new())
}

/// The sample rate of a file, read from its header alone.
///
/// A clip's `start_in_track`, `source_offset` and `length` are counted
/// in **its own source's** frames, not the project's (#234). Converting
/// between seconds and those fields therefore needs the source's rate,
/// and callers that only need the rate should not pay for a full decode
/// to learn it — `list_tracks` runs on every UI refresh and would decode
/// every clip on the timeline.
///
/// This reads the container's headers and stops. No packets are decoded.
pub fn probe_sample_rate(path: &Path) -> Result<u32> {
    let file = File::open(path)?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(map_probe_err)?;

    probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .and_then(|t| t.codec_params.sample_rate)
        .ok_or(DecodeError::NoAudioTrack)
}

fn decode_with_hint(bytes: Vec<u8>, hint: Hint) -> Result<DecodedAudio> {
    let cursor = Cursor::new(bytes);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(map_probe_err)?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(DecodeError::NoAudioTrack)?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| match e {
            SymphoniaError::Unsupported(_) => DecodeError::UnsupportedCodec,
            other => map_runtime_err(other),
        })?;

    let mut samples_out: Vec<f32> = Vec::new();
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // Symphonia signals end-of-stream as UnexpectedEof.
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(map_runtime_err(e)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_buf.is_none() {
                    // First-packet spec is held for the entire decode; mid-stream
                    // changes surface as ResetRequired and we break out of the loop.
                    let spec = *audio_buf.spec();
                    sample_rate = spec.rate;
                    channels = spec.channels.count() as u16;
                    let capacity = audio_buf.capacity() as u64;
                    sample_buf = Some(SampleBuffer::<f32>::new(capacity, spec));
                }
                if let Some(buf) = sample_buf.as_mut() {
                    // `copy_interleaved_ref` converts planar -> interleaved and
                    // any native sample format -> f32 in one call.
                    buf.copy_interleaved_ref(audio_buf);
                    samples_out.extend_from_slice(buf.samples());
                }
            }
            // Per symphonia docs, decode errors on a single packet are
            // recoverable; skip the packet and continue.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(map_runtime_err(e)),
        }
    }

    if sample_rate == 0 || channels == 0 {
        return Err(DecodeError::Corrupt);
    }

    Ok(DecodedAudio {
        samples: samples_out,
        sample_rate,
        channels,
    })
}

// Errors during initial probe (header parse, codec lookup) collapse to
// `Corrupt` unless they are clearly an OS I/O issue or unsupported codec.
fn map_probe_err(e: SymphoniaError) -> DecodeError {
    match e {
        SymphoniaError::IoError(io) if io.kind() != std::io::ErrorKind::UnexpectedEof => {
            DecodeError::Io(io)
        }
        SymphoniaError::Unsupported(_) => DecodeError::UnsupportedCodec,
        _ => DecodeError::Corrupt,
    }
}

// Errors during the streaming decode loop. UnexpectedEof in the middle of a
// packet means the file was truncated, so it's `Corrupt`, not real I/O.
fn map_runtime_err(e: SymphoniaError) -> DecodeError {
    match e {
        SymphoniaError::IoError(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {
            DecodeError::Corrupt
        }
        SymphoniaError::IoError(io) => DecodeError::Io(io),
        SymphoniaError::Unsupported(_) => DecodeError::UnsupportedCodec,
        _ => DecodeError::Corrupt,
    }
}
