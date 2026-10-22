// Turn what the native suite left in $OUT (videos/, results.json, promo/, shots/) into the promo's footage folder.
//
//   OUT=/tmp/edytlab-native DEST=marketing/promo-v0.7.0/footage node make-footage.mjs [story-id-substring ...]
//
// The suite films the real app's page at 3200x2000 (a 1600x1000 page at the display's scale of 2) with ffmpeg's x11grab
// at 15 fps, with PulseAudio's capture of the app's sound beside it. This script cuts each passing story's recording
// into a clip:
//
//   <clip>.mp4          the picture, H.264 High, yuv420p, 15 fps constant, from the moment the story marked "start"
//                       (or the first frame), with no audio track
//   <clip>.wav          S1 and S7 only: the app's own sound, mono 44.1 kHz 16-bit, lined up with the picture, exactly as
//                       long as the clip, peak -0.8 dBFS (the edit sets loudness and true peak)
//   <clip>.marks.json   named moments, in seconds into the clip
//   <clip>.notes.json   what was recorded: the model, the request, the reply, the tool calls, the capture's own log
//
// Sound and picture come from two clocks that do not agree (x11grab's frames run about a second behind PulseAudio's
// capture on this machine, and by how much changes from one recording to the next). Each recording with sound
// therefore opens on a "beacon": a flash of the page and a 1 kHz tone sent in the same breath (promo.mjs, `beacon`).
// The flash frame and the tone's first sample are found here, their gap is measured, and the sound is moved by it. The
// beacon is before the clip's start, so it is not in the footage.
//
// A recording that fell behind (ffmpeg could not keep the pace, so it ends before the story did, or repeats too many
// frames) is refused: such a clip is not used, it is recorded again.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const OUT = process.env.OUT ?? "/tmp/edytlab-native";
const DEST = process.env.DEST;
if (!DEST) throw new Error("set DEST to the footage folder");
mkdirSync(DEST, { recursive: true });
const only = process.argv.slice(2);

const CLIPS = {
  "9-promo-s1-describe-it-happens": { name: "s1-describe-it-happens", audio: true, assistant: true },
  "9-promo-s2-faders-stay-put": { name: "s2-faders-stay-put" },
  "9-promo-s3-history-graph": { name: "s3-history-graph", assistant: true },
  "9-promo-s4-200-tracks": { name: "s4-200-tracks", assistant: true },
  "9-promo-s5-runtime-bundled": { name: "s5-runtime-bundled" },
  "9-promo-s6-skills-note": { name: "s6-skills-note" },
  "9-promo-s7-mix-stays-playable": { name: "s7-mix-stays-playable", audio: true },
  "9-promo-s8a-broll-timeline": { name: "s8a-broll-timeline" },
  "9-promo-s8b-broll-graph": { name: "s8b-broll-graph" },
};
const FPS = 15;
const RATE = 44100;

const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "pipe"], ...opts });
const probe = (file, stream, entries) => {
  const out = run("ffprobe", ["-v", "error", ...(stream ? ["-select_streams", stream] : []), "-show_entries", entries, "-of", "csv=p=0", file]).toString().trim();
  return out.replace(/,/g, "");
};

/** Every video frame's presentation time (wall clock seconds: the recording keeps its timestamps), in order. */
function framePts(file) {
  return run("ffprobe", ["-v", "error", "-select_streams", "v:0", "-show_entries", "frame=pts_time", "-of", "csv=p=0", file])
    .toString()
    .split("\n")
    .map((l) => Number(l.replace(/,/g, "")))
    .filter((n) => Number.isFinite(n) && n > 0);
}

/** The mean brightness of every frame at 32x20, one per frame (no repeats), as a Float64Array. */
function brightness(file) {
  const raw = run("ffmpeg", ["-v", "error", "-i", file, "-an", "-fps_mode", "passthrough", "-vf", "scale=32:20,format=gray", "-f", "rawvideo", "-"]);
  const n = raw.length / 640;
  const out = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let k = 0; k < 640; k++) s += raw[i * 640 + k];
    out[i] = s / 640;
  }
  return out;
}

