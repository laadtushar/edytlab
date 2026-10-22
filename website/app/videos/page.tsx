import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { FeaturedVideo } from "@/components/landing/featured-video";
import { PageShell } from "@/components/landing/page-shell";
import { VideoLinkCard } from "@/components/landing/site-video";
import { VideoActions, VideoSection, VideosCta } from "@/components/landing/videos-sections";
import {
  breadcrumbJsonLd,
  pageAlternates,
  pageBreadcrumbs,
  pageSocial,
  videosHubJsonLd,
} from "@/lib/seo";
import {
  demoVideos,
  formatDuration,
  launchVideo,
  releaseVideo,
  reels,
  videoCaptionsSrc,
  videoPath,
  videoPosterSrc,
  videoSrc,
} from "@/lib/videos";

const title = "Videos: watch an AI audio editor at work";
const description =
  "Watch edytlab, an open-source AI audio editor, beatmatch two tracks, extend an intro and build a mix from plain-English requests.";
const path = "/videos";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/videos"),
  ...pageSocial("/videos", `${title} · edytlab`, description),
};

/**
 * The hub: a pitch, one featured video, then the rest in sections, and a
 * way to download. Every card links to the video's own page, where it is
 * the main content, which is what Google needs to index it. The page's
 * structured data is a `CollectionPage` listing those pages, not a
 * `VideoObject` apiece, and its markup holds no `<video>`: the featured
 * poster becomes a player only when pressed (`FeaturedVideo`).
 *
 * All of the words under a video's name come from `lib/videos.ts`
 * (`blurb`, `disclosure`), so a re-recording changes data, not this page.
 */
export default function VideosPage() {
  const featured = launchVideo;
  return (
    <PageShell
      wide
      eyebrow="Videos"
      title="Watch edytlab edit audio from a sentence"
      lead="Ask for a blend, a fade or a loudness target in plain English and see the agent do the work in the real desktop app, from the request to the exported file."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("Videos", path))} />
      <JsonLd data={videosHubJsonLd()} />

      <VideoActions className="mt-8" />

      <section aria-labelledby="videos-featured" className="mt-12">
        <FeaturedVideo
          href={videoPath(featured.slug)}
          title={featured.title}
          src={videoSrc(featured.slug)}
          poster={videoPosterSrc(featured.slug)}
          captions={featured.captionsSource ? videoCaptionsSrc(featured.slug) : undefined}
          width={featured.width}
          height={featured.height}
        />
        <div className="mt-5 max-w-3xl">
          <h2 id="videos-featured" className="text-balance text-xl font-semibold sm:text-2xl">
            <Link href={videoPath(featured.slug)} className="hover:text-primary">
              {featured.title}
            </Link>
          </h2>
          <p className="mt-2 leading-relaxed text-foreground/90">{featured.blurb}</p>
          <p className="mt-2 text-xs leading-relaxed text-muted-foreground/80">
            {formatDuration(featured.seconds)} · {featured.disclosure}
          </p>
        </div>
      </section>

      <VideoSection
        id="videos-walkthroughs"
        title="Full walkthroughs"
        lead={
          <>
            Three complete DJ jobs, from the request to an exported file. What edytlab does and does not do for a DJ is on the{" "}
            <Link href="/use-cases/dj" className="text-primary underline underline-offset-4">
              AI audio editor for DJs
            </Link>{" "}
            page.
          </>
        }
      >
        <div className="grid gap-5 md:grid-cols-3">
          {demoVideos.map((video) => (
            <VideoLinkCard key={video.slug} video={video} />
          ))}
        </div>
      </VideoSection>

      <VideoSection id="videos-latest" title="The latest release" lead="The one-minute overview of what changed.">
        <VideoLinkCard video={releaseVideo} row />
      </VideoSection>

      <VideoSection id="videos-clips" title="Short clips" lead="Vertical videos for your phone, each about one idea.">
        {/* Flex, not a grid, so the last row is centred whatever the count:
            two across on a phone, four from `lg`. */}
        <div className="flex flex-wrap justify-center gap-4">
          {reels.map((video) => (
            <VideoLinkCard
              key={video.slug}
              video={video}
              compact
              className="w-[calc((100%-1rem)/2)] lg:w-[calc((100%-3rem)/4)]"
            />
          ))}
        </div>
      </VideoSection>

      <VideosCta />
    </PageShell>
  );
}
