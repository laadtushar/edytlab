//! Time-stretch and pitch-shift primitives (Phase 2, M20).
//!
//! This crate is the seam between the session-level tools
//! (`time_stretch`, `pitch_shift`, `align_to_beat` in `crates/tools`)
//! and the eventual Rubber Band backend that will land in M28.
//!
//! ## Backend
//!
//! A pure-Rust phase vocoder, not the Rubber Band FFI the original plan
//! called for. Rubber Band needs `librubberband-dev` on Linux, vcpkg on
//! Windows and Homebrew on macOS, plus a C++ toolchain on each; for a
//! project whose CI builds all three targets that is a dependency which
//! breaks every build at once, and the cost of deferring it was that
//! these functions returned `NotImplemented` for long enough that the
//! tools above them started reporting success they had not earned.
//!
//! The vocoder is worse than Rubber Band and available everywhere.
//! Onsets are detected by spectral flux and the synthesis phase is reset
//! on them, so attacks survive a stretch rather than smearing across the
//! window. There is still no phase locking across bins, so dense
//! material keeps some "phasiness", and large factors make that worse.
//! `preserve_formants` is honoured by `pitch_shift` (see `formant.rs`)
//! and is a no-op for `time_stretch`, which moves no frequency and so
//! has no formants to hold in place. The remaining limits are
//! documented on the functions themselves, and none requires a public
//! API change to fix.
//!
//! See `vocoder.rs` for how it works.

pub use shift::pitch_shift;
pub use stretch::time_stretch;
pub use warp::warp_to_grid;

mod formant;
pub mod shift;
pub mod stretch;
mod vocoder;
pub mod warp;

/// Errors raised by the time-stretch / pitch-shift primitives.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    /// Stretch factor must be a positive, finite f32. `factor < 1.0`
    /// slows audio down; `factor > 1.0` speeds it up. The output
    /// duration equals `input_duration / factor`.
    #[error("invalid factor: {0} (must be finite and > 0)")]
    InvalidFactor(f32),

    /// Pitch shift in semitones must be finite and within ±48. Outside
    /// that range Rubber Band's quality degrades severely; the limit is
    /// generous for any realistic mashup use.
    #[error("invalid semitones: {0} (must be finite and within ±48)")]
    InvalidSemitones(f32),

    /// `samples.len()` is not a multiple of `channels`, or `channels`
    /// is zero.
    #[error("input/output channel mismatch: {0}")]
    ChannelMismatch(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Validate a `(samples, channels)` pair: channels must be > 0 and the
/// sample count must divide evenly by channels.
pub(crate) fn check_channels(samples_len: usize, channels: u16) -> Result<()> {
    if channels == 0 {
        return Err(Error::ChannelMismatch("channels must be > 0".into()));
    }
    if !samples_len.is_multiple_of(channels as usize) {
        return Err(Error::ChannelMismatch(format!(
            "{samples_len} samples not divisible by {channels} channels",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_channels_zero_rejected() {
        assert!(matches!(
            check_channels(100, 0),
            Err(Error::ChannelMismatch(_))
        ));
    }

    #[test]
    fn check_channels_uneven_rejected() {
        assert!(matches!(
            check_channels(101, 2),
            Err(Error::ChannelMismatch(_))
        ));
    }

    #[test]
    fn check_channels_ok() {
        assert!(check_channels(100, 2).is_ok());
        assert!(check_channels(100, 1).is_ok());
        assert!(check_channels(0, 1).is_ok());
    }
}
