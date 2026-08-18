"use client";

/**
 * The hero, told as scenes instead of shown all at once.
 *
 * ## Why this replaces a one-second entrance
 *
 * The old hero animated itself over about 1.2 seconds on load. That is
 * fine the first time and useless every time after: on a warm cache it
 * is finished before you have focused the window. The story it was
 * telling — *this app turns a sentence into an edit* — was told to
 * nobody.
 *
 * Here the scroll wheel *is* the timeline. The section pins, the page
 * stops moving, and scrolling scrubs through five scenes of one real
 * edit: audio arrives, you ask for something, tools run, an automation
 * curve appears, it renders. Nothing is on a timer, so nothing can be
 * missed, and a reader who scrolls back up watches it in reverse.
 *
 * ## What it degrades to
 *
 * The scenes are ordinary stacked sections in the markup. Pinning and
 * absolute positioning are added *by GSAP*, only when the reader has
 * not asked for reduced motion and only on a screen wide enough for a
 * pinned stage to make sense. Everywhere else — narrow screens, reduced
 * motion, no JavaScript, a crawler — the same five scenes simply flow
 * down the page as static content that says the same thing. The `h1`
 * is real text in the first scene either way.
 */

import { useRef } from "react";
import Link from "next/link";
import { Apple, Download } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Magnetic } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";
import type { ReleaseAssets } from "@/lib/releases";
import { WAVE_H, WAVE_W, duckPath, flatPath, wavePath, waveClosedPath } from "./waveform";

/** The dead air the story cuts out, as fractions of the track. */
const GAPS: Array<[number, number]> = [
  [0.18, 0.28],
  [0.52, 0.6],
  [0.78, 0.85],
];

/** Where the voice is, for the ducking scene. */
const SPEECH: Array<[number, number]> = [
  [0.08, 0.34],
  [0.46, 0.72],
];

const VOICE_RAW = wavePath(3, { gaps: GAPS });
const VOICE_CUT = waveClosedPath(3, GAPS);
const MUSIC = wavePath(11, { scale: 0.55 });
const FLAT = flatPath();

/**
 * The chips the third scene fires, and they are load-bearing: naming a
 * tool the agent cannot call turns the story into a lie. Held to that by
 * `website_tool_docs.rs`, which now reads this file too.
 */
const TOOLS = ["load", "transcribe", "truncate_silence", "set_clip_envelope", "render_final"];

/** Only pin where there is room for a stage; narrow screens read the stack. */
const STAGE = `${NO_PREFERENCE} and (min-width: 768px)`;

