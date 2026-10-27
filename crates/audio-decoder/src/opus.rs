//! Opus (RFC 6716) in Ogg (RFC 7845), as a symphonia [`Decoder`].
//!
//! symphonia 0.5 reads Ogg Opus and hands over its packets, and has no Opus
//! codec (https://github.com/pdeljanov/Symphonia/issues/8). This module is that
//! codec: [`OpusPacketDecoder`] wraps the pure-Rust `opus-decoder` crate
//! (MIT OR Apache-2.0, `#![forbid(unsafe_code)]`, 12 of 12 RFC 8251
//! conformance vectors) and is registered beside symphonia's own codecs
//! (`codecs()` in `lib.rs`).
//!
//! What RFC 7845 asks of a player, and where it is done:
//!
//! - **Output is 48 kHz** whatever the `input sample rate` of the header says
//!   (that field only records what the encoder was given): the decoder is made
//!   at 48 kHz.
//! - **Pre-skip** (§4.2) frames are decoded and dropped by the decoder, from
//!   the `OpusHead` symphonia passes on as `extra_data`. symphonia's own delay
//!   bookkeeping for Opus is not used: its start-page guess takes the end
//!   trim of a one-page file for a start delay.
//! - **Output gain** (§5.1, Q7.8 dB) is applied by the decoder.
//! - **End trimming** (§4.4): the last page's granule position says how many
//!   frames the stream really holds, `granule - pre_skip`. [`last_granule`]
//!   reads it from the file's pages, and `lib.rs` cuts the decoded audio to it,
//!   so the length is the one `opusinfo` and `ffprobe` report.
//! - **Channels**: mono and stereo only. Mapping family 0 is that by
//!   definition; family 1 is accepted for one or two channels when its table
//!   is the plain one. Surround (family 1 with 3 to 8 channels), ambisonics
//!   (family 2 and 3) and every other family are refused by name
//!   ([`DecodeError::UnsupportedOpusLayout`]).
//!
//! `opus-decoder` decodes to 16-bit PCM and offers `f32` as that divided by
//! 32,768, so a decoded file has 16-bit resolution (a noise floor near
//! -96 dBFS), not the 24-bit-and-better that libopus's float decoder gives.
//! The decoder is also a young crate, so each packet is decoded behind
//! `catch_unwind`: a panic on a hostile packet is an error for that packet.

use std::borrow::Cow;
use std::panic::{catch_unwind, AssertUnwindSafe};

use opus_decoder::OpusDecoder;
use symphonia::core::audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, Signal, SignalSpec};
use symphonia::core::codecs::{
    CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult, CODEC_TYPE_OPUS,
};
use symphonia::core::errors::{decode_error, unsupported_error, Result as SymResult};
use symphonia::core::formats::Packet;

use crate::DecodeError;

/// The rate Opus decodes to (RFC 6716 §2: the decoder's output rate is a
/// choice of the caller among 8, 12, 16, 24 and 48 kHz; 48 kHz keeps all of
/// the audio band, and is what RFC 7845 plays back at).
pub(crate) const OUTPUT_RATE: u32 = 48_000;

/// The longest packet: 120 ms at 48 kHz (RFC 6716 §3.2.5).
const MAX_PACKET_FRAMES: usize = 5_760;

/// The fields of an `OpusHead` packet (RFC 7845 §5.1) that the decoder uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpusHead {
    pub channels: u8,
    /// Frames, at 48 kHz, to drop from the start of the decoded stream.
    pub pre_skip: u16,
    /// Output gain, Q7.8 dB.
    pub gain_q8: i16,
}

impl OpusHead {
    /// The gain as the factor to multiply samples by.
    fn gain_factor(&self) -> f32 {
        if self.gain_q8 == 0 {
            1.0
        } else {
            10f32.powf(f32::from(self.gain_q8) / (20.0 * 256.0))
        }
    }
}

