//! Build a punch monitor without keeping decoded copies of its two renders.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

use super::{PunchError, Region, CROSSFADE_SEC};

type Reader = hound::WavReader<std::io::BufReader<std::fs::File>>;

fn read_error(e: &dyn std::fmt::Display) -> PunchError {
    PunchError::Convert(format!("could not read the mix: {e}"))
}

fn too_large() -> PunchError {
    PunchError::Convert("the punch monitor is too large".into())
}

struct Source {
    reader: Reader,
    rate: u32,
    channels: usize,
    frames: usize,
}

impl Source {
    fn open(path: &Path) -> Result<Self, PunchError> {
        let reader = Reader::open(path).map_err(|e| read_error(&e))?;
        let spec = reader.spec();
        match (spec.sample_format, spec.bits_per_sample) {
            (hound::SampleFormat::Float, 32) | (hound::SampleFormat::Int, 8..=32) => {}
            (fmt, bits) => return Err(read_error(&format!("a {bits}-bit {fmt:?} WAV"))),
        }
        // Hound has positioned this same file just after the data chunk's
        // length field. Check that exact length against the file extent before
        // allocating from it; its sample width can exceed bits_per_sample.
        let mut file = reader.into_inner();
        file.seek(SeekFrom::Current(-4))
            .map_err(|e| read_error(&e))?;
        let mut length = [0u8; 4];
        file.read_exact(&mut length).map_err(|e| read_error(&e))?;
        let start = file.stream_position().map_err(|e| read_error(&e))?;
        let end = start
            .checked_add(u64::from(u32::from_le_bytes(length)))
            .ok_or_else(too_large)?;
        if end > file.get_ref().metadata().map_err(|e| read_error(&e))?.len() {
            return Err(read_error(&"unexpected end of WAV"));
        }
        file.rewind().map_err(|e| read_error(&e))?;
        let reader = Reader::new(file).map_err(|e| read_error(&e))?;
        let channels = (spec.channels as usize).max(1);
        let frames = reader.len() as usize / channels;
        Ok(Self {
            reader,
            rate: spec.sample_rate,
            channels,
            frames,
        })
    }

    fn output_frames(&self, rate: u32) -> Result<usize, PunchError> {
        if self.rate == 0 || rate == 0 {
            return Err(PunchError::Convert("a sample rate of 0 Hz".into()));
        }
        if self.rate == rate {
            Ok(self.frames)
        } else {
            let frames = (self.frames as u64)
                .checked_mul(u64::from(rate))
                .and_then(|n| n.checked_add(u64::from(self.rate) / 2))
                .ok_or_else(too_large)?
                / u64::from(self.rate);
            usize::try_from(frames).map_err(|_| too_large())
        }
    }
}

/// Apply the same channel mapping as `channel_map`, to one complete frame.
fn map_frame(input: &[f32], output: &mut [f32]) {
    if input.len() == output.len() {
        output.copy_from_slice(input);
    } else if output.len() == 1 {
        output[0] = input.iter().sum::<f32>() / input.len() as f32;
    } else if input.len() == 1 {
        output.fill(input[0]);
    } else {
        for (i, sample) in output.iter_mut().enumerate() {
            *sample = input.get(i).copied().unwrap_or(0.0);
        }
    }
}

