// Turn the demo recordings into web videos: crop to the app's window,
// burn in each step's caption as a lower third, play the waits faster than
// the listening, encode for the web, and take a poster frame.
//
//   OUT=/tmp/edytlab-native DEST=…/website/public/demos node make-demo-videos.mjs
//
// Reads $OUT/results.json (written by suite.mjs with RECORD=1) for each
// 8-demo-* story's video, captions and pace marks. A demo that did not pass
// is skipped: the site only shows journeys that worked.
//
// A recording with sound (RECORD_AUDIO) keeps both streams' wall-clock
// timestamps; they are lined up here. Measured with a pointer move and a
// tone sent at the same instant, PulseAudio's capture still runs about
// 0.65 s ahead of x11grab's frames, so the sound is held back by AV_SKEW.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const OUT = process.env.OUT ?? "/tmp/edytlab-native";
const DEST = process.env.DEST;
// The pace of a recording without pace marks, and before the first one.
const SPEED = Number(process.env.SPEED ?? 1.25);
const AV_SKEW = Number(process.env.AV_SKEW ?? 0.65);
// The app window on the 1440x900 virtual display (openbox decorations
// included in the grab are cropped away).
const CROP = process.env.CROP ?? "1280:776:80:82";
const FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";
const ONLY = process.env.ONLY;
if (!DEST) throw new Error("set DEST to the directory the videos go to");
mkdirSync(DEST, { recursive: true });

// Every output is capped at 4 GB (`-fs`): an encode that runs away fills
// the disk of whatever else shares it, as an unbounded `apad` once did.
const ff = (...args) => execFileSync("ffmpeg", ["-nostdin", "-y", "-v", "error", ...args.slice(0, -1), "-fs", "4G", args[args.length - 1]], { stdio: ["ignore", "ignore", "inherit"] });
const probe = (file, ...args) => execFileSync("ffprobe", ["-v", "error", ...args, "-of", "csv=p=0", file]).toString().trim();
const duration = (file) => Number(probe(file, "-show_entries", "format=duration"));
const start = (file, kind) => {
  const s = probe(file, "-select_streams", kind, "-show_entries", "stream=start_time");
  return s ? Number(s) : null;
};

// Passing recordings kept by suite.mjs win over the latest run, which may
// have failed and rewritten the story's video.
const latest = JSON.parse(readFileSync(join(OUT, "results.json"), "utf8"));
const kept = (id) => {
  const f = join(OUT, "videos", "passed", `${id}.json`);
  if (!existsSync(f)) return null;
  const r = JSON.parse(readFileSync(f, "utf8"));
  return { ...r, video: `videos/passed/${r.video.split("/").pop()}` };
};

/** Pace marks to segments: [{ from, to, speed }] covering 0..length.
 * Listening is widened a little each side, so a play press and the first
 * and last notes are never caught in a fast stretch. */
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
  // Fold slivers, and runs at one pace, into the segment before them.
  const out = [];
  for (const s of segs) {
    const prev = out[out.length - 1];
    if (prev && (prev.speed === s.speed || (s.to - s.from < 0.4 && s.speed !== 1))) prev.to = s.to;
    else out.push({ ...s });
  }
  return out;
}

