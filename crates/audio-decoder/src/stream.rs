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
use std::io::BufReader;
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
    /// Frames already returned by [`read_frames`]; bounds [`frames_remaining`].
    frames_read: u64,
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
            frames_read: 0,
            inner,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

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
    /// Used by the render graph to seek past the source-offset prefix of a
    /// clip: the streaming WAV reader has no random access (we don't want
    /// to depend on `WavReader::seek`'s 32-bit `time` argument for long
    /// files), so we drain bytes through the existing iterator. This stays
    /// O(1) memory regardless of how far we skip.
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
