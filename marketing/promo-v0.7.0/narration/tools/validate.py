#!/usr/bin/env python3
import json, subprocess, sys, numpy as np, script, prof, mix
SR = 48000
def probe(p):
    j = json.loads(subprocess.run(['ffprobe','-v','error','-print_format','json','-show_streams','-show_format',p],capture_output=True,text=True).stdout)
    v = [s for s in j['streams'] if s['codec_type']=='video'][0]; a = [s for s in j['streams'] if s['codec_type']=='audio'][0]
    return dict(w=v['width'],h=v['height'],fps=v['r_frame_rate'],vcodec=v['codec_name']+'/'+v.get('profile',''),pix=v['pix_fmt'],nb=v.get('nb_frames'),vdur=float(v['duration']),acodec=a['codec_name'],sr=a['sample_rate'],ch=a['channels'],fdur=float(j['format']['duration']))
def check(metrics_path):
    m = json.load(open(metrics_path)); cut = m['cut']; slug = mix.CUTS[cut]; src = mix.SRC % slug; out = m['out']
    pa, pb = probe(src), probe(out)
    same = all(pa[k]==pb[k] for k in ('w','h','fps','nb','vdur','pix')) and abs(pa['fdur']-pb['fdur'])<0.02
    res = dict(cut=cut, slug=slug, probe_src=pa, probe_out=pb, same_video=same, I_orig=m['I_orig'], TP_orig=m['TP_orig'], I=m['I'], TP=m['TP'])
    # timing
    rows = []
    lines = {l['id']: l for l in script.L}
    for pl in m['placed']:
        l = lines[pl['id']]
        lo, hi = l['lo'], l['hi']
        ok = pl['start'] >= lo + 0.3 - 1e-6 and pl['end'] <= hi - 0.3 + 1e-6
        rows.append(dict(id=pl['id'], shot=l['shot'], shot_start=lo, shot_end=hi, start=pl['start'], end=pl['end'], ok=bool(ok), music=pl['music']))
    res['timing'] = rows
    # own sound, measured on the finished file: final - gain*narration
    # Own sound in the finished file, measured by energy (no waveform subtraction: the AAC delay would spoil that):
    # pre = ducked original + narration (exactly, sample for sample); the finished file is pre scaled by a gain, so
    # own-sound level = RMS(pre - narration) + (RMS(finished) - RMS(pre)) over each narration line.
    fin = prof.load(out, SR)
    pre = prof.load(f"work/{m['voice']}-{m['cut']}/pre.wav", SR)
    nar = prof.load(out.replace('edytlab-v0.7.0-%s-narrated-%s.mp4' % (slug, m['tag']), 'narration-%s-%s.wav' % (slug, m['tag'])), SR)
    org = prof.load(src, SR); n = min(len(fin), len(nar), len(org), len(pre))
    own = []
    for pl in m['placed']:
        a, b = int(pl['start']*SR), int(pl['end']*SR)
        r = lambda x: float(20*np.log10(np.sqrt((x[a:b]**2).mean()+1e-12)))
        step = r(fin) - r(pre)
        dk = pre[:n] - nar[:n]
        own.append(dict(id=pl['id'], music=pl['music'], orig_rms=r(org), own_in_mix_rms=r(dk)+step, voice_rms=r(nar)+step, drop_db=r(dk)+step-r(org)))
    res['own'] = own
    res['pass'] = dict(timing=all(r['ok'] for r in rows), peak=m['TP'] <= -1.0, loud=abs(m['I']-m['I_orig'])<=0.3, video=same,
                       codecs=(pb['vcodec'].startswith('h264') and pb['acodec']=='aac'),
                       own_audible=all(o['own_in_mix_rms'] > -38 for o in own if o['music']))
    return res
if __name__ == '__main__':
    r = check(sys.argv[1]); print(json.dumps(r, indent=1, default=float))
