import type { Metadata } from "next";
import Link from "next/link";
import type { ReactNode } from "react";

import { CompareMatrix } from "@/components/compare/compare-matrix";
import { Disclaimer } from "@/components/compare/disclaimer";
import { LastVerified } from "@/components/compare/last-verified";
import { NotBuiltYet } from "@/components/compare/not-built-yet";
import { SourcesList, type Source } from "@/components/compare/sources-list";
import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell } from "@/components/landing/page-shell";
import { claimFor, competitorBySlug, hostAndPath } from "@/lib/competitors";
import { isExternal, matrix, matrixCheckedOn } from "@/lib/compare-matrix";
import {
  breadcrumbJsonLd,
  compareHubJsonLd,
  pageAlternates,
  pageBreadcrumbs,
  pageSocial,
} from "@/lib/seo";
import { siteConfig } from "@/lib/site";

const title = "AI audio editors compared: local, open and cloud";
const description =
  "A dated, sourced comparison of AI audio editors: edytlab, Descript, Audacity with AI plugins, Audionaut and others. Local or cloud, price, license.";
const path = "/compare";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/compare"),
  ...pageSocial("/compare", `${title} · edytlab`, description),
};

const audacity = competitorBySlug("audacity");
const descript = competitorBySlug("descript");
const cleanvoice = competitorBySlug("cleanvoice");
const adobePodcast = competitorBySlug("adobe-podcast");
const auphonic = competitorBySlug("auphonic");

/** A tool name, set apart the way the other comparison pages set it. */
function Code({ children }: { children: string }) {
  return (
    <code className="rounded bg-secondary/60 px-1.5 py-0.5 text-[0.9em] before:content-none after:content-none">{children}</code>
  );
}

function Ext({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  );
}

/** The links the product notes below use, from the claims in `lib/competitors.ts`. */
const url = {
  audacityLicense: claimFor(audacity, "license").sourceUrl,
  audacityPlugins: claimFor(audacity, "ai-plugin-list").sourceUrl,
  audacityMac: claimFor(audacity, "ai-macos-beta").sourceUrl,
  audacityMcp: claimFor(audacity, "mcp-third-party").sourceUrl,
  descriptPricing: claimFor(descript, "text-based-editing").sourceUrl,
  descriptHelp: claimFor(descript, "underlord-credits").sourceUrl,
  adobePodcast: claimFor(adobePodcast, "enhance-speech").sourceUrl,
  auphonicHome: claimFor(auphonic, "loudness").sourceUrl,
  cleanvoicePricing: claimFor(cleanvoice, "pricing-note").sourceUrl,
  audionaut: "https://audionaut.app/",
  ocenaudioFeatures: "https://www.ocenaudio.com/features",
  ocenaudioDownload: "https://www.ocenaudio.com/download",
};

