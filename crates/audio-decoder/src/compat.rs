//! What symphonia 0.5.5 did that 0.6.1 no longer does, kept so that every
//! file decodes to the samples, length and errors it did before the upgrade.
//! `tests/pinned_output.rs` holds the decoded output to what 0.5.5 gave.
//!
//! Each of these is a choice 0.6 made differently, not a bug in either, so
//! dropping one is a change to what an import gives and is decided on its own:
//! gapless trimming (`decoder_options()` in `lib.rs`), Vorbis's first packet
//! and the clamp to full scale (`Output`), and the AIFF chunks after the
//! sound data (`read_forward_only`).

use symphonia::core::codecs::audio::well_known::{CODEC_ID_MP3, CODEC_ID_VORBIS};
use symphonia::core::codecs::audio::AudioCodecId;

/// What is done to one stream's decoded audio, packet by packet.
pub(crate) struct Output {
    /// 0.5.5's MP3 and Vorbis decoders clamped every sample to [-1, 1] (the
    /// synthesis in `symphonia-bundle-mp3` and `symphonia-codec-vorbis`);
    /// 0.6 passes a sample past full scale through.
    clamp: bool,
    /// 0.5.5's Vorbis decoder gave no audio for the first packet it decoded,
    /// which has no block before it to overlap with; 0.6's, with gapless
    /// playback off, gives half a block for it, overlapped with silence. Every
    /// packet after it decodes to the same samples in both.
    drop_next: bool,
}

impl Output {
    pub(crate) fn for_codec(codec: AudioCodecId) -> Self {
        Self {
            clamp: codec == CODEC_ID_MP3 || codec == CODEC_ID_VORBIS,
            drop_next: codec == CODEC_ID_VORBIS,
        }
    }

    /// Whether the audio of the packet just decoded is kept: false once, for
    /// a Vorbis stream's first packet.
    pub(crate) fn keep(&mut self) -> bool {
        !std::mem::take(&mut self.drop_next)
    }

    /// Shapes `samples`, the packet's audio just added to the output.
    pub(crate) fn shape(&self, samples: &mut [f32]) {
        if self.clamp {
            for s in samples {
                *s = s.clamp(-1.0, 1.0);
            }
        }
    }
}

/// Whether the file that starts with `head` is read forward-only: an AIFF or
/// AIFF-C file. Given a source it can seek, 0.6's AIFF reader skips over the
/// sound data to read the chunks after it, and so refuses a file cut short
/// inside its sound data (an interrupted recording or copy); 0.5.5's stopped
/// at the sound data and decoded what there was, and so does 0.6's when it
/// cannot seek.
pub(crate) fn read_forward_only(head: &[u8]) -> bool {
    head.len() >= 12 && &head[..4] == b"FORM" && matches!(&head[8..12], b"AIFF" | b"AIFC")
}
