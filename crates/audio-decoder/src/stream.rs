//! Streaming WAV reader for the M22 constant-memory multi-track render path.
//!
//! ### Why a separate streaming reader (and not symphonia)?
//!
//! [`decode_file`](crate::decode_file) decodes the entire file into a single
//! `Vec<f32>` up-front. That's fine for analysis tools and for short clips,
//! but the multi-track mixdown in `audio-engine::render` needs to keep peak
//! memory bounded regardless of session length and track count. Streaming
//! the source frame-by-frame (or chunk-by-chunk) sidesteps the full-decode
//! allocation entirely.
//!
//! Symphonia *does* support packet-by-packet decoding, but the WAV path in
//! particular is just raw interleaved PCM — wrapping `hound` here is simpler
//! and produces float values that are bit-exact equal to symphonia's i16 →
//! f32 conversion (`s as f32 / 32_768.0` for 16-bit PCM, the only format the
//! current Phase-1/Phase-2 pipeline produces). Producing the same float
//! values is load-bearing for the M22 byte-identity acceptance criterion: a
//! session rendered today (M21 buffered mixer) must produce a byte-equal
//! output WAV after M22 (streaming mixer).
//!
//! ### Scope and limits
//!
//! This is a WAV-only reader. The buffered [`decode_file`](crate::decode_file)
//! path still handles MP3/FLAC for analysis crates that need them; the M22
//! render graph only ever points at WAV sources (rendered intermediate
//! files, on-disk session assets after import) so there is no need to push
//! streaming through the symphonia pipeline yet.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use hound::{SampleFormat, WavReader};

use crate::DecodeError;

/// Streaming, constant-memory reader over a WAV file.
///
/// Created via [`WavStreamReader::open`]. Reads PCM frames in caller-sized
/// batches via [`WavStreamReader::read_frames`] and surfaces sample-rate /
/// channel / total-frame metadata up front so callers can plan output sizing
/// without buffering the full source.
pub struct WavStreamReader {
    inner: WavReader<BufReader<File>>,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    sample_format: SampleFormat,
    total_frames: u64,
    seekable: bool,
    /// Frames already returned by [`read_frames`]; bounds [`frames_remaining`].
    frames_read: u64,
}

/// Byte stride for the sample formats whose hound seeks are supported.
fn packed_sample_bytes(spec: hound::WavSpec) -> Option<u64> {
    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 8) => Some(1),
        (SampleFormat::Int, 16) => Some(2),
        (SampleFormat::Int, 24) => Some(3),
        (SampleFormat::Int, 32) | (SampleFormat::Float, 32) => Some(4),
        _ => None,
    }
}

/// Check the data chunk against both hound's sample count and the file extent.
/// The reader starts immediately after the data chunk length field.
fn checked_data_extent(
    reader: &mut BufReader<File>,
    samples: u32,
    bytes_per_sample: u64,
    file_length: u64,
) -> Option<u64> {
    let data_start = reader.stream_position().ok()?;
    reader
        .seek(SeekFrom::Start(data_start.checked_sub(4)?))
        .ok()?;
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length).ok()?;
    let data_length = u64::from(u32::from_le_bytes(length));
    if u64::from(samples).checked_mul(bytes_per_sample)? != data_length
        || data_start.checked_add(data_length)? > file_length
    {
        return None;
    }
    Some(data_start)
}

/// Validate the container stride independently of hound's sample count.
fn check_block_alignment(
    reader: &mut BufReader<File>,
    spec: hound::WavSpec,
    bytes_per_sample: u64,
    data_start: u64,
) -> Option<()> {
    reader.seek(SeekFrom::Start(12)).ok()?;
    let expected_alignment = bytes_per_sample.checked_mul(u64::from(spec.channels))?;
    loop {
        let position = reader.stream_position().ok()?;
        if position.checked_add(8)? > data_start {
            return None;
        }
        let mut header = [0_u8; 8];
        reader.read_exact(&mut header).ok()?;
        let size = u64::from(u32::from_le_bytes(header[4..].try_into().ok()?));
        if &header[..4] == b"fmt " {
            return check_format_alignment(reader, size, expected_alignment);
        }
        let next = position
            .checked_add(8)?
            .checked_add(size)?
            .checked_add(size % 2)?;
        if next > data_start {
            return None;
        }
        reader.seek(SeekFrom::Start(next)).ok()?;
    }
}

fn check_format_alignment(
    reader: &mut BufReader<File>,
    size: u64,
    expected_alignment: u64,
) -> Option<()> {
    if size < 16 {
        return None;
    }
    let mut format = [0_u8; 16];
    reader.read_exact(&mut format).ok()?;
    let alignment = u64::from(u16::from_le_bytes([format[12], format[13]]));
    if alignment != expected_alignment {
        return None;
    }
    Some(())
}