/** What each product's own pages say, a few sentences each, in the matrix's order. */
const notes: Record<string, ReactNode> = {
  audacity: (
    <>
      <p>
        Audacity is free and, per its <Ext href={url.audacityLicense}>Audacity 4 page</Ext>, licensed GPL v3. Its AI features
        are optional OpenVINO plugins. Its <Ext href={url.audacityPlugins}>plugin page</Ext> lists music separation into drums,
        bass, vocals and other instruments, noise suppression, music generation and continuation, and Whisper transcription
        into a label track, and says they run locally. A <Ext href={url.audacityMac}>June 2025 post</Ext> says the plugins
        reached macOS with Audacity 3.7.4, as a beta of sorts, and none of the pages we read says whether they work with
        Audacity 4.
      </p>
    </>
  ),
  "audacity-mcp": (
    <>
      <p>
        AudacityMCP is a separate project and not Audacity&apos;s own. Its <Ext href={url.audacityMcp}>README</Ext> gives the
        license as Apache License 2.0 and says it connects MCP-compatible AI assistants to Audacity through a tool set that its README counts at 144. It also
        says it works with Audacity 3.x and cannot control Audacity 4, which removed the scripting interface it uses, and it
        lists local transcription with faster-whisper as an experimental feature that needs separate setup. We have not run it.
      </p>
    </>
  ),
  audionaut: (
    <>
      <p>
        Audionaut&apos;s <Ext href={url.audionaut}>home page</Ext> calls it a free, open-source desktop application, under the
        GPLv3 license, for macOS, Windows and Linux. It says the app speaks MCP, so assistants such as Claude can inspect and
        edit projects, with each change arriving as one undo step. It also lists splitting a clip into drums, bass, other and
        vocals, which it says runs entirely on your machine with a model downloaded once. The assistant is a client you
        connect, such as Claude Code; edytlab has its own agent built in.
      </p>
    </>
  ),
  descript: (
    <>
      <p>
        Descript&apos;s <Ext href={url.descriptPricing}>pricing page</Ext> lists a free plan and paid plans, &ldquo;Text-based
        editing: Edit video by editing a doc&rdquo;, and transcription in 25 languages. Its Underlord AI co-editor is in beta:
        its <Ext href={url.descriptHelp}>help page</Ext> says each prompt uses credits and that when Underlord runs a tool you
        are &ldquo;charged for both the agent brain and the tool itself&rdquo;. Its podcasting page says &ldquo;Because it&rsquo;s all
        in the cloud, you&rsquo;ll be able to view different versions and restore them if you want&rdquo;, and the pages we read do
        not say where its AI runs.
      </p>
    </>
  ),
  "adobe-podcast": (
    <>
      <p>
        Adobe Podcast runs in the browser. Its <Ext href={url.adobePodcast}>plans page</Ext> describes Enhance Speech, which it
        says will &ldquo;Remove noise and echo from voice recordings&rdquo;, and Studio (beta), which records, edits and
        enhances in the browser. Premium comes with the Adobe Firefly Pro plan: Enhance Speech up to 4 hours a day, with files
        up to 1 GB. The plans page shows no price. edytlab does not have a speech-enhancement model; its{" "}
        <Code>noise_reduction</Code> tool is spectral subtraction.
      </p>
    </>
  ),
  auphonic: (
    <>
      <p>
        Auphonic is a hosted service: you upload audio or video. Its <Ext href="https://auphonic.com/pricing">pricing page</Ext>{" "}
        says it is free for 2 hours of processed audio a month and sells recurring and one-time credits. Its{" "}
        <Ext href={url.auphonicHome}>home page</Ext> lists noise and reverb reduction, an &ldquo;Intelligent Leveler&rdquo;,
        loudness specifications (&ldquo;Define a target loudness, true peak limit, MaxLRA, and more&rdquo;) and multilingual
        speech-to-text. edytlab also sets a track to a loudness target by name, with <Code>normalize_loudness</Code>, on your
        own computer.
      </p>
    </>
  ),
  cleanvoice: (
    <>
      <p>
        Cleanvoice is a cloud service and API. Its <Ext href={url.cleanvoicePricing}>pricing page</Ext> says &ldquo;We store both
        your original and edited files for 7 days. After 7 days, we remove your files permanently.&rdquo; and lists a filler
        words remover, a background noise remover, a silence remover, a mouth sounds and breath remover, and transcription and
        summary. It offers a free trial, then pay-as-you-go credits or a monthly subscription.
      </p>
    </>
  ),
  ocenaudio: (
    <>
      <p>
        ocenaudio&apos;s <Ext href={url.ocenaudioDownload}>download page</Ext> offers a free download. Its{" "}
        <Ext href={url.ocenaudioFeatures}>features page</Ext> lists VST plugin support, real-time preview of effects,
        multi-selection editing and a spectrogram view. The pages we read state no license and describe no AI features.
      </p>
    </>
  ),
  edytlab: (
    <>
      <p>
        edytlab is a free, MIT-licensed desktop app for macOS, Windows and Linux in which an AI agent calls local audio tools
        from a request you type. Audio is processed on your computer; the chat goes to the AI provider you pick, or stays on
        your machine with Ollama, and a hosted provider bills you for tokens. Transcription runs on your computer after a
        one-time download of the speech model that you start from Settings. Stem separation has not shipped, so the{" "}
        <Link href="/docs/tools">tools reference</Link> lists a tool that returns an error today.
      </p>
    </>
  ),
};

