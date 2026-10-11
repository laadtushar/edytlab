#!/usr/bin/env python3
"""Regenerate the golden fixtures for `crates/ml-whisper/src/mel.rs`.

`mel.rs` is a port of Whisper's `log_mel_spectrogram`. This script runs the
*upstream* function, under PyTorch, on two short deterministic signals and
writes what it produced, so the port is checked against Whisper itself
rather than against our reading of it.

Upstream (https://github.com/openai/whisper, MIT), the `openai-whisper`
release 20250625 (PyPI sdist `openai_whisper-20250625.tar.gz`, sha256
`37a91a3921809d9f44748ffc73c0a55c9f366c85a3ef5c2ae0cc09540432eb96`):

    whisper/audio.py                sha256 36ed287b2de3eaf40207cd2a44a8d45d57ea5c212a39df07401d4fcb4d5a5a1a
    whisper/assets/mel_filters.npz  sha256 7450ae70723a5ef9d341e3cee628c7cb0177f36ce42c44b7ed2bf3325f0f6d4c

`pad_or_trim` and `log_mel_spectrogram` below are copied from `audio.py`
with only these changes: `mel_filters` reads the `.npz` whose path you pass
on the command line (it is checked against the sha256 above) instead of
sitting next to the package, and the numpy branch of `pad_or_trim`, which
the tensor path never takes, is dropped. Importing `whisper` itself would
pull in numba, tiktoken and the rest of its dependency tree for two small
functions.

Run it with any recent PyTorch (the committed files came from 2.14.1, CPU,
numpy 2.4.6):

    python3 -m venv venv
    venv/bin/pip install numpy torch --index-url https://download.pytorch.org/whl/cpu \
        --extra-index-url https://pypi.org/simple
    # get mel_filters.npz out of the sdist above (do not import the package)
    venv/bin/python -I make_mel_fixture.py path/to/mel_filters.npz

It writes little-endian float32 files next to itself (no numpy needed to
read them):

    mel_1khz_1s.f32             [80, 100]   log_mel_spectrogram of one second of
                                            a 1 kHz sine at amplitude 0.5:
                                            sin(2 pi (n mod 16) / 16) * 0.5.
                                            No padding: the plain function
    mel_chirp_input.f32         [16000]     one second of a chirp (100 Hz to
                                            7.5 kHz) under a slow envelope,
                                            plus faint white noise. Committed
                                            rather than regenerated in Rust
                                            so the two sides cannot disagree
                                            about the input
    mel_chirp_window_head.f32   [80, 110]   the first 110 frames of
                                            log_mel_spectrogram(
                                            pad_or_trim(chirp)): the 30 s
                                            window the encoder is fed. The
                                            other 2890 frames are one constant
                                            (the floor), which the script
                                            asserts, so they are not stored

The values only need to match the Rust port to float32 tolerance, so a
different PyTorch build changes the low bits of the files and nothing else;
the tolerance is in `mel.rs`.
"""

import hashlib
import math
import sys
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

MEL_FILTERS_SHA256 = "7450ae70723a5ef9d341e3cee628c7cb0177f36ce42c44b7ed2bf3325f0f6d4c"

# ---- whisper/audio.py: constants, verbatim -----------------------------------
SAMPLE_RATE = 16000
N_FFT = 400
HOP_LENGTH = 160
CHUNK_LENGTH = 30
N_SAMPLES = CHUNK_LENGTH * SAMPLE_RATE  # 480000 samples in a 30-second chunk
N_FRAMES = N_SAMPLES // HOP_LENGTH  # 3000 frames in a mel spectrogram input

HEAD_FRAMES = 110  # how many frames of the padded window are stored


# ---- whisper/audio.py, tensor path only --------------------------------------
def pad_or_trim(array, length: int = N_SAMPLES, *, axis: int = -1):
    """
    Pad or trim the audio array to N_SAMPLES, as expected by the encoder.
    """
    if array.shape[axis] > length:
        array = array.index_select(
            dim=axis, index=torch.arange(length, device=array.device)
        )

    if array.shape[axis] < length:
        pad_widths = [(0, 0)] * array.ndim
        pad_widths[axis] = (0, length - array.shape[axis])
        array = F.pad(array, [pad for sizes in pad_widths[::-1] for pad in sizes])

    return array


