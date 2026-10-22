#!/usr/bin/env python3
"""Regenerate `speech_words.wav` and `speech_words.json` for the known-words test.

`tests/transcribe_known_words.rs` needs speech with word times that are exact,
and a licence that is clean. This builds both from synthesis instead of from a
recording: each word is spoken on its own by `espeak-ng`, cut down to the part
that is sound, and placed at a chosen time with chosen silence around it.
`espeak-ng` is GPL-3.0-or-later. The WAV is its output for twelve words of
the script's own choosing, not a copy of its code or data, and no third-party
recording, voice sample or text is in it; whether that is as clean as an
owner's own recording is the repository owner's call (flagged in the PR).
Because the script puts every word where it says, `speech_words.json` is not
a hand label with an error of its own: it is the construction.

Why isolated words, and why such short gaps between them: a synthetic
sentence has no true word boundaries to measure against (they blur into one
another), and a measured time is only worth testing against if it is exact.
Words are spoken one at a time and joined 30 to 50 ms apart, which is how
fluent speech runs words together; the test also builds a copy with long
pauses by inserting silence at the joins, so the committed file does not have
to be long. The cost of all this is that the speech is easier for the
recogniser than a recording of someone talking: the test checks the plumbing
(windows, mel, encoder, decoder with its cache, alignment, offsets), not the
model's accuracy on hard audio. Say so wherever its numbers are quoted.

The audio is `espeak-ng` output only. Nothing here was recorded.

Run it (Linux or macOS; `espeak-ng` is `apt install espeak-ng` or
`brew install espeak-ng`; Python 3 standard library only, no numpy):

    python3 -I make_speech.py [outdir]

The committed files came from espeak-ng 1.51 (Ubuntu 24.04), voice `en-us`,
speed 140 words per minute. A different espeak-ng version may sound a little
different, so regenerate both files together and keep the pair; do not mix a
new WAV with an old JSON.

Writes next to itself, or into `outdir`:

    speech_words.wav    16 kHz mono 16-bit PCM, about 7.3 s
    speech_words.json   [{"text", "start_s", "end_s"}, ...] in order

    speech_words_first_draft.wav / .json
                        the same construction for the words as first chosen
                        (see FIRST_DRAFT_WORDS), about 5.5 s

The first pair is the test's gate. The second is not: the gate's list was
changed after a model had heard the first one (see the comment on WORDS), so
a pass on the gate says less than it would have if the list had been fixed
beforehand. The test prints how the model does on the second as well, without
asserting on it, so that both numbers can be quoted.

`start_s` is the first 5 ms frame of the word whose level is within 26 dB of
the word's own peak, and `end_s` the end of the last such frame, measured on
the final 16 kHz audio after it is placed (the same arithmetic the test
could do, with nothing to disagree about). A Whisper word time is an
attention-based estimate and is not expected to match to the millisecond; the
test allows 0.25 s.
"""

import json
import math
import os
import struct
import subprocess
import sys
import tempfile
import wave

RATE = 16_000

# Words that are short, common, and read the same aloud and written down: no
# numbers (a recogniser may write "1"), no names, no homophones.
#
# This list was fitted to the model, and the test should be read that way. The
# first list (FIRST_DRAFT_WORDS) was changed after Whisper base heard it:
# "green" and "house" side by side were heard as "greenhouse", "water" as
# "walker" and "friend" as "friends", so those four were swapped for words it
# hears as written. The gate asserts on this list; the first draft is
# measured beside it and only printed.
WORDS = [
    "hello",
    "music",
    "table",
    "morning",
    "window",
    "yellow",
    "garden",
    "river",
    "summer",
    "paper",
    "orange",
    "coffee",
]

# The first list, as written down before any model heard it: the words
# `speech_words_first_draft.*` is made of. Not a gate (see the header); it is
# there so the cost of replacing "green", "house", "water" and "friend" can be
# measured, and quoted, instead of left out of the story.
FIRST_DRAFT_WORDS = [
    "hello",
    "green",
    "water",
    "house",
    "music",
    "friend",
    "table",
    "morning",
    "window",
]

# The silence before each word, in seconds: the first is the lead-in, and
# every other is the gap after the previous word's last sound. Short, as in
# fluent speech, and different on purpose, so a constant shift or a fixed
# stride cannot pass for alignment.
GAPS_BEFORE = [0.30, 0.04, 0.05, 0.03, 0.05, 0.04, 0.03, 0.05, 0.04, 0.03, 0.05, 0.04]
TAIL = 0.30

# The loudest sample of the file, as a fraction of full scale.
PEAK = 0.5

