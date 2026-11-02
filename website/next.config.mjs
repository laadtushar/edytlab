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

/**
 * Videos the site no longer serves: the v0.7.0 release video and its
 * three reels, taken down once v0.9.1 shipped, because a page of what an
 * old release changed reads as stale. Their sources stay under
 * `marketing/promo-v0.7.0/out/`. Search engines, the sitemap Google
 * already read and posts elsewhere still link to their pages and files,
 * so each of those moves permanently (308) to the hub rather than
 * answering 404. `lib/redirects.test.ts` pins the list.
 */
const REMOVED_VIDEOS = [
  { slug: "whats-new-v070", files: ["mp4", "jpg", "vtt"] },
  { slug: "reel-v070-whats-new", files: ["mp4", "jpg"] },
  { slug: "reel-v070-describe-the-edit", files: ["mp4", "jpg"] },
  { slug: "reel-v070-200-tracks", files: ["mp4", "jpg"] },
];

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  poweredByHeader: false,
  turbopack: {
    resolveAlias: {
      // Next puts `next/dist/build/polyfills/polyfill-module.js` in every
      // page's entry chunk, whatever the browsers it targets: feature-
      // detected stand-ins for `Array.prototype.at`, `flat`, `flatMap`,
      // `Object.fromEntries`, `Object.hasOwn`, `trimStart`, `trimEnd`,
      // `Symbol.prototype.description`, `Promise.prototype.finally` and
      // `URL.canParse`. Every one but the last is in Chrome 111, Edge 111,
      // Firefox 111 and Safari 16.4, the browsers Next documents as its
      // default targets, and nothing the site ships calls `URL.canParse`,
      // so for those browsers the file is dead weight, and Lighthouse's
      // "legacy JavaScript" audit lists it (13 KiB of the chunk). The
      // import is relative (`../build/polyfills/polyfill-module`, in
      // `next/dist/client/app-globals.js`), so that is the key; the value
      // is an empty module. `lib/scriptCost.test.ts` fails if Next stops
      // importing the file under that name, or polyfills something new.
      "../build/polyfills/polyfill-module": "./lib/no-polyfills.js",
    },
  },
  images: {
    remotePatterns: [],
  },
  async redirects() {
    return REMOVED_VIDEOS.flatMap(({ slug, files }) =>
      [`/videos/${slug}`, ...files.map((ext) => `/videos/${slug}.${ext}`)].map((source) => ({
        source,
        destination: "/videos",
        permanent: true,
      })),
    );
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