export function ScrollStory({ release }: { release: ReleaseAssets }) {
  const root = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();

      mm.add(STAGE, () => {
        const scenes = gsap.utils.toArray<HTMLElement>("[data-scene]");

        // Lift the scenes out of flow and stack them. Done here rather
        // than in the class list so the stacked fallback is what the
        // markup actually says, and this is the enhancement.
        gsap.set(scenes, { position: "absolute", inset: 0, autoAlpha: 0 });
        gsap.set(scenes[0], { autoAlpha: 1 });
        gsap.set("[data-stage]", { height: "100vh" });

        // ── Scene 1 plays on load, not on scroll ────────────────────
        //
        // This is the one thing that must not be scrubbed. A scrubbed
        // timeline sits at progress 0 until the reader scrolls, so
        // putting the opening headline in it means landing on an empty
        // hero and having to scroll to find out what the site is. The
        // entrance runs once on its own clock; the *story* is what the
        // scroll controls, and it starts from a finished scene 1.
        gsap
          .timeline({ defaults: { ease: "power3.out" } })
          .from("[data-s1-title] .word", {
            yPercent: 120,
            opacity: 0,
            rotateX: -50,
            stagger: 0.06,
            duration: 0.8,
          })
          .from("[data-s1-sub]", { opacity: 0, y: 16, duration: 0.6 }, "-=0.45")
          .fromTo(
            "[data-voice]",
            { drawSVG: "0%" },
            { drawSVG: "100%", duration: 1.4, ease: "power1.inOut" },
            "-=0.4",
          )
          .to("[data-voice]", { fillOpacity: 0.22, duration: 0.5 }, "-=0.4");

        // ── The story: scene to scene, driven by the scrollbar ──────
        const tl = gsap.timeline({
          scrollTrigger: {
            trigger: root.current,
            start: "top top",
            // Roughly one viewport of scrolling per transition. Longer
            // feels like the page has stopped responding; shorter and
            // the scenes blur past.
            end: () => `+=${(scenes.length - 1) * 90}%`,
            pin: "[data-stage]",
            scrub: 0.8,
            invalidateOnRefresh: true,
          },
        });

        // ── 2 · The ask ─────────────────────────────────────────────
        tl.addLabel("ask", 0.35)
          .to("[data-scene='1']", { autoAlpha: 0, duration: 0.45 }, "ask")
          // Overlapped by a fraction, not held together. A long
          // cross-fade means a reader who parks mid-transition — which
          // on a scrubbed timeline is entirely their choice — sees two
          // scenes' worth of text on top of each other.
          .to("[data-scene='2']", { autoAlpha: 1, duration: 0.45 }, "ask+=0.55")
          // The bubble reveals by unclipping rather than by typing a
          // character at a time: at scrub speed a per-character reveal
          // reads as a glitch when the reader scrolls quickly.
          .from("[data-bubble]", { opacity: 0, y: 24, scale: 0.96, duration: 0.8 }, "ask+=0.55")
          .from("[data-ask-text]", { clipPath: "inset(0 100% 0 0)", duration: 1.4 }, "ask+=0.8");

        // ── 3 · The tools run, and the waveform actually changes ────
        tl.addLabel("work", "+=0.5")
          .to("[data-scene='2']", { autoAlpha: 0, duration: 0.45 }, "work")
          // Overlapped by a fraction, not held together. A long
          // cross-fade means a reader who parks mid-transition — which
          // on a scrubbed timeline is entirely their choice — sees two
          // scenes' worth of text on top of each other.
          .to("[data-scene='3']", { autoAlpha: 1, duration: 0.45 }, "work+=0.55")
          .from("[data-tool]", { opacity: 0, y: 14, stagger: 0.28, duration: 0.5 }, "work+=0.55")
          // The whole point of the scene: the dead air comes out and
          // the tail slides left. Not a fade between two pictures — the
          // same path, reshaped.
          .to(
            "[data-voice-3]",
            { attr: { d: VOICE_CUT }, duration: 1.6, ease: "power2.inOut" },
            "work+=1.1",
          )
          .from("[data-saved]", { opacity: 0, y: 10, duration: 0.6 }, "work+=2.3");

        // ── 4 · The curve ───────────────────────────────────────────
        tl.addLabel("duck", "+=0.5")
          .to("[data-scene='3']", { autoAlpha: 0, duration: 0.45 }, "duck")
          // Overlapped by a fraction, not held together. A long
          // cross-fade means a reader who parks mid-transition — which
          // on a scrubbed timeline is entirely their choice — sees two
          // scenes' worth of text on top of each other.
          .to("[data-scene='4']", { autoAlpha: 1, duration: 0.45 }, "duck+=0.3")
          .fromTo(
            "[data-duck]",
            { drawSVG: "0%" },
            { drawSVG: "100%", duration: 1.8, ease: "power1.inOut" },
            "duck+=0.4",
          )
          .from("[data-duck-note]", { opacity: 0, y: 12, duration: 0.6 }, "duck+=1.8");

        // ── 5 · Rendered ────────────────────────────────────────────
        tl.addLabel("done", "+=0.5")
          .to("[data-scene='4']", { autoAlpha: 0, duration: 0.45 }, "done")
          // Overlapped by a fraction, not held together. A long
          // cross-fade means a reader who parks mid-transition — which
          // on a scrubbed timeline is entirely their choice — sees two
          // scenes' worth of text on top of each other.
          .to("[data-scene='5']", { autoAlpha: 1, duration: 0.45 }, "done+=0.3")
          .from("[data-done-h] .word", {
            yPercent: 120,
            opacity: 0,
            stagger: 0.1,
            duration: 0.8,
          }, "done+=0.4")
          // The download buttons land with the scene, not a beat after
          // it. On a scrubbed timeline "a beat after" means "only if the
          // reader keeps scrolling", and the primary call to action is
          // not something to make people earn.
          .from("[data-done-cta] > *", { opacity: 0, y: 18, stagger: 0.1, duration: 0.5 }, "done+=0.55")
          // Hold the pin after the last reveal so the finished scene is
          // readable rather than being scrolled off the instant it
          // completes. Without this the pin releases mid-reveal and the
          // buttons are never actually seen.
          .to({}, { duration: 1.4 });

        // The chapter rail tracks progress through the scenes.
        tl.eventCallback("onUpdate", () => {
          const i = Math.min(scenes.length - 1, Math.floor(tl.progress() * scenes.length));
          gsap.utils.toArray<HTMLElement>("[data-chapter]").forEach((el, n) => {
            gsap.to(el, { opacity: n === i ? 1 : 0.28, scaleX: n === i ? 1 : 0.4, duration: 0.3 });
          });
        });
      });

      return () => mm.revert();
    },
    { scope: root },
  );

  return (
    <div ref={root} className="relative">
      <div data-stage className="relative overflow-hidden">
        {/* ── 1 ─────────────────────────────────────────────────── */}
        <Scene n={1}>
          <Badge variant="outline" className="mb-6 border-primary/30 bg-primary/10 text-primary">
            Local-first AI audio editor · {release.version}
          </Badge>
          <h1
            data-s1-title
            className="text-balance text-5xl font-bold tracking-tight sm:text-6xl md:text-7xl"
          >
            <span className="mb-1 block">
              <Words text="Describe it." />
            </span>
            <span className="block">
              <Words text="Get pro-grade audio edits." className="gradient-text-split" />
            </span>
          </h1>
          <p data-s1-sub className="mx-auto mt-6 max-w-2xl text-pretty text-lg text-muted-foreground">
            Twenty-two minutes of raw tape. Two speakers, dead air, music
            that fights the voice. Watch it become an episode.
          </p>
          <Lanes>
            <path
              data-voice
              d={VOICE_RAW}
              className="fill-primary stroke-primary"
              fillOpacity={0}
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
            />
          </Lanes>
        </Scene>

        {/* ── 2 ─────────────────────────────────────────────────── */}
        <Scene n={2}>
          <p className="font-mono text-xs uppercase tracking-widest text-primary">You say</p>
          <div
            data-bubble
            className="mx-auto mt-6 max-w-2xl rounded-2xl rounded-br-sm border border-primary/30 bg-primary/10 px-6 py-5 text-left"
          >
            <p data-ask-text className="text-lg leading-relaxed text-foreground sm:text-xl">
              Cut the dead air, duck the music under my voice, and render it.
            </p>
          </div>
          <p className="mt-6 text-sm text-muted-foreground">
            No menu hunting. No preset chain. A sentence.
          </p>
        </Scene>

        {/* ── 3 ─────────────────────────────────────────────────── */}
        <Scene n={3}>
          <p className="font-mono text-xs uppercase tracking-widest text-primary">It works</p>
          <div className="mt-6 flex flex-wrap items-center justify-center gap-2">
            {TOOLS.map((t) => (
              <span
                key={t}
                data-tool
                className="rounded-full border border-amber-500/30 bg-amber-500/10 px-3 py-1 font-mono text-[11px] text-amber-400"
              >
                {t}
              </span>
            ))}
          </div>
          <Lanes>
            <path
              data-voice-3
              d={VOICE_RAW}
              className="fill-primary stroke-primary"
              fillOpacity={0.22}
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
            />
          </Lanes>
          <p data-saved className="mt-4 text-sm text-muted-foreground">
            Three stretches of silence gone. The tail closes up behind them —
            one undoable step, not a destructive render.
          </p>
        </Scene>

        {/* ── 4 ─────────────────────────────────────────────────── */}
        <Scene n={4}>
          <p className="font-mono text-xs uppercase tracking-widest text-primary">
            And the music gets out of the way
          </p>
          <Lanes>
            <path d={MUSIC} className="fill-fuchsia-400/40" />
            <path
              data-duck
              d={duckPath(SPEECH)}
              fill="none"
              className="stroke-primary"
              strokeWidth={2}
              vectorEffect="non-scaling-stroke"
            />
          </Lanes>
          <p data-duck-note className="mx-auto mt-6 max-w-xl text-sm text-muted-foreground">
            Keyed on the transcript, not on level — so a breath does not
            trigger it and a quiet line does not escape it. The result is an
            ordinary automation curve you can drag.
          </p>
        </Scene>

        {/* ── 5 ─────────────────────────────────────────────────── */}
        <Scene n={5}>
          <h2
            data-done-h
            className="text-balance text-4xl font-bold tracking-tight sm:text-5xl md:text-6xl"
          >
            <Words text="Rendered." className="gradient-text-split" />
          </h2>
          <p className="mx-auto mt-5 max-w-xl text-pretty text-lg text-muted-foreground">
            Every step is a node you can undo, branch, or export as a recipe
            and run over the next twelve episodes.
          </p>
          <div
            data-done-cta
            className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row"
          >
            <Magnetic>
              <Button asChild size="lg" className="glow w-full sm:w-auto">
                <Link href={release.macUrl}>
                  <Apple className="size-4" />
                  Download for Mac
                </Link>
              </Button>
            </Magnetic>
            <Magnetic>
              <Button asChild size="lg" variant="outline" className="w-full sm:w-auto">
                <Link href={release.winUrl}>
                  <Download className="size-4" />
                  Download for Windows
                </Link>
              </Button>
            </Magnetic>
          </div>
          <p className="mt-4 text-xs text-muted-foreground">
            Unsigned dev builds · Mac (universal) · Windows 10/11 · Linux
          </p>
        </Scene>

        {/* Chapter rail — where you are in the story, and the only hint
            that scrolling here does something other than scroll. */}
        <div
          aria-hidden
          className="pointer-events-none absolute bottom-8 left-1/2 hidden -translate-x-1/2 gap-2 md:flex"
        >
          {[1, 2, 3, 4, 5].map((n) => (
            <span
              key={n}
              data-chapter
              className="h-[3px] w-10 origin-center rounded-full bg-primary opacity-30"
            />
          ))}
        </div>
      </div>
    </div>
  );
}

