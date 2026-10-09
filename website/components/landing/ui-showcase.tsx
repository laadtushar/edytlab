"use client";

/**
 * Animated renders of the editor's own surfaces.
 *
 * These are drawn, not screenshotted — the app is unsigned and still at
 * 0.x on all three platforms, so a static screenshot would age badly and
 * says nothing about how the thing behaves. Each panel animates the
 * interaction it is describing: the fader moves, the curve draws itself,
 * the clip slides.
 *
 * Everything depicted exists. Mixer controls, the automation lane, the
 * clip strip and the session graph view all shipped; nothing here is a
 * mock of a feature that is only planned.
 */

import { useRef } from "react";

import { Stagger, TiltCard } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, undash, whileVisible } from "@/lib/gsap";

/**
 * Each panel renders its **finished** state and GSAP animates *from* the
 * starting one. That ordering is the whole trick: with motion reduced,
 * or before the script runs, the fader is already at -4.5 dB and the
 * curve is already drawn, so the panel illustrates the feature either
 * way. Rendering the start state instead — which is what `initial` on a
 * motion component does — leaves a reader with reduced motion looking at
 * a flat line and a fader at zero.
 */
function panelTrigger(el: Element | null) {
  return { trigger: el, start: "top 85%", once: true } as const;
}

/** Deterministic pseudo-random in [0, 1) — no hydration mismatch. */
function seeded(i: number, seed: number) {
  return ((Math.sin(i * 127.1 + seed * 311.7) * 43758.5453) % 1 + 1) / 2;
}

// ─── Panel chrome ─────────────────────────────────────────────────────────────

function Panel({
  title,
  caption,
  children,
}: {
  title: string;
  caption: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex h-full flex-col overflow-hidden rounded-xl border border-border/60 bg-card">
      <div className="flex items-center gap-2 border-b border-border/60 bg-secondary/30 px-4 py-2.5">
        <span className="h-2 w-2 rounded-full bg-primary/60" />
        <span className="font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
          {title}
        </span>
      </div>
      <div className="flex-1 p-4">{children}</div>
      <p className="border-t border-border/60 px-4 py-3 text-sm text-muted-foreground">
        {caption}
      </p>
    </div>
  );
}

// ─── Mixer ────────────────────────────────────────────────────────────────────

const MIXER_TRACKS = [
  { name: "drums", from: 0, to: -4.5, pan: 0, seed: 3 },
  { name: "bass", from: 0, to: -2, pan: -0.35, seed: 7 },
  { name: "vocal", from: 0, to: 1.5, pan: 0.2, seed: 11 },
];

function panLabel(pan: number) {
  const pct = Math.round(Math.abs(pan) * 100);
  if (pct === 0) return "C";
  return `${pan < 0 ? "L" : "R"}${pct}`;
}

function MixerPanel() {
  const ref = useRef<HTMLDivElement>(null);

  // -60..+24 dB mapped to 0..100% of the fader.
  const pos = (db: number) => ((db + 60) / 84) * 100;

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const st = panelTrigger(ref.current);
        MIXER_TRACKS.forEach((t, i) => {
          const from = `${pos(t.from)}%`;
          const delay = 0.2 + i * 0.15;
          gsap.from(`[data-fill="${t.name}"]`, {
            width: from,
            duration: 1.1,
            delay,
            scrollTrigger: st,
          });
          gsap.from(`[data-knob="${t.name}"]`, {
            left: from,
            duration: 1.1,
            delay,
            scrollTrigger: st,
          });
        });
        gsap.from("[data-mixer-btn]", {
          opacity: 0,
          y: 6,
          duration: 0.4,
          delay: 0.9,
          stagger: 0.1,
          scrollTrigger: st,
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref} className="space-y-3">
      {MIXER_TRACKS.map((t) => (
        <div key={t.name} className="flex items-center gap-3">
          <span className="w-12 shrink-0 truncate font-mono text-[10px] text-muted-foreground">
            {t.name}
          </span>
          <div className="relative h-1.5 flex-1 rounded-full bg-secondary">
            <div
              data-fill={t.name}
              className="absolute inset-y-0 left-0 rounded-full bg-primary/70"
              style={{ width: `${pos(t.to)}%` }}
            />
            <div
              data-knob={t.name}
              className="absolute top-1/2 h-3 w-3 -translate-y-1/2 rounded-full border border-primary bg-background"
              style={{ left: `${pos(t.to)}%`, marginLeft: -6 }}
            />
          </div>
          <span className="w-14 shrink-0 text-right font-mono text-[10px] tabular-nums text-muted-foreground">
            {t.to > 0 ? `+${t.to.toFixed(1)}` : t.to.toFixed(1)} dB
          </span>
          <span className="w-8 shrink-0 text-right font-mono text-[10px] text-primary">
            {panLabel(t.pan)}
          </span>
        </div>
      ))}
      <div className="flex gap-1.5 pt-1">
        {["mute", "solo"].map((b) => (
          <span
            key={b}
            data-mixer-btn
            className="rounded border border-border px-2 py-0.5 font-mono text-[9px] uppercase tracking-wider text-muted-foreground"
          >
            {b}
          </span>
        ))}
      </div>
    </div>
  );
}

