#!/usr/bin/env python3
"""Make the four stem files of the separation fixture, and say what is in them.

    python3 make_stems.py --fetch DIR      # download the four sources into DIR
    python3 make_stems.py --sources DIR    # cut, level and encode them here

`--fetch` and `--sources` can be given together. The output is deterministic
for the same sources and the same ffmpeg encoder settings (the sources are
checked by SHA-256 first), so regenerating gives the committed bytes of the
decoded audio: if the FLAC files differ, compare with `ffmpeg -i a -i b
-filter_complex "[0][1]amix"` or by decoding both, not with `cmp`, because
another ffmpeg build may write other metadata.

What it makes: `vocals.flac`, `drums.flac`, `bass.flac`, `other.flac`, each
8.5 s of 16-bit stereo at 44.1 kHz. The test (`tests/real_model.rs`) adds the
four up to make the mixture and checks what comes out of the separator against
them. They are four unrelated recordings, cut and levelled to sit like the
parts of one mix. They were not recorded together, and that is on purpose:
what is needed is four sources that really are separate, so that the "right
answer" is not an estimate.

  vocals  a solo singer (VocalSet, female 9, "Row, Row, Row Your Boat",
          straight tone), a studio recording; CC BY 4.0
  drums   a hip-hop drum loop at 110 BPM (Freesound 573948, blakengouda); CC0
  bass    an electric bass loop at 110 BPM (Freesound 483146, josefpres); CC0
  other   a funk guitar loop at 110 BPM (Freesound 483627, josefpres); CC0

The Freesound files are the site's own "high quality" preview encodes (MP3,
about 145 kbit/s), which is what can be fetched without an account; they are
CC0 like the originals. The credit and the licences are in README.md.

Needs `numpy` and `ffmpeg`.
"""
import argparse
import hashlib
import io
import os
import subprocess
import sys
import urllib.request
import zipfile

import numpy as np

RATE = 44100
SECONDS = 8.5
FRAMES = int(SECONDS * RATE)
FADE = int(0.03 * RATE)

# name -> (file name in the sources folder, sha256 of that file)
SOURCES = {
    "vocals": ("vocalset_f9_row_straight.wav", "5534fabe9e7fcf9ec919068ef2e9845835ee33b5edc9d0e2939ee5004ff1b4dc"),
    "drums": ("fs_573948.hq.mp3", "edfb567da00fe7fb84e538dff5f85df6a7340744805a7bd61e24012961c2d3bd"),
    "bass": ("fs_483146.hq.mp3", "68c4597148894eff5f4974cd80b47406a886923ce43d55b34fc9c1c99216749f"),
    "other": ("fs_483627.hq.mp3", "bf1090a7d1e930d152a1e4268d143dc4cd7bc2b63eb6cfbb884da8d0bd1bed83"),
}
FREESOUND = {
    "fs_573948.hq.mp3": "https://cdn.freesound.org/previews/573/573948_8033171-hq.mp3",
    "fs_483146.hq.mp3": "https://cdn.freesound.org/previews/483/483146_5674468-hq.mp3",
    "fs_483627.hq.mp3": "https://cdn.freesound.org/previews/483/483627_5674468-hq.mp3",
}
VOCALSET_URL = "https://zenodo.org/api/records/1193957/files/VocalSet.zip/content"
VOCALSET_MEMBER = "FULL/female9/excerpts/straight/f9_row_straight.wav"

