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
const EYEBROW = { launch: "Introduction", release: "Latest release", reel: "Short clip", demo: "Full walkthrough" } as const;

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
      <div className={vertical ? "mt-10 lg:grid lg:grid-cols-[22rem_minmax(0,1fr)] lg:items-start lg:gap-14" : "mt-10"}>
        <div className={vertical ? "mx-auto w-full max-w-[22rem] lg:sticky lg:top-24 lg:mx-0" : undefined}>
          <WatchPlayer video={video} />
          <p className="mt-3 text-xs leading-relaxed text-muted-foreground/80">
            {formatDuration(video.seconds)}
            {video.captionsSource ? " · Captions available" : ""} · {video.disclosure}
          </p>
        </div>

        <div className={vertical ? "mt-14 space-y-14 lg:mt-0" : "mt-14 max-w-3xl space-y-14"}>
          <section aria-labelledby="video-shows">
            <h2 id="video-shows" className="text-2xl font-semibold tracking-tight">
              About this video
            </h2>
            <p className="mt-4 leading-relaxed text-foreground/90">{forViewers(video.caption)}</p>
          </section>

          <section aria-labelledby="video-happens">
            <h2 id="video-happens" className="text-2xl font-semibold tracking-tight">
              What you will see
            </h2>
            <p className="mt-4 leading-relaxed text-muted-foreground">Each line is a moment, with the time it starts.</p>
            <ol className="mt-5 divide-y divide-border/40 rounded-xl border border-border/60 bg-card">
              {video.moments.map((m) => (
                <li key={m.at} className="flex gap-4 px-4 py-3 text-sm leading-relaxed sm:px-5">
                  <span className="w-10 shrink-0 font-mono tabular-nums text-muted-foreground">{formatMoment(m.at)}</span>
                  <span className="text-foreground/90">{forViewers(m.text)}</span>
                </li>
              ))}
            </ol>
          </section>
        </div>
      </div>

      <section aria-labelledby="video-more" className="mt-20">
        <h2 id="video-more" className="text-2xl font-semibold tracking-tight sm:text-3xl">
          More to watch
        </h2>
        <div className={vertical ? "mt-8 grid grid-cols-2 gap-4 lg:grid-cols-4" : "mt-8 grid gap-5 md:grid-cols-3"}>
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