def mel_filters(device, n_mels: int, filters_path: Path) -> torch.Tensor:
    assert n_mels in {80, 128}, f"Unsupported n_mels: {n_mels}"
    with np.load(filters_path, allow_pickle=False) as f:
        return torch.from_numpy(f[f"mel_{n_mels}"]).to(device)


def log_mel_spectrogram(
    audio,
    filters_path: Path,
    n_mels: int = 80,
    padding: int = 0,
    device=None,
):
    if device is not None:
        audio = audio.to(device)
    if padding > 0:
        audio = F.pad(audio, (0, padding))
    window = torch.hann_window(N_FFT).to(audio.device)
    stft = torch.stft(audio, N_FFT, HOP_LENGTH, window=window, return_complex=True)
    magnitudes = stft[..., :-1].abs() ** 2

    filters = mel_filters(audio.device, n_mels, filters_path)
    mel_spec = filters @ magnitudes

    log_spec = torch.clamp(mel_spec, min=1e-10).log10()
    log_spec = torch.maximum(log_spec, log_spec.max() - 8.0)
    log_spec = (log_spec + 4.0) / 4.0
    return log_spec


# ---- the signals ---------------------------------------------------------------
def tone_1khz() -> np.ndarray:
    # 1000 / 16000 = 1/16 of a cycle per sample, so the phase repeats every
    # 16 samples; the Rust test builds the same signal from `n % 16`.
    n = np.arange(SAMPLE_RATE)
    return (0.5 * np.sin(2.0 * math.pi * (n % 16) / 16.0)).astype(np.float32)


def chirp() -> np.ndarray:
    t = np.arange(SAMPLE_RATE) / SAMPLE_RATE
    f0, f1 = 100.0, 7500.0
    phase = 2.0 * math.pi * (f0 * t + 0.5 * (f1 - f0) * t * t)
    envelope = 0.1 + 0.4 * np.sin(2.0 * math.pi * 3.0 * t) ** 2
    noise = np.random.default_rng(384).standard_normal(SAMPLE_RATE) * 1e-3
    return (envelope * np.sin(phase) + noise).astype(np.float32)


def write_f32(path: Path, array) -> None:
    data = np.ascontiguousarray(array, dtype="<f4")
    path.write_bytes(data.tobytes())
    digest = hashlib.sha256(data.tobytes()).hexdigest()
    print(f"{path.name:28s} {str(tuple(data.shape)):12s} {data.nbytes:7d} bytes  sha256 {digest}")


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} path/to/mel_filters.npz")
    filters_path = Path(sys.argv[1])
    digest = hashlib.sha256(filters_path.read_bytes()).hexdigest()
    if digest != MEL_FILTERS_SHA256:
        sys.exit(f"{filters_path}: sha256 {digest}, expected {MEL_FILTERS_SHA256}")

    torch.set_num_threads(1)
    here = Path(__file__).resolve().parent

    # 1. The plain function on a pure tone, as `log_mel_spectrogram(audio)`.
    tone = torch.from_numpy(tone_1khz())
    mel_tone = log_mel_spectrogram(tone, filters_path)
    assert tuple(mel_tone.shape) == (80, 100), mel_tone.shape
    write_f32(here / "mel_1khz_1s.f32", mel_tone.numpy())

    # 2. The padded 30 s window the encoder takes, on a broadband signal.
    sig = torch.from_numpy(chirp())
    write_f32(here / "mel_chirp_input.f32", sig.numpy())
    window = log_mel_spectrogram(pad_or_trim(sig), filters_path)
    assert tuple(window.shape) == (80, N_FRAMES), window.shape
    tail = window[:, HEAD_FRAMES:]
    # Frames that see only the zero padding are all the same floor value; the
    # Rust test relies on this to store 110 frames instead of 3000.
    assert bool((tail == tail[0, 0]).all()), "padding frames are not constant"
    # Not all of the content frames are distinguishable from the floor, but
    # the signal must reach well above it.
    assert float(window.max()) > float(tail[0, 0]) + 0.5
    write_f32(here / "mel_chirp_window_head.f32", window[:, :HEAD_FRAMES].numpy())
    print(f"floor of the window: {float(tail[0, 0])!r}, max: {float(window.max())!r}")


if __name__ == "__main__":
    main()
