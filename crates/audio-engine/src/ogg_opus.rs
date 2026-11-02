//! Ogg Opus export (RFC 6716 codec, RFC 7845 mapping), pure Rust.
//!
//! The codec is `rusty-opus` (BSD-3-Clause, no dependencies, no `-sys`
//! crate, bit-exact on the RFC 8251 decoder vectors and interoperable
//! with libopus in both directions by its own matrix). It emits Opus
//! packets only; the Ogg pages and the `OpusHead` / `OpusTags` headers
//! are written here, because the container is a page header, a CRC and
//! two packets, and a dependency for that would buy less than it costs.
//!
//! What RFC 7845 asks of a writer, and where:
//!
//! - **48 kHz granule clock** (§4): positions count 48 kHz frames
//!   whatever the input rate. Opus takes 8, 12, 16, 24 or 48 kHz, so
//!   anything else (44.1 kHz, the usual) is resampled to 48 kHz first
//!   with rubato, and `OpusHead` records the original rate for
//!   information only.
//! - **Pre-skip** (§4.2): the encoder's delay, 120 frames at 48 kHz
//!   (see `PRE_SKIP`; the round-trip test fails at any other value, since
//!   it compares the decoded signal with no lag).
//! - **End trimming** (§4.4): the audio is zero-padded to a whole
//!   20 ms packet plus the pre-skip, and the last page's granule is
//!   `pre_skip + frames`, so a decoder cuts the padding off and the file
//!   is exactly as long as the input.
//! - **Tags** (§5.2): Vorbis comments. Chapters use the Vorbis chapter
//!   convention, `CHAPTERnnn=HH:MM:SS.mmm` with `CHAPTERnnnNAME=`.
//!
//! Mono and stereo only; more channels need multistream and are refused.

use std::io::Write;
use std::path::Path;

use rubato::{FftFixedInOut, Resampler};
use rusty_opus::{Application, OpusEncoder};

use crate::encode::quantise;
use crate::metadata::Tags;
use crate::{Error, Result};

/// Default target bitrate per channel pair, bits per second. 128 kbps
/// stereo is the usual "transparent for music" Opus setting; mono gets
/// half, since Opus spends bits per channel.
pub const OPUS_DEFAULT_KBPS: u32 = 128;
/// Lowest target bitrate Opus defines (RFC 6716 §2.1.1: 6 kb/s).
pub const OPUS_MIN_KBPS: u32 = 6;
/// Highest target bitrate per channel Opus defines (RFC 6716: 256 kb/s
/// a channel), so a mono export tops out at 256 and a stereo one at 512.
pub const OPUS_MAX_KBPS_PER_CHANNEL: u32 = 256;

const RATE: u32 = 48_000;
/// 20 ms, the frame size libopus recommends for music.
const FRAME: usize = 960;
/// `rusty-opus`'s algorithmic delay at 48 kHz: the 2.5 ms CELT overlap.
/// libopus advertises 312 because it also delays its input by 192 frames
/// of "delay compensation"; `rusty-opus` does not, and measured by
/// cross-correlating a decoded round trip (192 frames earlier than 312
/// predicts, error -48 dB at the right lag) the delay is 120.
const PRE_SKIP: u16 = 120;
/// Packets per page: 25 × 20 ms = 500 ms, as `opusenc` does.
const PACKETS_PER_PAGE: usize = 25;

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let mut r = (i as u32) << 24;
        for _ in 0..8 {
            r = if r & 0x8000_0000 != 0 {
                (r << 1) ^ 0x04C1_1DB7
            } else {
                r << 1
            };
        }
        *slot = r;
    }
    t
}

/// One Ogg page (RFC 3533 §6). `packets` are whole packets; the caller
/// keeps them within 255 segments.
struct PageWriter<W: Write> {
    out: W,
    serial: u32,
    seq: u32,
    table: [u32; 256],
}

impl<W: Write> PageWriter<W> {
    fn new(out: W, serial: u32) -> Self {
        Self {
            out,
            serial,
            seq: 0,
            table: crc_table(),
        }
    }

