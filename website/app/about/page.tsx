import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { REFRESH_AFTER_DAYS } from "@/lib/competitors";
import { FOUNDER, LABYNATOR, otherApps } from "@/lib/family";
import {
  ORGANIZATION_ID,
  WEBSITE_ID,
  absoluteUrl,
  breadcrumbJsonLd,
  pageAlternates,
  pageBreadcrumbs,
  pageSocial,
  pageUpdated,
} from "@/lib/seo";
import { siteConfig } from "@/lib/site";

const title = "About edytlab and how we write comparisons";
const description =
  "Who builds edytlab, how comparison pages are sourced and dated, how AI tools are used in writing, and how to report an error.";
const path = "/about";

/** "MemryLab, XpenseLab and HyredLab": the rest of the family, from `lib/family.ts`. */
const siblings = otherApps.map((app) => app.name);
const siblingList = `${siblings.slice(0, -1).join(", ")} and ${siblings[siblings.length - 1]}`;

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/about"),
  ...pageSocial("/about", `${title} · edytlab`, description),
};

/**
 * The page's own `AboutPage`: about the project (the home page's
 * `Organization`, by its `@id`), part of the site (its `WebSite`),
 * written by the founder. It claims no rating, review or count.
 */
function aboutPageJsonLd() {
  return {
    "@context": "https://schema.org",
    "@type": "AboutPage",
    name: title,
    description,
    url: absoluteUrl(path),
    inLanguage: "en",
    dateModified: pageUpdated(path),
    isPartOf: { "@id": WEBSITE_ID },
    about: { "@id": ORGANIZATION_ID },
    author: { "@type": "Person", name: FOUNDER.name, url: FOUNDER.url },
  };
}

export default function AboutPage() {
  return (
    <PageShell
      eyebrow="About"
      title="About edytlab"
      lead="edytlab is an open-source desktop audio editor you drive in plain English."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("About", path))} />
      <JsonLd data={aboutPageJsonLd()} />
      <Prose>
        <h2>Who builds edytlab</h2>
        <p>
          edytlab is built by <a href={FOUNDER.url}>{FOUNDER.name}</a> at{" "}
          <a href={LABYNATOR.url}>{LABYNATOR.name}</a>, the studio that also makes {siblingList}. The source,
          its history and its issues are public on{" "}
          <a href={siteConfig.github} target="_blank" rel="noopener noreferrer">
            GitHub
          </a>{" "}
          under the MIT license.
        </p>

        <h2>What edytlab is, and is not</h2>
        <p>
          edytlab is a desktop app for macOS, Windows and Linux in which you describe an edit in a chat and an AI agent calls
          local audio tools to make it. Audio is processed on your computer; only the chat goes to the AI provider you choose,
          one of six API providers (Ollama runs on your own machine and needs no key) or your ChatGPT plan. The app sends no
          telemetry. It costs nothing, but a hosted AI provider bills you for tokens unless you use Ollama, and your ChatGPT plan
          counts the usage toward the plan. The <Link href="/privacy">privacy page</Link> has the details, including what this
          website records.
        </p>
        <p>What it is not:</p>
        <ul>
          <li>It is not a DAW. It edits audio files, with no MIDI, no instruments and no plugin hosting.</li>
          <li>It is not a live DJ tool. It prepares and exports files.</li>
          <li>
            Stem separation has not shipped. The tool exists and returns an error today. Progress is tracked in{" "}
            <a href={`${siteConfig.github}/issues/385`} target="_blank" rel="noopener noreferrer">
              issue #385
            </a>{" "}
            on GitHub. Transcription runs on your computer, after you download the speech model once.
          </li>
          <li>
            The installers are{" "}
            <a href={`${siteConfig.github}/issues/386`} target="_blank" rel="noopener noreferrer">
              unsigned
            </a>
            , so macOS and Windows warn the first time you open it.
          </li>
        </ul>

        <h2 id="how-we-compare">How comparisons are made</h2>
        <p>
          The <Link href="/alternatives/audacity">Audacity alternative</Link> and{" "}
          <Link href="/alternatives/adobe-audition">Adobe Audition alternative</Link> pages and{" "}
          <Link href="/compare">the comparison hub</Link> follow these rules. The site&apos;s automated checks cover the ones
          a machine can check.
        </p>
        <ul>
          <li>
            Every fact about another product comes from that product&apos;s own pages, linked beside the fact with the day we
            read it.
          </li>
          <li>
            Facts are re-checked at least every {REFRESH_AFTER_DAYS} days. The automated checks fail when any fact is more than
            100 days old.
          </li>
          <li>
            Where the other product is better comes first, in a section of its own, before we say where edytlab differs.
          </li>
          <li>
            We say a product lacks something only when its own pages say so; otherwise we write that its pages do not
            describe it.
          </li>
          <li>
            No unqualified superlatives: nothing is &ldquo;best&rdquo; or &ldquo;cheapest&rdquo; here, outside a quotation.
          </li>
          <li>
            Product names are plain text, used only to identify the product: no logos, screenshots or other branding, and no
            implication that their makers sponsor or endorse edytlab.
          </li>
          <li>
            There are no affiliate links, sponsorships or paid placement on these pages. If that ever changes, the link will
            say so beside it.
          </li>
          <li>
            No ratings or scores. Unless a page says it reports a test of our own, it reports what a product&apos;s pages say,
            not how the product performed for us.
          </li>
          <li>
            A price is the price shown on the day we read it, in the currency shown, for the plan named.
          </li>
        </ul>

        <h2>How AI is used</h2>
        <p>
          edytlab&apos;s code, docs and this site are written with AI coding assistants (Claude Code) under the
          maintainer&apos;s direction. Assistants make mistakes. That is why each comparison fact carries its source link and
          its date: you can check it against the product&apos;s own page without trusting us.
        </p>
        <p>
          In the app, the agent is a language model you choose; it calls deterministic audio tools, which the{" "}
          <Link href="/docs/tools">tools reference</Link> lists.
        </p>

        <h2 id="corrections">Corrections</h2>
        <p>
          If something on this site is wrong, open an issue in the{" "}
          <a href={`${siteConfig.github}/issues`} target="_blank" rel="noopener noreferrer">
            GitHub issue tracker
          </a>
          . Say which page, quote the sentence, and link the page that shows the right fact. We correct a mistake as soon as
          we have checked it against that source, and move a page&apos;s checked date only after reading its sources again,
          never to make a page look newer.
        </p>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