/// Decode and convert a whole source, delivering interleaved frames in order.
/// The FFT's chunk size, zero flush, and delay removal match `resample`.
fn convert_source(
    mut source: Source,
    to_rate: u32,
    to_ch: usize,
    mut emit: impl FnMut(usize, &[f32]),
) -> Result<(), PunchError> {
    let wanted = source.output_frames(to_rate)?;
    let spec = source.reader.spec();
    let mut samples: Box<dyn Iterator<Item = Result<f32, hound::Error>> + '_> =
        match (spec.sample_format, spec.bits_per_sample) {
            (hound::SampleFormat::Float, 32) => Box::new(source.reader.samples::<f32>()),
            (hound::SampleFormat::Int, bits @ 8..=32) => {
                let scale = 1.0 / (1u64 << (bits - 1)) as f32;
                Box::new(
                    source
                        .reader
                        .samples::<i32>()
                        .map(move |s| s.map(|v| v as f32 * scale)),
                )
            }
            _ => unreachable!("Source::open checked the format"),
        };
    let mut source_frame = vec![0.0; source.channels];
    let work_ch = source.channels.min(to_ch);
    let mut mapped = vec![0.0; work_ch];
    let mut output_frame = vec![0.0; to_ch];
    let mut read_frame = |frame: &mut [f32]| -> Result<(), PunchError> {
        for sample in frame {
            *sample = samples
                .next()
                .ok_or_else(|| read_error(&"unexpected end of WAV"))?
                .map_err(|e| read_error(&e))?;
        }
        Ok(())
    };

    if source.rate == to_rate || source.frames == 0 {
        for f in 0..source.frames {
            read_frame(&mut source_frame)?;
            map_frame(&source_frame, &mut output_frame);
            emit(f, &output_frame);
        }
    } else {
        let conv = |e: &dyn std::fmt::Display| PunchError::Convert(e.to_string());
        let mut r = Fft::<f32>::new(
            source.rate as usize,
            to_rate as usize,
            1024,
            work_ch,
            FixedSync::Both,
        )
        .map_err(|e| conv(&e))?;
        let delay = r.output_delay();
        let mut in_buf = vec![vec![0.0f32; r.input_frames_max()]; work_ch];
        let mut out_buf = vec![vec![0.0f32; r.output_frames_max()]; work_ch];
        let mut pos = 0usize;
        let mut produced = 0usize;
        let mut emitted = 0usize;
        while produced < delay + wanted {
            let needed = r.input_frames_next();
            let available = source.frames.saturating_sub(pos).min(needed);
            for buf in &mut in_buf {
                buf.clear();
                buf.resize(needed, 0.0);
            }
            for f in 0..available {
                read_frame(&mut source_frame)?;
                map_frame(&source_frame, &mut mapped);
                for (channel, sample) in in_buf.iter_mut().zip(&mapped) {
                    channel[f] = *sample;
                }
            }
            let input =
                SequentialSliceOfVecs::new(&in_buf, work_ch, needed).map_err(|e| conv(&e))?;
            let mut output =
                SequentialSliceOfVecs::new_mut(&mut out_buf, work_ch, r.output_frames_next())
                    .map_err(|e| conv(&e))?;
            let (_, written) = r
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| conv(&e))?;
            for f in 0..written {
                if produced >= delay && emitted < wanted {
                    for (sample, channel) in mapped.iter_mut().zip(&out_buf) {
                        *sample = channel[f];
                    }
                    map_frame(&mapped, &mut output_frame);
                    emit(emitted, &output_frame);
                    emitted += 1;
                }
                produced += 1;
            }
            pos += available;
        }
        // A complete read checks the same source errors even when the FFT's
        // rounded output length does not need its final input frame.
        for _ in pos..source.frames {
            read_frame(&mut source_frame)?;
        }
    }
    // Also validate any incomplete trailing frame, as read_wav does.
    for sample in samples {
        sample.map_err(|e| read_error(&e))?;
    }
    Ok(())
}

