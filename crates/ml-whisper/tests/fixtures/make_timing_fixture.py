#!/usr/bin/env python3
"""Regenerate the golden fixtures for `crates/ml-whisper/src/timing.rs`.

`timing.rs` turns cross-attention weights into token times the way
OpenAI's `find_alignment` does: normalise, median-filter, average over the
alignment heads, run dynamic time warping, read the times where the path
moves to the next token. This script runs the *upstream* functions, under
PyTorch, on made-up attention weights and writes what they produced, so the
port is checked against upstream rather than against our reading of it.

Upstream (https://github.com/openai/whisper, MIT), the `openai-whisper`
release 20250625 (PyPI sdist `openai_whisper-20250625.tar.gz`, sha256
`37a91a3921809d9f44748ffc73c0a55c9f366c85a3ef5c2ae0cc09540432eb96`):

    whisper/timing.py   sha256 bb091c973c428697012f0f8d1b4f5f3a605cd93c5d1c129957f712a6a878a59c

`median_filter`, `backtrace` and `dtw_cpu` below are copied from `timing.py`
with only these changes: the numba decorators and the CUDA branches are
dropped (numba is a heavy dependency for three small functions; the bodies
are unchanged, so `dtw_cpu` stays plain Python over numpy). The glue in
`token_times` follows `find_alignment`, with one deliberate difference in
where the softmax happens, explained there.

Run it with any recent PyTorch (the committed files came from 2.14.1, CPU,
numpy 2.4.6):

    python3 -m venv venv
    venv/bin/pip install numpy torch --index-url https://download.pytorch.org/whl/cpu \
        --extra-index-url https://pypi.org/simple
    venv/bin/python -I make_timing_fixture.py

It writes little-endian files next to itself (no numpy needed to read them):

    timing_attn.f32   [3, 12, 200]  float32  made-up attention of 3 heads:
                                     12 decoded positions over 200 encoder
                                     frames, a ridge along the diagonal plus
                                     noise, softmaxed over the 200 frames
    timing_matrix.f32 [12, 160]     float32  the matrix DTW runs on (before the
                                     sign flip): content_frames = 160 columns,
                                     z-scored, median-filtered (width 7) and
                                     averaged over the heads
    timing_path.i32   [2, L]        int32    DTW's path over -matrix: row 0 the
                                     text index, row 1 the time index
    timing_times.f32  [12]          float32  seconds at which the path moves to
                                     each position (`jump_times`)
    dtw_cost.f32      [9, 14]       float32  small integer costs, full of ties
    dtw_path.i32      [2, L2]       int32    `dtw_cpu(dtw_cost)`

The values only need to match the Rust port to float32 tolerance, so a
different PyTorch build changes the low bits of the float files and nothing
else; the tolerances are in `timing.rs`.
"""

import hashlib
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F

HEADS, ROWS, COLS = 3, 12, 200
CONTENT_FRAMES = 160  # `num_frames // 2` upstream: encoder frames of real audio
MEDFILT_WIDTH = 7
TOKENS_PER_SECOND = 50  # 20 ms per encoder frame


