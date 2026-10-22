// `node scripts/cues.ts [outDir]` writes, for every cut, the sound cues (work/cues/<id>.json) and the .srt of the
// on-screen words (<outDir>/<id>.srt), both from src/cuts.ts, the file the compositions render.
//
// Sound is only ever (a) the app's own audio: the S1 and S7 recordings' .wav files, at the moments of the footage they
// belong to, and (b) UI sounds synthesised in code (marketing/launch-video/tools/sfx.py: a whoosh on a scene change, a
// tick when a callout lands, a chime under the wordmark). Nothing else.
import {spawnSync} from 'node:child_process';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {CUTS} from '../src/cuts.ts';
import {placements, segStarts, srt} from '../src/captions.ts';
import {CLIPS, joined, ringSpan, shown, srcToOut, type Cut} from '../src/timeline.ts';
import {PUBLIC_DIR, ROOT, WORK} from './common.ts';

const outDir = process.argv[2] ?? resolve(ROOT, '../out');
mkdirSync(resolve(WORK, 'cues'), {recursive: true});
mkdirSync(outDir, {recursive: true});

/** Peak (0..1) of a stretch of a wav, decoded by ffmpeg. */
function peak(file: string, a: number, b: number): number {
  const r = spawnSync('ffmpeg', ['-nostdin', '-v', 'error', '-ss', String(a), '-t', String(b - a), '-i', file, '-ac', '1', '-f', 's16le', '-'], {maxBuffer: 1 << 28});
  if (r.status !== 0) throw new Error(`ffmpeg could not read ${file}`);
  const buf = r.stdout as Buffer;
  let pk = 0;
  for (let i = 0; i + 1 < buf.length; i += 2) pk = Math.max(pk, Math.abs(buf.readInt16LE(i)) / 32768);
  return pk;
}

export interface CueFile {
  id: string;
  /** The output files' name: edytlab-v0.7.0-<slug>.* */
  slug: string;
  title: string;
  platform: string;
  total: number;
  clips: {wav: string; src0: number; dur: number; t0: number}[];
  /** [time, sound, dB, pitch factor] */
  sfx: [number, 'whoosh' | 'tick' | 'chime', number, number?][];
}

function build(cut: Cut): CueFile {
  const at = placements(cut);
  const clips: CueFile['clips'] = [];
  const sfx: CueFile['sfx'] = [];
  const last = at[at.length - 1];
  cut.parts.forEach((p, i) => {
    const T = at[i].start;
    if (i === 0 || !joined(cut, i)) sfx.push([Math.max(0, T - (i === 0 ? 0 : 0.04)), 'whoosh', i === 0 ? -9 : -7]);
    if (p.kind === 'end') {
      sfx.push([T + 0.3, 'chime', -4]);
      return;
    }
    const starts = segStarts(p);
    const wav = CLIPS[p.clip].wav;
    p.segs.forEach((g, k) => {
      if (!wav) return;
      const file = resolve(PUBLIC_DIR, wav);
      const rate = g.rate ?? 1;
      if (p.audio && rate === 1) clips.push({wav, src0: g.a, dur: g.b - g.a, t0: T + starts[k]});
      else if (peak(file, g.a, g.b) > 0.01) {
        throw new Error(`${cut.id}/${p.id}: the clip's sound is audible in ${g.a}..${g.b} s, but this stretch does not play it (rate ${rate}, audio ${!!p.audio})`);
      }
    });
    // a tick when a label lands
    for (const r of p.rings) {
      const labels = r.labels ?? (r.label ? [{t0: r.t0, t1: r.t1, text: r.label}] : []);
      for (const l of labels) {
        const s = ringSpan(p.segs, l);
        if (s) sfx.push([T + s[0] + 0.1, 'tick', -6]);
      }
    }
    for (const k of p.ticks) if (shown(p.segs, k.t)) sfx.push([T + srcToOut(p.segs, k.t), 'tick', -7, k.pitch ?? 1]);
  });
  // thin out ticks that land within 0.12 s of another
  sfx.sort((x, y) => x[0] - y[0]);
  const kept: CueFile['sfx'] = [];
  for (const s of sfx) {
    const prev = [...kept].reverse().find((k) => k[1] === s[1]);
    if (s[1] === 'tick' && prev && s[0] - prev[0] < 0.08) continue;
    kept.push(s);
  }
  return {id: cut.id, slug: cut.slug, title: cut.title, platform: cut.platform, total: last.start + last.dur, clips, sfx: kept};
}

for (const cut of CUTS) {
  const c = build(cut);
  writeFileSync(resolve(WORK, 'cues', `${cut.id}.json`), JSON.stringify(c, null, 1));
  writeFileSync(resolve(outDir, `edytlab-v0.7.0-${cut.slug}.srt`), srt(cut));
  console.log(`${cut.id}: ${c.total.toFixed(2)} s, ${c.clips.length} app-audio stretch(es), ${c.sfx.length} UI sounds`);
}
