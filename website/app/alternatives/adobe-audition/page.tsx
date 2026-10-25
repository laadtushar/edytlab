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
import { claimFor, competitorBySlug, oldestCheck } from "@/lib/competitors";
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

const title = "Adobe Audition alternative: free and open source";
const description =
  "A free, open-source Adobe Audition alternative for Linux, macOS and Windows. Prices, what Audition does that edytlab does not, and the reverse.";
const path = "/alternatives/adobe-audition";
// The day the page was first committed. The day it last changed in
// substance is `PAGE_UPDATED[path]` in `lib/seo-core.ts`.
const published = "2026-10-24";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/alternatives/adobe-audition"),
  ...pageSocial("/alternatives/adobe-audition", `${title} · edytlab`, description),
};

const audition = competitorBySlug("adobe-audition");

/** A tool name. Prose's typography wraps `code` in backticks; the name is already set apart. */
function Code({ children }: { children: string }) {
  return (
    <code className="rounded bg-secondary/60 px-1.5 py-0.5 text-[0.9em] before:content-none after:content-none">{children}</code>
  );
}

/** The claims the page below leans on, each linked where it is used and listed at the foot. */
const cited = ["overview", "help-topics", "remix", "activation", "help-silence-loudness"].map((topic) =>
  claimFor(audition, topic),
);

const href = (topic: string) => claimFor(audition, topic).sourceUrl;

/** Every page the facts come from, once each, named for a reader. */
const SOURCE_LABELS: Record<string, string> = {
  "https://www.adobe.com/products/audition.html": "Adobe Audition product page",
  "https://helpx.adobe.com/audition/desktop/introduction/system-requirements.html":
    "Adobe Audition help: system requirements",
  "https://helpx.adobe.com/audition/desktop.html": "Adobe Audition help: topic list",
};

