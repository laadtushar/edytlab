#!/usr/bin/env python3
"""Lay narration under a cut, duck, master, and mux (video stream copied).

Usage: mix.py CUT VOICE OUTDIR TAG
Inputs and outputs must stay in marketing/promo-v0.7.0. Copy external inputs,
including /tmp files, into that project before running the tool.
"""
import json
import os
import subprocess
import sys
import re
import numpy as np
import script
import prof

SR = 48000
CUTS = {'V1': '16x9', 'V1sq': '1x1', 'R1': 'whats-new-9x16',
        'R2': 'describe-the-edit-9x16', 'R3': '200-tracks-9x16'}
SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                   '..', '..', 'out', 'edytlab-v0.7.0-%s.mp4')
DUCK_MUSIC_DB, DUCK_UI_DB = -11.0, -12.0
VOICE_RMS_DB = -18.0
MAX_TEMPO = 1.12


def run(cmd, **kw):
    """Run a fixed media command and report its existing failure status."""
    p = subprocess.run(cmd, capture_output=True, text=True, check=kw.pop('check', False), **kw)
    if p.returncode:
        raise SystemExit('failed: ' + ' '.join(cmd[:8]) + '\n' + p.stderr[-2500:])
    return p


def dur(p):
    """Probe the duration of a project-local media input."""
    p = str(prof.checked_path(p))
    return float(run(
        ['ffprobe', '-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', p]
    ).stdout)


def read_wav_f32(p):
    """Decode a project-local input to mono float samples."""
    p = str(prof.checked_path(p))
    raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', p, '-ac', '1', '-ar',
                         str(SR), '-f', 'f32le', '-'], capture_output=True, check=False).stdout
    return np.frombuffer(raw, dtype=np.float32).copy()


def trimmed_line(voice, lid, tempo, workdir):
    """Trim and tempo-fit a narration line into the project work folder."""
    src = f'lines/{voice}/{lid}.mp3'
    out = f'{workdir}/{lid}.wav'
    src, out = str(prof.checked_path(src)), str(prof.checked_path(out))
    af = (
        'silenceremove=start_periods=1:start_threshold=-45dB:start_silence=0.05,'
        'areverse,silenceremove=start_periods=1:start_threshold=-45dB:'
        'start_silence=0.05,areverse'
    )
    if tempo > 1.0005:
        af += f',atempo={tempo:.4f}'
    run(['ffmpeg', '-y', '-v', 'error', '-i', src, '-af', af, '-ar', str(SR), '-ac', '1', out])
    return read_wav_f32(out)


def smooth_gain(db_target, rate, attack, release):
    """Smooth the control envelope with the original attack and release rates."""
    g = np.zeros_like(db_target)
    cur = 0.0
    for i, t in enumerate(db_target):
        a = attack if t < cur else release
        cur += (t - cur) * (1 - np.exp(-1.0 / (rate * a)))
        g[i] = cur
    return g


def measure(path):
    """Measure integrated loudness and true peak of a project-local file."""
    path = str(prof.checked_path(path))
    p = run(['ffmpeg', '-nostats', '-i', path, '-af', 'ebur128=peak=true', '-f', 'null', '-'])
    t = p.stderr[p.stderr.rfind('Summary'):]
    integrated_lufs = float(re.search(r'I:\s+(-?[\d.]+) LUFS', t).group(1))
    TP = float(re.search(r'Peak:\s+(-?[\d.]+) dBFS', t).group(1))
    return integrated_lufs, TP