/** The recording's sound as mono floats at RATE, with the wall-clock second its first sample is at. */
function readAudio(file) {
  const raw = run("ffmpeg", ["-v", "error", "-i", file, "-vn", "-ac", "1", "-ar", String(RATE), "-f", "f32le", "-"]);
  const a = new Float32Array(raw.buffer, raw.byteOffset, Math.floor(raw.length / 4));
  return { a: Float32Array.from(a), t0: Number(probe(file, "a:0", "stream=start_time")) };
}

const wavFile = (samples) => {
  const data = Buffer.alloc(samples.length * 2);
  for (let i = 0; i < samples.length; i++) data.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(samples[i] * 32767))), i * 2);
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write("WAVEfmt ", 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(RATE, 24);
  h.writeUInt32LE(RATE * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36);
  h.writeUInt32LE(data.length, 40);
  return Buffer.concat([h, data]);
};

/** The ffmpeg log's last progress line: frames, repeated and dropped, speed. */
function captureLog(file) {
  const log = `${file}.log`;
  if (!existsSync(log)) return null;
  const lines = readFileSync(log, "utf8").split(/[\r\n]/).filter((l) => /^frame=/.test(l));
  const last = lines.at(-1) ?? "";
  const num = (k) => Number(new RegExp(`${k}=\\s*([\\d.]+)`).exec(last)?.[1] ?? NaN);
  return { frames: num("frame"), dup: num("dup"), drop: num("drop"), speed: num("speed") };
}

