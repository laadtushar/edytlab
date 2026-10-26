/**
 * Screen recordings of the desktop app, played from `public/demos/`.
 *
 * The sections around this one are drawn: they show what the editor
 * does without a screenshot that ages. This one is the evidence — the
 * real app, with Claude as the agent, doing a whole job. The list lives
 * in `lib/demos.ts`; this file only lays it out.
 *
 * The structured data for each video is not here: it is on the video's
 * own page (`/videos/<slug>`), which each card links to, because Google
 * indexes a video only on a page where it is the main content.
 *
 * Nothing plays on its own. Each card is its poster, a link to the
 * video's page, until play is pressed (`FeaturedVideo`), which settles
 * autoplay-with-sound and reduced motion in one go. So the markup holds
 * no `<video>` for Google to report as "not on a watch page", and the
 * page fetches only the posters, lazily, until then.
 */

import Link from "next/link";
import { ArrowRight } from "lucide-react";

import { FeaturedVideo } from "@/components/landing/featured-video";
import { Reveal, Stagger } from "@/components/motion";
import { DEMO_FRAME, type Demo, demoPosterSrc, demoVideoSrc, demos } from "@/lib/demos";
import { getVideo, videoPath } from "@/lib/videos";

/**
 * `brief` is the home page's card: the one-line blurb in place of the
 * caption, and no counter. The caption is on the video's own page.
 */
export function DemoCard({ demo, index, brief = false }: { demo: Demo; index: number; brief?: boolean }) {
  const titleId = `demo-${demo.slug}-title`;
  const captionId = `demo-${demo.slug}-caption`;
  const text = brief ? (getVideo(demo.slug)?.blurb ?? demo.caption) : demo.caption;

  return (
    <figure className="card-hover overflow-hidden rounded-xl border border-border/60 bg-card lg:grid lg:grid-cols-[minmax(0,1.7fr)_minmax(0,1fr)]">
      <div className="bg-black">
        <FeaturedVideo
          href={videoPath(demo.slug)}
          title={demo.title}
          src={demoVideoSrc(demo.slug)}
          poster={demoPosterSrc(demo.slug)}
          width={DEMO_FRAME.width}
          height={DEMO_FRAME.height}
          lazy
          frame=""
        />
      </div>
      <figcaption className="flex flex-col gap-3 border-t border-border/60 p-4 sm:p-6 lg:border-l lg:border-t-0">
        {brief ? null : (
          <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
            <span aria-hidden className="h-2 w-2 rounded-full bg-primary/60" />
            <span>Demo {String(index + 1).padStart(2, "0")}</span>
            {demo.duration ? (
              <span className="tabular-nums">· {demo.duration}</span>
            ) : null}
          </div>
        )}
        <h3 id={titleId} className="text-balance text-lg font-semibold">
          {demo.title}
        </h3>
        <p id={captionId} className="text-pretty text-sm leading-relaxed text-muted-foreground">
          {text}
        </p>
        {/* Google indexes a video only on a page where it is the main
            content, so its structured data is on that page and on no
            other: this one links there. */}
        <Link
          href={videoPath(demo.slug)}
          className="mt-auto inline-flex items-center gap-1.5 text-sm font-medium text-primary underline-offset-4 hover:underline"
        >
          {brief ? "Watch" : "Watch on its own page"}
          <ArrowRight className="size-4" aria-hidden />
        </Link>
      </figcaption>
    </figure>
  );
}

export function RealDemos() {
  return (
    <section id="demos" className="py-12 md:py-28">
      <div className="container">
        <Reveal className="mx-auto mb-8 max-w-2xl text-center md:mb-12">
          <p className="font-mono text-xs uppercase tracking-widest text-primary">
            Demo videos
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            DJ jobs, recorded in the real app.
          </h2>
          <p className="mt-4 text-pretty text-lg text-muted-foreground">
            Screen recordings of the desktop app, each one a DJ job ending in an
            exported file. See also the{" "}
            <Link
              href="/use-cases/dj"
              className="text-primary underline underline-offset-4"
            >
              AI audio editor for DJs
            </Link>{" "}
            page.
          </p>
        </Reveal>
        <Stagger className="mx-auto grid max-w-6xl gap-5 md:gap-6" each={0.1} distance={28}>
          {demos.map((demo, i) => (
            <DemoCard key={demo.slug} demo={demo} index={i} brief />
          ))}
        </Stagger>
        <p className="mt-10 text-center">
          <Link
            href="/videos"
            className="inline-flex items-center gap-1.5 font-medium text-primary underline-offset-4 hover:underline"
          >
            More videos
            <ArrowRight className="size-4" aria-hidden />
          </Link>
        </p>
      </div>
    </section>
  );
}