# ---- whisper/timing.py, verbatim except for the decorators and CUDA branches ----
def median_filter(x: torch.Tensor, filter_width: int):
    """Apply a median filter of width `filter_width` along the last dimension of `x`"""
    pad_width = filter_width // 2
    if x.shape[-1] <= pad_width:
        # F.pad requires the padding width to be smaller than the input dimension
        return x

    if (ndim := x.ndim) <= 2:
        # `F.pad` does not support 1D or 2D inputs for reflect padding but supports 3D and 4D
        x = x[None, None, :]

    assert (
        filter_width > 0 and filter_width % 2 == 1
    ), "`filter_width` should be an odd number"

    result = None
    x = F.pad(x, (filter_width // 2, filter_width // 2, 0, 0), mode="reflect")

    if result is None:
        # sort() is faster than torch.median (https://github.com/pytorch/pytorch/issues/51450)
        result = x.unfold(-1, filter_width, 1).sort()[0][..., filter_width // 2]

    if ndim <= 2:
        result = result[0, 0]

    return result


def backtrace(trace: np.ndarray):
    i = trace.shape[0] - 1
    j = trace.shape[1] - 1
    trace[0, :] = 2
    trace[:, 0] = 1

    result = []
    while i > 0 or j > 0:
        result.append((i - 1, j - 1))

        if trace[i, j] == 0:
            i -= 1
            j -= 1
        elif trace[i, j] == 1:
            i -= 1
        elif trace[i, j] == 2:
            j -= 1
        else:
            raise ValueError("Unexpected trace[i, j]")

    result = np.array(result)
    return result[::-1, :].T


def dtw_cpu(x: np.ndarray):
    N, M = x.shape
    cost = np.ones((N + 1, M + 1), dtype=np.float32) * np.inf
    trace = -np.ones((N + 1, M + 1), dtype=np.float32)

    cost[0, 0] = 0
    for j in range(1, M + 1):
        for i in range(1, N + 1):
            c0 = cost[i - 1, j - 1]
            c1 = cost[i - 1, j]
            c2 = cost[i, j - 1]

            if c0 < c1 and c0 < c2:
                c, t = c0, 0
            elif c1 < c0 and c1 < c2:
                c, t = c1, 1
            else:
                c, t = c2, 2

            cost[i, j] = x[i - 1, j - 1] + c
            trace[i, j] = t

    return backtrace(trace)


# ---- find_alignment's glue, on made-up weights ------------------------------------
def make_attention() -> torch.Tensor:
    """3 heads of 12 x 200 attention: a ridge along the diagonal, plus noise.

    Position k looks at frame 10 + 12 k (so the 12th, the end position, at
    142, inside the 160 content frames), with a head-specific width and a
    jitter, and the weights are softmaxed over all 200 frames, the way
    Hugging Face's `output_attentions` are, which is what the `_timestamped`
    export is expected to emit (not yet checked against the real export).
    Upstream's own hooks capture the scores *before* the softmax and softmax
    them over the content frames only, after `weights[:, :, :num_frames // 2]`;
    Hugging Face's `_extract_token_timestamps` (transformers 5.19.0,
    `models/whisper/generation_whisper.py`, sha256
    `eadaed268ecae990416b953f5fc9dace2cf7c517106ac3d5f02db260e85be5cf`)
    crops after the softmax, and that is the order used here.
    """
    rng = np.random.default_rng(384)
    frames = np.arange(COLS)[None, :]
    centres = (10 + 12 * np.arange(ROWS))[:, None] + rng.integers(-3, 4, size=(ROWS, 1))
    heads = []
    for width in (6.0, 9.0, 14.0):
        scores = -((frames - centres) ** 2) / (2 * width**2)
        scores = scores + rng.normal(0.0, 0.35, size=(ROWS, COLS))
        heads.append(scores)
    scores = torch.from_numpy(np.stack(heads).astype(np.float32))
    return scores.softmax(dim=-1)


def token_times(weights: torch.Tensor):
    # heads * tokens * frames, cut to the frames that hold audio.
    weights = weights[:, :, :CONTENT_FRAMES]
    std, mean = torch.std_mean(weights, dim=-2, keepdim=True, unbiased=False)
    weights = (weights - mean) / std
    weights = median_filter(weights, MEDFILT_WIDTH)

    matrix = weights.mean(axis=0)
    text_indices, time_indices = dtw_cpu((-matrix).double().cpu().numpy())

    jumps = np.pad(np.diff(text_indices), (1, 0), constant_values=1).astype(bool)
    jump_times = time_indices[jumps] / TOKENS_PER_SECOND
    return matrix, text_indices, time_indices, jump_times


def write(path: Path, array, dtype: str) -> None:
    data = np.ascontiguousarray(array, dtype=dtype)
    path.write_bytes(data.tobytes())
    digest = hashlib.sha256(data.tobytes()).hexdigest()
    print(f"{path.name:18s} {str(tuple(data.shape)):12s} {data.nbytes:6d} bytes  sha256 {digest}")


def main() -> None:
    here = Path(__file__).resolve().parent
    torch.set_num_threads(1)

    weights = make_attention()
    write(here / "timing_attn.f32", weights.numpy(), "<f4")

    matrix, text_indices, time_indices, jump_times = token_times(weights)
    assert matrix.shape == (ROWS, CONTENT_FRAMES), matrix.shape
    assert len(jump_times) == ROWS, "one jump per position"
    assert bool(np.all(np.diff(jump_times) >= 0))
    write(here / "timing_matrix.f32", matrix.numpy(), "<f4")
    write(here / "timing_path.i32", np.stack([text_indices, time_indices]), "<i4")
    write(here / "timing_times.f32", jump_times.astype(np.float32), "<f4")
    print("jump times:", [round(float(t), 2) for t in jump_times])

    # DTW alone, on integer costs so that equal sums (ties) are exact and the
    # tie-breaking of `dtw_cpu` is what is being tested.
    rng = np.random.default_rng(385)
    cost = rng.integers(0, 4, size=(9, 14)).astype(np.float32)
    write(here / "dtw_cost.f32", cost, "<f4")
    write(here / "dtw_path.i32", np.stack(dtw_cpu(cost.astype(np.float64))), "<i4")


if __name__ == "__main__":
    main()
