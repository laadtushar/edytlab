/**
 * The pieces of `/videos` and `/videos/<slug>`.
 *
 * Google indexes a video only when it is the main content of its page,
 * and a page with several is told "Video is not the main content of the
 * page" for most of them. So each video, the three full demos included,
 * has a page of its own where it is the one thing on the screen
 * (`WatchPlayer`, with its `VideoObject`, which no other page carries),
 * and the hub, `/videos`, holds no player at all: a grid of poster cards
 * that link to those pages (`VideoLinkCard`).
 *
 * The player is a `<video>` that waits to be played, rendered on the
 * server. Nothing starts on its own (which settles autoplay with sound
 * and reduced motion at once), `preload="none"` means the page fetches
 * only the poster until a press of play, and the box is reserved from
 * the file's own size, so the page does not move when the poster
 * arrives.
 */

import Link from "next/link";
import { Play } from "lucide-react";

import { JsonLd } from "@/components/json-ld";
import { videoObjectJsonLd } from "@/lib/seo";
import {
  type WatchVideo,
  formatDuration,
  videoCaptionsSrc,
  videoPath,
  videoPosterSrc,
  videoSrc,
} from "@/lib/videos";

/**
 * The one player on a watch page, with its `VideoObject` beside it. A
 * reel is held to a phone-sized width and centred on a black ground, so
 * a 1080 by 1920 picture does not fill a laptop screen from top to
 * bottom; the launch video takes the width of the column.
 */
export function WatchPlayer({ video }: { video: WatchVideo }) {
  const titleId = `video-${video.slug}-title`;
  const captionId = `video-${video.slug}-caption`;
  const src = videoSrc(video.slug);
  return (
    <figure className="overflow-hidden rounded-xl border border-border/60 bg-black">
      <JsonLd
        data={videoObjectJsonLd(video, { video: src, poster: videoPosterSrc(video.slug) })}
      />
      <div className={video.kind === "reel" ? "mx-auto w-full max-w-[22rem]" : undefined}>
        <video
          controls
          playsInline
          preload="none"
          poster={videoPosterSrc(video.slug)}
          width={video.width}
          height={video.height}
          aria-labelledby={titleId}
          aria-describedby={captionId}
          className="block h-auto w-full max-w-full object-contain"
          // The box is reserved from the file's own size, so the page
          // does not move when the poster lands.
          style={{ aspectRatio: `${video.width} / ${video.height}` }}
        >
          <source src={src} type="video/mp4" />
          {video.captionsSource ? (
            // Offered, not switched on: the video's own words are burned
            // in, and the captions say the same thing for a screen reader
            // and for anyone who wants them as text.
            <track kind="captions" src={videoCaptionsSrc(video.slug)} srcLang="en" label="English" />
          ) : null}
          <p className="p-4 text-sm text-muted-foreground">
            This browser cannot play the video.{" "}
            <a href={src} className="text-primary underline underline-offset-4">
              Download the MP4
            </a>{" "}
            instead.
          </p>
        </video>
      </div>
      {/* The player's name and description for assistive technology; the
          page shows both in its own text. */}
      <span id={titleId} hidden>
        {video.title}
      </span>
      <span id={captionId} hidden>
        {video.description}
      </span>
    </figure>
  );
}

/** A poster, a title and a line: the card both kinds of link share. */
function PosterCard({
  href,
  poster,
  width,
  height,
  label,
  seconds,
  title,
  blurb,
}: {
  href: string;
  poster: string;
  width: number;
  height: number;
  label: string;
  seconds: number;
  title: string;
  blurb: string;
}) {
  const vertical = height > width;
  return (
    <Link
      href={href}
      className="group flex h-full flex-col overflow-hidden rounded-xl border border-border/60 bg-card transition-colors hover:border-primary/40"
    >
      <div className="relative bg-black">
        <div className={vertical ? "mx-auto w-full max-w-[14rem]" : undefined}>
          {/* The poster is a frame of the video, laid out at the video's
              own size so the card does not move as it loads. The link
              text names the card, so the picture is not read out again. */}
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={poster}
            alt=""
            width={width}
            height={height}
            loading="lazy"
            decoding="async"
            className="block h-auto w-full"
            style={{ aspectRatio: `${width} / ${height}` }}
          />
        </div>
        <span
          aria-hidden
          className="absolute inset-0 flex items-center justify-center bg-black/10 transition-colors group-hover:bg-black/0"
        >
          <span className="flex size-12 items-center justify-center rounded-full bg-background/80 text-foreground ring-1 ring-border/60 backdrop-blur transition-transform group-hover:scale-110">
            <Play className="size-5 translate-x-px" />
          </span>
        </span>
      </div>
      <div className="flex flex-1 flex-col gap-2 border-t border-border/60 p-5">
        <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
          <span aria-hidden className="h-2 w-2 rounded-full bg-primary/60" />
          <span>{label}</span>
          <span className="tabular-nums">· {formatDuration(seconds)}</span>
        </div>
        <h3 className="text-balance text-lg font-semibold group-hover:text-primary">{title}</h3>
        <p className="text-pretty text-sm leading-relaxed text-muted-foreground">{blurb}</p>
      </div>
    </Link>
  );
}

/** A card for a video that has a page of its own. */
export function VideoLinkCard({ video, label }: { video: WatchVideo; label: string }) {
  return (
    <PosterCard
      href={videoPath(video.slug)}
      poster={videoPosterSrc(video.slug)}
      width={video.width}
      height={video.height}
      label={label}
      seconds={video.seconds}
      title={video.title}
      blurb={video.description}
    />
  );
}
