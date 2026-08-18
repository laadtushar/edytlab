"use client";

/**
 * Animated renders of the editor's own surfaces.
 *
 * These are drawn, not screenshotted — the app is unsigned and
 * Apple-Silicon-only today, so a static screenshot would age badly and
 * says nothing about how the thing behaves. Each panel animates the
 * interaction it is describing: the fader moves, the curve draws itself,
 * the clip slides.
 *
 * Everything depicted exists. Mixer controls, the automation lane and
 * the clip strip all shipped; nothing here is a mock of a feature that
 * is only planned.
 */

import { useRef } from "react";

import { Reveal, Stagger } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

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
          // path back to zero. GSAP's DrawSVG plugin does this too and
          // is a paid extra; for a single open path the two lines below
          // are the whole of it.
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
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <div ref={ref} className="space-y-2">
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
];

export function UiShowcase() {
  return (
    <section id="interface" className="py-20 md:py-28">
      <div className="container">
        <Reveal className="mx-auto mb-12 max-w-2xl text-center">
          <p className="font-mono text-xs uppercase tracking-widest text-primary">
            The interface
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Talk to it — or reach in and move things yourself.
          </h2>
          <p className="mt-4 text-pretty text-lg text-muted-foreground">
            The agent is the fast path, not the only one. Faders, automation
            curves and clips are all directly editable, and every change lands
            in the same undoable session graph the agent writes to.
          </p>
        </Reveal>
        <Stagger className="mx-auto grid max-w-6xl gap-5 lg:grid-cols-3" each={0.1} distance={28}>
          {PANELS.map((p) => (
            <Panel key={p.title} title={p.title} caption={p.caption}>
              {p.render}
            </Panel>
          ))}
        </Stagger>
      </div>
    </section>
  );
}
