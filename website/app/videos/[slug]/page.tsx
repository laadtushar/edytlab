import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import { JsonLd } from "@/components/json-ld";
import { PageShell } from "@/components/landing/page-shell";
import { VideoLinkCard, WatchPlayer } from "@/components/landing/site-video";
import { VideosCta } from "@/components/landing/videos-sections";
import {
  breadcrumbJsonLd,
  pageAlternates,
  pageSocial,
  videoBreadcrumbs,
} from "@/lib/seo";
import {
  cardTitle,
  forViewers,
  formatDuration,
  formatMoment,
  getVideo,
  relatedVideos,
  videoPath,
  watchVideos,
} from "@/lib/videos";

/** The small label above the title: what sort of video this is, in a visitor's words. */
const EYEBROW = { launch: "Introduction", reel: "Short clip", demo: "Full walkthrough" } as const;

interface Props {
  params: Promise<{ slug: string }>;
}

export async function generateStaticParams() {
  return watchVideos.map((v) => ({ slug: v.slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const video = getVideo(slug);
  if (!video) return {};
  return {
    title: video.title,
    description: video.description,
    alternates: pageAlternates(videoPath(video.slug)),
    ...pageSocial(videoPath(video.slug), `${video.title} · edytlab`, video.description),
  };
}

/**
 * One video's page. The video is the main content: it comes straight
 * after the heading, ahead of everything else, and it is the only video
 * here, which is what Google needs to index it. Its `VideoObject` sits
 * beside the player (`WatchPlayer`). The related videos below are cards
 * that link to their own pages, not players.
 *
 * What the page says about the video comes from `lib/videos.ts`: the
 * `blurb` under the title, the `disclosure` under the player, the
 * `caption` and the `moments`.
 */
export default async function VideoPage({ params }: Props) {
  const { slug } = await params;
  const video = getVideo(slug);
  if (!video) notFound();

  const related = relatedVideos(video);
  const vertical = video.height > video.width;

  return (
    <PageShell wide eyebrow={EYEBROW[video.kind]} title={cardTitle(video)} lead={video.blurb}>
      <JsonLd data={breadcrumbJsonLd(videoBreadcrumbs(video))} />

      {/* A vertical video sits beside its words from `lg` up, so the page is not a phone-wide
          strip in a wide black box; a landscape one is full width, with its words below. */}
      <div className={vertical ? "mt-8 lg:grid lg:grid-cols-[22rem_minmax(0,1fr)] lg:items-start lg:gap-14" : "mt-8"}>
        <div className={vertical ? "mx-auto w-full max-w-[22rem] lg:sticky lg:top-24 lg:mx-0" : undefined}>
          <WatchPlayer video={video} />
          <p className="mt-3 text-xs leading-relaxed text-muted-foreground/80">
            {formatDuration(video.seconds)}
            {video.captionsSource ? " · Captions available" : ""} · {video.disclosure}
          </p>
        </div>

        <div className={vertical ? "mt-8 space-y-6 lg:mt-0" : "mt-8 max-w-3xl space-y-6"}>
          <p className="leading-relaxed text-foreground/90">{forViewers(video.caption)}</p>

          {/* Collapsed, not left out: the list is in the page's HTML for a
              reader who opens it and for a search engine, and the video's
              structured data does not depend on it. */}
          <details className="rounded-xl border border-border/60 bg-card">
            <summary className="cursor-pointer select-none px-4 py-3 text-sm font-medium sm:px-5">
              What you will see
              <span className="font-normal text-muted-foreground"> · {video.moments.length} moments</span>
            </summary>
            <ol className="divide-y divide-border/40 border-t border-border/60">
              {video.moments.map((m) => (
                <li key={m.at} className="flex gap-4 px-4 py-3 text-sm leading-relaxed sm:px-5">
                  <span className="w-10 shrink-0 font-mono tabular-nums text-muted-foreground">{formatMoment(m.at)}</span>
                  <span className="text-foreground/90">{forViewers(m.text)}</span>
                </li>
              ))}
            </ol>
          </details>
        </div>
      </div>

      <section aria-labelledby="video-more" className="mt-14">
        <h2 id="video-more" className="text-xl font-semibold tracking-tight sm:text-2xl">
          More to watch
        </h2>
        <div className={vertical ? "mt-6 grid grid-cols-2 gap-4 lg:grid-cols-4" : "mt-6 grid gap-4 md:grid-cols-3 md:gap-5"}>
          {related.map((v) => (
            <VideoLinkCard key={v.slug} video={v} compact={vertical} />
          ))}
        </div>
        <p className="mt-6 text-sm text-muted-foreground">
          <Link href="/videos" className="text-primary underline underline-offset-4">
            All videos
          </Link>
          {" · "}
          <Link href="/use-cases/dj" className="text-primary underline underline-offset-4">
            AI audio editor for DJs
          </Link>
        </p>
      </section>

      <VideosCta />
    </PageShell>
  );
}
