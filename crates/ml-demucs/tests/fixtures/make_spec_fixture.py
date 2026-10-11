#!/usr/bin/env python3
"""Regenerate the golden fixtures for `crates/ml-demucs/src/spec.rs`.

The Rust STFT/iSTFT in `spec.rs` is a port of Demucs' `HTDemucs._spec` and
`HTDemucs._ispec`. This script runs the *upstream* code, under PyTorch, on a
short deterministic signal and writes what it produced, so the port is
checked against PyTorch itself rather than against our reading of it.

Upstream (https://github.com/facebookresearch/demucs, MIT), commit
`e976d93ecc3865e5757426930257e200846a520a`:

    demucs/spec.py       sha256 8a63153a012703fd5ed2e3d012e91ad624597c7296e9d82f40a26c4249f2bff7
    demucs/hdemucs.py    sha256 5b98a5ee262bf6b872a0db23eeb60d024debc4be1ed237c551284af305343864
    demucs/htdemucs.py   sha256 3404ed68d35b946f9c54b54201851bb4171dd7861a498945ce53b1df1144b034

`spectro`, `ispectro`, `pad1d`, `_spec`, `_ispec`, `_magnitude` and `_mask`
below are copied from those files with only these changes: `self.nfft` and
`self.hop_length` are module constants (the pretrained htdemucs uses
`nfft=4096`, so `hop_length = nfft // 4 = 1024`), `self.cac` is always True
(htdemucs is trained with complex-as-channels), and the batch dimension is
kept at 1. Importing `demucs` itself would pull in its whole dependency
tree for six small functions.

Run it with any recent PyTorch (the committed files came from 2.6.0, CPU):

    python3 -m venv venv
    venv/bin/pip install torch --index-url https://download.pytorch.org/whl/cpu
    venv/bin/python -I make_spec_fixture.py

It writes four little-endian float32 files next to itself (no numpy needed):

    spec_input.f32       [2, 3000]            stereo signal: a chirp, and its
                                              time reversal at half amplitude
    spec_cac.f32         [4, 2048, 3]         _magnitude(_spec(input)): channels
                                              are (L.re, L.im, R.re, R.im)
    ispec_roundtrip.f32  [2, 3000]            _ispec(_spec(input), 3000)
    ispec_masked.f32     [2, 2, 3000]         _ispec(_mask(x_out), 3000) for a
                                              made-up two-source `x_out`, see
                                              `make_x_out`

The values only need to match the Rust port to float32 tolerance, so a
different PyTorch build changes the low bits of the files and nothing else;
the tolerances are in `spec.rs`.
"""

import math
import struct
import sys
from pathlib import Path

import torch
import torch.nn.functional as F

N_FFT = 4096
HOP = N_FFT // 4
SAMPLE_RATE = 44100
LENGTH = 3000  # 3 hops (the last one partial): small enough to commit


# ---- demucs/spec.py, verbatim ------------------------------------------------
def spectro(x, n_fft=512, hop_length=None, pad=0):
    *other, length = x.shape
    x = x.reshape(-1, length)
    z = torch.stft(x,
                   n_fft * (1 + pad),
                   hop_length or n_fft // 4,
                   window=torch.hann_window(n_fft).to(x),
                   win_length=n_fft,
                   normalized=True,
                   center=True,
                   return_complex=True,
                   pad_mode='reflect')
    _, freqs, frame = z.shape
    return z.view(*other, freqs, frame)


def ispectro(z, hop_length=None, length=None, pad=0):
    *other, freqs, frames = z.shape
    n_fft = 2 * freqs - 2
    z = z.view(-1, freqs, frames)
    win_length = n_fft // (1 + pad)
    x = torch.istft(z,
                    n_fft,
                    hop_length,
                    window=torch.hann_window(win_length).to(z.real),
                    win_length=win_length,
                    normalized=True,
                    length=length,
                    center=True)
    _, length = x.shape
    return x.view(*other, length)


# ---- demucs/hdemucs.py, verbatim -------------------------------------------
def pad1d(x, paddings, mode='constant', value=0.):
    """Tiny wrapper around F.pad, just to allow for reflect padding on small input.
    If this is the case, we insert extra 0 padding to the right before the reflection happen."""
    x0 = x
    length = x.shape[-1]
    padding_left, padding_right = paddings
    if mode == 'reflect':
        max_pad = max(padding_left, padding_right)
        if length <= max_pad:
            extra_pad = max_pad - length + 1
            extra_pad_right = min(padding_right, extra_pad)
            extra_pad_left = extra_pad - extra_pad_right
            paddings = (padding_left - extra_pad_left, padding_right - extra_pad_right)
            x = F.pad(x, (extra_pad_left, extra_pad_right))
    out = F.pad(x, paddings, mode, value)
    assert out.shape[-1] == length + padding_left + padding_right
    assert (out[..., padding_left: padding_left + length] == x0).all()
    return out


