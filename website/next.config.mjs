/**
 * How long a browser or a CDN may keep the files in `public/`.
 *
 * Next serves them with `Cache-Control: public, max-age=0`, so every
 * visit asks for each logo, poster and demo video again (Lighthouse's
 * "uses long cache lifetimes"). They are not fingerprinted either: a
 * demo that is re-recorded keeps its name (`/demos/<slug>.mp4`), so
 * `immutable` and a year would leave the old recording on every machine
 * that had seen it. A day is the compromise: a repeat visitor does not
 * download a 20 MB video again, a re-recorded one reaches everybody
 * within a day, and for the following week a cache may serve the old
 * file while it fetches the new one (`stale-while-revalidate`).
 *
 * `/_next/static/*` is not here: Next fingerprints those and already
 * marks them immutable. Pages, the feed and the sitemap are not here
 * either; they keep Next's own policy. `lib/pageExperience.test.ts`
 * checks every file under `public/` against these rules.
 */
const MEDIA_CACHE_CONTROL = "public, max-age=86400, stale-while-revalidate=604800";
const cacheMedia = [{ key: "Cache-Control", value: MEDIA_CACHE_CONTROL }];

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  poweredByHeader: false,
  images: {
    remotePatterns: [],
  },
  async headers() {
    return [
      // The folder the demos are served from, with whatever it holds:
      // posters, captions, the video itself. There is no page under it.
      { source: "/demos/:path*", headers: cacheMedia },
      // The folder the launch video and the reels are served from, by
      // extension. `/videos` is also a page (the hub), and `/videos/<slug>`
      // is a page for each video, so a rule for everything under the
      // folder would give those pages a day in the browser's cache and
      // leave a corrected caption stale for that long. Pages keep Next's
      // own policy.
      {
        source: "/videos/:file(.+\\.(?:png|jpe?g|gif|webp|avif|svg|mp4|webm|vtt))",
        headers: cacheMedia,
      },
      // The rest of `public/`: logos and icons, by extension. Not the two
      // folders above (they have their rule) and never anything under
      // `/_next/`.
      {
        source: "/:file((?!_next/|demos/|videos/).+\\.(?:png|jpe?g|gif|webp|avif|svg|ico|mp4|webm|vtt))",
        headers: cacheMedia,
      },
    ];
  },
};

export default nextConfig;
