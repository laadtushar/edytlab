import type { Metadata } from "next";

import { AreaCards } from "@/components/docs/reference-parts";
import { HashRedirect } from "@/components/docs/hash-redirect";
import { DocShell } from "@/components/docs/doc-shell";
import { hashTargets } from "@/lib/docs/reference";
import { USER_GUIDE_ALIASES, USER_GUIDE_BASE, userGuideReference } from "@/lib/docs/references";
import { pageSocial } from "@/lib/seo-core";

const title = "User Guide: sessions, tracks, chat and export";
const description =
  "The user guide for edytlab: sessions and the edit graph, tracks, chat, A/B compare, export, memory and skills, and what is not available yet.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs/user-guide" },
  ...pageSocial("/docs/user-guide", `${title} · edytlab`, description),
};

export default function UserGuidePage() {
  return (
    <DocShell title="User Guide" description="Every feature in edytlab.">
      {/* A link from before this page was split can carry a hash for a
          heading (`#transcription`, `#chatgpt-plan`); this sends it to the
          page that has it. A hash is never sent to the server, so only the
          browser can. */}
      <HashRedirect
        base={USER_GUIDE_BASE}
        targets={hashTargets(userGuideReference.areas, USER_GUIDE_ALIASES)}
      />

      <p>
        The guide is one page per part of the app. Pick the part you want, or
        read them in order: each page ends with a link to the next.
      </p>

      <h2 id="pages">Pages</h2>
      <AreaCards reference={userGuideReference} />
    </DocShell>
  );
}
