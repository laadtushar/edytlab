import type { Metadata } from "next";

import { DocShell } from "@/components/docs/doc-shell";
import { HashRedirect } from "@/components/docs/hash-redirect";
import { AreaCards, NameIndex } from "@/components/docs/reference-parts";
import { hashTargets } from "@/lib/docs/reference";
import { API_ALIASES, API_BASE, apiReference } from "@/lib/docs/references";
import { pageSocial } from "@/lib/seo-core";
import { siteConfig } from "@/lib/site";

const title = "API Reference: Tauri commands, types and events";
const description =
  "The API reference for edytlab by area: every Tauri command the frontend can call, the TypeScript bridge types and the events the backend emits, with examples.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs/api-reference" },
  ...pageSocial("/docs/api-reference", `${title} · edytlab`, description),
};

/** The events are the part of the reference that is not a command. */
const isEvents = (sectionTitle: string) => sectionTitle === "Events";

export default function ApiReferencePage() {
  return (
    <DocShell
      title="API Reference"
      description="All Tauri commands, TypeScript bridge types, and events."
    >
      {/* A link from before this page was split can carry a hash for a
          command; this sends it to the area that has it. */}
      <HashRedirect base={API_BASE} targets={hashTargets(apiReference.areas, API_ALIASES)} />

      <blockquote>
        The public API the frontend uses. The Rust implementation is in{" "}
        <a
          href={`${siteConfig.github}/blob/main/apps/desktop/src-tauri/src/commands.rs`}
          target="_blank"
          rel="noopener noreferrer"
        >
          commands.rs
        </a>{" "}
        and{" "}
        <a
          href={`${siteConfig.github}/blob/main/apps/desktop/src/lib/tauri-bridge.ts`}
          target="_blank"
          rel="noopener noreferrer"
        >
          tauri-bridge.ts
        </a>
        . The complete reference is{" "}
        <a
          href={`${siteConfig.github}/blob/main/docs/api-reference.md`}
          target="_blank"
          rel="noopener noreferrer"
        >
          docs/api-reference.md
        </a>
        .
      </blockquote>

      <p>
        All commands are invoked via <code>tauri-bridge.ts</code>. They return{" "}
        <code>Promise&lt;T&gt;</code> and throw a string on Rust{" "}
        <code>Err(_)</code>.
      </p>

      <h2 id="areas">Commands, events and types by area</h2>
      <AreaCards reference={apiReference} />

      <h2 id="all-commands">All commands, A to Z</h2>
      <NameIndex reference={apiReference} filter={(section) => !isEvents(section)} />

      <h2 id="all-events">All events, A to Z</h2>
      <NameIndex reference={apiReference} filter={isEvents} />
    </DocShell>
  );
}
