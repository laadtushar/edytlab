#!/usr/bin/env python3
"""Synthesise the few UI sounds the launch cuts use (code-driven sound design, no samples, no licence strings).

whoosh : band-limited noise whose low-pass sweeps up then down under a smooth envelope (scene change)
tick   : a 35 ms sine ping with a hair of noise (callout ring closes)
chime  : three soft sine partials and a low sub thump (the wordmark lands)
All 48 kHz mono 16-bit; peaks stay under 0.5 so the mix never clips.
"""
import math, random, struct, sys, wave

SR = 48000
OUT = sys.argv[1]


def save(name, x):
    pk = max(abs(v) for v in x) or 1
    x = [v / pk * 0.5 for v in x]
    with wave.open(f"{OUT}/{name}.wav", "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, v)) * 32767)) for v in x))


def whoosh(d=0.6):
    random.seed(7)
    n = int(SR * d)
    y = 0.0
    out = []
    for i in range(n):
        t = i / n
        fc = 300 + 3200 * math.sin(math.pi * t) ** 2          # sweep up then down
        a = 1 - math.exp(-2 * math.pi * fc / SR)
        y += a * (random.uniform(-1, 1) - y)
        env = math.sin(math.pi * min(1, t * 1.15)) ** 2 * (1 - t) ** 0.6
        out.append(y * env)
    return out


def tick(d=0.12):
    random.seed(3)
    n = int(SR * d)
    return [(math.sin(2 * math.pi * 1900 * i / SR) * math.exp(-i / (SR * 0.035)) * 0.8
             + math.sin(2 * math.pi * 3800 * i / SR) * math.exp(-i / (SR * 0.02)) * 0.2
             + random.uniform(-1, 1) * math.exp(-i / (SR * 0.004)) * 0.15) for i in range(n)]


def chime(d=1.6):
    n = int(SR * d)
    out = [0.0] * n
    for k, (f, delay, amp) in enumerate(((784.0, 0.0, 1.0), (987.8, 0.07, 0.8), (1174.7, 0.14, 0.7))):
        s0 = int(delay * SR)
        for i in range(n - s0):
            t = i / SR
            out[s0 + i] += amp * math.sin(2 * math.pi * f * t) * (1 - math.exp(-t / 0.008)) * math.exp(-t / 0.55)
    for i in range(int(SR * 0.5)):
        t = i / SR
        out[i] += 0.9 * math.sin(2 * math.pi * 82 * t) * math.exp(-t / 0.16) * (1 - math.exp(-t / 0.004))
    return out


for name, fn in (("whoosh", whoosh), ("tick", tick), ("chime", chime)):
    save(name, fn())
print("sfx ok")