/** Every page the notes and the matrix lean on, once each, named for a reader. */
const SOURCE_LABELS: Record<string, string> = {
  "https://www.audacityteam.org/": "Audacity home page",
  "https://www.audacityteam.org/audacity-4/": "Audacity 4 page",
  "https://plugins.audacityteam.org/ai-plugins/ai-plugins": "Audacity AI plugins page",
  "https://www.audacityteam.org/blog/openvino-mac/": "Audacity blog: OpenVINO AI plugins for macOS",
  "https://github.com/xDarkzx/Audacity-MCP": "AudacityMCP README (a third-party project)",
  "https://audionaut.app/": "Audionaut home page",
  "https://www.descript.com/pricing": "Descript pricing page",
  "https://www.descript.com/podcasting": "Descript podcasting page",
  "https://www.descript.com/download": "Descript download page",
  "https://help.descript.com/getting-started/underlord-beta-your-ai-co-editor-in-descript": "Descript help: Underlord (Beta)",
  "https://podcast.adobe.com/en/plans": "Adobe Podcast plans page",
  "https://auphonic.com/": "Auphonic home page",
  "https://auphonic.com/pricing": "Auphonic pricing page",
  "https://cleanvoice.ai/pricing/": "Cleanvoice AI pricing page",
  "https://www.ocenaudio.com/features": "ocenaudio features page",
  "https://www.ocenaudio.com/download": "ocenaudio download page",
};

/** Each page a fact on this one comes from, with the day it was read there. */
const leaned: { url: string; checkedOn: string }[] = [
  ...matrix.flatMap((row) =>
    Object.values(row.cells)
      .filter((c) => isExternal(c.href) && c.checkedOn)
      .map((c) => ({ url: c.href, checkedOn: c.checkedOn as string })),
  ),
  ...[
    claimFor(audacity, "ai-macos-beta"),
    claimFor(audacity, "license"),
    claimFor(descript, "text-based-editing"),
    claimFor(descript, "underlord-credits"),
    claimFor(adobePodcast, "enhance-speech"),
    claimFor(auphonic, "loudness"),
    claimFor(cleanvoice, "pricing-note"),
  ].map((c) => ({ url: c.sourceUrl, checkedOn: c.checkedOn })),
];

const sources: Source[] = [...new Set(leaned.map((l) => l.url))].map((u) => ({
  label: SOURCE_LABELS[u] ?? hostAndPath(u),
  href: u,
  checkedOn: leaned
    .filter((l) => l.url === u)
    .map((l) => l.checkedOn)
    .sort()
    .at(-1) as string,
}));

const picks: { need: string; body: ReactNode }[] = [
  {
    need: "You want to describe an edit in words and keep the audio on your computer.",
    body: (
      <>
        That is edytlab. Audionaut and AudacityMCP also let an assistant drive an editor on your computer, with the assistant a
        client that you bring. Descript&apos;s Underlord uses credits.
      </>
    ),
  },
  {
    need: "You need to split a mix into stems now, on your computer.",
    body: (
      <>
        Audionaut lists it as a built-in feature, and Audacity lists a music separation plugin, if the plugin supports the
        version of Audacity you run. edytlab has not shipped stem separation.
      </>
    ),
  },
  {
    need: "You want an assistant to drive an editor you already use.",
    body: (
      <>
        AudacityMCP connects assistants to Audacity 3.x, and its README says it cannot control Audacity 4. Audionaut speaks MCP
        itself. edytlab is its own editor, with the agent built in.
      </>
    ),
  },
  {
    need: "You want to edit audio by editing its transcript.",
    body: (
      <>
        Descript&apos;s pages describe a text-based editor, with projects in the cloud and credits for its AI. edytlab does it on your computer, after you
        download the speech model once. Auphonic and Cleanvoice list transcription too, as services that process what you
        upload.
      </>
    ),
  },
  {
    need: "You want one-click cleanup as a service.",
    body: (
      <>
        Auphonic, Cleanvoice and Adobe Podcast are hosted: you upload your files. Check each one&apos;s limits and how long it
        keeps them on its own site first.
      </>
    ),
  },
  {
    need: "You want a plain, free editor that takes plugins.",
    body: (
      <>
        Audacity lists VST3 and Nyquist plugin support on its home page, and ocenaudio lists VST support on its features page.
        edytlab has no plugin hosting.
      </>
    ),
  },
];

