// What is on screen, and when, as plain data: the source of the .srt and of the sound cues. Reads the same cuts the
// compositions render (cuts.ts), so the captions can never drift from the picture.
import {END, NOTE} from './brand.ts';
import {LAYOUTS} from './layout.ts';
import {FPS, partDur, place, ringSpan, segDur, titleLines, type Cut, type Shot} from './timeline.ts';

export interface Event {
  a: number;
  b: number;
  text: string;
  /** Sort key within a cue: titles first, then callouts, then the note. */
  rank: number;
}

export interface Placed {
  start: number;
  dur: number;
  /** Seconds the next part starts after this one starts (the part's visible length). */
  visible: number;
}

export function placements(cut: Cut): Placed[] {
  const {starts, frames, overlaps} = place(cut);
  return cut.parts.map((p, i) => ({
    start: starts[i] / FPS,
    dur: partDur(p),
    visible: ((starts[i + 1] ?? starts[i] + frames[i]) - starts[i]) / FPS + 0 * overlaps[i],
  }));
}

/** Start (seconds in shot time) of each stretch of a shot. */
export const segStarts = (shot: Shot): number[] => {
  let t = 0;
  return shot.segs.map((g) => {
    const s = t;
    t += segDur(g);
    return s;
  });
};

export function events(cut: Cut): Event[] {
  const L = LAYOUTS[cut.layout];
  const at = placements(cut);
  const out: Event[] = [];
  cut.parts.forEach((p, i) => {
    const T = at[i].start;
    const end = T + at[i].visible;
    const clip = (a: number, b: number, text: string, rank: number) => {
      const A = Math.max(T, T + a);
      const B = Math.min(end, T + b);
      if (B - A > 0.05) out.push({a: A, b: B, text, rank});
    };
    if (p.kind === 'end') {
      clip(0.4, p.dur, `${END.wordmark}\n${END.version}\n${END.url}\n${END.free}\n${END.platforms}\n${END.unsigned}`, 0);
      return;
    }
    for (const t of p.titles) {
      const shown = titleLines(t, L.subs || !!t.big, L.kind === 'wide' && !t.big);
      clip(t.t0, t.t1, [...shown.lines, ...(shown.sub ? [shown.sub] : [])].join('\n'), 0);
    }
    for (const c of p.chips) clip(c.t0, c.t1, c.text, 1);
    for (const r of p.rings) {
      const labels = r.labels ?? (r.label ? [{t0: r.t0, t1: r.t1, text: r.label}] : []);
      for (const l of labels) {
        const s = ringSpan(p.segs, l);
        if (s) clip(s[0], s[1], l.text, 1);
      }
    }
    const starts = segStarts(p);
    p.segs.forEach((g, k) => {
      if ((g.rate ?? 1) >= 1.45) clip(starts[k], starts[k] + segDur(g), `${Number.isInteger(g.rate) ? g.rate : (g.rate ?? 1).toFixed(1)}× speed`, 1);
    });
    if (p.note) clip(0, at[i].dur, NOTE, 2);
  });
  return out;
}

export interface Cue {
  a: number;
  b: number;
  text: string;
}

/** Non-overlapping subtitle cues: every moment's visible words, split wherever the set of words changes. */
export function cues(cut: Cut, minDur = 0.5): Cue[] {
  const ev = events(cut);
  const times = [...new Set(ev.flatMap((e) => [e.a, e.b].map((t) => Math.round(t * 1000) / 1000)))].sort((x, y) => x - y);
  const raw: Cue[] = [];
  for (let i = 0; i + 1 < times.length; i++) {
    const a = times[i];
    const b = times[i + 1];
    const mid = (a + b) / 2;
    const active = ev.filter((e) => e.a <= mid && mid < e.b).sort((x, y) => x.rank - y.rank);
    if (active.length === 0) continue;
    // one cue per run of the same words
    const text = active.map((e) => e.text).join('\n');
    const last = raw[raw.length - 1];
    if (last && last.text === text && Math.abs(last.b - a) < 0.002) last.b = b;
    else raw.push({a, b, text});
  }
  // a cue shorter than minDur is absorbed by the one before it (or the one after, at the start)
  const kept: Cue[] = [];
  for (const c of raw) {
    if (c.b - c.a < minDur && kept.length) kept[kept.length - 1].b = c.b;
    else kept.push({...c});
  }
  // ...and a short first cue is absorbed by the one after it
  if (kept.length > 1 && kept[0].b - kept[0].a < minDur) {
    kept[1].a = kept[0].a;
    kept.shift();
  }
  return kept;
}

const ts = (x: number) => {
  const ms = Math.round(x * 1000);
  const h = Math.floor(ms / 3600000);
  const m = Math.floor((ms % 3600000) / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  const r = ms % 1000;
  const p = (n: number, w = 2) => String(n).padStart(w, '0');
  return `${p(h)}:${p(m)}:${p(s)},${p(r, 3)}`;
};

export function srt(cut: Cut): string {
  return cues(cut)
    .map((c, i) => `${i + 1}\n${ts(c.a)} --> ${ts(c.b)}\n${c.text}\n`)
    .join('\n');
}