# The level, relative to a word's own peak, below which a 5 ms frame is not
# part of the word: 26 dB down.
FLOOR_DB = -26.0
FRAME = RATE // 200  # 5 ms


def synthesise(word, directory):
    """The 22.05 kHz mono samples (floats in -1..1) espeak-ng says for `word`."""
    path = os.path.join(directory, word + ".wav")
    subprocess.run(
        ["espeak-ng", "-v", "en-us", "-s", "140", "-w", path, word],
        check=True,
    )
    with wave.open(path, "rb") as w:
        assert w.getnchannels() == 1 and w.getsampwidth() == 2, w.getparams()
        rate = w.getframerate()
        raw = w.readframes(w.getnframes())
    ints = struct.unpack("<%dh" % (len(raw) // 2), raw)
    return rate, [s / 32768.0 for s in ints]


def resample(samples, rate_in, rate_out):
    """Windowed-sinc resampling, 32 taps, in pure Python.

    Slow, and exact enough for speech: the band above 7 kHz of a synthetic
    voice is not what a recogniser listens to. Anti-aliased for down-sampling
    (cutoff at the lower of the two Nyquists), Hann-windowed.
    """
    if rate_in == rate_out:
        return list(samples)
    step = rate_in / rate_out
    cutoff = min(1.0, rate_out / rate_in)  # fraction of the input Nyquist
    half = 16
    out = []
    n_out = int(len(samples) / step)
    for n in range(n_out):
        t = n * step
        centre = int(math.floor(t))
        acc = 0.0
        for k in range(centre - half + 1, centre + half + 1):
            if 0 <= k < len(samples):
                x = k - t
                arg = math.pi * x * cutoff
                sinc = 1.0 if x == 0 else math.sin(arg) / arg
                window = 0.5 + 0.5 * math.cos(math.pi * x / half)
                acc += samples[k] * sinc * window * cutoff
        out.append(acc)
    return out


def voiced_span(samples):
    """(first, last_exclusive) sample of the frames within FLOOR_DB of the peak."""
    frames = [
        samples[i : i + FRAME] for i in range(0, len(samples) - FRAME + 1, FRAME)
    ]
    levels = [math.sqrt(sum(s * s for s in f) / FRAME) for f in frames]
    top = max(levels)
    limit = top * 10 ** (FLOOR_DB / 20.0)
    loud = [i for i, level in enumerate(levels) if level >= limit]
    return loud[0] * FRAME, (loud[-1] + 1) * FRAME


def build(words):
    placed = []  # (word, samples cut to the sound)
    with tempfile.TemporaryDirectory() as directory:
        for word in words:
            rate, samples = synthesise(word, directory)
            samples = resample(samples, rate, RATE)
            first, last = voiced_span(samples)
            placed.append((word, samples[first:last]))

    audio = []
    labels = []
    for (word, sound), gap in zip(placed, GAPS_BEFORE):
        audio.extend([0.0] * round(gap * RATE))
        start = len(audio)
        audio.extend(sound)
        labels.append((word, start, len(audio)))
    audio.extend([0.0] * round(TAIL * RATE))

    # Scale so the loudest sample is PEAK of full scale, then to 16-bit.
    loudest = max(abs(s) for s in audio)
    gain = PEAK / loudest
    pcm = [max(-32768, min(32767, round(s * gain * 32768.0))) for s in audio]

    # Re-measure the span on the final integers, so the label is of the file.
    words = []
    for word, start, end in labels:
        segment = [s / 32768.0 for s in pcm[start:end]]
        first, last = voiced_span(segment)
        words.append(
            {
                "text": word,
                "start_s": round((start + first) / RATE, 3),
                "end_s": round((start + last) / RATE, 3),
            }
        )
    return pcm, words


def write(outdir, stem, words):
    pcm, labels = build(words)
    wav_path = os.path.join(outdir, stem + ".wav")
    with wave.open(wav_path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(struct.pack("<%dh" % len(pcm), *pcm))
    json_path = os.path.join(outdir, stem + ".json")
    with open(json_path, "w", encoding="utf-8") as f:
        json.dump(labels, f, indent=2)
        f.write("\n")
    print("%s: %.2f s, %d bytes" % (wav_path, len(pcm) / RATE, os.path.getsize(wav_path)))
    for w in labels:
        print("  %-8s %6.3f - %6.3f" % (w["text"], w["start_s"], w["end_s"]))


def main():
    outdir = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))
    write(outdir, "speech_words", WORDS)
    write(outdir, "speech_words_first_draft", FIRST_DRAFT_WORDS)


if __name__ == "__main__":
    main()
