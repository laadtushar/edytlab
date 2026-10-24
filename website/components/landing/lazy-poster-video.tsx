"use client";

import { type VideoHTMLAttributes, useEffect, useRef, useState } from "react";

/**
 * A `<video>` that fetches its poster when it is about to scroll into
 * view, not with the page.
 *
 * `poster` is the one image a browser cannot be told to load lazily
 * (`loading="lazy"` is for `<img>` and `<iframe>`), so a page with three
 * demo videos asked for three posters, about 320 KB, as the first thing
 * it did, at the same priority as its scripts. They sit far below the
 * fold. On a slow phone connection they queue with the JavaScript that
 * makes the page interactive: Lighthouse's simulated LCP drops from 5.0 s
 * to 3.1 s when they stop doing that.
 *
 * The server's markup has no `poster`; once the video is within
 * `ROOT_MARGIN` of the viewport the attribute is set and the browser
 * fetches it, well before the reader gets there. The box is already
 * reserved from the frame's aspect ratio (`DemoCard`), so nothing moves
 * when the poster lands. Where there is no `IntersectionObserver` it is
 * set at once. Search engines get the thumbnail from the video's
 * structured data (`videoObjectJsonLd`), which names the same file.
 */
const ROOT_MARGIN = "800px 0px";

export function LazyPosterVideo({
  poster,
  ...props
}: Omit<VideoHTMLAttributes<HTMLVideoElement>, "poster"> & { poster: string }) {
  const ref = useRef<HTMLVideoElement>(null);
  const [near, setNear] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- a browser without IntersectionObserver is only known after hydration; the poster is set at once, as documented above
      setNear(true);
      return;
    }
    const watch = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setNear(true);
          watch.disconnect();
        }
      },
      { rootMargin: ROOT_MARGIN },
    );
    watch.observe(el);
    return () => watch.disconnect();
  }, []);

  return <video ref={ref} poster={near ? poster : undefined} {...props} />;
}
