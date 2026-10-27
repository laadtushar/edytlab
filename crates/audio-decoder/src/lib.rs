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

mod opus;
mod os_aac;
mod stream;

pub use stream::WavStreamReader;

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::OnceLock;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, DecoderOptions, CODEC_TYPE_NULL, CODEC_TYPE_OPUS};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The file extensions the app imports on every platform, lower case and
/// without the dot.
///
/// [`supported_extensions`] is the list for the platform the app is running
/// on: this one plus [`OS_DECODER_EXTENSIONS`] where the system decodes them.
/// `batch_apply`'s folder filter reads that, and the window's file picker and
/// landing screen repeat it in TypeScript, which `tests/supported_extensions.rs`
/// holds equal. Every entry decodes a real fixture in that test. A format is
/// added here and in `Cargo.toml`'s symphonia features together.
///
/// `opus` is Opus in an Ogg file (RFC 7845), and so is an `ogg` file that
/// holds Opus rather than Vorbis: the container is found from the bytes.
///
/// Left out on purpose (#764): `m4a`, which is not on every platform (see
/// [`OS_DECODER_EXTENSIONS`]). A file of that kind that the platform cannot
/// open is named in [`DecodeError::UnsupportedFormat`].
pub const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "opus", "aif", "aiff"];

/// Extensions opened only by the operating system's own decoder, so only on
/// the platforms that have one: M4A/AAC on macOS (AudioToolbox) and Windows
/// (Media Foundation). No AAC decoder is built into the app, because AAC
/// decoders need a patent licence the project does not hold (#764); Linux has
/// no system decoder and refuses the file.
pub const OS_DECODER_EXTENSIONS: &[&str] = &["m4a"];

/// Whether this platform has an operating-system decoder for
/// [`OS_DECODER_EXTENSIONS`]: true on macOS and Windows.
pub const OS_DECODER_AVAILABLE: bool = os_aac::AVAILABLE;

/// The extensions the app imports on this platform: [`SUPPORTED_EXTENSIONS`],
/// and [`OS_DECODER_EXTENSIONS`] where the system decodes them.
pub fn supported_extensions() -> Vec<&'static str> {
    let mut all = SUPPORTED_EXTENSIONS.to_vec();
    if OS_DECODER_AVAILABLE {
        all.extend_from_slice(OS_DECODER_EXTENSIONS);
    }
    all
}

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

    /// The operating system's own decoder (used for M4A/AAC on macOS and
    /// Windows) is missing or cannot be used. The text is the whole message,
    /// written for the person, and says what to install or do instead.
    #[error("{0}")]
    OsDecoderUnavailable(String),

    /// A format the decoder recognises by its bytes and does not open. The
    /// name is for the person: it is what to convert from.
    #[error(
        "{0} files are not supported yet. Convert the file to WAV, FLAC, MP3, Ogg Vorbis, Opus or AIFF and load that"
    )]
    UnsupportedFormat(&'static str),

    /// An Opus file whose channel layout is not mono or stereo (surround,
    /// ambisonics, or a layout this decoder does not know). The text says
    /// which, for the person.
    #[error(
        "this Opus file uses {0}, and only mono and stereo Opus is supported. Convert the file to WAV, FLAC, MP3, Ogg Vorbis or AIFF and load that"
    )]
    UnsupportedOpusLayout(String),
}

pub type Result<T> = std::result::Result<T, DecodeError>;

/// symphonia's codecs for the enabled features, and Opus (symphonia 0.5 has
/// no Opus codec; see [`opus`]).
fn codecs() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<opus::OpusPacketDecoder>();
        registry
    })
}