// ─── Automation lane ──────────────────────────────────────────────────────────

/** Points in the lane's own 0–100 × 0–48 space. */
const CURVE = [
  { x: 0, y: 14 },
  { x: 22, y: 14 },
  { x: 38, y: 34 },
  { x: 62, y: 34 },
  { x: 78, y: 10 },
  { x: 100, y: 10 },
];

function AutomationPanel() {
  const ref = useRef<HTMLDivElement>(null);
  const d = CURVE.map((p, i) => `${i === 0 ? "M" : "L"}${p.x},${p.y}`).join(" ");

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const path = ref.current?.querySelector<SVGPathElement>("[data-curve]");
        if (path) {
          // Draw the stroke by animating a dash gap the length of the
          // path back to zero — what DrawSVG does, done by hand so the
          // dash pattern can be cleared the moment it lands (below).
          const len = path.getTotalLength();
          gsap.fromTo(
            path,
            { strokeDasharray: len, strokeDashoffset: len },
            {
              strokeDashoffset: 0,
              duration: 1.4,
              delay: 0.2,
              ease: "power2.inOut",
              scrollTrigger: panelTrigger(ref.current),
              // Leave no dash pattern behind once it has drawn, or the
              // curve stays subtly dotted at some zoom levels.
              onComplete: () => gsap.set(path, { clearProps: "strokeDasharray,strokeDashoffset" }),
            },
          );
        }
        gsap.from("[data-point]", {
          opacity: 0,
          scale: 0,
          duration: 0.3,
          delay: 0.5,
          stagger: 0.22,
          scrollTrigger: panelTrigger(ref.current),
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref}>
      <svg
        viewBox="0 0 100 48"
        preserveAspectRatio="none"
        className="h-24 w-full"
        role="img"
        aria-label="A volume automation curve dipping in the middle and rising at the end"
      >
        {/* 0 dB reference */}
        <line
          x1="0"
          x2="100"
          y1="14"
          y2="14"
          stroke="currentColor"
          className="text-border"
          strokeDasharray="2 2"
          strokeWidth="0.5"
          vectorEffect="non-scaling-stroke"
        />
        <path
          data-curve
          d={d}
          fill="none"
          stroke="currentColor"
          className="text-primary"
          strokeWidth="1.5"
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      <div className="relative -mt-24 h-24">
        {CURVE.slice(1, -1).map((p, i) => (
          <span
            key={i}
            data-point
            className="absolute h-2 w-2 rounded-full bg-primary ring-2 ring-card"
            style={{
              left: `${p.x}%`,
              top: `${(p.y / 48) * 100}%`,
              marginLeft: -4,
              marginTop: -4,
            }}
          />
        ))}
      </div>
    </div>
  );
}

// ─── Clip strip ───────────────────────────────────────────────────────────────

