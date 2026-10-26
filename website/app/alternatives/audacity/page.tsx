import type { Metadata } from "next";
import Link from "next/link";

import { Disclaimer } from "@/components/compare/disclaimer";
import { FactsTable, type FactRow } from "@/components/compare/facts-table";
import { LastVerified } from "@/components/compare/last-verified";
import { NotBuiltYet } from "@/components/compare/not-built-yet";
import { SourcesList, type Source } from "@/components/compare/sources-list";
import { Verdict } from "@/components/compare/verdict";
import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { VideoLinkCard } from "@/components/landing/site-video";
import { claimFor, competitorBySlug, newestCheck } from "@/lib/competitors";
import {
  articleJsonLd,
  breadcrumbJsonLd,
  compareBreadcrumbs,
  pageAlternates,
  pageSocial,
} from "@/lib/seo";
import { GA_MEASUREMENT_ID } from "@/lib/consent";
import { siteConfig } from "@/lib/site";
import { formatDuration, getVideo } from "@/lib/videos";

const title = "Audacity alternative: free, open source, AI-driven";
const description =
  "A free, open-source Audacity alternative you drive in plain English. See what edytlab does, where Audacity is better, and what each costs.";
const path = "/alternatives/audacity";
// The day the page was first committed. The day it last changed in
// substance is `PAGE_UPDATED[path]` in `lib/seo-core.ts`.
const published = "2026-10-22";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/alternatives/audacity"),
  ...pageSocial("/alternatives/audacity", `${title} · edytlab`, description),
};

const audacity = competitorBySlug("audacity");

/** A tool name. Prose's typography wraps `code` in backticks; the name is already set apart. */
function Code({ children }: { children: string }) {
  return (
    <code className="rounded bg-secondary/60 px-1.5 py-0.5 text-[0.9em] before:content-none after:content-none">{children}</code>
  );
}

/** The claims the page below leans on, each linked where it is used and listed at the foot. */
const cited = [
  "free-open-source",
  "plugins-vst",
  "plugins-lv2",
  "ai-plugin-list",
  "ai-macos-beta",
  "musehub-ai-bundles",
  "recording-and-effects",
  "anniversary",
  "cloud-storage",
  "musehub-paid",
  "privacy-notice",
  "label-format",
  "mcp-third-party",
].map((topic) => claimFor(audacity, topic));

const href = (topic: string) => claimFor(audacity, topic).sourceUrl;

/** Every page the facts come from, once each, named for a reader. */
const SOURCE_LABELS: Record<string, string> = {
  "https://www.audacityteam.org/": "Audacity home page",
  "https://www.audacityteam.org/audacity-4/": "Audacity 4 page",
  "https://www.audacityteam.org/download/": "Audacity download page",
  "https://www.audacityteam.org/faq/": "Audacity FAQ",
  "https://plugins.audacityteam.org/ai-plugins/ai-plugins": "Audacity AI plugins page",
  "https://www.audacityteam.org/legal/privacy-notice/": "Audacity privacy notice (Audacity 4 and newer)",
  "https://www.audacityteam.org/blog/openvino-mac/": "Audacity blog: OpenVINO AI plugins for macOS",
  "https://www.audacityteam.org/blog/25-years/": "Audacity blog: Audacity turns 25 years old",
  "https://manual.audacityteam.org/man/importing_and_exporting_labels.html": "Audacity manual: Importing and Exporting Labels",
  "https://github.com/xDarkzx/Audacity-MCP": "AudacityMCP README (a third-party project)",
};

/** Each fact the page states, as the page it was read on and the day. */
const leaned = [
  ...[audacity.pricing, audacity.openSource, audacity.platforms, audacity.localOrCloud, audacity.aiFeatures].map((f) => ({
    url: f.url,
    checkedOn: f.checkedOn,
  })),
  ...cited.map((c) => ({ url: c.sourceUrl, checkedOn: c.checkedOn })),
];

