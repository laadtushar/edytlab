// Turn the demo recordings into web videos: crop to the app's window,
// burn in each step's caption as a lower third, encode for the web, and
// take a poster frame.
//
//   OUT=/tmp/edytlab-native DEST=…/website/public/demos SPEED=1.5 node make-demo-videos.mjs
//
// Reads $OUT/results.json (written by suite.mjs with RECORD=1) for each
// 8-demo-* story's video and captions. A demo that did not pass is skipped:
// the site only shows journeys that worked.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const OUT = process.env.OUT ?? "/tmp/edytlab-native";
const DEST = process.env.DEST;
const SPEED = Number(process.env.SPEED ?? 1);
// The app window on the 1440x900 virtual display (openbox decorations
// included in the grab are cropped away).
const CROP = process.env.CROP ?? "1280:776:80:82";
const FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";
if (!DEST) throw new Error("set DEST to the directory the videos go to");
mkdirSync(DEST, { recursive: true });

// Passing recordings kept by suite.mjs win over the latest run, which may
// have failed and rewritten the story's video.
const latest = JSON.parse(readFileSync(join(OUT, "results.json"), "utf8"));
const kept = (id) => {
  const f = join(OUT, "videos", "passed", `${id}.json`);
  return existsSync(f) ? { ...JSON.parse(readFileSync(f, "utf8")), video: `videos/passed/${id}.mp4` } : null;
};
const results = latest.map((r) => kept(r.id) ?? r);
for (const r of results.filter((r) => r.id.startsWith("8-demo-"))) {
  const slug = r.id.replace(/^8-demo-/, "");
  if (r.status !== "pass") {
    console.log(`skip ${slug}: the story ${r.status === "fail" ? "failed" : r.status}`);
    continue;
  }
  const src = join(OUT, r.video ?? "");
  if (!r.video || !existsSync(src)) {
    console.log(`skip ${slug}: no recording`);
    continue;
  }
  const duration = Number(
    execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", src]).toString().trim(),
  );
  const work = mkdtempSync(join(tmpdir(), "demo-"));
  const caps = r.captions ?? [];
  const draws = caps.map((c, i) => {
    const file = join(work, `cap${i}.txt`);
    writeFileSync(file, c.text);
    const from = c.t.toFixed(2);
    const to = (i + 1 < caps.length ? caps[i + 1].t : duration).toFixed(2);
    return (
      `drawtext=fontfile=${FONT}:textfile=${file}:fontsize=26:fontcolor=white:` +
      `box=1:boxcolor=black@0.62:boxborderw=14:x=(w-text_w)/2:y=h-text_h-36:enable='between(t,${from},${to})'`
    );
  });
  const filters = [`crop=${CROP}`, ...draws];
  if (SPEED !== 1) filters.push(`setpts=PTS/${SPEED}`);
  const mp4 = join(DEST, `${slug}.mp4`);
  execFileSync("ffmpeg", [
    "-y", "-v", "error", "-i", src, "-vf", filters.join(","), "-an",
    "-c:v", "libx264", "-preset", "slow", "-crf", "27", "-pix_fmt", "yuv420p", "-r", "15",
    "-movflags", "+faststart", mp4,
  ]);
  // The poster is the finished arrangement, a few seconds before the end.
  const out = Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", mp4]).toString().trim());
  execFileSync("ffmpeg", ["-y", "-v", "error", "-ss", String(Math.max(0, out - 6)), "-i", mp4, "-frames:v", "1", "-q:v", "4", join(DEST, `${slug}.jpg`)]);
  const mb = (readFileSync(mp4).length / 1e6).toFixed(1);
  console.log(`${slug}: ${out.toFixed(0)} s, ${mb} MB, ${caps.length} captions`);
}