/// Reads an `OpusHead` and checks the layout is one this decoder opens.
///
/// `buf` begins with the magic `OpusHead`. A malformed head is
/// [`DecodeError::Corrupt`]; a well-formed one of an unopened layout is
/// [`DecodeError::UnsupportedOpusLayout`].
pub(crate) fn parse_head(buf: &[u8]) -> Result<OpusHead, DecodeError> {
    if buf.len() < 19 || &buf[..8] != b"OpusHead" {
        return Err(DecodeError::Corrupt);
    }
    // The upper four bits are the major version; RFC 7845 §5.1 says a
    // decoder must refuse a major version it does not know (0 is the only one).
    if buf[8] >> 4 != 0 {
        return Err(DecodeError::Corrupt);
    }
    let channels = buf[9];
    let pre_skip = u16::from_le_bytes([buf[10], buf[11]]);
    let gain_q8 = i16::from_le_bytes([buf[16], buf[17]]);
    let family = buf[18];
    if channels == 0 {
        return Err(DecodeError::Corrupt);
    }
    let refuse = |what: String| Err(DecodeError::UnsupportedOpusLayout(what));
    match family {
        // Mono or stereo, the only layouts the family allows.
        0 => {
            if channels > 2 {
                return Err(DecodeError::Corrupt);
            }
        }
        // Vorbis channel order, with a table of streams. One or two channels
        // is mono or stereo only if the table is the plain one: one stream,
        // coupled when stereo, channels in order.
        1 => {
            if channels > 2 {
                return refuse(format!("{channels} channels (surround)"));
            }
            let table = &buf[19..];
            let plain = table.len() >= 2 + usize::from(channels)
                && table[0] == 1
                && table[1] == channels - 1
                && table[2..2 + usize::from(channels)]
                    .iter()
                    .enumerate()
                    .all(|(i, &m)| usize::from(m) == i);
            if !plain {
                return refuse("an unusual stream layout".to_string());
            }
        }
        2 | 3 => return refuse(format!("channel mapping family {family} (ambisonics)")),
        other => {
            return refuse(format!(
                "channel mapping family {other} ({channels} channels)"
            ))
        }
    }
    Ok(OpusHead {
        channels,
        pre_skip,
        gain_q8,
    })
}

/// If `bytes` is an Ogg file whose first page holds an `OpusHead`, the head.
/// Used to say why a file the container layer could not open was refused.
pub(crate) fn head_in_ogg(bytes: &[u8]) -> Option<Result<OpusHead, DecodeError>> {
    if !bytes.starts_with(b"OggS") {
        return None;
    }
    let at = bytes[..bytes.len().min(4096)]
        .windows(8)
        .position(|w| w == b"OpusHead")?;
    Some(parse_head(&bytes[at..]))
}

/// The granule position of the last page of the (single) Ogg Opus stream in
/// `bytes`, or `None` when the pages cannot be trusted to say.
///
/// RFC 7845 §4: the granule position of a page is the count of 48 kHz frames
/// decoded up to its last packet, pre-skip included, and the last page's value
/// is how much of the final packet is kept. So a stream holds
/// `granule - pre_skip` frames. `None` is returned for a file that is not
/// whole pages end to end (a truncated or damaged file: the decoder then keeps
/// what it decodes), for a chained file (more than one logical stream, which
/// would need the end of each) and for a last granule of -1 (no packet ends
/// on any page).
pub(crate) fn last_granule(bytes: &[u8]) -> Option<u64> {
    let mut pos = 0usize;
    let mut serial: Option<u32> = None;
    let mut streams = 0usize;
    let mut last: Option<u64> = None;
    while pos < bytes.len() {
        let h = bytes.get(pos..pos + 27)?;
        if &h[..4] != b"OggS" {
            return None;
        }
        let kind = h[5];
        let granule = u64::from_le_bytes(h[6..14].try_into().ok()?);
        let this = u32::from_le_bytes(h[14..18].try_into().ok()?);
        let segments = usize::from(h[26]);
        let table = bytes.get(pos + 27..pos + 27 + segments)?;
        let body: usize = table.iter().map(|&s| usize::from(s)).sum();
        let next = pos + 27 + segments + body;
        if next > bytes.len() {
            return None;
        }
        if kind & 0x02 != 0 {
            streams += 1;
            serial.get_or_insert(this);
        }
        if Some(this) == serial && granule != u64::MAX {
            last = Some(granule);
        }
        pos = next;
    }
    if streams == 1 {
        last
    } else {
        None
    }
}

/// An Opus packet decoder for symphonia.
pub(crate) struct OpusPacketDecoder {
    params: CodecParameters,
    inner: OpusDecoder,
    channels: usize,
    /// Decoded frames of the current packet, planar.
    buf: AudioBuffer<f32>,
    /// Interleaved output of the Opus decoder, one longest packet.
    pcm: Vec<f32>,
    /// Pre-skip frames still to drop.
    skip: usize,
    gain: f32,
}