/// Decode a full audio file from disk.
///
/// An M4A/MP4 file (by its bytes) goes to the operating system's decoder on
/// macOS and Windows; on other platforms it is refused as
/// [`DecodeError::UnsupportedFormat`].
pub fn decode_file(path: &Path) -> Result<DecodedAudio> {
    if let Some(decoded) = os_aac::decode_file_if_m4a(path) {
        return decoded;
    }
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
    if let Some(decoded) = os_aac::decode_bytes_if_m4a(bytes) {
        return decoded;
    }
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
/// (An M4A file on macOS or Windows is the exception: the system decoder is
/// asked for the rate it decodes at, which is the file decoded. Clips never
/// name an M4A source, because `load` makes a WAV of it.)
pub fn probe_sample_rate(path: &Path) -> Result<u32> {
    if let Some(decoded) = os_aac::decode_file_if_m4a(path) {
        return decoded.map(|d| d.sample_rate);
    }
    probe_sample_rate_inner(path).map_err(|e| match e {
        DecodeError::Corrupt | DecodeError::UnsupportedCodec | DecodeError::NoAudioTrack => {
            let mut head = Vec::new();
            if let Ok(f) = File::open(path) {
                let _ = f.take(SNIFF_BYTES as u64).read_to_end(&mut head);
            }
            name_unsupported(e, &head)
        }
        other => other,
    })
}

fn probe_sample_rate_inner(path: &Path) -> Result<u32> {
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
    // Kept only to name the format if the decode fails.
    let head = bytes[..bytes.len().min(SNIFF_BYTES)].to_vec();
    decode_inner(bytes, hint).map_err(|e| name_unsupported(e, &head))
}

fn decode_inner(bytes: Vec<u8>, hint: Hint) -> Result<DecodedAudio> {
    // An Ogg Opus file says how long it is in its last page (RFC 7845 §4.4).
    let opus_end = opus::head_in_ogg(&bytes).and_then(|_| opus::last_granule(&bytes));
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

    // Opus: say why a layout is refused, which the codec registry cannot.
    let opus_head = if codec_params.codec == CODEC_TYPE_OPUS {
        let extra = codec_params.extra_data.as_deref().unwrap_or_default();
        Some(opus::parse_head(extra)?)
    } else {
        None
    };

    let mut decoder = codecs()
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
            // recoverable; skip the packet and continue. An Opus packet that
            // does not decode keeps its place as silence instead, so the
            // packets after it stay where the file put them.
            Err(SymphoniaError::DecodeError(_)) => {
                if opus_head.is_some() && channels > 0 {
                    let frames = (packet.dur() as usize).min(5_760);
                    samples_out.resize(samples_out.len() + frames * channels as usize, 0.0);
                }
                continue;
            }
            Err(e) => return Err(map_runtime_err(e)),
        }
    }

    if sample_rate == 0 || channels == 0 {
        return Err(DecodeError::Corrupt);
    }

    // Cut the last packet to the length the last page states.
    if let (Some(head), Some(granule)) = (&opus_head, opus_end) {
        let frames = granule.saturating_sub(u64::from(head.pre_skip));
        let keep = (frames as usize).saturating_mul(channels as usize);
        samples_out.truncate(keep);
    }

    Ok(DecodedAudio {
        samples: samples_out,
        sample_rate,
        channels,
    })
}

/// How much of the start of a file is read to name a format the decoder does
/// not open. An Ogg stream's first page, with its `OpusHead`, is well inside.
pub(crate) const SNIFF_BYTES: usize = 4096;

/// A failed decode of a file whose bytes say it is M4A/MP4 becomes
/// [`DecodeError::UnsupportedFormat`], and of one that is Opus of a layout
/// that is not opened, [`DecodeError::UnsupportedOpusLayout`]; any other failure is returned as it
/// was. Only called on a failure, so a file that decodes is never renamed.
fn name_unsupported(e: DecodeError, head: &[u8]) -> DecodeError {
    if !matches!(
        e,
        DecodeError::Corrupt | DecodeError::UnsupportedCodec | DecodeError::NoAudioTrack
    ) {
        return e;
    }
    // ISO base media: a box size, then `ftyp` (M4A, MP4, MOV, 3GP). Only a
    // platform with no system decoder gets here for one of these.
    if os_aac::is_m4a(head) {
        return DecodeError::UnsupportedFormat("M4A and MP4");
    }
    // Ogg whose first packet is an Opus header, of a layout that is not opened
    // (surround, ambisonics): the container layer refuses those itself.
    if let Some(Err(layout @ DecodeError::UnsupportedOpusLayout(_))) = opus::head_in_ogg(head) {
        return layout;
    }
    e
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
