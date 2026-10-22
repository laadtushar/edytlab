/**
 * The skills the release bundles, answered the way the Rust commands answer
 * them (`list_skills`, `read_skill` in `apps/desktop/src-tauri/src/commands.rs`).
 *
 * The files are `src-tauri/resources/skills/*.md`, the ones `tauri.conf.json`
 * bundles, so the list here is the list a first launch installs. The
 * frontmatter is read as `parse_skill_file` reads it and a summary's trigger
 * is written as `trigger_summary` writes it.
 */

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import type { SkillContent, SkillSummary } from "../../../../apps/desktop/src/lib/tauri-bridge";

const DIR = join(dirname(fileURLToPath(import.meta.url)), "../../../../apps/desktop/src-tauri/resources/skills");

function parse(name: string, raw: string): SkillContent {
  const text = raw.replace(/\r\n/g, "\n");
  const rest = text.startsWith("---\n") ? text.slice(4) : text;
  const end = rest.indexOf("\n---");
  const fm = rest.slice(0, end);
  const body = rest.slice(end + 4).replace(/^\n+/, "");
  const out: SkillContent = { name, description: "", trigger: "always", keywords: [], pattern: "", enabled: true, body };
  for (const line of fm.split("\n")) {
    const l = line.trim();
    const i = l.indexOf(":");
    if (!l || l.startsWith("#") || i < 0) continue;
    const k = l.slice(0, i).trim();
    const v = l.slice(i + 1).trim().replace(/^"(.*)"$/, "$1");
    if (k === "description") out.description = v;
    else if (k === "trigger") out.trigger = v as SkillContent["trigger"];
    else if (k === "pattern") out.pattern = v;
    else if (k === "enabled") out.enabled = v === "true";
    else if (k === "keywords" && v.startsWith("[") && v.endsWith("]")) {
      out.keywords = v.slice(1, -1).split(",").map((s) => s.trim()).filter(Boolean);
    }
  }
  return out;
}

/** Every bundled skill, in name order (`skills.sort_by(|a, b| a.name.cmp(&b.name))`). */
export function bundledSkills(): SkillContent[] {
  return readdirSync(DIR)
    .filter((f) => f.endsWith(".md"))
    .map((f) => parse(f.replace(/\.md$/, ""), readFileSync(join(DIR, f), "utf8")))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** `trigger_summary`: at most four keywords, then an ellipsis. */
function triggerSummary(s: SkillContent): string {
  if (s.trigger === "always") return "always";
  if (s.trigger === "regex") return "regex";
  const preview = s.keywords.slice(0, 4).join(", ");
  return `keywords: ${preview}${s.keywords.length > 4 ? ", …" : ""}`;
}

export function skillSummaries(): SkillSummary[] {
  return bundledSkills().map((s) => ({
    name: s.name,
    description: s.description,
    trigger: triggerSummary(s),
    enabled: s.enabled,
  }));
}
