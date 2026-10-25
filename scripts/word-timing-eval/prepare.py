#!/usr/bin/env python3
"""Build the real-speech word-timing evaluation set (#659).

Two chapters of LibriSpeech dev-clean (CC BY 4.0, Panayotov et al.,
https://www.openslr.org/12), each a consecutive run of one reader's
utterances, joined as they are (no gap is added), with the Montreal Forced
Aligner word times that Loren Lugosch published
(https://zenodo.org/records/2619474) as redistributed in the Hugging Face
dataset gilkeyio/librispeech-alignments (CC BY 4.0).

    python3 prepare.py OUT_DIR

writes, into OUT_DIR (not into the repository: the audio is not committed):

    <speaker>-<chapter>.wav               16 kHz mono 16-bit, several minutes
    <speaker>-<chapter>.words.tsv         start_s <TAB> end_s <TAB> word (the reference)
    <speaker>-<chapter>-pauses.wav        the same speech with a 4 s lead-in and, after
    <speaker>-<chapter>-pauses.words.tsv  every sixth utterance, a pause of 1.5, 3 and
                                          6 s in turn, made of noise at the level of
                                          the recording's own quietest frames (so it is
                                          not digital silence, which a trimmer would
                                          find trivially)

The parquet file (343 MB) is fetched at a pinned revision and checked against
its SHA-256 before it is read, and so is each WAV after it is written, so a run
that prints no error evaluated exactly the audio and the reference the numbers
in the PR were taken on.

Needs: python3, pyarrow, ffmpeg (to decode the FLAC; nothing else).
Reference times are an aligner's, not hand labels: an error here is the
distance from MFA's word, and MFA is itself about 10 to 20 ms off a hand label
on clean read speech.
"""
import array
import hashlib
import math
import random
import subprocess
import sys
import urllib.request
import wave
from pathlib import Path

REVISION = "0daa1eb43dda38ee6ce752e785555380e5628f5c"
URL = (
    "https://huggingface.co/datasets/gilkeyio/librispeech-alignments/resolve/"
    f"{REVISION}/data/dev_clean-00000-of-00001.parquet"
)
PARQUET_SHA256 = "927736928481fe2f05a7fe2582991cacf38b369c63754b0fc179baa6c171deee"
# speaker-chapter -> SHA-256 of the WAV written.
CHAPTERS = {
    "3853-163249": None,  # female, 7.8 min of reference words
    "422-122949": None,  # male, 8.2 min
}
RATE = 16_000
LEAD_IN_S = 4.0
PAUSE_EVERY = 6
PAUSES_S = (1.5, 3.0, 6.0)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch(dest: Path) -> None:
    if dest.exists() and sha256(dest) == PARQUET_SHA256:
        return
    print(f"downloading {URL}", file=sys.stderr)
    urllib.request.urlretrieve(URL, dest)
    got = sha256(dest)
    if got != PARQUET_SHA256:
        sys.exit(f"{dest}: sha256 {got}, expected {PARQUET_SHA256}")


def decode(flac: bytes) -> bytes:
    """16 kHz mono s16le PCM of a FLAC file."""
    run = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", "pipe:0", "-f", "s16le", "-ac", "1", "-ar", str(RATE), "pipe:1"],
        input=flac,
        stdout=subprocess.PIPE,
        check=True,
    )
    return run.stdout


def room_tone_sigma(pcm: bytes) -> float:
    """The RMS (in 16-bit units) of the quietest 5% of 10 ms frames of `pcm`."""
    samples = array.array("h")
    samples.frombytes(pcm)
    frame = RATE // 100
    levels = sorted(
        math.sqrt(sum(x * x for x in samples[i : i + frame]) / frame)
        for i in range(0, len(samples) - frame, frame)
    )
    return levels[len(levels) // 20]


def noise(seconds: float, sigma: float, rng: random.Random) -> bytes:
    n = round(seconds * RATE)
    out = array.array("h", (max(-32768, min(32767, round(rng.gauss(0, sigma)))) for _ in range(n)))
    return out.tobytes()


def write_wav(path: Path, pcm: bytes) -> None:
    with wave.open(str(path), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(RATE)
        f.writeframes(pcm)


def write_tsv(path: Path, reference) -> None:
    with open(path, "w") as f:
        for start, end, word in reference:
            f.write(f"{start:.3f}\t{end:.3f}\t{word}\n")


def main() -> None:
    import pyarrow.parquet as pq

    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    parquet = out / "dev_clean.parquet"
    fetch(parquet)
    table = pq.read_table(parquet, columns=["id", "audio", "words"]).to_pylist()
    for chapter in CHAPTERS:
        rows = sorted(
            (r for r in table if r["id"].rsplit("-", 1)[0] == chapter),
            key=lambda r: int(r["id"].rsplit("-", 1)[1]),
        )
        plain, reference = bytearray(), []
        utterances = []
        for row in rows:
            offset = len(plain) / 2 / RATE
            for w in row["words"]:
                reference.append((offset + w["start"], offset + w["end"], w["word"]))
            pcm = decode(row["audio"]["bytes"])
            plain += pcm
            utterances.append((pcm, row["words"]))
        wav = out / f"{chapter}.wav"
        write_wav(wav, bytes(plain))
        write_tsv(out / f"{chapter}.words.tsv", reference)
        print(f"{wav}  {len(plain) / 2 / RATE:.1f} s  {len(reference)} words  sha256 {sha256(wav)}")

        # The same speech with pauses.
        sigma = room_tone_sigma(bytes(plain))
        rng = random.Random(659)
        gapped, reference = bytearray(noise(LEAD_IN_S, sigma, rng)), []
        pauses = iter(PAUSES_S * (len(utterances) // PAUSE_EVERY + 1))
        for k, (pcm, words) in enumerate(utterances):
            offset = len(gapped) / 2 / RATE
            for w in words:
                reference.append((offset + w["start"], offset + w["end"], w["word"]))
            gapped += pcm
            if k % PAUSE_EVERY == PAUSE_EVERY - 1:
                gapped += noise(next(pauses), sigma, rng)
        wav = out / f"{chapter}-pauses.wav"
        write_wav(wav, bytes(gapped))
        write_tsv(out / f"{chapter}-pauses.words.tsv", reference)
        print(f"{wav}  {len(gapped) / 2 / RATE:.1f} s  {len(reference)} words  sha256 {sha256(wav)}")


if __name__ == "__main__":
    main()