def build(cut, voice, outdir, tempos, tag):
    """Mix, master and mux a cut after preflighting its data paths."""
    slug = CUTS[cut]
    src = SRC % slug
    lines = [line for line in script.L if line['cut'] == ('V1' if cut == 'V1sq' else cut)]
    work = f'work/{voice}-{cut}'
    # Validate all derived paths before creating directories or processing media.
    paths = [src, outdir, work, f'{work}/pre.wav', f'{work}/pre_lim.wav',
             f'{work}/narr24.wav', f'{outdir}/narration-{slug}-{tag}.wav',
             f'{outdir}/edytlab-v0.7.0-{slug}-narrated-{tag}.mp4',
             f'{outdir}/metrics-{slug}-{tag}.json']
    for line in lines:
        paths.extend((f'lines/{voice}/{line["id"]}.mp3', f'{work}/{line["id"]}.wav'))
    for path in paths:
        prof.checked_path(path)
    os.makedirs(prof.checked_path(outdir), exist_ok=True)
    os.makedirs(prof.checked_path(work), exist_ok=True)
    total = dur(src)
    n = int(round(total * SR))
    orig = prof.load(src, SR)
    orig = np.pad(orig, ((0, max(0, n - len(orig))), (0, 0)))[:n]
    narr = np.zeros(n, dtype=np.float32)
    duck_db = np.zeros(int(total * 100) + 2)  # 100 Hz control rate
    placed = []
    for line in lines:
        w = trimmed_line(voice, line['id'], tempos.get(line['id'], 1.0), work)
        rms = np.sqrt((w ** 2).mean())
        w = w * (10 ** (VOICE_RMS_DB / 20) / rms)
        f = int(0.015 * SR)
        w[:f] *= np.linspace(0, 1, f)
        w[-f:] *= np.linspace(1, 0, f)
        s = int(round(line['at'] * SR))
        w = w[:max(0, n - s)]
        narr[s:s + len(w)] += w
        t0, t1 = line['at'], line['at'] + len(w) / SR
        i0, i1 = int(max(0, (t0 - 0.15)) * 100), int((t1 + 0.25) * 100)
        depth = DUCK_MUSIC_DB if line['music'] else DUCK_UI_DB
        duck_db[i0:i1] = np.minimum(duck_db[i0:i1], depth)
        placed.append(dict(id=line['id'], start=t0, end=t1, music=line['music']))
    g_db = smooth_gain(duck_db, 100, 0.08, 0.35)
    g = 10 ** (
        np.interp(np.arange(n) / SR, np.arange(len(g_db)) / 100, g_db) / 20
    ).astype(np.float32)
    ducked = orig * g[:, None]
    mixed = ducked + narr[:, None]
    pre = f'{work}/pre.wav'

    def write_f32(arr, path):
        """Write float samples to a checked project-local destination."""
        path = str(prof.checked_path(path))
        subprocess.run(
            ['ffmpeg', '-y', '-v', 'error', '-f', 'f32le', '-ar', str(SR),
             '-ac', str(arr.shape[1] if arr.ndim > 1 else 1),
             '-i', '-', '-c:a', 'pcm_s24le', path],
            input=np.ascontiguousarray(arr.astype(np.float32)).tobytes(),
            check=True)
    write_f32(mixed, pre)
    I_orig, TP_orig = measure(src)
    # narration alone, for the deliverable (stereo, same length as the video)
    nar_wav = f'{outdir}/narration-{slug}-{tag}.wav'
    write_f32(narr[:, None], f'{work}/narr24.wav')
    run([
        'ffmpeg', '-y', '-v', 'error', '-i', str(prof.checked_path(f'{work}/narr24.wav')),
        '-c:a', 'pcm_s16le', str(prof.checked_path(nar_wav)),
    ])
    # Two-pass loudnorm to the cut's own loudness, under -1 dBTP on the finished AAC.
    tp_target = -1.5
    out_mp4 = f'{outdir}/edytlab-v0.7.0-{slug}-narrated-{tag}.mp4'
    boost = 0.0
    src_pre = pre
    for _attempt in range(8):
        if boost > 0:
            src_pre = f'{work}/pre_lim.wav'
            run([
                'ffmpeg', '-y', '-v', 'error', '-i', str(prof.checked_path(pre)), '-af',
                f'volume={boost:.2f}dB,alimiter=limit=0.79:attack=3:release=60:level=disabled',
                '-c:a', 'pcm_s24le', str(prof.checked_path(src_pre)),
            ])
        p = run(['ffmpeg', '-nostats', '-i', str(prof.checked_path(src_pre)), '-af',
                f'loudnorm=I={I_orig}:TP={tp_target}:LRA=20:print_format=json', '-f', 'null', '-'])
        j = json.loads(p.stderr[p.stderr.rfind('{'):p.stderr.rfind('}')+1])
        ln = (
            f"loudnorm=I={I_orig}:TP={tp_target}:LRA=20:measured_I={j['input_i']}"
            f":measured_TP={j['input_tp']}:measured_LRA={j['input_lra']}"
            f":measured_thresh={j['input_thresh']}:offset={j['target_offset']}"
            ":linear=true,aresample=48000"
        )
        run(['ffmpeg', '-y', '-v', 'error', '-i', str(prof.checked_path(src)),
             '-i', str(prof.checked_path(src_pre)),
             '-map', '0:v:0', '-map', '1:a:0', '-c:v', 'copy', '-af', ln, '-c:a', 'aac', '-b:a',
             '160k', '-ar', '48000', '-ac', '2', '-t', f'{total:.6f}', '-movflags', '+faststart',
             str(prof.checked_path(out_mp4))])
        integrated_lufs, TP = measure(out_mp4)
        if TP > -1.05:
            tp_target -= 0.5
            continue
        if integrated_lufs < I_orig - 0.15:
            boost += (I_orig - integrated_lufs) + 0.1
            continue
        break
    gain = integrated_lufs - measure(pre)[0]  # includes any limiter boost
    metrics = dict(
        cut=cut, voice=voice, tag=tag, out=out_mp4, I_orig=I_orig,
        TP_orig=TP_orig, I=integrated_lufs, TP=TP, final_gain_db=gain, tempos=tempos,
        placed=placed,
    )
    # own-sound check: RMS of the ducked original vs the original, inside each narration line
    chk = []
    for pl in placed:
        a, b = int(pl['start'] * SR), int(pl['end'] * SR)
        ro = np.sqrt((orig[a:b] ** 2).mean() + 1e-12)
        rd = np.sqrt((ducked[a:b] ** 2).mean() + 1e-12)
        rn = np.sqrt((narr[a:b] ** 2).mean() + 1e-12)

        def db(x):
            """Convert the measured RMS amplitude to decibels."""
            return float(20 * np.log10(x))

        chk.append(dict(id=pl['id'], music=pl['music'], orig_rms=db(ro), ducked_rms=db(
            rd) + gain, voice_rms=db(rn) + gain, duck_db=db(rd) - db(ro)))
    metrics['own_sound'] = chk
    with prof.checked_path(f'{outdir}/metrics-{slug}-{tag}.json').open('w') as stream:
        json.dump(metrics, stream, indent=1, default=float)
    return metrics


if __name__ == '__main__':
    if sys.argv[1:] in (['-h'], ['--help']):
        print(__doc__)
        raise SystemExit(0)
    cut, voice, outdir, tag = sys.argv[1:5]
    tempos = {}
    import math
    for line in script.L:
        if line['cut'] != ('V1' if cut == 'V1sq' else cut):
            continue
        d = dur(f'trim/{voice}/{line["id"]}.wav')
        room = (line['hi'] - 0.3) - max(line['at'], line['lo'] + 0.3)
        need = d / room
        if need > 1.0:
            tempos[line['id']] = math.ceil(need * 100 + 1) / 100
        if need > MAX_TEMPO:
            raise SystemExit(f'{line["id"]} needs tempo {need:.2f}')
    m = build(cut, voice, outdir, tempos, tag)
    print(json.dumps({k: m[k] for k in ('cut', 'I_orig',
          'TP_orig', 'I', 'TP', 'final_gain_db', 'tempos')}))
