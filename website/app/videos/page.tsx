import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell } from "@/components/landing/page-shell";
import { DemoLinkCard, VideoLinkCard } from "@/components/landing/site-video";
import { Reveal, Stagger } from "@/components/motion";
import { demos } from "@/lib/demos";
import {
  breadcrumbJsonLd,
  pageAlternates,
  pageBreadcrumbs,
  pageSocial,
  videosHubJsonLd,
} from "@/lib/seo";
import { siteConfig } from "@/lib/site";
import { launchVideo, reels } from "@/lib/videos";

const title = "Videos: the launch video, demos and short reels";
const description =
  "Watch the edytlab desktop app do real DJ jobs: the launch video, three full demo recordings and five short vertical reels. Open source and local-first.";
const path = "/videos";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/videos"),
  ...pageSocial("/videos", `${title} · edytlab`, description),
};

/**
 * The hub. It holds no player: each video it lists has a page of its own
 * where it is the main content, which is what Google needs to index it,
 * and the three full demos are linked to the pages where they already
 * play. The page's structured data is a `CollectionPage` listing those
 * pages, not a `VideoObject` apiece.
 */
export default function VideosPage() {
  return (
    <PageShell
      wide
      eyebrow="Videos"
      title="edytlab at work"
      lead="Screen recordings of the desktop app, with an AI agent doing DJ jobs from a typed request to an exported file: the launch video, three full demos, and short vertical reels cut from them. Each video has a page of its own. Nothing plays until you press play."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("Videos", path))} />
      <JsonLd data={videosHubJsonLd()} />

      <section aria-labelledby="videos-launch" className="mt-12">
        <Reveal>
          <h2 id="videos-launch" className="text-2xl font-semibold">
            The launch video
          </h2>
          <p className="mt-3 max-w-3xl leading-relaxed text-foreground/90">
            About a minute, with captions you can switch on. The app footage in
            it comes from the recordings below, not from a mock-up.
          </p>
        </Reveal>
        <div className="mt-6 max-w-xl">
          <VideoLinkCard video={launchVideo} label="Launch video" />
        </div>
      </section>

      <section aria-labelledby="videos-demos" className="mt-16">
        <Reveal>
          <h2 id="videos-demos" className="text-2xl font-semibold">
            Three full demos
          </h2>
          <p className="mt-3 max-w-3xl leading-relaxed text-foreground/90">
            Each is one job, from the request to an exported file, played in
            real time with sound. Waits for the agent are sped up. The tracks
            are short synthetic loops. They play on the{" "}
            <Link href="/use-cases/dj" className="text-primary underline underline-offset-4">
              AI audio editor for DJs
            </Link>{" "}
            page, which also says what edytlab can and cannot do for a DJ, and
            in the{" "}
            <Link href="/#demos" className="text-primary underline underline-offset-4">
              demos section of the home page
            </Link>
            .
          </p>
        </Reveal>
        <Stagger className="mt-6 grid gap-5 sm:grid-cols-2 lg:grid-cols-3" each={0.08} distance={24}>
          {demos.map((demo, i) => (
            <DemoLinkCard key={demo.slug} demo={demo} index={i} />
          ))}
        </Stagger>
      </section>

      <section aria-labelledby="videos-reels" className="mt-16">
        <Reveal>
          <h2 id="videos-reels" className="text-2xl font-semibold">
            Short reels
          </h2>
          <p className="mt-3 max-w-3xl leading-relaxed text-foreground/90">
            Vertical cuts of the demos, with the app&apos;s own sound and no
            voice-over, in the shape a phone plays them.
          </p>
        </Reveal>
        <Stagger className="mt-6 grid gap-5 sm:grid-cols-2 lg:grid-cols-3" each={0.08} distance={24}>
          {reels.map((video, i) => (
            <VideoLinkCard key={video.slug} video={video} label={`Reel ${String(i + 1).padStart(2, "0")}`} />
          ))}
        </Stagger>
      </section>

      <p className="mt-16 max-w-3xl leading-relaxed text-foreground/90">
        edytlab is free and open source (MIT) for macOS, Windows and Linux. Stem
        separation and transcription are not built yet, and the current builds
        are unsigned, so macOS and Windows warn on first launch;{" "}
        <Link href="/docs/getting-started" className="text-primary underline underline-offset-4">
          the setup guide
        </Link>{" "}
        has the steps. Download it from the{" "}
        <Link href={siteConfig.releases} className="text-primary underline underline-offset-4">
          releases page
        </Link>
        .
      </p>
      <PageLinks />
    </PageShell>
  );
}
