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
};

/** What each product's own pages say beyond the matrix's cells, in the matrix's order. */
const notes: Record<string, ReactNode> = {
  audacity: (
    <p>
      Its <Ext href={url.audacityPlugins}>plugin page</Ext> also lists noise suppression and music generation and
      continuation. A <Ext href={url.audacityMac}>June 2025 post</Ext> says the plugins reached macOS with Audacity 3.7.4, as a
      beta of sorts, and none of the pages we read says whether they work with Audacity 4.
    </p>
  ),
  "audacity-mcp": (
    <p>
      Its <Ext href={url.audacityMcp}>README</Ext> says Audacity 4 removed the scripting interface it uses. We have not run it.
    </p>
  ),
  audionaut: (
    <p>
      Its <Ext href={url.audionaut}>home page</Ext> says assistants such as Claude can inspect and edit projects, and that the
      stem model is downloaded once. The assistant is a client you connect; edytlab has its own agent built in.
    </p>
  ),
  descript: (
    <p>
      Its <Ext href={url.descriptPricing}>pricing page</Ext> lists &ldquo;Text-based editing: Edit video by editing a
      doc&rdquo;. Its <Ext href={url.descriptHelp}>Underlord help page</Ext> says that when Underlord runs a tool you are
      &ldquo;charged for both the agent brain and the tool itself&rdquo;.
    </p>
  ),
  "adobe-podcast": (
    <p>
      Its <Ext href={url.adobePodcast}>plans page</Ext> describes Enhance Speech, which it says will &ldquo;Remove noise and
      echo from voice recordings&rdquo;, and Studio (beta), which records, edits and enhances in the browser. On Premium,
      Enhance Speech is up to 4 hours a day, with files up to 1 GB. edytlab does not have a speech-enhancement model; its{" "}
      <Code>noise_reduction</Code> tool is spectral subtraction.
    </p>
  ),
  auphonic: (
    <p>
      Its <Ext href={url.auphonicHome}>home page</Ext> lists noise and reverb reduction, an &ldquo;Intelligent
      Leveler&rdquo; and loudness specifications (&ldquo;Define a target loudness, true peak limit, MaxLRA, and more&rdquo;).
      edytlab also sets a track to a loudness target by name, with <Code>normalize_loudness</Code>, on your own computer.
    </p>
  ),
  cleanvoice: (
    <p>
      Its <Ext href={url.cleanvoicePricing}>pricing page</Ext> lists a filler words remover, a background noise remover, a
      silence remover and a mouth sounds and breath remover.
    </p>
  ),
  ocenaudio: (
    <p>
      Its <Ext href={url.ocenaudioFeatures}>features page</Ext> lists VST plugin support, real-time preview of effects,
      multi-selection editing and a spectrogram view, and describes no AI features.
    </p>
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
        That is edytlab. Audionaut and AudacityMCP let an assistant you bring drive an editor on your computer; AudacityMCP
        works with Audacity 3.x, and its README says it cannot control Audacity 4.
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
    need: "You want to edit audio by editing its transcript.",
    body: (
      <>
        Descript&apos;s pages describe a text-based editor, with projects in the cloud and credits for its AI. edytlab does it
        on your computer, after you download the speech model once. Auphonic and Cleanvoice list transcription too, as
        services that process what you upload.
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
          Eight other tools that edit or clean up audio with AI help, set beside edytlab: where each runs, its license and
          price, and whether it takes chat or agent control, splits stems or transcribes speech. edytlab has not shipped stem
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
          Each cell says what that product&apos;s own page says, with a link to the page and the day we read it. Prices are the
          ones the page showed that day, in the currency shown.
        </p>
        <ul>
          <li>
            &ldquo;Not described&rdquo; means the page we read does not say. It is a statement about that page, not a claim
            that the product lacks the feature.
          </li>
          <li>
            &ldquo;Chat or agent control&rdquo; means you can ask in words for the editor to act, through a built-in assistant
            or an MCP client you connect.
          </li>
          <li>
            Cells in edytlab&apos;s row link edytlab&apos;s docs, which you can check against the app, so they have no read
            date.
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
        <ul>
          {picks.map((p) => (
            <li key={p.need}>
              <strong>{p.need}</strong> {p.body}
            </li>
          ))}
        </ul>

        <h2 id="in-detail">Comparisons in detail</h2>
        <p>
          <Link href="/alternatives/audacity">Audacity</Link> and{" "}
          <Link href="/alternatives/adobe-audition">Adobe Audition</Link> each have a page with a facts table, a section on where
          the other product is better and a verdict. Audition is not in the matrix: it is a multitrack workstation, and its
          product page does not describe chat or agent control. What stays on your machine and what leaves it is on the{" "}
          <Link href="/use-cases/local-ai-audio-editor">local-first page</Link>.
        </p>

        <SourcesList sources={sources} />
        <Disclaimer competitor="the other products" checkedOn={checkedOn} plural />
      </div>
      <div className="max-w-3xl">
        <PageLinks />
      </div>
    </PageShell>
  );
}