function ClipStripPanel() {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const st = panelTrigger(ref.current);
        gsap.from("[data-clip-a]", { opacity: 0, duration: 0.4, scrollTrigger: st });
        // The second clip slides from where it sat before the cut, which
        // is the whole point of the panel: the gap opened, the tail moved.
        gsap.from("[data-clip-b]", {
          left: "36%",
          opacity: 0,
          duration: 1.2,
          delay: 0.6,
          ease: "power2.inOut",
          scrollTrigger: st,
        });
        gsap.from("[data-bar]", {
          height: 2,
          duration: 0.5,
          stagger: 0.008,
          scrollTrigger: st,
        });
        // Then it plays: the playhead crosses the arrangement — over the
        // seam, through the gap — on a loop that only runs while the
        // panel is on screen. A full-width track translated by its own
        // width, so the line moves on the compositor.
        const play = gsap.fromTo(
          "[data-playhead]",
          { xPercent: 0 },
          { xPercent: 100, duration: 4.5, ease: "none", repeat: -1, delay: 1.6 },
        );
        whileVisible(play, ref.current);
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref} className="relative space-y-2 overflow-hidden">
      {/* The playhead, parked at 0:00 in the markup. */}
      <div data-playhead aria-hidden className="pointer-events-none absolute inset-0 z-10">
        <div className="h-full w-px bg-foreground/60 shadow-[0_0_8px_hsl(var(--primary))]" />
      </div>
      {/* Chips: one cut into two, the second sitting later. */}
      <div className="relative h-6">
        <div
          data-clip-a
          className="absolute top-0 h-6 rounded border border-primary/50 bg-primary/15 px-2 font-mono text-[9px] leading-6 text-primary"
          style={{ left: "0%", width: "34%" }}
        >
          take.wav
        </div>
        <div
          data-clip-b
          className="absolute top-0 h-6 overflow-hidden rounded border border-primary/50 bg-primary/15 px-2 font-mono text-[9px] leading-6 text-primary"
          style={{ left: "56%", width: "44%" }}
        >
          take.wav
        </div>
      </div>
      {/* Waveform underneath, with a gap where the cut is. */}
      <div className="flex h-16 items-center gap-[2px]">
        {Array.from({ length: 64 }).map((_, i) => {
          const inGap = i > 22 && i < 36;
          const h = inGap ? 2 : 8 + seeded(i, 5) * 46;
          return (
            <span
              key={i}
              data-bar
              className={inGap ? "flex-1 rounded-sm bg-border" : "flex-1 rounded-sm bg-primary/45"}
              style={{ height: h }}
            />
          );
        })}
      </div>
    </div>
  );
}

// ─── Session graph ────────────────────────────────────────────────────────────

/** Node centres in the panel's own 200 × 96 space. */
const GRAPH_NODES: Array<[number, number]> = [
  [14, 48],
  [54, 48],
  [94, 48],
  [134, 24],
  [178, 24],
  [134, 72],
  [178, 72],
];

/** Edges as paths, in history order: the trunk, then the two branches. */
const GRAPH_EDGES = [
  "M14,48 L54,48",
  "M54,48 L94,48",
  "M94,48 C114,48 114,24 134,24",
  "M134,24 L178,24",
  "M94,48 C114,48 114,72 134,72",
  "M134,72 L178,72",
];

/** The head — the node the session is on. */
const HEAD = 4;

