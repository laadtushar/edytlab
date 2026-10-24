#!/usr/bin/env python3
"""Lay the narration lines under a cut, duck the cut's own audio under the voice, master, mux (video stream copied)."""
import json, os, subprocess, sys, re, wave
import numpy as np
import script, prof

SR = 48000
CUTS = {'BT': 'dj-beatmatched-transition', 'EI': 'dj-extended-club-intro', 'MM': 'dj-mini-mix', 'V1': '16x9', 'V1sq': '1x1', 'R1': 'whats-new-9x16', 'R2': 'describe-the-edit-9x16', 'R3': '200-tracks-9x16'}
SRCD = None  # the promo cuts are not narrated from here
DEMOD = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '..', 'website', 'public', 'demos', '%s.mp4')
class _Src:
    def __mod__(self, slug): return (DEMOD if slug.startswith('dj-') else SRCD) % slug
SRC = _Src()
def out_name(slug, tag): return (f'{slug}-narrated-{tag}.mp4' if slug.startswith('dj-') else f'edytlab-v0.7.0-{slug}-narrated-{tag}.mp4')
DUCK_MUSIC_DB, DUCK_UI_DB = -11.0, -12.0
VOICE_RMS_DB = -18.0
MAX_TEMPO = 1.12

def run(cmd, **kw):
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if p.returncode: raise SystemExit('failed: ' + ' '.join(cmd[:8]) + '\n' + p.stderr[-2500:])
    return p

def dur(p):
    return float(run(['ffprobe','-v','error','-show_entries','format=duration','-of','csv=p=0',p]).stdout)

def read_wav_f32(p):
    raw = subprocess.run(['ffmpeg','-v','error','-i',p,'-ac','1','-ar',str(SR),'-f','f32le','-'],capture_output=True).stdout
    return np.frombuffer(raw, dtype=np.float32).copy()

def trimmed_line(voice, lid, tempo, workdir):
    src = f'lines/{voice}/{lid}.mp3'
    out = f'{workdir}/{lid}.wav'
    af = 'silenceremove=start_periods=1:start_threshold=-45dB:start_silence=0.05,areverse,silenceremove=start_periods=1:start_threshold=-45dB:start_silence=0.05,areverse'
    if tempo > 1.0005: af += f',atempo={tempo:.4f}'
    run(['ffmpeg','-y','-v','error','-i',src,'-af',af,'-ar',str(SR),'-ac','1',out])
    return read_wav_f32(out)

def smooth_gain(db_target, rate, attack, release):
    g = np.zeros_like(db_target); cur = 0.0
    for i, t in enumerate(db_target):
        a = attack if t < cur else release
        cur += (t - cur) * (1 - np.exp(-1.0 / (rate * a)))
        g[i] = cur
    return g

def measure(path):
    p = run(['ffmpeg','-nostats','-i',path,'-af','ebur128=peak=true','-f','null','-'])
    t = p.stderr[p.stderr.rfind('Summary'):]
    I = float(re.search(r'I:\s+(-?[\d.]+) LUFS', t).group(1)); TP = float(re.search(r'Peak:\s+(-?[\d.]+) dBFS', t).group(1))
    return I, TP