const results = JSON.parse(readFileSync(join(OUT, "results.json"), "utf8"));
for (const [id, spec] of Object.entries(CLIPS)) {
  if (only.length && !only.some((o) => id.includes(o))) continue;
  const kept = join(OUT, "videos", "passed", `${id}.json`);
  const r = existsSync(kept) ? JSON.parse(readFileSync(kept, "utf8")) : results.find((x) => x.id === id);
  if (!r) {
    console.log(`skip ${spec.name}: not run`);
    continue;
  }
  if (r.status !== "pass") {
    console.log(`skip ${spec.name}: the story ${r.status}`);
    continue;
  }
  const rec = existsSync(kept) ? join(OUT, "videos", "passed", r.video.split("/").pop()) : join(OUT, r.video);
  const recLog = captureLog(join(OUT, r.video));
  const pts = framePts(rec);
  const v0 = pts[0];
  const vEnd = pts.at(-1);
  const at = (c) => c.at / 1000;
  const marks = r.captions ?? [];
  const startMark = marks.find((m) => m.text === "start");
  const start = startMark ? at(startMark) : v0;
  const last = marks.length ? at(marks.at(-1)) : vEnd;
  // The picture must reach the story's last mark; an ffmpeg that fell behind ends early.
  if (vEnd < last + 0.5) throw new Error(`${spec.name}: the recording ends ${(last - vEnd).toFixed(1)} s before the story's last moment: ffmpeg fell behind (${JSON.stringify(recLog)}); record it again`);
  if (recLog && (recLog.speed < 0.85 || recLog.drop > 0.15 * recLog.frames)) throw new Error(`${spec.name}: the capture fell behind (${JSON.stringify(recLog)}); record it again`);
  const length = Math.round((vEnd - start) * 100) / 100;

  // Sound: line it up with the picture by the beacon, cut it to the clip.
  let wav = null;
  let sync = null;
  let spans = null;
  if (spec.audio) {
    const beacon = marks.find((m) => m.text === "beacon");
    if (!beacon) throw new Error(`${spec.name}: no beacon mark`);
    const b = brightness(rec);
    const jump = [...b.keys()].find((i) => i > 0 && at(beacon) - 0.3 < pts[i] && pts[i] < at(beacon) + 1.5 && b[i] - b[i - 1] > 40);
    if (jump === undefined) throw new Error(`${spec.name}: the beacon's flash is not in the picture`);
    const { a, t0 } = readAudio(rec);
    const from = Math.max(0, Math.round((at(beacon) - 0.5 - t0) * RATE));
    let onset = -1;
    for (let i = from; i < Math.min(a.length, from + 4 * RATE); i++) {
      if (Math.abs(a[i]) > 0.1) {
        onset = i;
        break;
      }
    }
    if (onset < 0) throw new Error(`${spec.name}: the beacon's tone is not in the sound`);
    const flashAt = pts[jump];
    const toneAt = t0 + onset / RATE;
    const shift = flashAt - toneAt; // seconds the sound is moved later (negative: earlier)
    sync = { flashAt: +flashAt.toFixed(3), toneAt: +toneAt.toFixed(3), soundIsAheadBy: +shift.toFixed(3) };
    // The clip's sample i is the recording's sample at wall time start + i/RATE - shift... i.e. the sound as it is
    // when it is moved by `shift`.
    const n = Math.round(length * RATE);
    const first = Math.round((start - shift - t0) * RATE);
    const out = new Float32Array(n);
    let peak = 0;
    for (let i = 0; i < n; i++) {
      const v = a[first + i] ?? 0;
      out[i] = v;
      peak = Math.max(peak, Math.abs(v));
    }
    const k = peak > 0 ? 0.912 / peak : 1;
    for (let i = 0; i < n; i++) out[i] *= k;
    wav = `${spec.name}.wav`;
    writeFileSync(join(DEST, wav), wavFile(out));
    // Where the sound plays: stretches above -60 dBFS RMS in 50 ms windows, merged across gaps under 0.4 s.
    const win = Math.round(RATE * 0.05);
    const on = [];
    for (let i = 0; i + win <= n; i += win) {
      let s = 0;
      for (let j = 0; j < win; j++) s += out[i + j] ** 2;
      on.push(Math.sqrt(s / win) > 0.001);
    }
    spans = [];
    on.forEach((v, i) => {
      if (!v) return;
      const t = i * 0.05;
      const prev = spans.at(-1);
      if (prev && t - prev.footageEnd < 0.4) prev.footageEnd = t + 0.05;
      else spans.push({ footageStart: t, footageEnd: t + 0.05 });
    });
    spans = spans.filter((s) => s.footageEnd - s.footageStart > 0.5).map((s) => ({ footageStart: +s.footageStart.toFixed(2), footageEnd: +s.footageEnd.toFixed(2) }));
  }

  // The picture: from the start mark, at a constant 15 fps.
  const mp4 = join(DEST, `${spec.name}.mp4`);
  if (!existsSync(mp4) || process.env.REENCODE) run("ffmpeg", ["-nostdin", "-y", "-v", "error", "-ss", (start - v0).toFixed(3), "-i", rec, "-t", length.toFixed(3), "-an", "-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p", "-profile:v", "high", "-r", String(FPS), "-fps_mode", "cfr", "-movflags", "+faststart", mp4]);
  const secs = Number(probe(mp4, "v:0", "format=duration"));
  const frames = Number(probe(mp4, "v:0", "stream=nb_frames"));

  const clipMarks = marks.filter((m) => at(m) >= start - 0.001).map((m) => ({ label: m.text, t: +(at(m) - start).toFixed(2) }));
  writeFileSync(join(DEST, `${spec.name}.marks.json`), `${JSON.stringify(clipMarks, null, 2)}\n`);
  const storyNotes = existsSync(join(OUT, "promo", `${id.split("-").slice(2, 3)[0]}.notes.json`)) ? JSON.parse(readFileSync(join(OUT, "promo", `${id.split("-").slice(2, 3)[0]}.notes.json`), "utf8")) : {};
  const toolCallsFile = join(OUT, "shots", `${id}-tool-calls.json`);
  const toolCalls = existsSync(toolCallsFile) ? JSON.parse(readFileSync(toolCallsFile, "utf8")) : null;
  writeFileSync(
    join(DEST, `${spec.name}.notes.json`),
    `${JSON.stringify({ ...storyNotes, ...(spec.assistant ? {} : { live: true, model: null, effort: null, assistant: "none in this shot" }), file: `${spec.name}.mp4`, seconds: +secs.toFixed(2), frames, fps: FPS, size: "3200x2000", marks: clipMarks, wav, spans, sync, capture: recLog, toolCalls }, null, 2)}\n`,
  );
  console.log(`${spec.name}: ${secs.toFixed(1)} s, ${frames} frames${wav ? `, sound ran ${sync.soundIsAheadBy} s ahead of the picture, moved` : ""}, capture ${recLog ? `${recLog.frames} frames, ${recLog.drop} dropped, ${recLog.speed}x` : "(no log)"}`);
}