/** Each fact the page states, as the page it was read on and the day. */
const leaned = [
  ...[audition.pricing, audition.openSource, audition.platforms, audition.localOrCloud, audition.aiFeatures].map((f) => ({
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
    theirs: audition.pricing.text,
    source: { href: audition.pricing.url },
    checkedOn: audition.pricing.checkedOn,
  },
  {
    label: "License",
    ours: "MIT.",
    oursSource: { href: `${siteConfig.github}/blob/main/LICENSE`, label: "LICENSE on GitHub" },
    theirs: audition.openSource.text,
    source: { href: audition.openSource.url },
    checkedOn: audition.openSource.checkedOn,
  },
  {
    label: "Platforms",
    ours: "macOS (universal), Windows and Linux.",
    oursSource: { href: "/docs/getting-started", label: "Getting started" },
    theirs: audition.platforms.text,
    source: { href: audition.platforms.url },
    checkedOn: audition.platforms.checkedOn,
  },
  {
    label: "Where the audio is processed",
    ours: "On your computer. Only the chat goes to the AI provider you pick, and with Ollama it stays on your machine too. The app sends no telemetry.",
    oursSource: { href: "/privacy", label: "Privacy" },
    theirs: audition.localOrCloud.text,
    source: { href: audition.localOrCloud.url },
    checkedOn: audition.localOrCloud.checkedOn,
  },
  {
    label: "AI features",
    ours: "An agent that calls edytlab's editing tools from your request. Transcription runs on your computer from v0.8.0, after a one-time model download. Stem separation has not shipped.",
    oursSource: { href: "/docs/tools", label: "Tools reference" },
    theirs: audition.aiFeatures.text,
    source: { href: audition.aiFeatures.url },
    checkedOn: audition.aiFeatures.checkedOn,
  },
  {
    label: "Third-party plugins",
    ours: "None: edytlab has no plugin hosting.",
    oursSource: { href: `${siteConfig.github}/blob/main/README.md`, label: "README" },
    theirs: claimFor(audition, "help-topics").text,
    source: { href: href("help-topics") },
    checkedOn: claimFor(audition, "help-topics").checkedOn,
  },
];

const demo = (() => {
  const video = getVideo("dj-extended-club-intro");
  if (!video) throw new Error("the dj-extended-club-intro recording is gone; update the Adobe Audition page");
  return video;
})();

export default function AuditionAlternativePage() {
  const checkedOn = oldestCheck(audition);
  return (
    <PageShell
      eyebrow="Alternatives"
      title="An Adobe Audition alternative that is free, open source and runs on Linux"
      lead={
        <>
          edytlab is a free, MIT-licensed desktop editor for macOS, Windows and Linux in which an AI agent runs local audio
          tools from a sentence you type; a hosted AI provider bills you for tokens unless you use Ollama. Adobe Audition is a
          paid multitrack audio editor with third-party plugin support, which edytlab does not have.
        </>
      }
    >
      <LastVerified checkedOn={checkedOn} />
      <JsonLd data={breadcrumbJsonLd(compareBreadcrumbs("Adobe Audition alternative", path))} />
      <JsonLd data={articleJsonLd({ path, headline: title, description, datePublished: published })} />
      <Prose>
        <h2 id="quick-facts">Quick facts</h2>
        <FactsTable theirName="Adobe Audition" rows={rows} />

        <h2 id="where-audition-is-better">Where Adobe Audition is better</h2>
        <ul>
          <li>
            <strong>Third-party plugins and the Essential Sound panel.</strong>{" "}
            Adobe&apos;s{" "}
            <a href={href("help-topics")} target="_blank" rel="noopener noreferrer">
              help site
            </a>{" "}
            documents both. edytlab has no plugin hosting and no such panel: you ask the agent, which calls tools such as{" "}
            <Code>eq</Code>, <Code>compressor</Code>, <Code>de_esser</Code> and <Code>noise_reduction</Code>.
          </li>
          <li>
            <strong>A full workstation toolset.</strong>{" "}
            <a href={href("overview")} target="_blank" rel="noopener noreferrer">
              Adobe describes
            </a>{" "}
            Audition as a toolset that includes multitrack, waveform and spectral display for creating, mixing, editing and
            restoring audio. edytlab also has multitrack sessions and can draw a spectrogram in place of the waveform, but its
            tools take time ranges (and, since v0.8.0, words), and none of them edits a selection of frequencies on the
            spectrogram.
          </li>
          <li>
            <strong>Remix.</strong>{" "}
            Adobe&apos;s{" "}
            <a href={href("remix")} target="_blank" rel="noopener noreferrer">
              product page
            </a>{" "}
            names Remix: &ldquo;Easily and automatically rearrange any song to fit any duration with Remix in Audition.&rdquo;
            edytlab has no tool that rearranges a song to a target length by itself. You can still ask the agent to copy, cut,
            repeat and join sections, one instruction at a time.
          </li>
        </ul>

        <NotBuiltYet />

        <h2 id="jobs-both-can-do">Jobs both can do</h2>
        <p>
          Adobe&apos;s{" "}
          <a href={href("help-silence-loudness")} target="_blank" rel="noopener noreferrer">
            help site
          </a>{" "}
          lists the topics &ldquo;Remove silences from your audio recordings&rdquo; and &ldquo;Matching loudness across multiple
          audio files&rdquo;. edytlab does both as well: <Code>silence_finder</Code> and <Code>truncate_silence</Code> find and
          remove silent stretches, <Code>normalize_loudness</Code> sets a track&apos;s gain to a loudness target such as −14
          LUFS, and <Code>batch_apply</Code> runs a saved recipe on every audio file in a folder. Neither product is the only
          one to offer these; what differs is how you ask for them.
        </p>

        <h2 id="where-edytlab-differs">Where edytlab differs</h2>
        <ul>
          <li>
            <strong>It builds for Linux.</strong>{" "}
            edytlab has installers for macOS, Windows and Linux. Adobe&apos;s{" "}
            <a href={audition.platforms.url} target="_blank" rel="noopener noreferrer">
              requirements page
            </a>{" "}
            for Audition 26.5 lists Windows 11 and macOS Sonoma or later and no Linux.
          </li>
          <li>
            <strong>Describe a multi-step job in words.</strong>{" "}
            Ask for something like &ldquo;extend the intro by eight bars, hold back the low end until the drop and export a
            WAV&rdquo;, and the agent calls the tools in order. You hear the result at each step and can undo it.
          </li>
          <li>
            <strong>A branching history, not one undo stack.</strong>{" "}
            Every state is a node in a graph. You can fork it, name it, compare two versions and revert, and undo follows the
            path you took.
          </li>
          <li>
            <strong>Recipes you can replay over a folder.</strong>{" "}
            <Code>export_recipe</Code> writes a session&apos;s steps to a JSON file with no audio in it, and{" "}
            <Code>batch_apply</Code> runs that file on every audio file in a folder, each file becoming its own project.
          </li>
          <li>
            <strong>Speech to text on your computer.</strong>{" "}
            From v0.8.0 the agent can transcribe a track with the Whisper base speech model and cut words or filler words from
            the transcript. The model (about 139 MB) is not installed with the app: you download it once from Settings, and
            nothing downloads it for you. The audio never leaves your computer, but the words the model finds reach your AI
            provider as text, unless you use Ollama.
          </li>
        </ul>
        <p>
          The recording below ({formatDuration(demo.seconds)}) copies the first eight bars to make a sixteen-bar intro: ordinary
          edits the agent was asked for, and not an automatic remix.
        </p>
        <div className="not-prose mt-6 max-w-xl">
          <VideoLinkCard video={demo} />
        </div>

        <h2 id="what-each-costs">What each costs</h2>
        <p>
          Adobe&apos;s{" "}
          <a href={audition.pricing.url} target="_blank" rel="noopener noreferrer">
            product page
          </a>{" "}
          shows US$22.99 a month on an annual plan billed monthly for Audition alone. Creative Cloud Pro is US$69.99 a month, or
          US$34.99 a month for the first 3 months for new subscribers, and a free trial is offered; the page does not say how
          long it lasts. Those are the figures on the day we read it, in the currency shown, and they may differ for you or
          change.
        </p>
        <p>
          edytlab&apos;s app costs nothing; a hosted provider bills you for tokens at its own rates, and Ollama on your machine
          bills nothing. We make no estimate of a month&apos;s cost for either product, because it depends on how much you edit
          and which model you use.
        </p>

        <h2 id="activation-and-privacy">Activation and privacy</h2>
        <p>
          Adobe&apos;s{" "}
          <a href={href("activation")} target="_blank" rel="noopener noreferrer">
            requirements page
          </a>{" "}
          says an internet connection and registration are necessary for software activation, validation of subscriptions and
          access to online services. This page says nothing about Adobe&apos;s handling of data beyond that.
        </p>
        <p>
          edytlab&apos;s side is in the table above, and the <Link href="/use-cases/local-ai-audio-editor">local-first page</Link>{" "}
          lists what stays, what leaves, and what to expect from local models. The app has no update check. This website records
          visits with anonymous Vercel analytics{GA_MEASUREMENT_ID ? " and, only if you accept it in the cookie banner, Google Analytics" : ""}; the <Link href="/privacy">privacy page</Link> says what.
        </p>

        <h2 id="platforms-and-cost">An Adobe Audition alternative for Linux, macOS and Windows</h2>
        <p>
          edytlab is an Adobe Audition alternative for Linux, an Adobe Audition alternative on macOS and an Adobe Audition
          alternative for Windows, and a free Adobe Audition alternative that is also an open-source Adobe Audition alternative,
          under the MIT license. It needs an AI provider, or Ollama, for the chat.
        </p>
        <p>
          If a plain, free editor is what you need, the <Link href="/alternatives/audacity">Audacity alternative</Link> page sets
          edytlab beside Audacity, which is also free and open source and runs on Linux.
        </p>

        <h2 id="use-both">Use both</h2>
        <p>
          edytlab loads WAV, MP3, FLAC and other common audio formats and exports WAV, FLAC or MP3, so audio moves between the
          two as ordinary files. It has no importer for Audition&apos;s own project files, so export the audio from Audition
          first and load that.
        </p>

        <Verdict
          competitor="Adobe Audition"
          theirs={[
            "you rely on third-party plugins, or on the Essential Sound panel, in your work;",
            "you work in multitrack and spectral displays and want them as part of one toolset;",
            "you want Remix to rearrange a song to a given length for you;",
            "you already pay for Creative Cloud and want Audition in it.",
          ]}
          ours={[
            "you would rather describe a multi-step job in a sentence than build it by hand;",
            "you want it free and open source, or you use Linux;",
            "you want a branching history, recipes you can replay over a folder, or loudness targets by name;",
            "you can work without plugins, and without stem separation, which has not shipped.",
          ]}
          closing={
            <p>
              The <Link href="/compare">comparison hub</Link> sets edytlab beside other AI audio editors.
            </p>
          }
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
        <Disclaimer competitor="Adobe Audition" checkedOn={checkedOn} />
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
