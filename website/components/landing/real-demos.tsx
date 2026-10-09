/**
 * Screen recordings of the desktop app, played from `public/demos/`.
 *
 * The sections around this one are drawn: they show what the editor
 * does without a screenshot that ages. This one is the evidence — the
 * real app, with Claude as the agent, doing a whole job. The list lives
 * in `lib/demos.ts`; this file only lays it out.
 *
 * Nothing plays on its own. Each video waits for a press of play, which
 * settles autoplay-with-sound and reduced motion in one go, and with
 * `preload="none"` the page fetches only the posters until then.
 */

import { Reveal, Stagger } from "@/components/motion";
import { DEMO_FRAME, type Demo, demoPosterSrc, demoVideoSrc, demos } from "@/lib/demos";

function DemoCard({ demo, index }: { demo: Demo; index: number }) {
  const titleId = `demo-${demo.slug}-title`;
  const captionId = `demo-${demo.slug}-caption`;
  const src = demoVideoSrc(demo.slug);

  return (
    <figure className="overflow-hidden rounded-xl border border-border/60 bg-card lg:grid lg:grid-cols-[minmax(0,1.7fr)_minmax(0,1fr)]">
      <div className="bg-black">
        <video
          controls
          muted
          playsInline
          preload="none"
          poster={demoPosterSrc(demo.slug)}
          width={DEMO_FRAME.width}
          height={DEMO_FRAME.height}
          aria-labelledby={titleId}
          aria-describedby={captionId}
          className="block h-auto w-full max-w-full object-contain"
          // The box is reserved from the frame size, so the page does
          // not move when the poster lands; a recording at another
          // ratio is letterboxed inside it.
          style={{ aspectRatio: `${DEMO_FRAME.width} / ${DEMO_FRAME.height}` }}
        >
          <source src={src} type="video/mp4" />
          <p className="p-4 text-sm text-muted-foreground">
            This browser cannot play the video.{" "}
            <a href={src} className="text-primary underline underline-offset-4">
              Download the MP4
            </a>{" "}
            instead.
          </p>
        </video>
      </div>
      <figcaption className="flex flex-col gap-3 border-t border-border/60 p-5 sm:p-6 lg:border-l lg:border-t-0">
        <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
          <span aria-hidden className="h-2 w-2 rounded-full bg-primary/60" />
          <span>Demo {String(index + 1).padStart(2, "0")}</span>
          {demo.duration ? (
            <span className="tabular-nums">· {demo.duration}</span>
          ) : null}
        </div>
        <h3 id={titleId} className="text-balance text-lg font-semibold">
          {demo.title}
        </h3>
        <p id={captionId} className="text-pretty text-sm leading-relaxed text-muted-foreground">
          {demo.caption}
        </p>
      </figcaption>
    </figure>
  );
}

export function RealDemos() {
  return (
    <section id="demos" className="py-20 md:py-28">
      <div className="container">
        <Reveal className="mx-auto mb-12 max-w-2xl text-center">
          <p className="font-mono text-xs uppercase tracking-widest text-primary">
            Demo videos
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            DJ jobs, recorded in the real app.
          </h2>
          <p className="mt-4 text-pretty text-lg text-muted-foreground">
            Screen recordings of the desktop app with Claude as the agent. Each
            one is a single DJ job that ends in an exported file.
          </p>
        </Reveal>
        <Stagger className="mx-auto grid max-w-6xl gap-6" each={0.1} distance={28}>
          {demos.map((demo, i) => (
            <DemoCard key={demo.slug} demo={demo} index={i} />
          ))}
        </Stagger>
      </div>
    </section>
  );
}
