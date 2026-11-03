#!/usr/bin/env python3
"""Check narration timing, levels and preservation of the source video.

Usage: validate.py METRICS_JSON
Inputs must stay in marketing/promo-v0.7.0. Copy external inputs, including
/tmp files, into that project before running the tool.
"""
import json
import subprocess
import sys
import numpy as np
import script
import prof
import mix
SR = 48000


def probe(p):
    """Probe stream and container metadata from a checked local input."""
    p = str(prof.checked_path(p))
    j = json.loads(
        subprocess.run(
            ['ffprobe', '-v', 'error', '-print_format', 'json', '-show_streams', '-show_format', p],
            capture_output=True, text=True, check=False).stdout)
    v = [s for s in j['streams'] if s['codec_type'] == 'video'][0]
    a = [s for s in j['streams'] if s['codec_type'] == 'audio'][0]
    return dict(
        w=v['width'],
        h=v['height'],
        fps=v['r_frame_rate'],
        vcodec=v['codec_name'] + '/' + v.get('profile', ''),
        pix=v['pix_fmt'],
        nb=v.get('nb_frames'),
        vdur=float(v['duration']),
        acodec=a['codec_name'],
        sr=a['sample_rate'],
        ch=a['channels'],
        fdur=float(j['format']['duration']))


def check(metrics_path):
    """Validate a checked metrics file and the media paths it references."""
    with prof.checked_path(metrics_path).open() as stream:
        m = json.load(stream)
    cut = m['cut']
    slug = mix.CUTS[cut]
    src = mix.SRC % slug
    out = m['out']
    pre_path = f"work/{m['voice']}-{m['cut']}/pre.wav"
    nar_path = out.replace(f"edytlab-v0.7.0-{slug}-narrated-{m['tag']}.mp4",
                           f"narration-{slug}-{m['tag']}.wav")
    for path in (src, out, pre_path, nar_path):
        prof.checked_path(path)
    pa, pb = probe(src), probe(out)
    same = all(pa[k] == pb[k] for k in ('w', 'h', 'fps', 'nb', 'vdur', 'pix')
               ) and abs(pa['fdur']-pb['fdur']) < 0.02
    res = dict(cut=cut, slug=slug, probe_src=pa, probe_out=pb, same_video=same,
               I_orig=m['I_orig'], TP_orig=m['TP_orig'], I=m['I'], TP=m['TP'])
    # timing
    rows = []
    lines = {line['id']: line for line in script.L}
    for pl in m['placed']:
        line = lines[pl['id']]
        lo, hi = line['lo'], line['hi']
        ok = pl['start'] >= lo + 0.3 - 1e-6 and pl['end'] <= hi - 0.3 + 1e-6
        rows.append(dict(id=pl['id'], shot=line['shot'], shot_start=lo, shot_end=hi,
                    start=pl['start'], end=pl['end'], ok=bool(ok), music=pl['music']))
    res['timing'] = rows
    # own sound, measured on the finished file: final - gain*narration
    # Measure own sound by energy: AAC delay would spoil waveform subtraction.
    # pre = ducked original + narration exactly; the finished file is pre scaled by a gain, so
    # own-sound level = RMS(pre - narration) + (RMS(finished) - RMS(pre)) over each narration line.
    fin = prof.load(out, SR)
    pre = prof.load(pre_path, SR)
    nar = prof.load(nar_path, SR)
    org = prof.load(src, SR)
    n = min(len(fin), len(nar), len(org), len(pre))
    own = []
    for pl in m['placed']:
        a, b = int(pl['start']*SR), int(pl['end']*SR)

        def r(x, start=a, end=b):
            """Measure RMS decibels over the current narration window."""
            return float(20*np.log10(np.sqrt((x[start:end]**2).mean()+1e-12)))

        step = r(fin) - r(pre)
        dk = pre[:n] - nar[:n]
        own.append(dict(id=pl['id'], music=pl['music'], orig_rms=r(org), own_in_mix_rms=r(
            dk)+step, voice_rms=r(nar)+step, drop_db=r(dk)+step-r(org)))
    res['own'] = own
    res['pass'] = dict(timing=all(r['ok'] for r in rows),
                       peak=m['TP'] <= -1.0, loud=abs(m['I'] - m['I_orig']) <= 0.3, video=same,
                       codecs=(pb['vcodec'].startswith('h264') and pb['acodec'] == 'aac'),
                       own_audible=all(o['own_in_mix_rms'] > -38 for o in own if o['music']))
    return res


if __name__ == '__main__':
    if sys.argv[1:] in (['-h'], ['--help']):
        print(__doc__)
        raise SystemExit(0)
    result = check(sys.argv[1])
    print(json.dumps(result, indent=1, default=float))
