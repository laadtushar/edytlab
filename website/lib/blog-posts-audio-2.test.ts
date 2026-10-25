/**
 * The five audio how-to posts of 2026-10-24, held to the rules their
 * file states (`blog-posts-audio-2/index.ts`), through the checks the first
 * four share (`howto-checks.ts`), and to what is particular to these five:
 * the tool names and parameters they quote exist in the generated tools
 * reference, and the places where the tool does less than its name or a
 * reader would assume are said where the tool is described.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { audioHowtoPosts2 } from "./blog-posts-audio-2";
import { COMMIT, READ, UPDATED } from "./blog-posts-audio-2/shared";
import { allText, describeHowtoPosts } from "./howto-checks";
import { stripInline } from "./inline";

const ROOT = fileURLToPath(new URL("../", import.meta.url));

/** The slugs, and the question each post has to answer in its first paragraph. */
const EXPECTED: [string, RegExp][] = [
  ["how-to-find-the-bpm-and-key-of-a-song", /find a song's BPM and key[\s\S]*analyze_track/],
  ["remove-background-noise-from-audio", /remove background noise from audio[\s\S]*noise-only[\s\S]*noise_reduction/],
  ["true-peak-vs-sample-peak", /sample peak[\s\S]*true peak[\s\S]*-1 dBTP[\s\S]*sample-peak ceiling, not an inter-sample true-peak one/],
  ["crossfade-two-songs", /crossfade two songs[\s\S]*equal-power[\s\S]*linear/],
  ["edit-audio-with-a-local-model-ollama", /edit audio with a local model[\s\S]*Ollama[\s\S]*no API key[\s\S]*audio/],
];

describeHowtoPosts({
  name: "the five audio how-to posts of 2026-10-24",
  batch: audioHowtoPosts2,
  expected: EXPECTED,
  read: READ,
  commit: COMMIT,
  reread: UPDATED,
});

const post = (slug: string) => audioHowtoPosts2.find((p) => p.slug === slug)!;
const text = (slug: string) => allText(post(slug)).join("\n");

describe("tool names and parameters are the ones the tools reference generates", () => {
  const reference = readFileSync(join(ROOT, "../docs/tools-reference.md"), "utf8");

  /** The tools each post names in code type, and the parameters it quotes for them. */
  const QUOTED: Record<string, { tools: string[]; params: Record<string, string[]> }> = {
    "how-to-find-the-bpm-and-key-of-a-song": {
      tools: ["analyze_track", "time_stretch"],
      params: { analyze_track: ["track", "path"] },
    },
    "remove-background-noise-from-audio": {
      tools: ["noise_reduction", "notch_filter", "click_removal", "noise_gate"],
      params: { noise_reduction: ["noise_duration_sec", "strength", "floor", "track"] },
    },
    "true-peak-vs-sample-peak": {
      tools: ["limiter", "normalize", "normalize_loudness", "render_final", "analyze_track", "add_effect"],
      params: { limiter: ["ceiling_db", "release_ms"], normalize: ["target_dbfs"], normalize_loudness: ["true_peak_ceiling_db"] },
    },
    "crossfade-two-songs": {
      tools: ["time_stretch", "time_shift", "fade", "set_clip_envelope", "analyze_track", "mix_to_new_track", "align_to_beat"],
      params: {},
    },
    "edit-audio-with-a-local-model-ollama": {
      tools: [
        "analyze_track", "compressor", "cut_range", "eq", "fade", "gain", "high_pass_filter", "mute_track", "noise_reduction",
        "normalize", "normalize_loudness", "render_final", "revert_to", "set_pan", "set_track_gain", "list_session", "time_stretch",
      ],
      params: {},
    },
  };

  /** One tool's section of the reference. */
  const section = (tool: string): string => {
    const start = reference.indexOf(`## \`${tool}\`\n`);
    expect(start, `no section for ${tool} in docs/tools-reference.md`).toBeGreaterThanOrEqual(0);
    const end = reference.indexOf("\n## ", start + 1);
    return reference.slice(start, end < 0 ? undefined : end);
  };

  it.each(Object.entries(QUOTED))("%s", (slug, { tools, params }) => {
    const body = text(slug);
    for (const t of tools) {
      expect(reference, `${t} is not a tool`).toContain(`## \`${t}\`\n`);
      expect(body, `${slug} does not name ${t}`).toContain(`\`${t}\``);
    }
    for (const [tool, names] of Object.entries(params)) {
      const sec = section(tool);
      for (const n of names) expect(sec, `${tool} has no parameter ${n}`).toContain(`\`${n}\``);
    }
  });

  it("the CLAUDE.md-relevant defaults are the tool's", () => {
    // The noise post states 0.85 and 0.05; the tool's own description does.
    const noise = section("noise_reduction");
    expect(noise).toContain("default 0.85");
    expect(noise).toContain("default 0.05");
    expect(text("remove-background-noise-from-audio")).toMatch(/defaults to 0\.85[\s\S]*0\.05/);
  });
});

describe("what each post says plainly about the tool", () => {
  it("BPM and key: names the placeholders and the whole-file answer", () => {
    const t = text("how-to-find-the-bpm-and-key-of-a-song");
    expect(t).toMatch(/\bsections\b[\s\S]*placeholder/);
    expect(t).toMatch(/downbeats[\s\S]*every fourth beat[\s\S]*assumes 4\/4/);
    expect(t).toMatch(/one tempo, one key/);
    // The wrong answers v0.8.1 gave are reported as such, not left out, and the post says
    // v0.9.0 fixed them (#696) and what the fix returns.
    expect(t).toMatch(/read this track as 179\.0/);
    expect(t).toMatch(/F major/);
    expect(t).toMatch(/v0\.8\.1[\s\S]*v0\.9\.0 reworked the tempo, beat grid and key detection/);
    expect(t).toMatch(/90\.0 BPM/);
    expect(t).toMatch(/A# minor/);
    expect(t).toMatch(/bpm_confidence[\s\S]*bpm_alternatives/);
    expect(t).toMatch(/key_runner_up/);
    // What v0.9.0 does not do is still said: no real sections, no detected downbeats.
    expect(t).toMatch(/assumed_4_4/);
    expect(t).toMatch(/whole_track/);
    expect(post("how-to-find-the-bpm-and-key-of-a-song").dateModified).toBe(UPDATED);
  });

  it("noise reduction: says the profile is the start of the track, and that the click of issue 692 was fixed in v0.9.0", () => {
    const t = text("remove-background-noise-from-audio");
    expect(t).toMatch(/profile is always the start of the track/);
    expect(t).toMatch(/issues\/692/);
    expect(t).toMatch(/fixed in v0\.9\.0/);
    // It no longer calls the defect open or tells the reader to work around it.
    expect(t).not.toMatch(/is not fixed in this post's version|one known defect|Until it is, trimming/);
    expect(post("remove-background-noise-from-audio").dateModified).toBe(UPDATED);
    expect(t).toMatch(/musical noise/i);
    expect(t).toMatch(/no smoothing of either kind/);
  });

  it("true peak: says every edytlab ceiling is a sample peak, and how to measure the real one", () => {
    const t = text("true-peak-vs-sample-peak");
    expect(t).toMatch(/nothing in edytlab measures or limits true peak/);
    expect(t).toMatch(/analyze_track[\s\S]*reports no peak at all/);
    expect(t).toMatch(/ebur128=peak=sample\+true/);
    expect(post("true-peak-vs-sample-peak").body.some((b) => b.type === "code" && b.text.includes("ebur128=peak=sample+true"))).toBe(true);
  });

  it("crossfade: gives both curves' levels and says what was not measured", () => {
    const t = text("crossfade-two-songs");
    expect(t).toMatch(/-3\.1 dB at the middle/);
    expect(t).toMatch(/\+3\.0 dB at the middle/);
    expect(t).toMatch(/Fewer points give a rougher curve, which was not measured/);
  });

  it("Ollama: says the chat is local only while Ollama is, that cloud models are not, and that no model was run", () => {
    const t = text("edit-audio-with-a-local-model-ollama");
    expect(t).toContain("ToolSet::Slim");
    expect(t).toMatch(/cloud-hosted models/);
    expect(t).toMatch(/If you change it, the chat goes to that machine/);
    expect(t).toMatch(/no Ollama model was run for this post/);
    expect(t).toMatch(/assumed three bytes per token[\s\S]*not a measurement/);
    // The demo videos were not recorded on Ollama, and the post says so.
    expect(t).toMatch(/not with Ollama/);
  });
});

describe("the posts claim nothing about stem separation, and say transcription runs (v0.8.0)", () => {
  it("never mention stems at all", () => {
    for (const p of audioHowtoPosts2) expect(allText(p).join("\n"), p.slug).not.toMatch(/\bstems?\b|demucs/i);
  });
});

describe("first paragraphs read as an answer without markup", () => {
  it.each(audioHowtoPosts2.map((p) => [p.slug, p] as const))("%s", (_slug, p) => {
    const first = p.body[0];
    expect(first.type).toBe("p");
    const plain = stripInline(first.type === "p" ? first.text : "");
    expect(plain).not.toMatch(/[`*\[\]]/);
    expect(plain.length).toBeGreaterThan(300);
  });
});
