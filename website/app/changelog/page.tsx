import type { Metadata } from "next";

import { LegalShell } from "@/components/landing/legal-shell";

export const metadata: Metadata = {
  title: "Changelog",
  description: "Recent updates to edytlab.",
};

interface Entry {
  version: string;
  date: string;
  bullets: string[];
}

// Static stub. Updated manually as releases land. The full git history lives on
// GitHub Releases — link below.
const entries: Entry[] = [
  {
    version: "v0.1.0-dev (in progress)",
    date: "2026-05",
    bullets: [
      "Extensibility: MCP servers now start automatically at launch, so a registered server contributes its tools without a manual restart. Requests are deadline-bounded and server stderr is surfaced in error messages.",
      "Agent profiles that pin a model on another provider now authenticate against that provider instead of reusing the active provider's key.",
      "MCP server editor: the args, env, and headers fields accept multi-line input correctly.",
      "Plugins: install skills and agent profiles from a GitHub repo (github:org/repo) or a local path.",
      "Eight audio skills ship pre-installed — podcast cleanup, vocal chain, loudness mastering, noise reduction, dialog enhancement, music mixing, silence cleanup, and an export guide.",
      "Agent profiles: save a model override, a tool whitelist, and a system-prompt addition, then switch between them.",
      "Skills: markdown files with always/keyword/regex triggers, editable in Settings.",
      "Memory: global and per-project notes spliced into the system prompt.",
      "Microphone recording via CPAL, captured straight to WAV.",
      "Spectrogram view toggle in the timeline.",
      "Plan steps can be edited inline before you approve them.",
      "Individual tools can be disabled from the capabilities menu.",
      "69 built-in audio tools, including reverb, echo, noise gate, limiter, de-esser, tone and noise generators, Audacity-format label import/export, and per-track export.",
      "OpenAI provider added alongside Anthropic and OpenRouter; model picker is catalogue-backed.",
      "Per-provider keychain slots; legacy unsuffixed Anthropic key still read for back-compat.",
    ],
  },
];

export default function ChangelogPage() {
  return (
    <LegalShell title="Changelog">
      <p>
        Recent landed work. The authoritative list of binaries and tags lives
        on{" "}
        <a href="https://github.com/laadtushar/edytlab/releases">
          GitHub Releases
        </a>
        .
      </p>
      {entries.map((e) => (
        <section key={e.version} className="mt-10">
          <h2>
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
          </h2>
          <ul>
            {e.bullets.map((b, i) => (
              <li key={i}>{b}</li>
            ))}
          </ul>
        </section>
      ))}
    </LegalShell>
  );
}
