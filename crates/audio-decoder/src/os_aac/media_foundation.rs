//! M4A/AAC on Windows through Media Foundation's Source Reader (#764).
//!
//! `IMFSourceReader` opens the file with Windows' own MPEG-4 file source and
//! loads the Microsoft AAC decoder MFT (`CLSID_CMSAACDecMFT`: AAC-LC, HE-AAC
//! v1 and v2, up to six channels, 8 to 48 kHz) behind it, so asking for
//! 32-bit float output is all the decoding setup there is. The decoder is
//! Microsoft's, so the AAC patent licence is Microsoft's too. Docs:
//! <https://learn.microsoft.com/en-us/windows/win32/medfound/aac-decoder> and
//! <https://learn.microsoft.com/en-us/windows/win32/medfound/source-reader>.
//!
//! **Media Foundation is looked up at run time, not linked.** Windows N and KN
//! editions ship without it until the Media Feature Pack is installed, and a
//! program that imports `mfplat.dll` or `mfreadwrite.dll` does not start at
//! all when they are missing. So the four functions used (`MFStartup`,
//! `MFShutdown`, `MFCreateMediaType`, `MFCreateSourceReaderFromURL`) are found
//! with `LoadLibraryExW` and `GetProcAddress`, and a missing library is the
//! "install the Media Feature Pack" answer instead of a program that will not
//! open. Everything else is a call through a COM interface, which imports
//! nothing.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;

use windows::core::{Interface, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HMODULE, RPC_E_CHANGED_MODE};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader, MFAudioFormat_Float,
    MFMediaType_Audio, MFSTARTUP_NOSOCKET, MF_E_INVALIDMEDIATYPE, MF_E_TOPO_CODEC_NOT_FOUND,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_MEDIASOURCE, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
use windows::Win32::System::Variant::VT_UI8;

use crate::{DecodeError, DecodedAudio, Result};

/// Said when Media Foundation itself is not on this Windows (`mfplat.dll` or
/// `mfreadwrite.dll` cannot be loaded, or `MFStartup` fails).
const MEDIA_FEATURE_PACK: &str = "Windows' own audio decoder for M4A and AAC files is not available on this computer. \
Windows N and KN editions leave it out until the Media Feature Pack is installed (Settings, System, Optional features). \
Install it, or convert the file to WAV, FLAC, MP3, Ogg Vorbis, Opus or AIFF and load that";

/// Said when Media Foundation is there and has no decoder for the audio in this
/// particular file (`MF_E_TOPO_CODEC_NOT_FOUND`): AAC above 48 kHz or with more
/// than six channels, AC-3 or Opus in an MP4, ALAC, and the like.
const NO_DECODER_FOR_FILE: &str = "Windows has no decoder for the audio in this file. \
Convert the file to WAV, FLAC, MP3, Ogg Vorbis, Opus or AIFF and load that";

/// The stream index the Source Reader uses for "the first audio stream".
const AUDIO_STREAM: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
/// The stream index that stands for every stream.
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;

/// Most channels taken. The decoder itself stops at six; this is a bound on
/// what a media type may claim.
const MAX_CHANNELS: u32 = 8;