# ---- demucs/htdemucs.py (HTDemucs methods, cac=True) -------------------------
def _spec(x):
    hl = HOP
    nfft = N_FFT
    assert hl == nfft // 4
    le = int(math.ceil(x.shape[-1] / hl))
    pad = hl // 2 * 3
    x = pad1d(x, (pad, pad + le * hl - x.shape[-1]), mode="reflect")

    z = spectro(x, nfft, hl)[..., :-1, :]
    assert z.shape[-1] == le + 4, (z.shape, x.shape, le)
    z = z[..., 2: 2 + le]
    return z


def _ispec(z, length=None, scale=0):
    hl = HOP // (4**scale)
    z = F.pad(z, (0, 0, 0, 1))
    z = F.pad(z, (2, 2))
    pad = hl // 2 * 3
    le = hl * int(math.ceil(length / hl)) + 2 * pad
    x = ispectro(z, hl, length=le)
    x = x[..., pad: pad + length]
    return x


def _magnitude(z):
    B, C, Fr, T = z.shape
    m = torch.view_as_real(z).permute(0, 1, 4, 2, 3)
    m = m.reshape(B, C * 2, Fr, T)
    return m


def _mask(m):
    B, S, C, Fr, T = m.shape
    out = m.view(B, S, -1, 2, Fr, T).permute(0, 1, 2, 4, 5, 3)
    out = torch.view_as_complex(out.contiguous())
    return out


# ---- the fixture -------------------------------------------------------------
def make_input():
    """A 50 Hz -> 20 kHz linear chirp, and its time reversal at half level."""
    n = torch.arange(LENGTH, dtype=torch.float64)
    t = n / SAMPLE_RATE
    duration = LENGTH / SAMPLE_RATE
    f0, f1 = 50.0, 20000.0
    phase = 2 * math.pi * (f0 * t + (f1 - f0) / (2 * duration) * t * t)
    left = (0.8 * torch.sin(phase)).to(torch.float32)
    right = 0.5 * torch.flip(left, dims=[0])
    return torch.stack([left, right])[None]  # [1, 2, LENGTH]


def make_x_out(cac):
    """A stand-in for the network's output, `[1, S*C*2, Fr, T]` with S=2, C=2.

    Source 0 is the mixture's own spectrogram, so it is a consistent STFT.
    Source 1 is not one: the channels are swapped and every odd frequency bin
    is scaled by 0.25. That is the point: the model's output is not a
    consistent STFT either, so `ispec` has to be right for any input, not
    only for what `spec` produced. All of it is exactly reproducible in Rust
    from the first source (a swap and a scale by a power of two).
    """
    B, C2, Fr, T = cac.shape
    assert C2 == 4
    swapped = torch.cat([cac[:, 2:4], cac[:, 0:2]], dim=1)
    mask = torch.ones(Fr)
    mask[1::2] = 0.25
    scaled = swapped * mask[None, None, :, None]
    return torch.cat([cac, scaled], dim=1)  # [1, 8, Fr, T]


def write_f32(path, tensor):
    flat = tensor.detach().to(torch.float32).contiguous().flatten().tolist()
    path.write_bytes(struct.pack("<%df" % len(flat), *flat))
    print(f"{path.name}: shape {list(tensor.shape)}, {path.stat().st_size} bytes")


def main():
    here = Path(__file__).resolve().parent
    x = make_input()
    z = _spec(x)
    cac = _magnitude(z)
    roundtrip = _ispec(z, LENGTH)

    x_out = make_x_out(cac)
    B, SC2, Fr, T = x_out.shape
    S = 2
    zout = _mask(x_out.view(B, S, SC2 // S, Fr, T))
    masked = _ispec(zout, LENGTH)  # [1, S, C, LENGTH]

    write_f32(here / "spec_input.f32", x[0])
    write_f32(here / "spec_cac.f32", cac[0])
    write_f32(here / "ispec_roundtrip.f32", roundtrip[0])
    write_f32(here / "ispec_masked.f32", masked[0])

    # What the Rust doc comments quote; recomputed here so they can be checked.
    err = (roundtrip - x).abs()
    pad = HOP // 2 * 3
    le = math.ceil(LENGTH / HOP)
    print(f"torch {torch.__version__}")
    print(f"frames T={T}, Fr={Fr}")
    print(f"round trip |error|: max {err.max():.3e}; "
          f"first {pad} samples max {err[..., :pad].max():.3e}; "
          f"after {le * HOP - pad}: max {err[..., le * HOP - pad:].max():.3e}")


if __name__ == "__main__":
    sys.exit(main())