impl WavStreamReader {
    /// Open `path` and parse the WAV header. Does not read any samples.
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let inner = WavReader::open(path).map_err(map_hound_err)?;
        let spec = inner.spec();
        let total_frames = inner.duration() as u64;
        Ok(Self {
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            bits_per_sample: spec.bits_per_sample,
            sample_format: spec.sample_format,
            total_frames,
            seekable: false,
            frames_read: 0,
            inner,
        })
    }

    /// Open a regular, complete, packed PCM WAV for direct frame seeks.
    ///
    /// Hound's seek uses `bits_per_sample / 8`, not the WAV container's
    /// block alignment. In particular it cannot seek 24 valid bits stored
    /// in four bytes. Probe the same file handle and fall back to `open`
    /// for any layout or file whose seek safety cannot be established.
    pub fn open_seekable(path: &Path) -> Option<Self> {
        // Do not consume a FIFO producer just to check eligibility. This
        // path check is point-in-time: a concurrent replacement between it
        // and File::open can still change the kind of file being opened.
        if !std::fs::metadata(path).ok()?.is_file() {
            return None;
        }
        let file = File::open(path).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        let mut reader = BufReader::new(file);
        let (spec, samples) = {
            let probe = WavReader::new(&mut reader).ok()?;
            (probe.spec(), probe.len())
        };
        let bytes_per_sample = packed_sample_bytes(spec)?;
        let data_start =
            checked_data_extent(&mut reader, samples, bytes_per_sample, metadata.len())?;
        check_block_alignment(&mut reader, spec, bytes_per_sample, data_start)?;
        reader.rewind().ok()?;
        let inner = WavReader::new(reader).ok()?;
        Some(Self {
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            bits_per_sample: spec.bits_per_sample,
            sample_format: spec.sample_format,
            total_frames: inner.duration() as u64,
            seekable: true,
            frames_read: 0,
            inner,
        })
    }

    /// Seek an `open_seekable` reader to a frame at or before its end.
    ///
    /// Readers made by [`Self::open`] reject this operation; only
    /// [`Self::open_seekable`] validates hound's byte stride.
    pub fn seek_frame(&mut self, frame: u64) -> Result<(), DecodeError> {
        if !self.seekable
            || frame > self.total_frames
            || frame
                .checked_mul(u64::from(self.channels))
                .is_none_or(|n| n > u64::from(u32::MAX))
        {
            return Err(DecodeError::Corrupt);
        }
        let frame = u32::try_from(frame).map_err(|_| DecodeError::Corrupt)?;
        self.inner.seek(frame)?;
        self.frames_read = u64::from(frame);
        Ok(())
    }

    /// Sample rate in Hz from the WAV header.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of interleaved samples in each frame.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Total frame count from the WAV header. Each frame contains
    /// [`channels`](Self::channels) samples.
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Frames not yet returned by [`read_frames`].
    pub fn frames_remaining(&self) -> u64 {
        self.total_frames.saturating_sub(self.frames_read)
    }

    /// Skip up to `frames` frames forward without returning them.
    ///
    /// Drains the existing iterator, including malformed and unusual WAV
    /// layouts for which direct seeking could change decoding behaviour.
    /// This stays O(1) memory regardless of how far we skip.
    pub fn skip_frames(&mut self, frames: u64) -> Result<(), DecodeError> {
        let available = self.frames_remaining();
        let to_skip = frames.min(available);
        let total_samples = to_skip
            .checked_mul(self.channels as u64)
            .ok_or(DecodeError::Corrupt)?;
        let mut remaining = total_samples;
        match self.sample_format {
            SampleFormat::Int => match self.bits_per_sample {
                8 | 16 => {
                    let mut iter = self.inner.samples::<i16>();
                    while remaining > 0 {
                        match iter.next() {
                            Some(Ok(_)) => remaining -= 1,
                            Some(Err(e)) => return Err(map_hound_err(e)),
                            None => break,
                        }
                    }
                }
                24 | 32 => {
                    let mut iter = self.inner.samples::<i32>();
                    while remaining > 0 {
                        match iter.next() {
                            Some(Ok(_)) => remaining -= 1,
                            Some(Err(e)) => return Err(map_hound_err(e)),
                            None => break,
                        }
                    }
                }
                other => return Err(unsupported_bits(other)),
            },
            SampleFormat::Float => {
                let mut iter = self.inner.samples::<f32>();
                while remaining > 0 {
                    match iter.next() {
                        Some(Ok(_)) => remaining -= 1,
                        Some(Err(e)) => return Err(map_hound_err(e)),
                        None => break,
                    }
                }
            }
        }
        let frames_actually_skipped = (total_samples - remaining) / self.channels as u64;
        self.frames_read += frames_actually_skipped;
        Ok(())
    }

    /// Read up to `dst.len() / channels` frames into `dst` as interleaved
    /// `f32` PCM in `[-1.0, 1.0]`. Returns the number of FRAMES (not samples)
    /// written. A return of `0` means EOF — the entire source has been
    /// drained.
    ///
    /// `dst.len()` MUST be a multiple of `channels()`; otherwise the call
    /// returns [`DecodeError::Corrupt`] (unaligned destination buffer).
    ///
    /// The conversion from native sample format to `f32` matches symphonia's
    /// (`s as f32 / 32_768.0` for 16-bit PCM, `s as f32 / 2^31` for 32-bit,
    /// etc.) so values produced here are bit-equal to the buffered
    /// [`decode_file`](crate::decode_file) path. That equality is what lets
    /// the M22 streaming render preserve byte-identical output relative to
    /// the M21 buffered render.
    pub fn read_frames(&mut self, dst: &mut [f32]) -> Result<usize, DecodeError> {
        let chans = self.channels as usize;
        if chans == 0 {
            return Err(DecodeError::Corrupt);
        }
        if !dst.len().is_multiple_of(chans) {
            return Err(DecodeError::Corrupt);
        }
        let want_frames = dst.len() / chans;
        if want_frames == 0 {
            return Ok(0);
        }
        let mut written_samples = 0usize;
        let want_samples = want_frames * chans;

        match self.sample_format {
            SampleFormat::Int => match self.bits_per_sample {
                8 => {
                    let mut iter = self.inner.samples::<i16>();
                    while written_samples < want_samples {
                        match iter.next() {
                            Some(Ok(s)) => {
                                // hound returns 8-bit PCM widened to i16 (it
                                // upcasts `(byte - 128)` to i16). To match
                                // symphonia's 8-bit -> f32 (`s as f32 /
                                // 128.0`) we re-narrow.
                                let s8 = (s.clamp(-128, 127)) as i8;
                                dst[written_samples] = s8 as f32 / 128.0;
                                written_samples += 1;
                            }
                            Some(Err(e)) => return Err(map_hound_err(e)),
                            None => break,
                        }
                    }
                }
                16 => {
                    let mut iter = self.inner.samples::<i16>();
                    while written_samples < want_samples {
                        match iter.next() {
                            Some(Ok(s)) => {
                                dst[written_samples] = s as f32 / 32_768.0;
                                written_samples += 1;
                            }
                            Some(Err(e)) => return Err(map_hound_err(e)),
                            None => break,
                        }
                    }
                }
                24 => {
                    let mut iter = self.inner.samples::<i32>();
                    let denom = (1i32 << 23) as f32; // 2^23 = 8_388_608
                    while written_samples < want_samples {
                        match iter.next() {
                            Some(Ok(s)) => {
                                dst[written_samples] = s as f32 / denom;
                                written_samples += 1;
                            }
                            Some(Err(e)) => return Err(map_hound_err(e)),
                            None => break,
                        }
                    }
                }
                32 => {
                    let mut iter = self.inner.samples::<i32>();
                    let denom = (1i64 << 31) as f32; // 2^31 = 2_147_483_648
                    while written_samples < want_samples {
                        match iter.next() {
                            Some(Ok(s)) => {
                                dst[written_samples] = s as f32 / denom;
                                written_samples += 1;
                            }
                            Some(Err(e)) => return Err(map_hound_err(e)),
                            None => break,
                        }
                    }
                }
                other => return Err(unsupported_bits(other)),
            },
            SampleFormat::Float => {
                let mut iter = self.inner.samples::<f32>();
                while written_samples < want_samples {
                    match iter.next() {
                        Some(Ok(s)) => {
                            dst[written_samples] = s;
                            written_samples += 1;
                        }
                        Some(Err(e)) => return Err(map_hound_err(e)),
                        None => break,
                    }
                }
            }
        }

        // Partial trailing frame would mean the source was truncated mid-
        // frame; surface that as Corrupt so callers don't accidentally
        // operate on a misaligned tail.
        if !written_samples.is_multiple_of(chans) {
            return Err(DecodeError::Corrupt);
        }
        let frames = written_samples / chans;
        self.frames_read += frames as u64;
        Ok(frames)
    }
}

fn map_hound_err(e: hound::Error) -> DecodeError {
    match e {
        hound::Error::IoError(io) => DecodeError::Io(io),
        hound::Error::FormatError(_)
        | hound::Error::TooWide
        | hound::Error::UnfinishedSample
        | hound::Error::InvalidSampleFormat => DecodeError::Corrupt,
        hound::Error::Unsupported => DecodeError::UnsupportedCodec,
    }
}

fn unsupported_bits(_bits: u16) -> DecodeError {
    DecodeError::UnsupportedCodec
}
