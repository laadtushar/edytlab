import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell } from "@/components/landing/page-shell";
import { VideoLinkCard } from "@/components/landing/site-video";
import { Reveal, Stagger } from "@/components/motion";
import {
  breadcrumbJsonLd,
  pageAlternates,
  pageBreadcrumbs,
  pageSocial,
  videosHubJsonLd,
} from "@/lib/seo";
import { siteConfig } from "@/lib/site";
import { demoVideos, launchVideo, releaseVideo, reels } from "@/lib/videos";

const title = "Videos: what's new, the launch video, demos and reels";
const description =
  "Watch the edytlab desktop app at work: what's new in v0.7.0, the launch video, three full DJ demos and short vertical reels. Open source and local-first.";
const path = "/videos";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/videos"),
  ...pageSocial("/videos", `${title} · edytlab`, description),
};

/**
 * The hub. It holds no player: each video it lists, the three full demos
 * included, has a page of its own where it is the main content, which is
 * what Google needs to index it. The page's structured data is a
 * `CollectionPage` listing those pages, not a `VideoObject` apiece.
 */
export default function VideosPage() {
  return (
    <PageShell
      wide
      eyebrow="Videos"
      title="edytlab at work"
      lead="Videos of the desktop app: what's new in v0.7.0, the launch video, three full DJ demos recorded start to finish, and short vertical reels. Each video has a page of its own. Nothing plays until you press play."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("Videos", path))} />
      <JsonLd data={videosHubJsonLd()} />

      <section aria-labelledby="videos-release" className="mt-12">
        <Reveal>
          <h2 id="videos-release" className="text-2xl font-semibold">
            What&apos;s new in v0.7.0
          </h2>
          <p className="mt-3 max-w-3xl leading-relaxed text-foreground/90">
            Under a minute, with captions you can switch on. Every app shot is
            the real v0.7.0 frontend with a scripted assistant, and the video
            says so on each one.
          </p>
        </Reveal>
        <div className="mt-6 max-w-xl">
          <VideoLinkCard video={releaseVideo} label="v0.7.0" />
        </div>
      </section>

      <section aria-labelledby="videos-launch" className="mt-16">
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
            are short synthetic loops. Each has a page of its own, and all
            three also play on the{" "}
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
          {demoVideos.map((video, i) => (
            <VideoLinkCard key={video.slug} video={video} label={`Demo ${String(i + 1).padStart(2, "0")}`} />
          ))}
        </Stagger>
      </section>

      <section aria-labelledby="videos-reels" className="mt-16">
        <Reveal>
          <h2 id="videos-reels" className="text-2xl font-semibold">
            Short reels
          </h2>
          <p className="mt-3 max-w-3xl leading-relaxed text-foreground/90">
            Vertical videos, in the shape a phone plays them, with no
            voice-over: three about v0.7.0 (real app, scripted assistant), and
            five cut from the demos with the app&apos;s own sound.
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