/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`, `ERROR_PATH_NOT_FOUND` and
/// `ERROR_ACCESS_DENIED`, which Media Foundation passes up from the file open.
const HR_FILE_NOT_FOUND: HRESULT = HRESULT(0x8007_0002_u32 as i32);
const HR_PATH_NOT_FOUND: HRESULT = HRESULT(0x8007_0003_u32 as i32);
const HR_ACCESS_DENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);
/// `MF_E_INVALIDSTREAMNUMBER`: the file has no stream of the kind asked for.
const MF_E_INVALIDSTREAMNUMBER: HRESULT = HRESULT(0xC00D_36B3_u32 as i32);

type MfStartup = unsafe extern "system" fn(version: u32, flags: u32) -> HRESULT;
type MfShutdown = unsafe extern "system" fn() -> HRESULT;
type MfCreateMediaType = unsafe extern "system" fn(out: *mut *mut c_void) -> HRESULT;
type MfCreateSourceReaderFromUrl = unsafe extern "system" fn(
    url: PCWSTR,
    attributes: *mut c_void,
    out: *mut *mut c_void,
) -> HRESULT;

/// The four free functions of Media Foundation this module calls.
struct MfApi {
    startup: MfStartup,
    shutdown: MfShutdown,
    create_media_type: MfCreateMediaType,
    create_source_reader_from_url: MfCreateSourceReaderFromUrl,
}

/// Looked up once. The libraries are never unloaded: Media Foundation keeps
/// worker threads that outlive a call, and unloading under them is a crash.
static API: OnceLock<Option<MfApi>> = OnceLock::new();

fn api() -> Result<&'static MfApi> {
    API.get_or_init(load_api)
        .as_ref()
        .ok_or_else(|| DecodeError::OsDecoderUnavailable(MEDIA_FEATURE_PACK.to_string()))
}

/// Load `mfplat.dll` and `mfreadwrite.dll` from the system folder only (a
/// copy next to the program or on the path is never run) and find the four
/// functions. `None` when any step fails.
fn load_api() -> Option<MfApi> {
    let wide = |name: &str| -> Vec<u16> { name.encode_utf16().chain(Some(0)).collect() };
    let plat_name = wide("mfplat.dll");
    let readwrite_name = wide("mfreadwrite.dll");
    // SAFETY: both names are NUL-terminated UTF-16 that outlive the calls, no
    // file handle is passed, and the flag restricts the search to System32.
    let plat = unsafe {
        LoadLibraryExW(
            PCWSTR(plat_name.as_ptr()),
            None,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .ok()?;
    // SAFETY: as above.
    let readwrite = unsafe {
        LoadLibraryExW(
            PCWSTR(readwrite_name.as_ptr()),
            None,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .ok()?;
    // SAFETY: each module handle is a library loaded above, each name is a
    // NUL-terminated byte string, and each function's real signature is the
    // one in its alias (MSDN: MFStartup, MFShutdown, MFCreateMediaType and
    // MFCreateSourceReaderFromURL, all `STDAPI`).
    unsafe {
        Some(MfApi {
            startup: resolve(plat, PCSTR(c"MFStartup".as_ptr().cast()))?,
            shutdown: resolve(plat, PCSTR(c"MFShutdown".as_ptr().cast()))?,
            create_media_type: resolve(plat, PCSTR(c"MFCreateMediaType".as_ptr().cast()))?,
            create_source_reader_from_url: resolve(
                readwrite,
                PCSTR(c"MFCreateSourceReaderFromURL".as_ptr().cast()),
            )?,
        })
    }
}

/// `GetProcAddress` for a function pointer type `F`.
///
/// # Safety
/// `F` must be the `extern "system"` function pointer type of the exported
/// function called `name`, and `module` a loaded library.
unsafe fn resolve<F: Copy>(module: HMODULE, name: PCSTR) -> Option<F> {
    assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<usize>());
    // SAFETY: the caller's contract.
    let found = unsafe { GetProcAddress(module, name) }?;
    // SAFETY: a function pointer is copied to a function pointer of the same
    // size (asserted above); the caller says the signature is the right one.
    Some(unsafe { std::mem::transmute_copy::<_, F>(&found) })
}

/// COM and Media Foundation started on this thread, and stopped when dropped.
struct Session {
    api: &'static MfApi,
    /// Whether this call started COM on the thread and so must stop it. A
    /// thread that was already an apartment of the other kind (the window's
    /// main thread is one) is used as it is.
    uninitialize_com: bool,
}

impl Session {
    fn start(api: &'static MfApi) -> Result<Self> {
        // SAFETY: plain COM start-up on the calling thread; no pointer is
        // passed. The result is checked and balanced in `drop`.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let uninitialize_com = if com.is_ok() {
            // `S_OK` and `S_FALSE` both count one start that needs one stop.
            true
        } else if com == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(DecodeError::Io(std::io::Error::other(
                "COM could not be started on this thread",
            )));
        };
        // SAFETY: `MFStartup` may be called on any thread after COM start-up;
        // it is balanced by `MFShutdown` in `drop`.
        let started = unsafe { (api.startup)(MF_VERSION, MFSTARTUP_NOSOCKET) };
        if started.is_err() {
            if uninitialize_com {
                // SAFETY: balances the `CoInitializeEx` above.
                unsafe { CoUninitialize() };
            }
            return Err(DecodeError::OsDecoderUnavailable(
                MEDIA_FEATURE_PACK.to_string(),
            ));
        }
        Ok(Self {
            api,
            uninitialize_com,
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: balances the successful `MFStartup` and `CoInitializeEx` of
        // `start`. Every COM object of the decode is dropped before this
        // runs, because `decode_path` keeps them in an inner function.
        unsafe {
            let _ = (self.api.shutdown)();
            if self.uninitialize_com {
                CoUninitialize();
            }
        }
    }
}

/// A media buffer locked for reading, unlocked when dropped.
struct Locked<'a>(&'a IMFMediaBuffer);

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        // SAFETY: only built after a successful `Lock` on this buffer.
        let _ = unsafe { self.0.Unlock() };
    }
}

/// What the person is told for an `HRESULT` Media Foundation answered.
fn map_hresult(hr: HRESULT) -> DecodeError {
    if hr == HR_FILE_NOT_FOUND || hr == HR_PATH_NOT_FOUND {
        DecodeError::Io(std::io::ErrorKind::NotFound.into())
    } else if hr == HR_ACCESS_DENIED {
        DecodeError::Io(std::io::ErrorKind::PermissionDenied.into())
    } else if hr == MF_E_TOPO_CODEC_NOT_FOUND {
        // The file parsed and no decoder is registered for its audio. On a
        // normal Windows that is about this file's codec or layout, not a
        // missing Media Feature Pack (that is `api()` / `MFStartup` failing).
        DecodeError::OsDecoderUnavailable(NO_DECODER_FOR_FILE.to_string())
    } else if hr == MF_E_INVALIDSTREAMNUMBER {
        DecodeError::NoAudioTrack
    } else if hr == MF_E_INVALIDMEDIATYPE {
        DecodeError::UnsupportedCodec
    } else {
        DecodeError::Corrupt
    }
}

fn from_windows(e: windows::core::Error) -> DecodeError {
    map_hresult(e.code())
}

/// The path as the Source Reader wants it: UTF-16, NUL-terminated, and
/// without the `\\?\` prefix `canonicalize` adds, which Media Foundation's
/// file open does not take.
fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    if wide.starts_with(&verbatim) && !wide.starts_with(&unc) {
        wide.drain(..verbatim.len());
    }
    if wide.is_empty() || wide.contains(&0) {
        return Err(DecodeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the file path is empty or holds a NUL",
        )));
    }
    wide.push(0);
    Ok(wide)
}

/// The rate and channel count of a float output type.
fn float_format(media_type: &IMFMediaType) -> Result<(u32, u32)> {
    // SAFETY: attribute reads on a live media type with `'static` keys.
    let (subtype, bits, rate, channels) = unsafe {
        (
            media_type.GetGUID(&MF_MT_SUBTYPE).map_err(from_windows)?,
            media_type
                .GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE)
                .map_err(from_windows)?,
            media_type
                .GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)
                .map_err(from_windows)?,
            media_type
                .GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)
                .map_err(from_windows)?,
        )
    };
    let float: GUID = MFAudioFormat_Float;
    if subtype != float || bits != 32 {
        return Err(DecodeError::UnsupportedCodec);
    }
    if !(1..=768_000).contains(&rate) || channels == 0 {
        return Err(DecodeError::Corrupt);
    }
    if channels > MAX_CHANNELS {
        // A real stream the app does not take, not a damaged one.
        return Err(DecodeError::UnsupportedCodec);
    }
    Ok((rate, channels))
}