const sources: Source[] = [...new Set(leaned.map((l) => l.url))].map((url) => ({
  label: SOURCE_LABELS[url] ?? url,
  href: url,
  checkedOn: leaned
    .filter((l) => l.url === url)
    .map((l) => l.checkedOn)
    .sort()
    .at(-1) as string,
}));

const rows: FactRow[] = [
  {
    label: "Price",
    ours: "The app is free. A hosted provider bills you for tokens; Ollama is free and runs on your machine.",
    oursSource: { href: "/docs/getting-started", label: "Getting started" },
    theirs: audacity.pricing.text,
    source: { href: audacity.pricing.url },
    checkedOn: audacity.pricing.checkedOn,
  },
  {
    label: "License",
    ours: "MIT.",
    oursSource: { href: `${siteConfig.github}/blob/main/LICENSE`, label: "LICENSE on GitHub" },
    theirs: audacity.openSource.text,
    source: { href: audacity.openSource.url },
    checkedOn: audacity.openSource.checkedOn,
  },
  {
    label: "Platforms",
    ours: "macOS (universal), Windows and Linux.",
    oursSource: { href: "/docs/getting-started", label: "Getting started" },
    theirs: audacity.platforms.text,
    source: { href: audacity.platforms.url },
    checkedOn: audacity.platforms.checkedOn,
  },
  {
    label: "Where the audio is processed",
    ours: "On your computer. Only the chat goes to the AI provider you pick, and with Ollama it stays on your machine too. The app sends no telemetry.",
    oursSource: { href: "/privacy", label: "Privacy" },
    theirs: audacity.localOrCloud.text,
    source: { href: audacity.localOrCloud.url },
    checkedOn: audacity.localOrCloud.checkedOn,
  },
  {
    label: "AI features",
    ours: "An agent that calls edytlab's editing tools from your request. Transcription runs on your computer, after a one-time model download. Stem separation has not shipped.",
    oursSource: { href: "/docs/tools", label: "Tools reference" },
    theirs: audacity.aiFeatures.text,
    source: { href: audacity.aiFeatures.url },
    checkedOn: audacity.aiFeatures.checkedOn,
  },
  {
    label: "Audio plugins",
    ours: "None: edytlab has no plugin hosting.",
    oursSource: { href: `${siteConfig.github}/blob/main/README.md`, label: "README" },
    theirs: claimFor(audacity, "plugins-vst").text,
    source: { href: href("plugins-vst") },
    checkedOn: claimFor(audacity, "plugins-vst").checkedOn,
  },
];

const demo = (() => {
  const video = getVideo("dj-mini-mix");
  if (!video) throw new Error("the dj-mini-mix recording is gone; update the Audacity page");
  return video;
})();