const results = latest.map((r) => kept(r.id) ?? r);
for (const r of results.filter((r) => r.id.startsWith("8-demo-"))) {
  const slug = r.id.replace(/^8-demo-/, "");
  if (ONLY && !slug.includes(ONLY)) continue;
  if (r.status !== "pass") {
    console.log(`skip ${slug}: the story ${r.status === "fail" ? "failed" : r.status}`);
    continue;
  }
  const src = join(OUT, r.video ?? "");
  if (!r.video || !existsSync(src)) {
    console.log(`skip ${slug}: no recording`);
    continue;
  }
  const work = mkdtempSync(join(tmpdir(), "demo-"));
  const v0 = start(src, "v");
  const a0 = start(src, "a");
  // Wall-clock timestamps (-copyts) are seconds since 1970; an older
  // recording starts near zero and its captions carry `t` only.
  const wallClock = v0 > 1e9;
  const at = (m) => (wallClock && m.at ? m.at / 1000 - v0 : m.t);

  // 1. Full length, real time: cropped, captioned, sound lined up.
  const caps = (r.captions ?? []).map((c) => ({ ...c, t: at(c) }));
  const draws = caps.map((c, i) => {
    const file = join(work, `cap${i}.txt`);
    writeFileSync(file, c.text);
    const to = i + 1 < caps.length ? caps[i + 1].t : 1e6;
    return (
      `drawtext=fontfile=${FONT}:textfile=${file}:fontsize=26:fontcolor=white:` +
      `box=1:boxcolor=black@0.62:boxborderw=14:x=(w-text_w)/2:y=h-text_h-36:enable='between(t,${c.t.toFixed(2)},${to.toFixed(2)})'`
    );
  });
  const video = `[0:v]setpts=PTS-STARTPTS,crop=${CROP}${draws.length ? "," + draws.join(",") : ""}[v]`;
  const inter = join(work, "inter.mkv");
  if (a0 !== null) {
    const offset = (wallClock ? a0 - v0 : 0) + AV_SKEW;
    const shift = offset >= 0 ? `adelay=${Math.round(offset * 1000)}:all=1` : `atrim=start=${(-offset).toFixed(3)},asetpts=PTS-STARTPTS`;
    // The sound is padded to the picture's length and cut there. `apad`
    // alone is endless, and `-shortest` does not end it inside a filter
    // graph: it once wrote silence until the disk was full. With
    // wall-clock timestamps ffprobe's format duration is the *end* time
    // (seconds since 1970), so the length is that minus the start.
    const length = wallClock ? duration(src) - Math.min(v0, a0) : duration(src);
    if (!(length > 0 && length < 4 * 3600)) throw new Error(`${slug}: implausible recording length ${length} s`);
    const audio = `[0:a]asetpts=PTS-STARTPTS,${shift},apad=whole_dur=${length.toFixed(3)},atrim=end=${length.toFixed(3)}[a]`;
    ff("-i", src, "-filter_complex", `${video};${audio}`, "-map", "[v]", "-map", "[a]", "-t", (length + 1).toFixed(3),
      "-c:v", "libx264", "-preset", "ultrafast", "-crf", "16", "-pix_fmt", "yuv420p", "-c:a", "pcm_s16le", inter);
  } else {
    ff("-i", src, "-filter_complex", video, "-map", "[v]", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "16", "-pix_fmt", "yuv420p", inter);
  }

  // 2. Each stretch at its pace.
  const paces = (r.paces ?? []).map((p) => ({ ...p, t: at(p) }));
  const total = duration(inter);
  const segs = segments(paces, total);
  const list = [];
  segs.forEach((s, i) => {
    const seg = join(work, `seg${String(i).padStart(3, "0")}.mkv`);
    const len = s.to - s.from;
    const outLen = len / s.speed;
    // `-t` before `-i` bounds what is *read*. After it, it bounds what is
    // written, and a stretch played 3.5x fast then read 3.5x too much
    // source, repeating the stretches after it.
    const args = ["-ss", s.from.toFixed(3), "-t", len.toFixed(3), "-i", inter, "-vf", `setpts=(PTS-STARTPTS)/${s.speed}`, "-r", "15"];
    if (a0 !== null) {
      // atempo keeps pitch; the fades stop a click at every join.
      const fade = Math.min(0.03, outLen / 4);
      args.push("-af", `asetpts=PTS-STARTPTS${s.speed !== 1 ? `,atempo=${s.speed}` : ""},afade=t=in:d=${fade},afade=t=out:st=${Math.max(0, outLen - fade).toFixed(3)}:d=${fade}`, "-c:a", "pcm_s16le");
    }
    args.push("-c:v", "libx264", "-preset", "ultrafast", "-crf", "16", "-pix_fmt", "yuv420p", seg);
    ff(...args);
    list.push(`file '${seg}'`);
  });
  writeFileSync(join(work, "list.txt"), list.join("\n"));

  // 3. One web encode of the whole.
  const mp4 = join(DEST, `${slug}.mp4`);
  ff("-f", "concat", "-safe", "0", "-i", join(work, "list.txt"),
    "-c:v", "libx264", "-preset", "slow", "-crf", "27", "-pix_fmt", "yuv420p", "-r", "15",
    ...(a0 !== null ? ["-c:a", "aac", "-b:a", "128k", "-ac", "2"] : ["-an"]),
    "-movflags", "+faststart", mp4);
  // The poster is the finished arrangement, a few seconds before the end.
  const out = duration(mp4);
  ff("-ss", String(Math.max(0, out - 6)), "-i", mp4, "-frames:v", "1", "-q:v", "4", join(DEST, `${slug}.jpg`));
  const mb = (readFileSync(mp4).length / 1e6).toFixed(1);
  const real = segs.filter((s) => s.speed === 1).reduce((a, s) => a + s.to - s.from, 0);
  const whole = Math.round(out);
  console.log(
    `${slug}: ${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")} (${out.toFixed(1)} s from ${total.toFixed(0)} s; ` +
      `${real.toFixed(0)} s in real time), ${mb} MB, ${caps.length} captions, ${a0 !== null ? "with sound" : "silent"}`,
  );
  rmSync(work, { recursive: true, force: true });
}
