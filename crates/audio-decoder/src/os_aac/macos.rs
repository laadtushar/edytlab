//! M4A/AAC on macOS through AudioToolbox's Extended Audio File Services (#764).
//!
//! `ExtAudioFileOpenURL` opens the file with whatever AudioToolbox has for its
//! container and codec (AAC-LC, HE-AAC, ALAC), and a *client data format* of
//! interleaved 32-bit float makes `ExtAudioFileRead` hand back decoded PCM.
//! The decoder is Apple's, so the AAC patent licence is Apple's too. Docs:
//! <https://developer.apple.com/documentation/audiotoolbox/extended_audio_file_services>.
//!
//! The client format keeps the file's own sample rate and channel count, so
//! nothing is resampled or mixed down here. Gapless trimming (encoder delay
//! and padding) is AudioToolbox's: it reads the file's packet table.

use std::path::Path;
use std::ptr::{self, NonNull};

use objc2_audio_toolbox::{
    kAudioFileFileNotFoundError, kAudioFilePermissionsError, kAudioFileUnsupportedDataFormatError,
    kExtAudioFileProperty_ClientDataFormat, kExtAudioFileProperty_FileDataFormat,
    kExtAudioFileProperty_FileLengthFrames, ExtAudioFileDispose, ExtAudioFileGetProperty,
    ExtAudioFileOpenURL, ExtAudioFileRead, ExtAudioFileRef, ExtAudioFileSetProperty,
};
use objc2_core_audio_types::{
    kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM, AudioBuffer, AudioBufferList,
    AudioStreamBasicDescription,
};
use objc2_core_foundation::CFURL;

use crate::{DecodeError, DecodedAudio, Result};

/// Frames asked of `ExtAudioFileRead` at a time.
const CHUNK_FRAMES: u32 = 16_384;

/// Most channels taken: AAC carries up to 48, the app's engine is built for a
/// handful. A file over the bound is refused instead of read as noise.
const MAX_CHANNELS: u32 = 8;

/// The file opened by `ExtAudioFileOpenURL`, disposed when dropped.
struct ExtFile(ExtAudioFileRef);

impl Drop for ExtFile {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a successful `ExtAudioFileOpenURL` and is
        // disposed exactly once, here. The status is ignored: nothing can be
        // done about a failed close, and the file was only read.
        let _ = unsafe { ExtAudioFileDispose(self.0) };
    }
}

fn blank_format() -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    }
}

/// What the person is told for an `OSStatus` AudioToolbox answered.
fn map_status(status: i32) -> DecodeError {
    if status == kAudioFileFileNotFoundError {
        DecodeError::Io(std::io::ErrorKind::NotFound.into())
    } else if status == kAudioFilePermissionsError {
        DecodeError::Io(std::io::ErrorKind::PermissionDenied.into())
    } else if status == kAudioFileUnsupportedDataFormatError {
        DecodeError::UnsupportedCodec
    } else {
        // `typ?`, `dta?`, `wht?` and the rest: AudioToolbox could not make
        // audio of the file, which is what "corrupt or unrecognised" says.
        DecodeError::Corrupt
    }
}

/// Decode the M4A/MP4 file at `path` with AudioToolbox.
pub(crate) fn decode_path(path: &Path) -> Result<DecodedAudio> {
    let url = CFURL::from_file_path(path).ok_or_else(|| {
        DecodeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the file path cannot be turned into a URL",
        ))
    })?;

    let mut raw: ExtAudioFileRef = ptr::null_mut();
    // SAFETY: `url` is a live CFURL and `raw` is a valid place for the handle
    // to be written.
    let status = unsafe { ExtAudioFileOpenURL(&url, NonNull::from(&mut raw)) };
    if status != 0 || raw.is_null() {
        return Err(map_status(status));
    }
    let file = ExtFile(raw);

    // The file's own format: its rate and channel count are what is kept.
    let mut file_format = blank_format();
    let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
    // SAFETY: `file.0` is open; `size` is the byte size of the struct that
    // `file_format` is, and both are valid for the call.
    let status = unsafe {
        ExtAudioFileGetProperty(
            file.0,
            kExtAudioFileProperty_FileDataFormat,
            NonNull::from(&mut size),
            NonNull::from(&mut file_format).cast(),
        )
    };
    if status != 0 {
        return Err(map_status(status));
    }

    let rate = file_format.mSampleRate;
    let channels = file_format.mChannelsPerFrame;
    if !rate.is_finite() || !(1.0..=768_000.0).contains(&rate) {
        return Err(DecodeError::Corrupt);
    }
    if channels == 0 {
        return Err(DecodeError::Corrupt);
    }
    if channels > MAX_CHANNELS {
        // A real file the app does not take, not a damaged one.
        return Err(DecodeError::UnsupportedCodec);
    }

    // Ask for interleaved 32-bit float at that rate and channel count.
    let frame_bytes = 4 * channels;
    let client_format = AudioStreamBasicDescription {
        mSampleRate: rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagsNativeFloatPacked,
        mBytesPerPacket: frame_bytes,
        mFramesPerPacket: 1,
        mBytesPerFrame: frame_bytes,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    };
    // SAFETY: `file.0` is open; the pointer and size describe `client_format`,
    // which the call only reads.
    let status = unsafe {
        ExtAudioFileSetProperty(
            file.0,
            kExtAudioFileProperty_ClientDataFormat,
            std::mem::size_of::<AudioStreamBasicDescription>() as u32,
            NonNull::from(&client_format).cast(),
        )
    };
    if status != 0 {
        return Err(map_status(status));
    }

    // The length is only a hint for the first allocation, and a header can
    // claim any length, so it is capped at 16M samples (64 MB) and a failed
    // reservation is ignored: the vector grows as it is read.
    let mut length_frames: i64 = 0;
    let mut size = std::mem::size_of::<i64>() as u32;
    // SAFETY: as for the file format above, with an `i64` as the value.
    let status = unsafe {
        ExtAudioFileGetProperty(
            file.0,
            kExtAudioFileProperty_FileLengthFrames,
            NonNull::from(&mut size),
            NonNull::from(&mut length_frames).cast(),
        )
    };
    let channels_usize = channels as usize;
    let hint_samples = if status == 0 {
        (length_frames.clamp(0, 50_000_000) as usize).saturating_mul(channels_usize)
    } else {
        0
    };
    let mut samples: Vec<f32> = Vec::new();
    let _ = samples.try_reserve(hint_samples.min(16 * 1024 * 1024));
    let mut chunk = vec![0.0_f32; CHUNK_FRAMES as usize * channels_usize];
    loop {
        let mut frames = CHUNK_FRAMES;
        let mut list = AudioBufferList {
            mNumberBuffers: 1,
            mBuffers: [AudioBuffer {
                mNumberChannels: channels,
                mDataByteSize: (chunk.len() * std::mem::size_of::<f32>()) as u32,
                mData: chunk.as_mut_ptr().cast(),
            }],
        };
        // SAFETY: `file.0` is open with the client format set above. `list`
        // has one buffer of `mDataByteSize` bytes backed by `chunk`, which
        // outlives the call, and `frames` is at most what the buffer holds.
        let status = unsafe {
            ExtAudioFileRead(file.0, NonNull::from(&mut frames), NonNull::from(&mut list))
        };
        if status != 0 {
            return Err(map_status(status));
        }
        if frames == 0 {
            break;
        }
        let got = (frames as usize * channels_usize).min(chunk.len());
        samples.extend_from_slice(&chunk[..got]);
    }

    Ok(DecodedAudio {
        samples,
        sample_rate: rate.round() as u32,
        channels: channels as u16,
    })
}
