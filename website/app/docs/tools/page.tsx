import type { Metadata } from "next";

import { AreaCards, NameIndex } from "@/components/docs/reference-parts";
import { HashRedirect } from "@/components/docs/hash-redirect";
import { DocShell } from "@/components/docs/doc-shell";
import { hashTargets } from "@/lib/docs/reference";
import { TOOLS_ALIASES, TOOLS_BASE, toolCount, toolsReference } from "@/lib/docs/references";
import { pageSocial } from "@/lib/seo-core";

const title = `Audio Tools Reference: all ${toolCount} edytlab agent tools`;
const description = `All ${toolCount} audio-editing tools the edytlab AI agent can call, from cut and normalize to time-stretch and render, by area, with example prompts and what is not implemented yet.`;

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs/tools" },
  ...pageSocial("/docs/tools", `${title} · edytlab`, description),
};

export default function ToolsPage() {
  return (
    <DocShell
      title="Audio Tools Reference"
      description={`All ${toolCount} tools the AI agent can call to edit your audio session.`}
    >
      {/* A link from before this page was split can carry a hash for a
          tool; this sends it to the area that has it. */}
      <HashRedirect base={TOOLS_BASE} targets={hashTargets(toolsReference.areas, TOOLS_ALIASES)} />

      <p>
        Tools are the functions the agent calls on your session. You describe
        what you want and the agent picks the tools. Every tool that changes
        the session creates a new session node (non-destructive); tools that
        only read, like <code>list_session</code> and{" "}
        <code>analyze_track</code>, change nothing.
      </p>

      <h2 id="prompt-tips">Prompt tips</h2>
      <ul>
        <li>
          <strong>Name the track</strong> when you have multiple:{" "}
          <code>normalize track 1</code> not just <code>normalize</code>.
        </li>
        <li>
          <strong>Use minutes:seconds</strong> for time:{" "}
          <code>cut from 1:30 to 2:00</code>.
        </li>
        <li>
          <strong>Chain operations</strong> in one message — the agent plans the
          full sequence before executing.
        </li>
        <li>
          <strong>Correct inline</strong> — if the agent misunderstood, say what
          was wrong: <code>not that track — the second one</code>.
        </li>
      </ul>

      <h2 id="areas">Tools by area</h2>
      <AreaCards reference={toolsReference} />

      <h2 id="all-tools">All tools, A to Z</h2>
      <NameIndex reference={toolsReference} />
    </DocShell>
  );
}