    fn page(&mut self, flags: u8, granule: i64, packets: &[&[u8]]) -> Result<()> {
        let mut segments: Vec<u8> = Vec::new();
        for p in packets {
            let mut left = p.len();
            while left >= 255 {
                segments.push(255);
                left -= 255;
            }
            segments.push(left as u8); // 0..=254: ends the packet
        }
        if segments.len() > 255 {
            return Err(Error::Encode(
                "ogg: page would need over 255 segments".into(),
            ));
        }
        let mut buf: Vec<u8> = Vec::with_capacity(27 + segments.len() + 4096);
        buf.extend_from_slice(b"OggS");
        buf.push(0); // version
        buf.push(flags);
        buf.extend_from_slice(&granule.to_le_bytes());
        buf.extend_from_slice(&self.serial.to_le_bytes());
        buf.extend_from_slice(&self.seq.to_le_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0]); // checksum, patched below
        buf.push(segments.len() as u8);
        buf.extend_from_slice(&segments);
        for p in packets {
            buf.extend_from_slice(p);
        }
        let mut crc = 0u32;
        for &b in &buf {
            crc = (crc << 8) ^ self.table[((crc >> 24) as u8 ^ b) as usize];
        }
        buf[22..26].copy_from_slice(&crc.to_le_bytes());
        self.out.write_all(&buf)?;
        self.seq += 1;
        Ok(())
    }
}

fn head(channels: u8, input_rate: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(19);
    v.extend_from_slice(b"OpusHead");
    v.push(1); // version
    v.push(channels);
    v.extend_from_slice(&PRE_SKIP.to_le_bytes());
    v.extend_from_slice(&input_rate.to_le_bytes());
    v.extend_from_slice(&0i16.to_le_bytes()); // output gain
    v.push(0); // mapping family 0: mono or stereo
    v
}