function GraphPanel() {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, (context) => {
        // History is written in the order it happened: each edge draws,
        // then the node it leads to pops — the trunk first, then the
        // fork into two branches.
        const tl = gsap.timeline({ scrollTrigger: panelTrigger(ref.current) });
        tl.from("[data-gnode='0']", {
          scale: 0,
          transformOrigin: "50% 50%",
          duration: 0.3,
          ease: "back.out(2.5)",
        });
        GRAPH_EDGES.forEach((_, i) => {
          const to = i + 1;
          tl.fromTo(
            `[data-gedge='${i}']`,
            { drawSVG: "0%" },
            {
              drawSVG: "100%",
              duration: 0.35,
              ease: "power1.inOut",
              onComplete: undash(ref.current?.querySelectorAll(`[data-gedge='${i}']`) ?? []),
            },
            i === 4 ? "-=0.9" : "-=0.05",
          ).from(
            `[data-gnode='${to}']`,
            { scale: 0, transformOrigin: "50% 50%", duration: 0.3, ease: "back.out(2.5)" },
            "-=0.1",
          );
        });
        tl.from("[data-head-label]", { opacity: 0, y: 4, duration: 0.3 });

        // Once the head exists it keeps a slow pulse, like a cursor —
        // and only while the panel is on screen.
        const pulse = gsap.fromTo(
          "[data-head-ring]",
          { scale: 1, opacity: 0.8 },
          {
            scale: 2.2,
            opacity: 0,
            transformOrigin: "50% 50%",
            duration: 1.6,
            ease: "power2.out",
            repeat: -1,
            repeatDelay: 0.4,
            paused: true,
          },
        );
        gsap.set("[data-head-ring]", { opacity: 0 });
        tl.eventCallback("onComplete", () => {
          context.add(() => whileVisible(pulse, ref.current));
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  const [hx, hy] = GRAPH_NODES[HEAD];
  return (
    <div ref={ref}>
      <svg
        viewBox="0 0 200 96"
        className="h-24 w-full"
        role="img"
        aria-label="A session graph: three edits in a row, then a fork into two branches of two edits each, with the head on the upper branch"
      >
        {GRAPH_EDGES.map((d, i) => (
          <path
            key={d}
            data-gedge={i}
            d={d}
            fill="none"
            className="stroke-primary/50"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
        ))}
        <circle data-head-ring cx={hx} cy={hy} r="6" fill="none" className="stroke-primary" strokeWidth="1" />
        {GRAPH_NODES.map(([cx, cy], i) => (
          <circle
            key={i}
            data-gnode={i}
            cx={cx}
            cy={cy}
            r={i === HEAD ? 5.5 : 4.5}
            className={i === HEAD ? "fill-primary" : "fill-card stroke-primary/70"}
            strokeWidth="1.5"
          />
        ))}
        <text
          data-head-label
          x={hx}
          y={hy - 11}
          textAnchor="middle"
          className="fill-primary font-mono text-[8px] uppercase tracking-widest"
        >
          head
        </text>
      </svg>
    </div>
  );
}

// ─── Section ──────────────────────────────────────────────────────────────────

const PANELS = [
  {
    title: "mixer",
    caption:
      "Gain, pan, mute and solo per track — by hand, without asking the agent. Every move is one undoable step in the session graph.",
    render: <MixerPanel />,
  },
  {
    title: "automation",
    caption:
      "Draw a volume curve on the clip. Click to add a point, drag to move it, arrows to nudge. The render interpolates between them per frame.",
    render: <AutomationPanel />,
  },
  {
    title: "clips",
    caption:
      "Cut a track and the seam is visible. Select a clip, drag it later, delete it — the waveform and the arrangement stay in step.",
    render: <ClipStripPanel />,
  },
  {
    title: "graph",
    caption:
      "Every edit is a node in the session graph. Click one to hear it; right-click to set it as the head, compare it with another or rename it.",
    render: <GraphPanel />,
  },
];

export function UiShowcase() {
  return (
    <section id="interface" className="py-20 md:py-28">
      <div className="container">
        <SectionHeading
          eyebrow="The interface"
          title="Talk to it — or reach in and move things yourself."
          lead="The agent is the fast path, not the only one. Faders, automation curves and clips are all directly editable, and every change lands in the same undoable session graph the agent writes to."
        />
        {/* Each panel animates the interaction it describes, and tilts
            toward the cursor once it has landed. */}
        <Stagger className="mx-auto grid max-w-5xl gap-5 md:grid-cols-2" each={0.1} distance={28}>
          {/* The stagger moves the wrapper and the tilt moves the card
              inside it — two tweens on one element would fight over its
              transform. */}
          {PANELS.map((p) => (
            <div key={p.title}>
              <TiltCard className="h-full">
                <Panel title={p.title} caption={p.caption}>
                  {p.render}
                </Panel>
              </TiltCard>
            </div>
          ))}
        </Stagger>
      </div>
    </section>
  );
}
