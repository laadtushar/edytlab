import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { DemoCard } from "@/components/landing/real-demos";
import { VideoLinkCard } from "@/components/landing/site-video";
import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { demos } from "@/lib/demos";
import {
  DEFAULT_OG_IMAGE,
  articleJsonLd,
  breadcrumbJsonLd,
  pageAlternates,
  pageBreadcrumbs,
} from "@/lib/seo";
import { siteConfig } from "@/lib/site";
import { getVideo } from "@/lib/videos";

const title = "AI audio editor for DJs: beatmatch, blend and master";
const description =
  "Prep blends by describing them. edytlab's AI agent beatmatches two tracks, crossfades, masters to a loudness target and exports a file. Free and open source.";
const path = "/use-cases/dj";
const url = `${siteConfig.url}/use-cases/dj`;
// The day the page was first committed (`git log --diff-filter=A`). The
// date it last changed is `PAGE_UPDATED[path]` in `lib/seo-core.ts`.
const published = "2026-10-09";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/use-cases/dj"),
  openGraph: {
    type: "website",
    url,
    title: `${title} · edytlab`,
    description,
    siteName: siteConfig.name,
    images: [DEFAULT_OG_IMAGE],
  },
  twitter: {
    card: "summary_large_image",
    title: `${title} · edytlab`,
    description,
    images: [DEFAULT_OG_IMAGE.url],
  },
};

/** The first recording plays here; the others are a click away on their own pages. */
const [featured, ...others] = demos;
const moreRecordings = others.flatMap((d) => getVideo(d.slug) ?? []);

export default function DjUseCasePage() {
  return (
    <PageShell
      eyebrow="Use case"
      title="AI audio editor for DJs"
      lead="Describe a blend, a longer intro or a loudness target in plain English. An AI agent runs real audio tools on your files, on your computer, and you can A/B every take before you keep one."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("AI audio editor for DJs", path))} />
      <JsonLd data={articleJsonLd({ path, headline: title, description, datePublished: published })} />
      <section aria-labelledby="dj-demo" className="mt-12">
        <h2 id="dj-demo" className="text-2xl font-semibold">
          A real beatmatch, recorded
        </h2>
        <p className="mt-4 leading-relaxed text-foreground/90">
          A screen recording of the desktop app, not a mock-up. The DJ asks for a transition and the agent does the rest.
        </p>
        <div className="mt-6 grid gap-6">
          <DemoCard demo={featured} index={0} />
          <div className="grid gap-6 sm:grid-cols-2">
            {moreRecordings.map((video) => (
              <VideoLinkCard key={video.slug} video={video} />
            ))}
          </div>
        </div>
      </section>

      <Prose>
        <h2>What you can ask for</h2>
        <p>
          Each of these is a tool the agent can call; the <Link href="/docs/tools">tools reference</Link> lists them all.
        </p>
        <ul>
          <li>
            <strong>Beatmatching.</strong>{" "}
            It reads each track&apos;s tempo and beat grid, then time-stretches one to match the other without changing its
            pitch. For a track whose timing drifts, it can warp the audio so the beats land on a grid.
          </li>
          <li>
            <strong>Key matching.</strong>{" "}
            It reads each track&apos;s key and can shift pitch in semitones without changing the length. This is most reliable
            within about a fifth.
          </li>
          <li>
            <strong>Extended intros and edits.</strong>{" "}
            Copy a region, repeat it, paste it elsewhere, cut a range out or move a clip. Because the agent has the beat grid it
            can talk in bars rather than seconds, as in the recording above, where the incoming track starts 8 bars before the
            outgoing one ends.
          </li>
          <li>
            <strong>Transitions.</strong>{" "}
            Fades, crossfades, and low-pass or high-pass filters on a track over the overlap, plus EQ, reverb and echo.
          </li>
          <li>
            <strong>Loudness and delivery.</strong>{" "}
            It measures integrated loudness (EBU R128), compresses and limits, brings the mix to a target such as &minus;14
            LUFS, and exports WAV, FLAC or MP3.
          </li>
        </ul>

        <h2>Try the blend more than one way</h2>
        <p>
          Every edit is a node in a branching history. Fork the session, ask for the incoming track to start 4 bars earlier,
          A/B the two takes and keep the one you like. Nothing you try is lost, and undo steps back through it.
        </p>

        <h2>Your files stay on your computer</h2>
        <p>
          Tracks are never uploaded. Only the chat goes to the AI provider you pick, or nowhere at all if you run a local model
          with Ollama. The{" "}
          <Link href="/use-cases/local-ai-audio-editor">local-first AI audio editor</Link> page has the details.
        </p>

        <h2>What it does not do</h2>
        <ul>
          <li>It is a prep tool, not a live one. There are no decks, controllers or real-time mixing.</li>
          <li>
            It cannot split a track into stems or an acapella. Stem separation is not built yet.
          </li>
          <li>
            Beat and key detection are software estimates. A track with a drifting tempo or a long ambient intro can be off,
            and in the recording above the agent itself points out a start where the bars land a beat apart.
          </li>
          <li>
            Time-stretch and pitch-shift are clean on sustained material, but dense mixes can sound smeared, and the further
            the change is from 1.0 the more it shows.
          </li>
          <li>It needs an AI provider: your own key for a hosted one, or Ollama for a local model.</li>
        </ul>

        <h2>Get started</h2>
        <p>
          edytlab is free and open source (MIT) for macOS, Windows and Linux. Download it from the{" "}
          <Link href={siteConfig.releases}>releases page</Link>; the current builds are unsigned, so macOS and Windows warn on
          first launch, and the <Link href="/docs/getting-started">getting started guide</Link> has the steps. The{" "}
          <Link href="/docs/user-guide">user guide</Link> covers the other things the agent can do.
        </p>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