/// Append the float samples of one decoded `IMFSample` to `out`.
fn append_sample(sample: &IMFSample, out: &mut Vec<f32>) -> Result<()> {
    // SAFETY: `sample` is live. The buffer is locked for reading, held by a
    // guard that unlocks it, and read only while locked: `data` points at
    // `len` bytes valid until the unlock.
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer().map_err(from_windows)?;
        let mut data: *mut u8 = ptr::null_mut();
        let mut len: u32 = 0;
        buffer
            .Lock(&mut data, None, Some(&mut len))
            .map_err(from_windows)?;
        let _unlock = Locked(&buffer);
        if data.is_null() || len == 0 {
            return Ok(());
        }
        let bytes = std::slice::from_raw_parts(data, len as usize);
        // Read as bytes, not as `f32`s: the buffer's alignment is not ours.
        out.extend(
            bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        );
    }
    Ok(())
}

/// The presentation duration in 100 ns units (`MF_PD_DURATION` of the media
/// source), if the source reports one.
fn presentation_duration(reader: &IMFSourceReader) -> Option<u64> {
    // SAFETY: a call on a live reader with a `'static` key. The value is read
    // as a `u64` only when its type tag says it is one, and the `PROPVARIANT`
    // clears itself when dropped.
    unsafe {
        let value = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .ok()?;
        let inner = &*value.Anonymous.Anonymous;
        (inner.vt == VT_UI8).then_some(inner.Anonymous.uhVal)
    }
}

