"use client";

import { Play } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";

/**
 * The hub's one featured video, at the width of the page.
 *
 * It starts as a link to the video's own page, with the poster as its
 * picture, so the markup the server sends (and a crawler reads) holds no
 * `<video>`: Google indexes a video on the page where it is the main
 * content, and that page is `/videos/<slug>`. Pressing it swaps the
 * poster for a player and starts the video, which is playing on request
 * and nothing else; with scripts off, or a modified click (new tab), it
 * is the link it was.
 *
 * The demo cards on the home page and the DJ page use it too (`lazy`),
 * for the same reason: a `<video>` there is one Google finds on a page
 * where it is not the main content, and reports as "Video isn't on a
 * watch page". Their posters are below the fold, so `lazy` leaves the
 * fetch to the browser's lazy loading instead of asking for it at once.
 *
 * The props are resolved paths, not a `WatchVideo`, so the client bundle
 * carries none of the video list.
 */
export function FeaturedVideo({
  href,
  title,
  src,
  poster,
  captions,
  width,
  height,
  lazy = false,
  frame = "rounded-2xl border border-border/60",
}: {
  href: string;
  title: string;
  src: string;
  poster: string;
  /** A WebVTT file, offered as a captions track and not switched on. */
  captions?: string;
  width: number;
  height: number;
  /** Below the fold: the poster loads lazily, not at high priority. */
  lazy?: boolean;
  /** The box's corners and border; a card that draws its own passes "". */
  frame?: string;
}) {
  const [playing, setPlaying] = useState(false);
  const player = useRef<HTMLVideoElement>(null);
  // The poster is gone once it is pressed, so focus goes to the player.
  useEffect(() => {
    if (playing) player.current?.focus();
  }, [playing]);

  const box = `relative block w-full overflow-hidden bg-black ${frame}`;
  const ratio = { aspectRatio: `${width} / ${height}` };

  if (playing) {
    return (
      <div className={box} style={ratio}>
        <video
          ref={player}
          controls
          autoPlay
          playsInline
          poster={poster}
          aria-label={title}
          className="block h-full w-full object-contain"
        >
          <source src={src} type="video/mp4" />
          {captions ? <track kind="captions" src={captions} srcLang="en" label="English" /> : null}
        </video>
      </div>
    );
  }

  const press = (event: MouseEvent<HTMLAnchorElement>) => {
    // Let a new-tab or new-window click do what it says.
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    setPlaying(true);
  };

  return (
    <a
      href={href}
      onClick={press}
      aria-label={`Play the video: ${title}`}
      className={`${box} group focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background`}
      style={ratio}
    >
      {/* On the hub, the first thing in view, so it is fetched at once;
          its box is reserved from the file's own size. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={poster}
        alt=""
        width={width}
        height={height}
        decoding="async"
        // `fetchpriority` in lower case: React 18 does not know the
        // camel-case prop and warns, though the browser reads either.
        {...(lazy ? { loading: "lazy" as const } : { fetchpriority: "high" })}
        className="block h-full w-full object-cover"
      />
      <span aria-hidden className="absolute inset-0 flex items-center justify-center bg-black/20 transition-colors group-hover:bg-black/10">
        <span className="flex size-16 items-center justify-center rounded-full bg-background/85 text-foreground shadow-lg ring-1 ring-border/60 backdrop-blur transition-transform group-hover:scale-110 sm:size-20">
          <Play className="size-7 translate-x-0.5 sm:size-8" />
        </span>
      </span>
    </a>
  );
}