/// Read the two renderer WAVs and build the speaker-format monitor with
/// bounded conversion scratch. `None` means the muted render was silent.
/// The caller must finish and unlock the renderer before calling this.
pub fn build_monitor_from_wavs(
    as_is: &Path,
    muted: Option<&Path>,
    region: Region,
    out_sr: u32,
    out_ch: u16,
) -> Result<Vec<f32>, PunchError> {
    region.check()?;
    let a = Source::open(as_is)?;
    let m = muted.map(Source::open).transpose()?;
    let ch = (out_ch as usize).max(1);
    let a_frames = a.output_frames(out_sr)?;
    let m_frames = m
        .as_ref()
        .map(|s| s.output_frames(out_sr))
        .transpose()?
        .unwrap_or(0);
    let start = (region.start_sec * f64::from(out_sr)).round();
    let end = (region.end_sec * f64::from(out_sr)).round();
    if !end.is_finite() || end >= usize::MAX as f64 {
        return Err(too_large());
    }
    let s = start as usize;
    let e = end as usize;
    let frames = a_frames.max(m_frames).max(e);
    let half =
        ((CROSSFADE_SEC * f64::from(out_sr) / 2.0).round() as usize).clamp(1, ((e - s) / 2).max(1));
    e.checked_add(half).ok_or_else(too_large)?;
    let samples = frames.checked_mul(ch).ok_or_else(too_large)?;
    samples
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(too_large)?;
    let mut out = Vec::new();
    out.try_reserve_exact(samples).map_err(|_| too_large())?;
    out.resize(samples, 0.0f32);
    // The second fade overwrites the first if a very short region makes
    // them overlap, just as the two loops in build_monitor do.
    let weights = |f: usize| {
        let mut w = None;
        for (edge, entering) in [(s, true), (e, false)] {
            if f >= edge.saturating_sub(half) && f < (edge + half).min(frames) {
                let offset = f as u128 + half as u128 - edge as u128;
                let t = (offset as f32 + 0.5) / (2 * half) as f32;
                w = Some(if entering { (1.0 - t, t) } else { (t, 1.0 - t) });
            }
        }
        w
    };
    convert_source(a, out_sr, ch, |f, src| {
        if let Some((wa, _)) = weights(f) {
            for c in 0..ch {
                out[f * ch + c] = src[c] * wa;
            }
        } else if f < s || f >= e {
            out[f * ch..(f + 1) * ch].copy_from_slice(src);
        }
    })?;
    let mut mixed = 0;
    if let Some(m) = m {
        convert_source(m, out_sr, ch, |f, src| {
            if let Some((_, wm)) = weights(f) {
                for c in 0..ch {
                    out[f * ch + c] += src[c] * wm;
                }
            } else if f >= s && f < e {
                out[f * ch..(f + 1) * ch].copy_from_slice(src);
            }
            mixed = f + 1;
        })?;
    }
    // A missing muted sample is +0.0, including in the fade arithmetic.
    for f in mixed..frames {
        if let Some((_, wm)) = weights(f) {
            for c in 0..ch {
                out[f * ch + c] += 0.0 * wm;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::punch::{build_monitor, write_wav, Rendered};

    fn rendered(rate: u32, channels: u16, frames: usize, phase: usize) -> Rendered {
        let samples = (0..frames * channels as usize)
            .map(|i| {
                if i % 97 == 0 {
                    -0.0
                } else {
                    (((i * 31 + phase) % 251) as f32 - 125.0) / 256.0
                }
            })
            .collect();
        Rendered {
            samples,
            sample_rate: rate,
            channels,
        }
    }

    #[test]
    fn wav_monitor_matches_the_existing_monitor_sample_for_sample() {
        let dir = tempfile::tempdir().unwrap();
        let a_path = dir.path().join("as-is.wav");
        let m_path = dir.path().join("muted.wav");
        for (a_rate, a_ch, m_rate, m_ch, out_rate, out_ch, region, m_frames) in [
            (
                48_000,
                2,
                48_000,
                2,
                48_000,
                2,
                Region {
                    start_sec: 0.02,
                    end_sec: 0.1,
                },
                8_000,
            ),
            (
                44_100,
                1,
                44_100,
                1,
                48_000,
                2,
                Region {
                    start_sec: 0.0,
                    end_sec: 0.1,
                },
                4_000,
            ),
            (
                48_000,
                2,
                44_100,
                3,
                44_100,
                1,
                Region {
                    start_sec: 0.02,
                    end_sec: 0.3,
                },
                2_000,
            ),
            (
                44_100,
                3,
                48_000,
                1,
                48_000,
                4,
                Region {
                    start_sec: 0.02,
                    end_sec: 0.020_01,
                },
                7_000,
            ),
            (
                48_000,
                2,
                48_000,
                2,
                44_100,
                2,
                Region {
                    start_sec: 0.02,
                    end_sec: 0.1,
                },
                0,
            ),
        ] {
            let a = rendered(a_rate, a_ch, 6_000, 7);
            let m = rendered(m_rate, m_ch, m_frames, 19);
            write_wav(&a_path, &a.samples, a.sample_rate, a.channels).unwrap();
            write_wav(&m_path, &m.samples, m.sample_rate, m.channels).unwrap();
            let expected = build_monitor(&a, &m, region, out_rate, out_ch).unwrap();
            let actual =
                build_monitor_from_wavs(&a_path, Some(&m_path), region, out_rate, out_ch).unwrap();
            let expected: Vec<_> = expected.into_iter().map(f32::to_bits).collect();
            let actual: Vec<_> = actual.into_iter().map(f32::to_bits).collect();
            assert_eq!(
                actual, expected,
                "{a_rate}/{a_ch} and {m_rate}/{m_ch} to {out_rate}/{out_ch}, {region:?}"
            );
        }
    }

    #[test]
    fn absent_muted_render_is_silence_even_across_the_fades() {
        let dir = tempfile::tempdir().unwrap();
        let a_path = dir.path().join("as-is.wav");
        let a = rendered(48_000, 2, 6_000, 7);
        let m = rendered(48_000, 2, 0, 0);
        let region = Region {
            start_sec: 0.02,
            end_sec: 0.1,
        };
        write_wav(&a_path, &a.samples, a.sample_rate, a.channels).unwrap();
        let expected = build_monitor(&a, &m, region, 44_100, 2).unwrap();
        let actual = build_monitor_from_wavs(&a_path, None, region, 44_100, 2).unwrap();
        assert_eq!(
            actual.into_iter().map(f32::to_bits).collect::<Vec<_>>(),
            expected.into_iter().map(f32::to_bits).collect::<Vec<_>>()
        );
    }

    #[test]
    fn padded_24_bit_wav_and_trailing_chunk_match_the_existing_monitor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("padded.wav");
        let spec = hound::WavSpecEx {
            spec: hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 24,
                sample_format: hound::SampleFormat::Int,
            },
            bytes_per_sample: 4,
        };
        let mut writer =
            hound::WavWriter::new_with_spec_ex(std::fs::File::create(&path).unwrap(), spec)
                .unwrap();
        for sample in [2_000_000i32, -2_000_000, 4_000_000, -4_000_000] {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        let mut wav = std::fs::read(&path).unwrap();
        wav.extend_from_slice(b"JUNK\x00\x00\x00\x00");
        let riff_len = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_len.to_le_bytes());
        std::fs::write(&path, wav).unwrap();

        let as_is = crate::punch::read_wav(&path).unwrap();
        let muted = Rendered {
            samples: Vec::new(),
            sample_rate: 48_000,
            channels: 2,
        };
        let region = Region {
            start_sec: 0.001,
            end_sec: 0.002,
        };
        let expected = build_monitor(&as_is, &muted, region, 48_000, 2).unwrap();
        let actual = build_monitor_from_wavs(&path, None, region, 48_000, 2).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn a_truncated_muted_wav_is_an_error_even_when_its_tail_is_outside_the_region() {
        let dir = tempfile::tempdir().unwrap();
        let a_path = dir.path().join("as-is.wav");
        let m_path = dir.path().join("muted.wav");
        let a = rendered(48_000, 2, 6_000, 7);
        let m = rendered(48_000, 2, 6_000, 19);
        write_wav(&a_path, &a.samples, a.sample_rate, a.channels).unwrap();
        write_wav(&m_path, &m.samples, m.sample_rate, m.channels).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&m_path)
            .unwrap()
            .set_len(1_000)
            .unwrap();
        assert!(crate::punch::read_wav(&m_path).is_err());
        let region = Region {
            start_sec: 0.0,
            end_sec: 0.001,
        };
        assert!(build_monitor_from_wavs(&a_path, Some(&m_path), region, 48_000, 2).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn forged_large_wav_length_returns_an_error_without_large_allocation() {
        use std::process::Command;
        use std::time::{Duration, Instant};

        const FIXTURE: &str = "EDYTLAB_PUNCH_FORGED_WAV";
        const DONE: &str = "EDYTLAB_PUNCH_FORGED_WAV_DONE";
        if let Some(path) = std::env::var_os(FIXTURE) {
            let region = Region {
                start_sec: 0.0,
                end_sec: 0.1,
            };
            assert!(build_monitor_from_wavs(Path::new(&path), None, region, 48_000, 2).is_err());
            std::fs::write(std::env::var_os(DONE).unwrap(), b"checked").unwrap();
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("forged-length.wav");
        let done = dir.path().join("child-completed");
        write_wav(&path, &[0.25, 0.5], 48_000, 2).unwrap();
        let mut wav = std::fs::read(&path).unwrap();
        let data = wav.windows(4).position(|chunk| chunk == b"data").unwrap();
        let declared_data_len = 2_147_483_000u32;
        wav[data + 4..data + 8].copy_from_slice(&declared_data_len.to_le_bytes());
        let declared_riff_len = data as u32 + declared_data_len;
        wav[4..8].copy_from_slice(&declared_riff_len.to_le_bytes());
        std::fs::write(&path, wav).unwrap();

        // The malformed header would otherwise provoke a multi-gigabyte
        // allocation. Limit the child before exec, and forbid core dumps.
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("ulimit -c 0; ulimit -v 524288 || exit 125; exec \"$@\"")
            .arg("punch-memory-child")
            .arg(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("punch::monitor::tests::forged_large_wav_length_returns_an_error_without_large_allocation")
            .arg("--nocapture")
            .env(FIXTURE, &path)
            .env(DONE, &done)
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("memory-limited child timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(), "memory-limited child failed: {status}");
        assert_eq!(std::fs::read(done).unwrap(), b"checked");
    }
}