/// Most frames of priming (before time zero) or of padding after the end that
/// are taken off. An AAC frame is 1,024 samples (2,048 for HE-AAC), so more than
/// a few frames means the timestamps are not what this assumes, and nothing
/// is cut.
const MAX_TRIM_FRAMES: usize = 8_192;

/// Decode the M4A/MP4 file at `path` with Media Foundation.
pub(crate) fn decode_path(path: &Path) -> Result<DecodedAudio> {
    let api = api()?;
    let wide = wide_path(path)?;
    let _session = Session::start(api)?;
    decode_with(api, &wide)
}

/// The decode proper. Its COM objects are all dropped when it returns, which
/// is before the caller's `Session` shuts Media Foundation down.
fn decode_with(api: &MfApi, wide_path: &[u16]) -> Result<DecodedAudio> {
    let mut raw: *mut c_void = ptr::null_mut();
    // SAFETY: `wide_path` is NUL-terminated and lives through the call; no
    // attributes are passed; `raw` is a valid place for the interface.
    let created = unsafe {
        (api.create_source_reader_from_url)(PCWSTR(wide_path.as_ptr()), ptr::null_mut(), &mut raw)
    };
    if created.is_err() || raw.is_null() {
        return Err(map_hresult(created));
    }
    // SAFETY: on success the call returned one owned reference to an
    // `IMFSourceReader`, which `from_raw` takes over.
    let reader = unsafe { IMFSourceReader::from_raw(raw) };

    // Only the first audio stream is read (a video file's picture is not).
    // SAFETY: calls on a live reader with plain values.
    unsafe {
        reader
            .SetStreamSelection(ALL_STREAMS, false)
            .map_err(from_windows)?;
        reader
            .SetStreamSelection(AUDIO_STREAM, true)
            .map_err(from_windows)?;
        // Fails with `MF_E_INVALIDSTREAMNUMBER` when there is no audio.
        reader
            .GetNativeMediaType(AUDIO_STREAM, 0)
            .map_err(from_windows)?;
    }

    // Ask for float output and leave the rate and channels to the decoder, so
    // they are the file's own (the Source Reader fills in the rest of the type
    // and loads the AAC decoder to produce it).
    let mut raw_type: *mut c_void = ptr::null_mut();
    // SAFETY: `raw_type` is a valid place for the interface.
    let made = unsafe { (api.create_media_type)(&mut raw_type) };
    if made.is_err() || raw_type.is_null() {
        return Err(map_hresult(made));
    }
    // SAFETY: on success one owned `IMFMediaType` reference, taken over here.
    let wanted = unsafe { IMFMediaType::from_raw(raw_type) };
    // SAFETY: calls on live objects with `'static` keys and values.
    unsafe {
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(from_windows)?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)
            .map_err(from_windows)?;
        wanted
            .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32)
            .map_err(from_windows)?;
        reader
            .SetCurrentMediaType(AUDIO_STREAM, None, &wanted)
            .map_err(from_windows)?;
    }

    // SAFETY: a call on a live reader.
    let current = unsafe { reader.GetCurrentMediaType(AUDIO_STREAM) }.map_err(from_windows)?;
    let (mut rate, mut channels) = float_format(&current)?;

    let mut samples: Vec<f32> = Vec::new();
    let mut empty_reads = 0_u32;
    let mut sample_index = 0_u32;
    let mut lead_frames: Option<usize> = None;
    loop {
        let mut flags: u32 = 0;
        let mut stamp: i64 = 0;
        let mut sample: Option<IMFSample> = None;
        // SAFETY: a synchronous read on a live reader; the out-places are
        // valid for the call.
        unsafe {
            reader
                .ReadSample(
                    AUDIO_STREAM,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut stamp),
                    Some(&mut sample),
                )
                .map_err(from_windows)?;
        }
        let flags = flags as i32;
        if flags & MF_SOURCE_READERF_ERROR.0 != 0 {
            return Err(DecodeError::Corrupt);
        }
        if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 != 0 {
            // The decoder settled on another format (HE-AAC can announce its
            // doubled rate late). Before any audio that is just the format;
            // after some, the samples would not be one stream.
            // SAFETY: a call on a live reader.
            let now = unsafe { reader.GetCurrentMediaType(AUDIO_STREAM) }.map_err(from_windows)?;
            let (new_rate, new_channels) = float_format(&now)?;
            if (new_rate, new_channels) != (rate, channels) {
                if !samples.is_empty() {
                    return Err(DecodeError::Corrupt);
                }
                rate = new_rate;
                channels = new_channels;
            }
        }
        let ended = flags & MF_SOURCE_READERF_ENDOFSTREAM.0 != 0;
        match sample {
            Some(sample) => {
                empty_reads = 0;
                // The sample's time in 100 ns units, as a frame count at the
                // output rate.
                let start_frame = (i128::from(stamp) * i128::from(rate) + 5_000_000) / 10_000_000;
                let before_frames = samples.len() / channels as usize;
                append_sample(&sample, &mut samples)?;
                sample_index += 1;
                // The encoder's priming is the first packet(s): the MP4's edit
                // list starts the timeline after them, so their time is at or
                // before zero while the frames decoded so far are not. The gap
                // between the frames already decoded and the time this sample
                // starts at is the priming, and it is the same for every later
                // sample, so it is read once, at the second (the first one's
                // time is clamped to zero). Frames before time zero are
                // dropped at the end.
                if sample_index == 2 && lead_frames.is_none() {
                    lead_frames = Some(before_frames.saturating_sub(start_frame.max(0) as usize));
                } else if sample_index == 1 && start_frame < 0 {
                    // A source that reports the priming as negative time.
                    lead_frames = Some(usize::try_from(-start_frame).unwrap_or(usize::MAX));
                }
            }
            None => {
                empty_reads += 1;
                // A read that neither ends nor gives audio, over and over,
                // is a reader that is stuck.
                if !ended && empty_reads > 10_000 {
                    return Err(DecodeError::Corrupt);
                }
            }
        }
        if ended {
            break;
        }
    }

    // Whole frames only.
    samples.truncate(samples.len() - samples.len() % channels as usize);
    let ch = channels as usize;
    let decoded_frames = samples.len() / ch;

    // Drop the priming, then the padding after the end (the MP4's duration
    // is the edit list's, so frames past it are the last packet's remainder).
    let lead = lead_frames
        .filter(|&lead| lead <= MAX_TRIM_FRAMES && lead < decoded_frames)
        .unwrap_or(0);
    samples.drain(..lead * ch);
    let duration = presentation_duration(&reader);
    let frames_after_lead = samples.len() / ch;
    if let Some(hns) = duration {
        let wanted = ((u128::from(hns) * u128::from(rate) + 5_000_000) / 10_000_000) as usize;
        if wanted > 0 && wanted < frames_after_lead && frames_after_lead - wanted <= MAX_TRIM_FRAMES
        {
            samples.truncate(wanted * ch);
        }
    }
    Ok(DecodedAudio {
        samples,
        sample_rate: rate,
        channels: channels as u16,
    })
}
