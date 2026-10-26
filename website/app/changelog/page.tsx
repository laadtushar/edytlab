import type { Metadata } from "next";
import { Fragment } from "react";

import { LegalShell } from "@/components/landing/legal-shell";
import { pageSocial } from "@/lib/seo";

const title = "Changelog: release notes";
const description =
  "What changed in the latest edytlab releases, newest first. Every release and its installers are on GitHub Releases.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/changelog" },
  ...pageSocial("/changelog", `${title} · edytlab`, description),
};

type Bullet = string | { text: string; href: string };

interface Entry {
  version: string;
  date: string;
  summary: string;
  bullets: Bullet[];
}

// Static, updated by hand as releases land: the newest three, at most three
// one-line bullets each. Older releases are on GitHub Releases, not here.
const entries: Entry[] = [
  {
    version: "v0.9.2",
    date: "2026-10",
    summary: "One keychain entry for your keys, and transcription that no longer skips speech.",
    bullets: [
      "Changed: keys and the ChatGPT sign-in share one keychain entry, so macOS asks for your password at most once per launch. Existing keys move over on the first launch.",
      "Fixed: transcription no longer misses stretches of speech, and word times are closer.",
      "Fixed: Retry sends your message once, empty tracks get a lane, and tone and noise work in an empty project.",
    ],
  },
  {
    version: "v0.9.1",
    date: "2026-10",
    summary: "Fixes the first message after signing in with ChatGPT.",
    bullets: [
      "Fixed: the first message after Sign in with ChatGPT failed with “Unsupported content type”.",
      "Changed: a similar OpenAI refusal now shows a plan card with a copyable request ID.",
    ],
  },
  {
    version: "v0.9.0",
    date: "2026-10",
    summary: "Sign in with ChatGPT (preview), and better noise reduction, tempo and key.",
    bullets: [
      { text: "New: Sign in with ChatGPT (preview) uses your Plus or Pro plan instead of an API key.", href: "/docs/user-guide#chatgpt-plan" },
      "Fixed: noise reduction no longer adds a loud click at the start and end of a track.",
      "Fixed: tempo and key detection, now with confidence.",
    ],
  },
];

function Bullets({ items }: { items: Bullet[] }) {
  return (
    <ul>
      {items.map((b, i) => (
        <li key={i}>
          {typeof b === "string" ? (
            b
          ) : (
            <>
              {b.text} <a href={b.href}>Details</a>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}

export default function ChangelogPage() {
  return (
    <LegalShell title="Changelog" path="/changelog">
      <p>
        Newest first. Every earlier release, with installers and tags, is on{" "}
        <a href="https://github.com/laadtushar/edytlab/releases">
          GitHub Releases
        </a>
        .
      </p>
      {/* Flat — heading, summary, list — so the shell's cascade reveals
          each part as it arrives instead of a whole release at once. */}
      {entries.map((e) => (
        <Fragment key={e.version}>
          <h2>
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
          </h2>
          <p>{e.summary}</p>
          <Bullets items={e.bullets} />
        </Fragment>
      ))}
    </LegalShell>
  );
}