export default function AudacityAlternativePage() {
  const checkedOn = newestCheck(audacity);
  return (
    <PageShell
      eyebrow="Alternatives"
      title="An Audacity alternative you control by describing the edit"
      lead={
        <>
          edytlab is a free, MIT-licensed desktop editor in which an AI agent runs local audio tools from a sentence you type; a
          hosted AI provider bills you for tokens unless you use Ollama. Audacity is also free and open source, and is the better
          fit if you record, use audio plugins or edit waveforms by hand.
        </>
      }
    >
      <LastVerified checkedOn={checkedOn} />
      <JsonLd data={breadcrumbJsonLd(compareBreadcrumbs("Audacity alternative", path))} />
      <JsonLd data={articleJsonLd({ path, headline: title, description, datePublished: published })} />
      <Prose>
        <h2 id="quick-facts">Quick facts</h2>
        <FactsTable theirName="Audacity" rows={rows} />

        <h2 id="where-audacity-is-better">Where Audacity is better</h2>
        <ul>
          <li>
            <strong>Audio plugins.</strong>{" "}
            Besides the table row, Audacity&apos;s{" "}
            <a href={href("plugins-lv2")} target="_blank" rel="noopener noreferrer">
              Audacity 4 page
            </a>{" "}
            says LV2 plugins load &ldquo;right alongside VST3 and Audio Units&rdquo;. edytlab has no plugin hosting.
          </li>
          <li>
            <strong>Local AI plugins today.</strong>{" "}
            Audacity&apos;s{" "}
            <a href={href("ai-plugin-list")} target="_blank" rel="noopener noreferrer">
              plugin page
            </a>{" "}
            lists a &ldquo;Music Separation&rdquo; plugin among its local ones; edytlab has not shipped stem separation. A{" "}
            <a href={href("ai-macos-beta")} target="_blank" rel="noopener noreferrer">
              June 2025 post
            </a>{" "}
            says the plugins reached macOS with Audacity 3.7.4, as a beta of sorts. We could not find a statement on
            Audacity&apos;s site that they work with Audacity 4, the current version, so check before you rely on them there.
          </li>
          <li>
            <strong>AI in Audacity&apos;s plugin store.</strong>{" "}
            Audacity&apos;s{" "}
            <a href={href("musehub-ai-bundles")} target="_blank" rel="noopener noreferrer">
              home page
            </a>{" "}
            also promotes MuseHub plugins, one described as &ldquo;Clean up spoken voice and create instant transcripts&rdquo;
            and another as &ldquo;Automatically split mixed-speaker audio into separate tracks&rdquo;.
          </li>
          <li>
            <strong>Recording and hand editing.</strong>{" "}
            Audacity&apos;s{" "}
            <a href={href("recording-and-effects")} target="_blank" rel="noopener noreferrer">
              FAQ
            </a>{" "}
            lists recording live with a microphone or mixer and a spectrogram view among its features, and its{" "}
            <a href={href("anniversary")} target="_blank" rel="noopener noreferrer">
              blog
            </a>{" "}
            marked the project&apos;s 25th anniversary in May 2025. edytlab is a new project.
          </li>
          <li>
            <strong>No AI provider needed.</strong>{" "}
            Audacity works without an AI provider or a key. edytlab&apos;s chat needs one: a hosted provider with your own key,
            or Ollama running on your machine.
          </li>
        </ul>

        <NotBuiltYet />

        <h2 id="where-edytlab-differs">Where edytlab differs</h2>
        <ul>
          <li>
            <strong>Describe a multi-step job in words.</strong>{" "}
            Ask for something like &ldquo;match this track to 124 BPM,
            crossfade it into the next, bring both to −14 LUFS and export a WAV&rdquo;, and the agent calls the tools in order.
            You can hear the result at each step and undo it.
          </li>
          <li>
            <strong>A branching history, not one undo stack.</strong>{" "}
            Every state is a node in a graph. You can fork it, name it,
            compare two versions and revert, and undo follows the path you took.
          </li>
          <li>
            <strong>Loudness targets by name.</strong>{" "}
            <Code>normalize_loudness</Code>{" "}
            takes a preset (Spotify and YouTube −14 LUFS, Apple Podcasts −16, broadcast −23) and sets a track&apos;s gain to
            reach it, capped at a peak ceiling (−1 dBFS by default).
          </li>
          <li>
            <strong>Recipes you can replay over a folder.</strong>{" "}
            <Code>export_recipe</Code>{" "}
            writes a session&apos;s steps to a
            JSON file with no audio in it, and <Code>batch_apply</Code>{" "}
            runs that file on every audio file in a folder, each
            file becoming its own project.
          </li>
        </ul>
        <div className="not-prose mt-6 max-w-xl">
          <VideoLinkCard video={demo} />
        </div>

        <h2 id="use-both">Use both: exports and labels</h2>
        <p>
          edytlab loads WAV, MP3, FLAC and other common audio formats and exports WAV, FLAC or MP3, so audio moves between the
          two as ordinary files. It has no importer for Audacity&apos;s own project files, so export the audio from Audacity
          first and load that.
        </p>
        <p>
          Labels are the one place the two share a format. Audacity&apos;s{" "}
          <a href={href("label-format")} target="_blank" rel="noopener noreferrer">
            manual
          </a>{" "}
          says label tracks export to and import from tab-delimited plain text: start time in seconds, end time, then the
          label&apos;s text. edytlab&apos;s <Code>import_labels</Code> and <Code>export_labels</Code> read and write that
          layout. <Code>export_labels</Code> returns the text in the chat and writes no file, so save it as a .txt file to
          import into Audacity (File, Import, Labels); paste Audacity&apos;s exported text to the agent to go the other way.
        </p>

        <h2 id="chat-control-in-audacity">Chat control inside Audacity</h2>
        <p>
          Third-party projects connect MCP-capable AI assistants to Audacity. One of them, AudacityMCP, says in its{" "}
          <a href={href("mcp-third-party")} target="_blank" rel="noopener noreferrer">
            README
          </a>{" "}
          that it works with Audacity 3.x and cannot control Audacity 4, and it points to a separate early-alpha project for
          Audacity 4 that needs a companion fork built from source. We have not tried either project.
        </p>

        <h2 id="privacy">Privacy</h2>
        <p>
          Audacity&apos;s{" "}
          <a href={href("privacy-notice")} target="_blank" rel="noopener noreferrer">
            privacy notice
          </a>{" "}
          for Audacity 4 and newer, last updated 31 August 2026, is summarised in the table above. This page says nothing about
          Audacity&apos;s privacy beyond that notice.
        </p>
        <p>
          edytlab&apos;s side is in the table too, and the <Link href="/use-cases/local-ai-audio-editor">local-first page</Link>{" "}
          lists what stays, what leaves, and what to expect from local models. The app has no update check. This website records
          visits with anonymous Vercel analytics{GA_MEASUREMENT_ID ? " and, only if you accept it in the cookie banner, Google Analytics" : ""}; the <Link href="/privacy">privacy page</Link> says what.
        </p>

        <h2 id="platforms-and-cost">An Audacity alternative for Linux, macOS and Windows, and what each costs</h2>
        <p>
          Both run on all three. edytlab is an Audacity alternative for Linux, an Audacity alternative on macOS and an Audacity
          alternative for Windows, and a free Audacity alternative that is also an open-source Audacity alternative, under the
          MIT license. Audacity itself is GPL v3, per its{" "}
          <a href={audacity.openSource.url} target="_blank" rel="noopener noreferrer">
            Audacity 4 page
          </a>
          .
        </p>
        <p>
          Audacity is free, though its{" "}
          <a href={href("cloud-storage")} target="_blank" rel="noopener noreferrer">
            FAQ
          </a>{" "}
          notes that extra Audacity Cloud storage is something people pay for, and its{" "}
          <a href={href("musehub-paid")} target="_blank" rel="noopener noreferrer">
            Audacity 4 page
          </a>{" "}
          says MuseHub lists free and paid plugins. edytlab&apos;s app is free too; a hosted provider bills you for tokens, at
          the rates on that provider&apos;s own price list.
        </p>

        <Verdict
          competitor="Audacity"
          theirs={[
            "you record audio, or edit waveforms by hand and want that to be the main way you work;",
            "you rely on VST3, LV2, Audio Unit or Nyquist plugins;",
            "you want Audacity's local AI plugins today, and have checked that they support the version you run;",
            "you want no AI provider or key involved at all.",
          ]}
          ours={[
            "you would rather describe a multi-step job in a sentence than do each step by hand;",
            "you want a branching history, recipes you can replay over a folder, or loudness targets by name;",
            "you are happy to bring an AI provider's key, or run Ollama on your machine;",
            "you can work without audio plugins, and without stem separation, which has not shipped.",
          ]}
        />

        <h2 id="install-warning">Why macOS and Windows warn when you open edytlab</h2>
        <p>
          The installers are{" "}
          <a href={`${siteConfig.github}/issues/386`} target="_blank" rel="noopener noreferrer">
            not signed
          </a>
          . The source is public, and the <Link href="/docs/getting-started">getting started guide</Link> has the steps.
        </p>

        <SourcesList sources={sources} />
        <Disclaimer competitor="Audacity" checkedOn={checkedOn} />
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
