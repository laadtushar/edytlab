//! Time-stretch and pitch-shift primitives (Phase 2, M20).
//!
//! This crate is the seam between the session-level tools
//! (`time_stretch`, `pitch_shift`, `align_to_beat` in `crates/tools`)
//! and the eventual Rubber Band backend that will land in M28.
//!
//! ## Phase 2 status: stub
//!
//! The plan calls for `rubberband-sys` (FFI to the Rubber Band C++
//! library) as the backend. Building it requires:
//!
//! * `librubberband-dev` (or equivalent) on Linux,
//! * a `vcpkg` install on Windows,
//! * `brew install rubberband` (or `vcpkg`) on macOS,
//! * plus a working C++ toolchain on each.
//!
//! M20's risk register flags this FFI layer as Medium risk. To keep the
//! M20 PR scoped to API + caching + tool integration, the actual DSP is
//! deferred to M28. Until then, [`time_stretch`] and [`pitch_shift`]
//! validate their arguments and return [`Error::NotImplemented`].
//!
//! What this crate **does** ship right now:
//!
//! * The public function signatures the M28 backend will fill in.
//! * Argument validation (factor > 0, |semitones| ≤ 48, channel
//!   sanity) so wiring bugs surface early.
//! * The `Error` enum surface for downstream tools to match against.
//!
//! What it **does not** do: any sample-rate conversion, time-stretch,
//! or pitch-shift. A caller that needs working DSP today should treat
//! the parameter as session metadata applied at render time once the
//! audio engine learns about it (M22+) — see
//! `crates/tools/src/tool/{time_stretch,pitch_shift,align_to_beat}.rs`.

pub use shift::pitch_shift;
pub use stretch::time_stretch;

pub mod shift;
pub mod stretch;

/// Errors raised by the time-stretch / pitch-shift primitives.
///
/// `NotImplemented` is the **expected** return value of every
/// successful argument-validation call until M28 wires up Rubber Band.
/// Tools in `crates/tools` translate it into a clear "applied at next
/// render" message.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    /// The DSP backend has not been wired up yet. See the crate-level
    /// docs and `crates/audio-time/README.md`.
    #[error("audio-time backend not yet wired up; see crates/audio-time/README.md")]
    NotImplemented,

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
    if samples_len % channels as usize != 0 {
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
