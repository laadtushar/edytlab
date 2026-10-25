/**
 * The site shows original videos only.
 *
 * An original video is a recording of the real app in which the
 * assistant's replies and tool calls came from a real model at recording
 * time. A scripted one (fake LLM, canned replies) is kept under
 * `marketing/archive/**` in a folder whose name says so, such as
 * `promo-v0.7.0/2026-10-22-scripted/`, and must never be served again.
 *
 * Two checks hold that to the files:
 *
 * - no `.mp4` the site serves (`public/videos/`, `public/demos/`) is
 *   byte-identical to an `.mp4` in an archived scripted folder;
 * - the v0.7.0 footage (`marketing/promo-v0.7.0/`, which the site's
 *   v0.7.0 videos were cut from until v0.9.1) is live: every clip's
 *   notes say `live`, and each clip with an assistant
 *   in it carries the tool calls the metering proxy recorded from
 *   api.anthropic.com (`toolu_` ids), not a scripted list.
 *
 * The DJ demos (`public/demos/`) and the launch video and reels cut from
 * them come from `apps/desktop/e2e-native/stories/demos.mjs`, which is
 * registered only when `ANTHROPIC_E2E_KEY` is set, so it cannot run
 * against `fake-llm.mjs`.
 */

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
const SITE_DIRS = ["website/public/videos", "website/public/demos"];
const ARCHIVE = join(REPO, "marketing/archive");
const FOOTAGE = join(REPO, "marketing/promo-v0.7.0/footage");

function mp4sIn(dir: string): string[] {
  if (!existsSync(dir)) return [];
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...mp4sIn(path));
    else if (name.toLowerCase().endsWith(".mp4")) out.push(path);
  }
  return out;
}

/** Every `.mp4` under an archive folder whose name contains "scripted". */
function scriptedArchive(dir: string, inside = false): string[] {
  if (!existsSync(dir)) return [];
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (!statSync(path).isDirectory()) {
      if (inside && name.toLowerCase().endsWith(".mp4")) out.push(path);
      continue;
    }
    out.push(...scriptedArchive(path, inside || /scripted/i.test(name)));
  }
  return out;
}

const sha = (path: string) => createHash("sha256").update(readFileSync(path)).digest("hex");

describe("the site serves no scripted video", () => {
  const scripted = scriptedArchive(ARCHIVE);
  const served = SITE_DIRS.flatMap((d) => mp4sIn(join(REPO, d)));

  it("finds the archived scripted set and the served videos", () => {
    expect(scripted.length).toBeGreaterThan(0);
    expect(served.length).toBeGreaterThan(0);
  });

  it("serves no file that is byte-identical to an archived scripted one", () => {
    const bySize = new Map<number, string[]>();
    for (const p of scripted) {
      const size = statSync(p).size;
      bySize.set(size, [...(bySize.get(size) ?? []), p]);
    }
    for (const p of served) {
      const same = bySize.get(statSync(p).size);
      if (!same) continue;
      const mine = sha(p);
      for (const other of same) {
        expect(mine, `${p} is the archived scripted ${other}`).not.toBe(sha(other));
      }
    }
  });
});

describe("the v0.7.0 footage is live", () => {
  const notes = readdirSync(FOOTAGE)
    .filter((f) => f.endsWith(".notes.json"))
    .map((f) => [f, JSON.parse(readFileSync(join(FOOTAGE, f), "utf8"))] as const);

  it("has the nine clips' notes", () => {
    expect(notes.length).toBe(9);
  });

  it.each(notes)("%s was filmed live", (_file, n) => {
    expect(n.live).toBe(true);
  });

  it.each(notes.filter(([, n]) => n.model))("%s: the assistant is a real model, and its tool calls came through the proxy", (_file, n) => {
    expect(n.model).toMatch(/^claude-/);
    expect(n.toolCalls?.source).toBe("claude-proxy");
    const calls = (n.toolCalls?.turns ?? []).flatMap((t: { calls: { id: string }[] }) => t.calls);
    expect(calls.length).toBeGreaterThan(0);
    for (const c of calls) expect(c.id, "an id the Anthropic API issued").toMatch(/^toolu_/);
  });
});
