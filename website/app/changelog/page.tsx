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
      "OpenAI provider added alongside Anthropic and OpenRouter; model picker is catalogue-backed.",
      "Per-provider keychain slots; legacy unsuffixed Anthropic key still read for back-compat.",
      "Release pipeline: pre-create release job avoids duplicate-tag race in matrix builds.",
      "Project-level CLAUDE.md added for repository workflow rules.",
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
            {e.bullets.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        </section>
      ))}
    </LegalShell>
  );
}
