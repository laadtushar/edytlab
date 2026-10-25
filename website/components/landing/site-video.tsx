/**
 * The pieces of `/videos` and `/videos/<slug>`.
 *
 * Google indexes a video only when it is the main content of its page,
 * and a page with several is told "Video is not the main content of the
 * page" for most of them. So each video, the three full demos included,
 * has a page of its own where it is the one thing on the screen
 * (`WatchPlayer`, with its `VideoObject`, which no other page carries),
 * and the hub, `/videos`, holds no `<video>` in its markup: poster cards
 * that link to those pages (`VideoLinkCard`), and a featured poster that
 * only becomes a player when it is pressed (`FeaturedVideo`).
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
import { cn } from "@/lib/utils";
import {
  type WatchVideo,
  cardTitle,
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

/**
 * A card for a video that has a page of its own: a poster with its
 * length, the title, one line about what happens and, in small type, how
 * it was made (`disclosure`).
 *
 * The whole card is the link, through the title (a stretched link), so a
 * screen reader hears the title and not the paragraph under it. The card
 * says nothing about what kind of video it is: the section it sits in
 * does that.
 *
 * - `compact` is for a grid of vertical clips, which are narrow on a
 *   phone: the one-line description waits for a wide screen.
 * - `row` lays a landscape card out as a poster beside its text from
 *   `md` up, for a section that holds one video.
 */
export function VideoLinkCard({
  video,
  compact = false,
  row = false,
  disclosure = true,
  className,
}: {
  video: WatchVideo;
  compact?: boolean;
  row?: boolean;
  /** Under the blurb. The hub says it once for a whole row of cards instead. */
  disclosure?: boolean;
  className?: string;
}) {
  return (
    <article
      className={cn(
        "group relative flex flex-col overflow-hidden rounded-xl border border-border/60 bg-card transition-colors focus-within:border-primary/60 hover:border-primary/40",
        row && "md:flex-row",
        className,
      )}
    >
      <div className={cn("relative bg-black", row && "md:w-3/5 md:shrink-0")}>
        {/* The poster is a frame of the video, laid out at the video's
            own size so the card does not move as it loads. The title
            names the card, so the picture is not read out again. */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={videoPosterSrc(video.slug)}
          alt=""
          width={video.width}
          height={video.height}
          loading="lazy"
          decoding="async"
          className="block h-auto w-full"
          style={{ aspectRatio: `${video.width} / ${video.height}` }}
        />
        <span
          aria-hidden
          className="absolute inset-0 flex items-center justify-center bg-black/10 transition-colors group-hover:bg-black/0"
        >
          <span className="flex size-12 items-center justify-center rounded-full bg-background/80 text-foreground ring-1 ring-border/60 backdrop-blur transition-transform group-hover:scale-110">
            <Play className="size-5 translate-x-px" />
          </span>
        </span>
        <span className="absolute bottom-2 right-2 rounded bg-black/75 px-1.5 py-0.5 text-xs font-medium tabular-nums text-white">
          <span className="sr-only">Length </span>
          {formatDuration(video.seconds)}
        </span>
      </div>
      <div className={cn("flex flex-1 flex-col gap-2 p-4 sm:p-5", row && "md:justify-center md:p-8")}>
        <h3 className={cn("text-balance text-base font-semibold leading-snug sm:text-lg", row && "md:text-2xl")}>
          <Link
            href={videoPath(video.slug)}
            className="outline-none after:absolute after:inset-0 after:content-[''] group-hover:text-primary"
          >
            {cardTitle(video)}
          </Link>
        </h3>
        <p
          className={cn(
            "text-pretty text-sm leading-relaxed text-muted-foreground",
            compact && "hidden lg:block",
            row && "md:text-base",
          )}
        >
          {video.blurb}
        </p>
        {disclosure ? (
          <p className={cn("mt-auto pt-1 text-xs leading-relaxed text-muted-foreground/80", row && "md:mt-2")}>{video.disclosure}</p>
        ) : null}
      </div>
    </article>
  );
}
