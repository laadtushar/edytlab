import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";

import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell } from "@/components/landing/page-shell";
import { WatchPlayer } from "@/components/landing/site-video";
import {
  breadcrumbJsonLd,
  pageAlternates,
  pageSocial,
  videoBreadcrumbs,
} from "@/lib/seo";
import { siteConfig } from "@/lib/site";
import {
  formatDuration,
  formatMoment,
  getVideo,
  videoPath,
  videos,
} from "@/lib/videos";

interface Props {
  params: Promise<{ slug: string }>;
}

export async function generateStaticParams() {
  return videos.map((v) => ({ slug: v.slug }));
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
 * beside the player (`WatchPlayer`).
 */
export default async function VideoPage({ params }: Props) {
  const { slug } = await params;
  const video = getVideo(slug);
  if (!video) notFound();

  const others = videos.filter((v) => v.slug !== video.slug);

  return (
    <PageShell
      eyebrow={video.kind === "launch" ? "Launch video" : "Short reel"}
      title={video.title}
      lead={video.description}
    >
      <JsonLd data={breadcrumbJsonLd(videoBreadcrumbs(video))} />

      <div className="mt-10">
        <WatchPlayer video={video} />
        <p className="mt-3 text-sm text-muted-foreground">
          {formatDuration(video.seconds)}
          {video.kind === "reel" ? ", vertical, with the app's own sound and no voice-over" : ", with captions you can switch on"}
          . Nothing plays until you press play.
        </p>
      </div>

      <section aria-labelledby="video-shows" className="mt-12">
        <h2 id="video-shows" className="text-2xl font-semibold">
          What it shows
        </h2>
        <p className="mt-4 leading-relaxed text-foreground/90">{video.caption}</p>
      </section>

      <section aria-labelledby="video-happens" className="mt-12">
        <h2 id="video-happens" className="text-2xl font-semibold">
          What happens, in order
        </h2>
        <p className="mt-4 leading-relaxed text-foreground/90">
          There is no voice-over. Each line is a moment in the video, from the
          second it starts.
        </p>
        <ol className="mt-4 divide-y divide-border/40 rounded-xl border border-border/60">
          {video.moments.map((m) => (
            <li key={m.at} className="flex gap-4 px-4 py-3 text-sm leading-relaxed sm:px-5">
              <span className="w-10 shrink-0 font-mono tabular-nums text-muted-foreground">
                {formatMoment(m.at)}
              </span>
              <span className="text-foreground/90">{m.text}</span>
            </li>
          ))}
        </ol>
      </section>

      <section aria-labelledby="video-more" className="mt-12">
        <h2 id="video-more" className="text-2xl font-semibold">
          More videos
        </h2>
        <ul className="mt-4 list-disc space-y-1 pl-6">
          {others.map((v) => (
            <li key={v.slug}>
              <Link href={videoPath(v.slug)} className="text-primary underline underline-offset-4">
                {v.title}
              </Link>
            </li>
          ))}
          <li>
            <Link href="/use-cases/dj" className="text-primary underline underline-offset-4">
              The three full demos, on the AI audio editor for DJs page
            </Link>
          </li>
        </ul>
        <p className="mt-4">
          <Link href="/videos" className="text-primary underline underline-offset-4">
            All videos
          </Link>
        </p>
      </section>

      <p className="mt-12 max-w-3xl leading-relaxed text-foreground/90">
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
