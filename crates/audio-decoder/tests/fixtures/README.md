# Decode fixtures

A short synthetic sine tone in each format the decoder opens (M4A/AAC only where
the operating system decodes it), plus Opus files of layouts it refuses. They are made here from `aevalsrc`, so there is no recording
and no third-party audio in them. They are the project's own work, dedicated
to the public domain under CC0 1.0.

Made with `ffmpeg 6.1.1` (Ubuntu 24.04 package `6.1.1-3ubuntu5`) from this
directory. `-map_metadata -1` and the bit-exact flags keep the encoder's name
and version out of the files. The amplitude is 0.5 in every file; where a
file has two channels the left is 440 Hz and the right is 880 Hz.

```sh
# Ogg Vorbis, mono 44.1 kHz, and stereo 48 kHz (libvorbis, quality 4)
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t):s=44100:d=0.5" -c:a libvorbis -q:a 4 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tone_440_mono_44100.ogg
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*880*t):s=48000:d=0.5:c=stereo" -c:a libvorbis -q:a 4 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tones_440_880_stereo_48000.ogg

# AIFF, 16-bit mono 44.1 kHz, and 24-bit stereo 48 kHz (big-endian PCM)
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t):s=44100:d=0.5" -c:a pcm_s16be -map_metadata -1 -fflags +bitexact tone_440_mono_44100_s16.aiff
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*880*t):s=48000:d=0.5:c=stereo" -c:a pcm_s24be -map_metadata -1 -fflags +bitexact tones_440_880_stereo_48000_s24.aiff

# FLAC, 16-bit mono 44.1 kHz
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t):s=44100:d=0.5" -c:a flac -sample_fmt s16 -compression_level 8 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tone_440_mono_44100.flac

# Opus in Ogg (tests/opus.rs): mono 0.5 s, and stereo 0.97 s (not a whole
# number of 20 ms packets, so the end trim cuts the last one). libopus, ffmpeg's
# default pre-skip of 312 frames.
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t):s=48000:d=0.5" -c:a libopus -b:a 32k -map_metadata -1 -fflags +bitexact -flags:a +bitexact tone_440_mono_48000.opus
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*880*t):s=48000:d=0.97:c=stereo" -c:a libopus -b:a 48k -map_metadata -1 -fflags +bitexact -flags:a +bitexact tones_440_880_stereo_48000.opus

# Opus the decoder refuses by name (tests/opus.rs): 5.1 surround (mapping
# family 1, 6 channels), and two discrete channels (mapping family 255)
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*550*t)|0.5*sin(2*PI*660*t)|0.5*sin(2*PI*770*t)|0.5*sin(2*PI*880*t)|0.5*sin(2*PI*990*t):s=48000:d=0.1:c=5.1" -c:a libopus -b:a 96k -mapping_family 1 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tones_surround_6ch_48000.opus
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*880*t):s=48000:d=0.1:c=stereo" -c:a libopus -b:a 48k -mapping_family 255 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tones_discrete_2ch_48000.opus


# AAC-LC in M4A (ffmpeg's native `aac` encoder), mono 44.1 kHz and stereo 48 kHz.
# Decoded by the operating system on macOS and Windows (tests/formats.rs); refused
# on Linux, which has no system decoder (tests/unsupported.rs). ffmpeg has no
# HE-AAC encoder (`libfdk_aac` is not built into the Ubuntu package), so there is
# no HE-AAC fixture. The stereo file uses `-aac_coder fast -aac_pns 0`: with the
# default coder this two-tone signal decodes with peaks of 0.71 and 0.78 (ffmpeg's
# own decoder and Media Foundation read it alike), past the 0.1 level tolerance.
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t):s=44100:d=0.5" -c:a aac -b:a 64k -map_metadata -1 -fflags +bitexact -flags:a +bitexact tone_440_mono_44100.m4a
ffmpeg -y -f lavfi -i "aevalsrc=0.5*sin(2*PI*440*t)|0.5*sin(2*PI*880*t):s=48000:d=0.5:c=stereo" -c:a aac -b:a 96k -aac_coder fast -aac_pns 0 -map_metadata -1 -fflags +bitexact -flags:a +bitexact tones_440_880_stereo_48000.m4a
```

The encoded properties are what the tests assert (`tests/formats.rs`):

| File | Rate | Channels | Tones | Length |
|---|---|---|---|---|
| `tone_440_mono_44100.ogg` | 44,100 | 1 | 440 Hz | 0.5 s, plus up to 0.06 s of codec padding |
| `tones_440_880_stereo_48000.ogg` | 48,000 | 2 | 440 / 880 Hz | same |
| `tone_440_mono_44100_s16.aiff` | 44,100 | 1 | 440 Hz | exactly 22,050 frames, each sample within 1.5 LSB |
| `tones_440_880_stereo_48000_s24.aiff` | 48,000 | 2 | 440 / 880 Hz | exactly 24,000 frames, each sample within 1e-4 |
| `tone_440_mono_44100.flac` | 44,100 | 1 | 440 Hz | exactly 22,050 frames, each sample within 1.5 LSB |
| `tone_440_mono_48000.opus` | 48,000 | 1 | 440 Hz | exactly 24,000 frames (`ffprobe` `duration_ts` 24,312 less 312 pre-skip) |
| `tones_440_880_stereo_48000.opus` | 48,000 | 2 | 440 / 880 Hz | exactly 46,560 frames (`duration_ts` 46,872 less 312), the last packet trimmed |
| `tone_440_mono_44100.m4a` | 44,100 | 1 | 440 Hz | 0.5 s, within 0.12 s (the two system decoders differ in trimming the encoder's priming samples) |
| `tones_440_880_stereo_48000.m4a` | 48,000 | 2 | 440 / 880 Hz | same |

Measured: the mono Vorbis decodes to 22,080 frames (30 more than encoded,
0.7 ms) and the stereo one to 24,256 (256 more, 5.3 ms); the tolerance is
60 ms. The frequency of each channel is found to within 2 Hz.