/** One scene. Ordinary block in the markup; GSAP stacks them. */
function Scene({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <section
      data-scene={n}
      className="flex min-h-[60vh] flex-col items-center justify-center px-4 py-16 text-center md:min-h-0 md:py-0"
    >
      <div className="mx-auto w-full max-w-4xl">{children}</div>
    </section>
  );
}

/** The two audio lanes, at a fixed aspect so the paths never distort. */
function Lanes({ children }: { children: React.ReactNode }) {
  return (
    <svg
      viewBox={`0 0 ${WAVE_W} ${WAVE_H}`}
      className="mx-auto mt-10 h-24 w-full max-w-3xl sm:h-32"
      role="img"
      aria-label="A waveform of the episode being edited"
    >
      {children}
    </svg>
  );
}

/**
 * Words wrapped for a stagger.
 *
 * Hand-split rather than `SplitText` because these headings are inside a
 * scrubbed timeline: SplitText re-splits on resize and hands back new
 * elements, which would leave the timeline animating nodes that are no
 * longer in the document. A fixed split has no such lifecycle.
 */
function Words({ text, className }: { text: string; className?: string }) {
  const words = text.split(" ");
  return (
    <span>
      {words.map((w, i) => (
        <span key={`${w}-${i}`}>
          <span className="inline-block overflow-hidden py-[0.08em] align-bottom">
            <span className={`word inline-block ${className ?? ""}`}>{w}</span>
          </span>
          {i < words.length - 1 ? " " : null}
        </span>
      ))}
    </span>
  );
}