# Where each stem is cut from its source (seconds), and the level it is given
# (RMS over the cut, dBFS). The loops are cut a little after their start, which
# is where the MP3 encoder's priming is. The singer is cut after the recording's
# silent lead-in (the first 0.1 s are below -60 dBFS), so the cut holds her
# whole phrase with only the short rest at 5.5 s.
CUT = {"vocals": 0.3, "drums": 0.05, "bass": 0.05, "other": 0.05}
LEVEL_DB = {"vocals": -17.0, "drums": -18.0, "bass": -20.0, "other": -21.0}
# The sum of the four may not peak above this; if it does, all four come down.
PEAK_LIMIT = 0.89


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch(directory):
    os.makedirs(directory, exist_ok=True)
    for name, url in FREESOUND.items():
        with urllib.request.urlopen(url, timeout=120) as r, open(os.path.join(directory, name), "wb") as f:
            f.write(r.read())

    class HttpFile(io.RawIOBase):
        """The zip is 2 GB; read only the central directory and one member."""

        def __init__(self, url):
            self.url, self.pos = url, 0
            self.size = int(urllib.request.urlopen(urllib.request.Request(url, method="HEAD"), timeout=60).headers["Content-Length"])

        def seekable(self):
            return True

        def readable(self):
            return True

        def tell(self):
            return self.pos

        def seek(self, off, whence=0):
            self.pos = {0: off, 1: self.pos + off, 2: self.size + off}[whence]
            return self.pos

        def readinto(self, b):
            if not len(b) or self.pos >= self.size:
                return 0
            end = min(self.pos + len(b), self.size) - 1
            data = urllib.request.urlopen(
                urllib.request.Request(self.url, headers={"Range": "bytes=%d-%d" % (self.pos, end)}), timeout=120
            ).read()
            b[: len(data)] = data
            self.pos += len(data)
            return len(data)

    archive = zipfile.ZipFile(io.BufferedReader(HttpFile(VOCALSET_URL), buffer_size=1 << 20))
    with archive.open(VOCALSET_MEMBER) as src, open(os.path.join(directory, SOURCES["vocals"][0]), "wb") as dst:
        dst.write(src.read())


def decode(path):
    """The file as stereo float at 44.1 kHz, (frames, 2)."""
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", path, "-f", "f32le", "-ac", "2", "-ar", str(RATE), "-"],
        check=True,
        capture_output=True,
    ).stdout
    return np.frombuffer(raw, dtype="<f4").reshape(-1, 2).astype(np.float64)


def encode(path, pcm16):
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y", "-f", "s16le", "-ar", str(RATE), "-ac", "2", "-i", "-",
         "-map_metadata", "-1", "-c:a", "flac", "-compression_level", "12", path],
        check=True,
        input=pcm16.astype("<i2").tobytes(),
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fetch", metavar="DIR")
    ap.add_argument("--sources", metavar="DIR")
    args = ap.parse_args()
    if args.fetch:
        fetch(args.fetch)
    sources = args.sources or args.fetch
    if not sources:
        sys.exit("give --sources DIR (or --fetch DIR)")
    here = os.path.dirname(os.path.abspath(__file__))

    cut = {}
    for name, (file, digest) in SOURCES.items():
        path = os.path.join(sources, file)
        if sha256(path) != digest:
            sys.exit("%s is not the file this was made from (sha256 differs)" % path)
        x = decode(path)
        start = int(CUT[name] * RATE)
        x = x[start : start + FRAMES]
        if len(x) < FRAMES:
            sys.exit("%s is too short" % file)
        rms = np.sqrt(np.mean(x**2))
        x = x * (10 ** (LEVEL_DB[name] / 20) / rms)
        fade = np.minimum(1.0, np.minimum(np.arange(FRAMES), np.arange(FRAMES)[::-1]) / FADE)
        cut[name] = x * fade[:, None]

    peak = np.abs(sum(cut.values())).max()
    gain = min(1.0, PEAK_LIMIT / peak)
    for name, x in cut.items():
        pcm = np.clip(np.round(x * gain * 32768), -32768, 32767)
        encode(os.path.join(here, name + ".flac"), pcm)
        print("%-7s rms %.1f dBFS, peak %.3f" % (name, 20 * np.log10(np.sqrt(np.mean((pcm / 32768) ** 2))), np.abs(pcm).max() / 32768))
    print("mix peak before the common gain: %.3f (gain %.3f)" % (peak, gain))


if __name__ == "__main__":
    main()