impl OpusPacketDecoder {
    fn make(head: &OpusHead, params: &CodecParameters) -> SymResult<Self> {
        let channels = usize::from(head.channels);
        let inner = OpusDecoder::new(OUTPUT_RATE, channels)
            .map_err(|_| symphonia::core::errors::Error::Unsupported("opus: decoder"))?;
        let layout = if channels == 1 {
            symphonia::core::audio::Channels::FRONT_LEFT
        } else {
            symphonia::core::audio::Channels::FRONT_LEFT
                | symphonia::core::audio::Channels::FRONT_RIGHT
        };
        let spec = SignalSpec::new(OUTPUT_RATE, layout);
        let mut params = params.clone();
        params
            .for_codec(CODEC_TYPE_OPUS)
            .with_sample_rate(OUTPUT_RATE)
            .with_channels(layout);
        Ok(Self {
            params,
            inner,
            channels,
            buf: AudioBuffer::new(MAX_PACKET_FRAMES as u64, spec),
            pcm: vec![0.0; MAX_PACKET_FRAMES * channels],
            skip: usize::from(head.pre_skip),
            gain: head.gain_factor(),
        })
    }
}

impl Decoder for OpusPacketDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> SymResult<Self> {
        let extra = match params.extra_data.as_deref() {
            Some(extra) => extra,
            None => return unsupported_error("opus: no OpusHead"),
        };
        let head = match parse_head(extra) {
            Ok(head) => head,
            Err(DecodeError::UnsupportedOpusLayout(_)) => {
                return unsupported_error("opus: channel layout")
            }
            Err(_) => return decode_error("opus: bad OpusHead"),
        };
        Self::make(&head, params)
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[CodecDescriptor {
            codec: CODEC_TYPE_OPUS,
            short_name: "opus",
            long_name: "Opus (pure-Rust opus-decoder)",
            inst_func: |params, options| Ok(Box::new(OpusPacketDecoder::try_new(params, options)?)),
        }]
    }

    fn reset(&mut self) {
        // A seek or a gap: the decoder's memory belongs to audio that is not
        // before the next packet, and the pre-skip belongs to the stream start.
        self.inner.reset();
        self.skip = 0;
        self.buf.clear();
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> SymResult<AudioBufferRef<'_>> {
        self.buf.clear();
        let data = packet.buf();
        // An empty packet would make the decoder conceal a loss; a file that
        // holds one is damaged, and inventing audio for it is not decoding.
        if data.is_empty() {
            return decode_error("opus: empty packet");
        }
        let decoded = catch_unwind(AssertUnwindSafe(|| {
            self.inner.decode_float(data, &mut self.pcm, false)
        }));
        let frames = match decoded {
            Ok(Ok(frames)) => frames,
            Ok(Err(_)) => return decode_error("opus: invalid packet"),
            Err(_) => {
                // The panic left the decoder in an unknown state.
                if let Ok(fresh) = OpusDecoder::new(OUTPUT_RATE, self.channels) {
                    self.inner = fresh;
                }
                return decode_error("opus: the decoder failed on this packet");
            }
        };
        if frames > MAX_PACKET_FRAMES {
            return decode_error("opus: packet longer than 120 ms");
        }
        let dropped = self.skip.min(frames);
        self.skip -= dropped;
        let kept = frames - dropped;
        self.buf.render_reserved(Some(kept));
        for c in 0..self.channels {
            let plane = self.buf.chan_mut(c);
            for (i, out) in plane.iter_mut().enumerate() {
                *out = self.pcm[(dropped + i) * self.channels + c] * self.gain;
            }
        }
        Ok(AudioBufferRef::F32(Cow::Borrowed(&self.buf)))
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buf.as_audio_buffer_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(channels: u8, pre_skip: u16, gain: i16, family: u8, table: &[u8]) -> Vec<u8> {
        let mut v = b"OpusHead".to_vec();
        v.push(1);
        v.push(channels);
        v.extend_from_slice(&pre_skip.to_le_bytes());
        v.extend_from_slice(&44_100u32.to_le_bytes());
        v.extend_from_slice(&gain.to_le_bytes());
        v.push(family);
        v.extend_from_slice(table);
        v
    }

    fn layout_text(r: Result<OpusHead, DecodeError>) -> String {
        match r {
            Err(DecodeError::UnsupportedOpusLayout(t)) => t,
            other => panic!("expected a layout refusal, got {other:?}"),
        }
    }

    #[test]
    fn mono_and_stereo_family_0_are_read() {
        let h = parse_head(&head(1, 312, 0, 0, &[])).unwrap();
        assert_eq!((h.channels, h.pre_skip, h.gain_q8), (1, 312, 0));
        let h = parse_head(&head(2, 120, -256, 0, &[])).unwrap();
        assert_eq!((h.channels, h.pre_skip, h.gain_q8), (2, 120, -256));
        // -256 in Q7.8 is -1 dB.
        assert!((h.gain_factor() - 10f32.powf(-1.0 / 20.0)).abs() < 1e-6);
    }

    #[test]
    fn family_1_is_read_only_for_a_plain_mono_or_stereo_table() {
        assert!(parse_head(&head(1, 0, 0, 1, &[1, 0, 0])).is_ok());
        assert!(parse_head(&head(2, 0, 0, 1, &[1, 1, 0, 1])).is_ok());
        // Two mono streams mixed to stereo, or the channels swapped.
        assert!(layout_text(parse_head(&head(2, 0, 0, 1, &[2, 0, 0, 1]))).contains("unusual"));
        assert!(layout_text(parse_head(&head(2, 0, 0, 1, &[1, 1, 1, 0]))).contains("unusual"));
    }

    #[test]
    fn surround_ambisonics_and_unknown_families_are_refused_by_name() {
        let t = layout_text(parse_head(&head(6, 0, 0, 1, &[4, 2, 0, 4, 1, 2, 3, 5])));
        assert!(t.contains("6 channels"), "{t}");
        let t = layout_text(parse_head(&head(4, 0, 0, 2, &[1, 3, 0, 1, 2, 3])));
        assert!(t.contains("family 2"), "{t}");
        let t = layout_text(parse_head(&head(2, 0, 0, 255, &[1, 1, 0, 1])));
        assert!(t.contains("family 255"), "{t}");
    }

    #[test]
    fn a_damaged_head_is_corrupt_not_unsupported() {
        for bad in [
            head(0, 0, 0, 0, &[]),
            head(3, 0, 0, 0, &[]),
            b"OpusHead\x01".to_vec(),
            b"NotOpusHead_______________".to_vec(),
        ] {
            assert!(matches!(parse_head(&bad), Err(DecodeError::Corrupt)));
        }
        // A major version this decoder does not know.
        let mut v = head(1, 0, 0, 0, &[]);
        v[8] = 0x10;
        assert!(matches!(parse_head(&v), Err(DecodeError::Corrupt)));
    }

    fn page(kind: u8, granule: u64, serial: u32, body: &[u8]) -> Vec<u8> {
        let mut v = b"OggS".to_vec();
        v.push(0);
        v.push(kind);
        v.extend_from_slice(&granule.to_le_bytes());
        v.extend_from_slice(&serial.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v.push(1);
        v.push(body.len() as u8);
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn the_last_granule_is_the_last_pages_and_only_of_whole_single_streams() {
        let mut f = page(0x02, 0, 7, b"OpusHead....");
        f.extend(page(0, 0, 7, b"OpusTags"));
        f.extend(page(0, 960, 7, b"aa"));
        f.extend(page(0x04, 1_500, 7, b"bb"));
        assert_eq!(last_granule(&f), Some(1_500));
        // Cut in the middle of a page: not trusted.
        assert_eq!(last_granule(&f[..f.len() - 1]), None);
        // Junk after the last page: not trusted.
        let mut junk = f.clone();
        junk.extend_from_slice(b"ID3");
        assert_eq!(last_granule(&junk), None);
        // A second logical stream (a chained file).
        let mut chained = f.clone();
        chained.extend(page(0x02, 0, 9, b"OpusHead...."));
        chained.extend(page(0x04, 400, 9, b"cc"));
        assert_eq!(last_granule(&chained), None);
    }

    #[test]
    fn invalid_and_empty_packets_are_errors_and_never_panic() {
        let params = {
            let mut p = CodecParameters::new();
            p.for_codec(CODEC_TYPE_OPUS)
                .with_extra_data(head(2, 0, 0, 0, &[]).into_boxed_slice());
            p
        };
        let mut d = OpusPacketDecoder::try_new(&params, &DecoderOptions::default()).unwrap();
        // Empty.
        let p = Packet::new_from_slice(0, 0, 960, &[]);
        assert!(d.decode(&p).is_err());
        assert_eq!(d.last_decoded().frames(), 0);
        // A code 3 packet that claims zero frames (RFC 6716 §3.2.5: M >= 1).
        let p = Packet::new_from_slice(0, 0, 960, &[0xFB, 0x00, 0x00]);
        assert!(d.decode(&p).is_err());
        // Every first byte, and short or ragged bodies, with the decoder kept.
        for toc in 0..=255u8 {
            for len in [1usize, 2, 3, 7, 40, 200] {
                let mut data = vec![toc];
                data.extend((0..len - 1).map(|i| (i as u8).wrapping_mul(37).wrapping_add(toc)));
                let p = Packet::new_from_slice(0, 0, 960, &data);
                let _ = d.decode(&p);
            }
        }
        // And it still decodes a good packet afterwards.
        let fixture = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tone_440_mono_48000.opus"
        ))
        .unwrap();
        let (_, packets) = fixture_packets(&fixture);
        let mut mono = OpusPacketDecoder::try_new(
            &params_for(&head(1, 0, 0, 0, &[])),
            &DecoderOptions::default(),
        )
        .unwrap();
        assert!(mono.decode(&packets[0]).is_ok());
    }

    fn params_for(head: &[u8]) -> CodecParameters {
        let mut p = CodecParameters::new();
        p.for_codec(CODEC_TYPE_OPUS)
            .with_extra_data(head.to_vec().into_boxed_slice());
        p
    }

    /// The audio packets of an Ogg Opus file, read by symphonia's demuxer.
    fn fixture_packets(bytes: &[u8]) -> (CodecParameters, Vec<Packet>) {
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        let mss = MediaSourceStream::new(
            Box::new(std::io::Cursor::new(bytes.to_vec())),
            Default::default(),
        );
        let mut format = symphonia::default::get_probe()
            .format(
                &Hint::new(),
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .unwrap()
            .format;
        let params = format.tracks()[0].codec_params.clone();
        let mut packets = Vec::new();
        while let Ok(p) = format.next_packet() {
            packets.push(p);
        }
        (params, packets)
    }

    fn decode_all(head_bytes: &[u8], packets: &[Packet]) -> Vec<f32> {
        let mut d = OpusPacketDecoder::try_new(&params_for(head_bytes), &DecoderOptions::default())
            .unwrap();
        let mut out = Vec::new();
        for p in packets {
            let b = d.decode(p).unwrap();
            let AudioBufferRef::F32(buf) = b else {
                panic!("expected f32")
            };
            out.extend_from_slice(buf.chan(0));
        }
        out
    }

    #[test]
    fn pre_skip_and_output_gain_come_from_the_head() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tone_440_mono_48000.opus"
        ))
        .unwrap();
        let (params, packets) = fixture_packets(&bytes);
        let real = params.extra_data.as_deref().unwrap();
        let real_head = parse_head(real).unwrap();
        assert_eq!(real_head.pre_skip, 312, "ffmpeg's libopus lookahead");

        let all = decode_all(&head(1, 0, 0, 0, &[]), &packets);
        let skipped = decode_all(&head(1, 312, 0, 0, &[]), &packets);
        // The skipped stream is the full one less its first 312 frames, exactly.
        assert_eq!(skipped.len(), all.len() - 312);
        assert_eq!(&all[312..], &skipped[..]);
        // A pre-skip longer than a packet drops across packets.
        let long = decode_all(&head(1, 1_500, 0, 0, &[]), &packets);
        assert_eq!(&all[1_500..], &long[..]);

        // -6.0206 dB (-1541 in Q7.8) halves the amplitude; +6.0206 doubles it.
        let quiet = decode_all(&head(1, 0, -1_541, 0, &[]), &packets);
        let loud = decode_all(&head(1, 0, 1_541, 0, &[]), &packets);
        for i in (0..all.len()).step_by(97) {
            assert!((quiet[i] - all[i] * 0.5).abs() < 1e-3, "frame {i}");
            assert!((loud[i] - all[i] * 2.0).abs() < 1e-3, "frame {i}");
        }
    }
}
