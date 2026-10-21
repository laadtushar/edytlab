// Replicates make-demo-videos.mjs timing: raw caption/pace times -> final mp4 timeline.
import { readFileSync, writeFileSync } from "node:fs";
const SPEED = 1.25;
const slugs = ["dj-beatmatched-transition", "dj-extended-club-intro", "dj-mini-mix"];
const v0s = { "dj-beatmatched-transition": 1791577463.867, "dj-extended-club-intro": 1791577874.667, "dj-mini-mix": 1791579088.533 };
const ends = { "dj-beatmatched-transition": 1791577828.067, "dj-extended-club-intro": 1791578054.267, "dj-mini-mix": 1791579357.8 };
const finalDur = { "dj-beatmatched-transition": 187.7333, "dj-extended-club-intro": 115.8667, "dj-mini-mix": 137.8 };

function segments(paces, length) {
  const marks = [{ t: 0, speed: SPEED }, ...paces.filter((p) => p.t > 0 && p.t < length)];
  let segs = marks.map((m, i) => ({ from: m.t, to: i + 1 < marks.length ? marks[i + 1].t : length, speed: m.speed }));
  for (let i = 0; i < segs.length; i++) {
    if (segs[i].speed !== 1) continue;
    if (i > 0) segs[i].from = Math.max(segs[i - 1].from, segs[i].from - 0.8);
    if (i + 1 < segs.length) segs[i].to = Math.min(segs[i + 1].to, segs[i].to + 1.0);
    if (i > 0) segs[i - 1].to = segs[i].from;
    if (i + 1 < segs.length) segs[i + 1].from = segs[i].to;
  }
  segs = segs.filter((s) => s.to - s.from > 0.05);
  const out = [];
  for (const s of segs) {
    const prev = out[out.length - 1];
    if (prev && (prev.speed === s.speed || (s.to - s.from < 0.4 && s.speed !== 1))) prev.to = s.to;
    else out.push({ ...s });
  }
  return out;
}
const result = {};
for (const slug of slugs) {
  const r = JSON.parse(readFileSync(`/tmp/edytlab-native/videos/passed/8-demo-${slug}.json`, "utf8"));
  const v0 = v0s[slug];
  const at = (m) => m.at / 1000 - v0;
  const caps = r.captions.map((c) => ({ ...c, raw: at(c) }));
  const paces = r.paces.map((p) => ({ ...p, t: at(p) }));
  const total = ends[slug] - v0; // audio padded to this length; inter is ~this long
  const segs = segments(paces, total);
  // output start of each seg
  let acc = 0;
  segs.forEach((s) => { s.out0 = acc; acc += (s.to - s.from) / s.speed; });
  const sumOut = acc;
  const map = (t) => {
    if (t <= 0) return 0;
    for (const s of segs) if (t >= s.from && t < s.to) return s.out0 + (t - s.from) / s.speed;
    return sumOut;
  };
  result[slug] = { sumOut, finalDur: finalDur[slug], total, segs: segs.map((s) => ({ ...s })), caps: caps.map((c, i) => ({ text: c.text, raw: +c.raw.toFixed(3), final: +map(c.raw).toFixed(3) })) };
  console.log(slug, "sumOut", sumOut.toFixed(2), "final", finalDur[slug], "segments", segs.length);
  result[slug].caps.forEach((c) => console.log("  ", c.raw.toFixed(2), "->", c.final.toFixed(2), c.text));
}
writeFileSync("captions-final.json", JSON.stringify(result, null, 1));
