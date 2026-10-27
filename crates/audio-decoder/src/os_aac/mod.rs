//! M4A/AAC through the operating system's own decoder (#764).
//!
//! AAC decoders need a patent licence from Via LA for "manufacturers or
//! developers of end-user encoder and/or decoder products", and the project
//! has none, so no AAC decoder is built into the app: not symphonia's `aac`
//! and `isomp4`, not FFmpeg, fdk-aac or faad2. macOS and Windows ship a
//! decoder of their own, licensed by Apple and Microsoft, and the app asks
//! the system to decode (owner's decision on issue #764):
//!
//! - macOS: AudioToolbox, Extended Audio File Services (`macos`).
//! - Windows: Media Foundation's Source Reader and AAC decoder
//!   (`media_foundation`).
//! - Linux and everything else: no system decoder, so the file is refused
//!   with [`crate::DecodeError::UnsupportedFormat`] as before.
//!
//! A file is M4A/MP4 by its bytes (an `ftyp` box), never by its extension.
//! Only such files reach the system decoder; every other format keeps going
//! through symphonia.

use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod media_foundation;

#[cfg(target_os = "macos")]
use macos::decode_path;
#[cfg(target_os = "windows")]
use media_foundation::decode_path;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::{DecodedAudio, Result};

/// Whether this platform has a system decoder to hand M4A files to.
pub(crate) const AVAILABLE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Whether the start of a file says it is ISO base media: a box size, then
/// `ftyp` (M4A, MP4, MOV, 3GP).
pub(crate) fn is_m4a(head: &[u8]) -> bool {
    head.len() >= 12 && &head[4..8] == b"ftyp"
}

/// Decode the M4A/MP4 file at `path` with the system decoder.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn decode_m4a(path: &Path) -> Result<DecodedAudio> {
    decode_path(path)
}

/// Decode M4A/MP4 bytes already in memory. Both system decoders open files,
/// so the bytes go to a file of their own in the temporary folder for the
/// length of the call.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn decode_m4a_bytes(bytes: &[u8]) -> Result<DecodedAudio> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Removes the file when the call ends, however it ends.
    struct Remove<'a>(&'a Path);
    impl Drop for Remove<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "edytlab-m4a-{}-{}.m4a",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // `create_new` refuses a name that exists, so a file or link someone else
    // put there is never written through.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let _remove = Remove(&path);
    file.write_all(bytes)?;
    drop(file);
    decode_path(&path)
}

/// `Some(result)` when `head`, the first bytes the caller read from `path`,
/// is M4A/MP4 and this platform decodes it; `None` for any other file, and
/// for any file on a platform with no system decoder, which the caller then
/// handles as before. The caller reads only the first bytes of `path` and
/// passes them, so the check does not consume a path that can be read only
/// once (a pipe); an M4A is then decoded by reopening `path`, which a pipe
/// cannot be.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn decode_file_if_m4a(path: &Path, head: &[u8]) -> Option<Result<DecodedAudio>> {
    is_m4a(&head[..head.len().min(crate::SNIFF_BYTES)]).then(|| decode_m4a(path))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn decode_file_if_m4a(
    _path: &Path,
    _head: &[u8],
) -> Option<crate::Result<crate::DecodedAudio>> {
    None
}

/// Like [`decode_file_if_m4a`], for bytes already in memory.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn decode_bytes_if_m4a(bytes: &[u8]) -> Option<Result<DecodedAudio>> {
    is_m4a(&bytes[..bytes.len().min(crate::SNIFF_BYTES)]).then(|| decode_m4a_bytes(bytes))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn decode_bytes_if_m4a(_bytes: &[u8]) -> Option<crate::Result<crate::DecodedAudio>> {
    None
}