fn timestamp(sec: f64) -> String {
    let ms = (sec.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

fn tags_packet(tags: &Tags) -> Vec<u8> {
    let mut comments: Vec<String> = Vec::new();
    let mut add = |k: &str, v: &Option<String>| {
        if let Some(v) = v {
            comments.push(format!("{k}={v}"));
        }
    };
    add("TITLE", &tags.title);
    add("ARTIST", &tags.artist);
    add("ALBUM", &tags.album);
    add("DATE", &tags.year);
    add("COMMENT", &tags.comment);
    for (i, c) in tags.chapters.iter().enumerate() {
        comments.push(format!("CHAPTER{:03}={}", i + 1, timestamp(c.start_sec)));
        comments.push(format!("CHAPTER{:03}NAME={}", i + 1, c.title));
    }
    let vendor = b"edytlab";
    let mut v = Vec::new();
    v.extend_from_slice(b"OpusTags");
    v.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    v.extend_from_slice(vendor);
    v.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for c in &comments {
        v.extend_from_slice(&(c.len() as u32).to_le_bytes());
        v.extend_from_slice(c.as_bytes());
    }
    v
}

/// Interleaved `samples` at `rate` to 48 kHz, `round(frames * 48000 / rate)`
/// frames long, sample 0 aligned to sample 0 (the resampler's delay is
/// taken out, as the renderer's is).
fn to_48k(samples: &[f32], rate: u32, channels: usize) -> Result<Vec<f32>> {
    if rate == RATE || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let in_frames = samples.len() / channels;
    let wanted =
        ((in_frames as u128 * u128::from(RATE) + u128::from(rate) / 2) / u128::from(rate)) as usize;
    let mut rs = FftFixedInOut::<f32>::new(rate as usize, RATE as usize, 1024, channels)
        .map_err(Error::ResamplerInit)?;
    let delay = rs.output_delay();
    let mut in_buf: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut out_buf = vec![vec![0.0f32; rs.output_frames_max()]; channels];
    let mut planes: Vec<Vec<f32>> = vec![Vec::with_capacity(wanted + delay); channels];
    let mut pos = 0usize;
    while planes[0].len() < wanted + delay {
        let need = rs.input_frames_next();
        let avail = in_frames.saturating_sub(pos).min(need);
        for (ch, buf) in in_buf.iter_mut().enumerate() {
            buf.clear();
            buf.extend((0..avail).map(|f| samples[(pos + f) * channels + ch]));
            buf.resize(need, 0.0);
        }
        let (_, written) = rs
            .process_into_buffer(&in_buf, &mut out_buf, None)
            .map_err(Error::ResamplerProcess)?;
        if written == 0 {
            break;
        }
        for (plane, out) in planes.iter_mut().zip(&out_buf) {
            plane.extend_from_slice(&out[..written]);
        }
        pos += avail;
    }
    let mut out = Vec::with_capacity(wanted * channels);
    for f in 0..wanted {
        for plane in &planes {
            out.push(plane.get(delay + f).copied().unwrap_or(0.0));
        }
    }
    Ok(out)
}

/// Write interleaved `samples` to an Ogg Opus file at `out`.
///
/// `bitrate_kbps` is the target for the whole stream (VBR around it);
/// `None` uses [`OPUS_DEFAULT_KBPS`] for stereo and half for mono. Lossy:
/// a round trip is a psychoacoustic approximation, not the input.
pub fn write_ogg_opus(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    bitrate_kbps: Option<u32>,
    tags: &Tags,
    out: &Path,
) -> Result<()> {
    if !(1..=2).contains(&channels) {
        return Err(Error::UnsupportedChannelMap {
            from: channels,
            to: 2,
        });
    }
    if sample_rate == 0 {
        return Err(Error::Encode("ogg: sample rate is 0".into()));
    }
    let ch = channels as usize;
    // The 16-bit grid the other encoders write, so one render exported
    // as WAV and as OGG starts from the same numbers.
    let quantised: Vec<f32> = samples[..samples.len() / ch * ch]
        .iter()
        .map(|&s| quantise(s) as f32 / 32_768.0)
        .collect();
    let mut pcm = to_48k(&quantised, sample_rate, ch)?;
    let frames = pcm.len() / ch;

    // Whole packets covering the audio plus the encoder's delay.
    let packets = (frames + PRE_SKIP as usize).div_ceil(FRAME).max(1);
    pcm.resize(packets * FRAME * ch, 0.0);

    let mut enc = OpusEncoder::new(RATE as i32, ch, Application::Audio)
        .map_err(|e| Error::Encode(format!("opus: {e:?}")))?;
    let kbps = bitrate_kbps.unwrap_or(OPUS_DEFAULT_KBPS * channels as u32 / 2);
    // Checked here rather than left to the encoder: a young crate, and a
    // release build aborts on a panic (#908), so what it is handed stays
    // inside the range Opus defines.
    let max_kbps = OPUS_MAX_KBPS_PER_CHANNEL * channels as u32;
    if !(OPUS_MIN_KBPS..=max_kbps).contains(&kbps) {
        return Err(Error::Encode(format!(
            "ogg: bitrate_kbps must be between {OPUS_MIN_KBPS} and {max_kbps} for {channels} channel(s); got {kbps}"
        )));
    }
    enc.bitrate_bps = (kbps * 1000) as i32;

    let serial = {
        // Any value will do; derive one from the content so an export is
        // reproducible byte for byte.
        let mut h = 0x811C_9DC5u32;
        for &b in out.as_os_str().as_encoded_bytes() {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        h ^ frames as u32
    };
    let file = std::fs::File::create(out)?;
    let mut ogg = PageWriter::new(std::io::BufWriter::new(file), serial);
    ogg.page(0x02, 0, &[&head(channels as u8, sample_rate)])?;
    let tags = tags_packet(tags);
    // One page holds 255 lacing values, and a packet of n bytes needs
    // n / 255 + 1 of them, so 65,024 bytes is the most it can carry.
    if tags.len() >= 255 * 255 {
        return Err(Error::Encode(format!(
            "ogg: the tags and chapters take {} bytes, over the 65,024 an Ogg page holds; shorten them",
            tags.len()
        )));
    }
    ogg.page(0, 0, &[&tags])?;

    let final_granule = i64::from(PRE_SKIP) + frames as i64;
    let mut batch: Vec<Vec<u8>> = Vec::new();
    let mut buf = vec![0u8; 4000];
    for i in 0..packets {
        let n = enc
            .encode(&pcm[i * FRAME * ch..(i + 1) * FRAME * ch], FRAME, &mut buf)
            .map_err(|e| Error::Encode(format!("opus encode: {e:?}")))?;
        batch.push(buf[..n].to_vec());
        let last = i + 1 == packets;
        if batch.len() == PACKETS_PER_PAGE || last {
            let granule = if last {
                final_granule
            } else {
                ((i + 1) * FRAME) as i64
            };
            let refs: Vec<&[u8]> = batch.iter().map(Vec::as_slice).collect();
            ogg.page(if last { 0x04 } else { 0 }, granule, &refs)?;
            batch.clear();
        }
    }
    ogg.out.flush()?;
    Ok(())
}