def build(cut, voice, outdir, tempos, tag):
    slug = CUTS[cut]; src = SRC % slug
    lines = [l for l in script.L if l['cut'] == ('V1' if cut == 'V1sq' else cut)]
    os.makedirs(outdir, exist_ok=True); work = f'work/{voice}-{cut}'; os.makedirs(work, exist_ok=True)
    total = dur(src); n = int(round(total * SR))
    orig = prof.load(src, SR)
    orig = np.pad(orig, ((0, max(0, n - len(orig))), (0, 0)))[:n]
    narr = np.zeros(n, dtype=np.float32)
    duck_db = np.zeros(int(total * 100) + 2)  # 100 Hz control rate
    placed = []
    for l in lines:
        w = trimmed_line(voice, l['id'], tempos.get(l['id'], 1.0), work)
        rms = np.sqrt((w ** 2).mean()); w = w * (10 ** (VOICE_RMS_DB / 20) / rms)
        f = int(0.015 * SR); w[:f] *= np.linspace(0, 1, f); w[-f:] *= np.linspace(1, 0, f)
        s = int(round(l['at'] * SR)); w = w[:max(0, n - s)]
        narr[s:s + len(w)] += w
        t0, t1 = l['at'], l['at'] + len(w) / SR
        i0, i1 = int(max(0, (t0 - 0.15)) * 100), int((t1 + 0.25) * 100)
        depth = DUCK_MUSIC_DB if l['music'] else DUCK_UI_DB
        duck_db[i0:i1] = np.minimum(duck_db[i0:i1], depth)
        placed.append(dict(id=l['id'], start=t0, end=t1, music=l['music']))
    g_db = smooth_gain(duck_db, 100, 0.08, 0.35)
    g = 10 ** (np.interp(np.arange(n) / SR, np.arange(len(g_db)) / 100, g_db) / 20).astype(np.float32)
    ducked = orig * g[:, None]
    mixed = ducked + narr[:, None]
    pre = f'{work}/pre.wav'
    def write_f32(arr, path):
        subprocess.run(['ffmpeg','-y','-v','error','-f','f32le','-ar',str(SR),'-ac',str(arr.shape[1] if arr.ndim>1 else 1),'-i','-','-c:a','pcm_s24le',path],input=np.ascontiguousarray(arr.astype(np.float32)).tobytes(),check=True)
    write_f32(mixed, pre)
    I_orig, TP_orig = measure(src)
    # narration alone, for the deliverable (stereo, same length as the video)
    nar_wav = f'{outdir}/narration-{slug}-{tag}.wav'
    write_f32(narr[:, None], f'{work}/narr24.wav')
    run(['ffmpeg','-y','-v','error','-i',f'{work}/narr24.wav','-c:a','pcm_s16le',nar_wav])
    # two-pass loudnorm to the cut's own integrated loudness, true peak under -1 dBTP on the finished AAC
    tp_target = -1.5
    out_mp4 = f'{outdir}/' + out_name(slug, tag)
    boost = 0.0
    src_pre = pre
    for attempt in range(8):
        if boost > 0:
            src_pre = f'{work}/pre_lim.wav'
            run(['ffmpeg','-y','-v','error','-i',pre,'-af',f'volume={boost:.2f}dB,alimiter=limit=0.79:attack=3:release=60:level=disabled','-c:a','pcm_s24le',src_pre])
        p = run(['ffmpeg','-nostats','-i',src_pre,'-af',f'loudnorm=I={I_orig}:TP={tp_target}:LRA=20:print_format=json','-f','null','-'])
        j = json.loads(p.stderr[p.stderr.rfind('{'):p.stderr.rfind('}')+1])
        ln = (f"loudnorm=I={I_orig}:TP={tp_target}:LRA=20:measured_I={j['input_i']}:measured_TP={j['input_tp']}:measured_LRA={j['input_lra']}"
              f":measured_thresh={j['input_thresh']}:offset={j['target_offset']}:linear=true,aresample=48000")
        run(['ffmpeg','-y','-v','error','-i',src,'-i',src_pre,'-map','0:v:0','-map','1:a:0','-c:v','copy','-af',ln,'-c:a','aac','-b:a','160k','-ar','48000','-ac','2','-t',f'{total:.6f}','-movflags','+faststart',out_mp4])
        I, TP = measure(out_mp4)
        if TP > -1.05:
            tp_target -= 0.5; continue
        if I < I_orig - 0.15:
            boost += (I_orig - I) + 0.1; continue
        break
    gain = I - measure(pre)[0]  # includes any limiter boost
    metrics = dict(cut=cut, voice=voice, tag=tag, out=out_mp4, I_orig=I_orig, TP_orig=TP_orig, I=I, TP=TP, final_gain_db=gain, tempos=tempos, placed=placed)
    # own-sound check: RMS of the ducked original vs the original, inside each narration line
    chk = []
    for pl in placed:
        a, b = int(pl['start'] * SR), int(pl['end'] * SR)
        ro = np.sqrt((orig[a:b] ** 2).mean() + 1e-12); rd = np.sqrt((ducked[a:b] ** 2).mean() + 1e-12)
        rn = np.sqrt((narr[a:b] ** 2).mean() + 1e-12)
        db = lambda x: float(20 * np.log10(x))
        chk.append(dict(id=pl['id'], music=pl['music'], orig_rms=db(ro), ducked_rms=db(rd) + gain, voice_rms=db(rn) + gain, duck_db=db(rd) - db(ro)))
    metrics['own_sound'] = chk
    json.dump(metrics, open(f'{outdir}/metrics-{slug}-{tag}.json', 'w'), indent=1, default=float)
    return metrics

if __name__ == '__main__':
    cut, voice, outdir, tag = sys.argv[1:5]
    tempos = {}
    import math
    for l in script.L:
        if l['cut'] != ('V1' if cut == 'V1sq' else cut): continue
        d = dur(f'trim/{voice}/{l["id"]}.wav')
        room = (l['hi'] - 0.3) - max(l['at'], l['lo'] + 0.3)
        need = d / room
        if need > 1.0: tempos[l['id']] = math.ceil(need * 100 + 1) / 100
        if need > MAX_TEMPO: raise SystemExit(f'{l["id"]} needs tempo {need:.2f}')
    m = build(cut, voice, outdir, tempos, tag)
    print(json.dumps({k: m[k] for k in ('cut','I_orig','TP_orig','I','TP','final_gain_db','tempos')}))