export default function ComparePage() {
  const checkedOn = matrixCheckedOn();
  return (
    <PageShell
      wide
      eyebrow="Compare"
      title="AI audio editors compared: where they run and what they do"
      lead={
        <>
          This page sets edytlab beside eight other tools that edit or clean up audio with AI help: where each runs, its
          license and price model, whether you can drive it by chat or through an agent, and whether it splits stems or
          transcribes speech. Every cell links the page that states it and the day we read it. edytlab has not shipped stem
          separation, and its row says so.
        </>
      }
    >
      <div className="max-w-3xl">
        <LastVerified checkedOn={checkedOn} />
      </div>
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("Compare", path))} />
      <JsonLd
        data={compareHubJsonLd({
          path,
          name: title,
          description,
          items: matrix.map((row) => ({ name: row.name, anchor: row.id })),
        })}
      />

      <div className="prose prose-invert mt-10 max-w-3xl text-foreground/90 [&_a]:text-primary [&_h2]:mt-12 [&_h2]:text-2xl [&_h2]:font-semibold [&_h3]:mt-8 [&_p]:mt-4 [&_p]:leading-relaxed [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_li]:mt-1">
        <h2 id="how-to-read" className="!mt-0">
          How to read the table
        </h2>
        <p>
          Each cell says what that product&apos;s own page says, in a line or two, and links the page and the day we read it.
          How we compare, and why, is on <Link href="/about#how-we-compare">the about page</Link>. A few things to know:
        </p>
        <ul>
          <li>
            &ldquo;Not described&rdquo; means the page we read does not say. It is a statement about that page, and not a
            claim that the product lacks the feature; the product may do it, or may have said so elsewhere.
          </li>
          <li>
            &ldquo;Chat or agent control&rdquo; means you can ask in words for the editor to act, through a built-in assistant
            or an MCP client you connect.
          </li>
          <li>
            Cells with no read date in edytlab&apos;s own row link edytlab&apos;s docs, which you can check against the app. The
            row is last, with the same columns as the rest.
          </li>
          <li>
            Prices are the ones the page showed on the day, in the currency shown. Check the product&apos;s site before you
            decide.
          </li>
        </ul>
      </div>

      <h2 id="matrix" className="mt-12 text-2xl font-semibold">
        The matrix
      </h2>
      <CompareMatrix />

      <div className="prose prose-invert max-w-3xl text-foreground/90 [&_a]:text-primary [&_h2]:mt-12 [&_h2]:text-2xl [&_h2]:font-semibold [&_h3]:mt-8 [&_h3]:text-lg [&_h3]:font-semibold [&_p]:mt-4 [&_p]:leading-relaxed [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_li]:mt-3">
        <NotBuiltYet />

        <h2 id="product-notes">Product by product</h2>
        <p>
          A few sentences on each, taken from its own pages. Where a product does something edytlab does not, the row above and
          the note below say so.
        </p>
        {matrix.map((row) => (
          <section key={row.id} aria-labelledby={`${row.id}-heading`}>
            <h3 id={row.id}>
              <span id={`${row.id}-heading`}>{row.name}</span>
            </h3>
            {notes[row.id]}
            {row.detail ? (
              <p>
                More: <Link href={row.detail.href}>{row.detail.label}</Link>.
              </p>
            ) : null}
          </section>
        ))}

        <h2 id="which-to-pick">Which should you pick?</h2>
        <p>
          It depends on the job. Find the sentence that describes yours.
        </p>
        <ul>
          {picks.map((p) => (
            <li key={p.need}>
              <strong>{p.need}</strong> {p.body}
            </li>
          ))}
        </ul>

        <h2 id="in-detail">Comparisons in detail</h2>
        <p>
          Two products have a page of their own, with a table of facts, a section on where the other product is better and a
          verdict: <Link href="/alternatives/audacity">Audacity</Link> and{" "}
          <Link href="/alternatives/adobe-audition">Adobe Audition</Link>. Adobe Audition is not in the matrix: it is a multitrack
          workstation, and its product page does not describe chat or agent control. For how edytlab handles what stays on your
          machine and what leaves it, see the{" "}
          <Link href="/use-cases/local-ai-audio-editor">local-first page</Link>.
        </p>

        <SourcesList competitor="the products above" sources={sources} />
        <Disclaimer competitor="the other products" checkedOn={checkedOn} plural />
      </div>
      <div className="max-w-3xl">
        <PageLinks />
      </div>
    </PageShell>
  );
}
