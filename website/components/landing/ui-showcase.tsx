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

import { useEffect, useRef, useState } from "react";
import { motion, useInView } from "framer-motion";

import { FadeIn } from "./fade-in";

const EASE = [0.21, 0.47, 0.32, 0.98] as const;

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
  const inView = useInView(ref, { once: true, margin: "-60px" });

  return (
    <div ref={ref} className="space-y-3">
      {MIXER_TRACKS.map((t, i) => {
        // -60..+24 dB mapped to 0..100% of the fader.
        const pos = (db: number) => ((db + 60) / 84) * 100;
        return (
          <div key={t.name} className="flex items-center gap-3">
            <span className="w-12 shrink-0 truncate font-mono text-[10px] text-muted-foreground">
              {t.name}
            </span>
            <div className="relative h-1.5 flex-1 rounded-full bg-secondary">
              <motion.div
                className="absolute inset-y-0 left-0 rounded-full bg-primary/70"
                initial={{ width: `${pos(t.from)}%` }}
                animate={inView ? { width: `${pos(t.to)}%` } : undefined}
                transition={{ duration: 1.1, delay: 0.2 + i * 0.15, ease: EASE }}
              />
              <motion.div
                className="absolute top-1/2 h-3 w-3 -translate-y-1/2 rounded-full border border-primary bg-background"
                initial={{ left: `${pos(t.from)}%` }}
                animate={inView ? { left: `${pos(t.to)}%` } : undefined}
                transition={{ duration: 1.1, delay: 0.2 + i * 0.15, ease: EASE }}
                style={{ marginLeft: -6 }}
              />
            </div>
            <span className="w-14 shrink-0 text-right font-mono text-[10px] tabular-nums text-muted-foreground">
              {t.to > 0 ? `+${t.to.toFixed(1)}` : t.to.toFixed(1)} dB
            </span>
            <span className="w-8 shrink-0 text-right font-mono text-[10px] text-primary">
              {panLabel(t.pan)}
            </span>
          </div>
        );
      })}
      <div className="flex gap-1.5 pt-1">
        {["mute", "solo"].map((b, i) => (
          <motion.span
            key={b}
            className="rounded border border-border px-2 py-0.5 font-mono text-[9px] uppercase tracking-wider text-muted-foreground"
            initial={{ opacity: 0, y: 6 }}
            animate={inView ? { opacity: 1, y: 0 } : undefined}
            transition={{ duration: 0.4, delay: 0.9 + i * 0.1 }}
          >
            {b}
          </motion.span>
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
  const inView = useInView(ref, { once: true, margin: "-60px" });
  const d = CURVE.map((p, i) => `${i === 0 ? "M" : "L"}${p.x},${p.y}`).join(" ");

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
        <motion.path
          d={d}
          fill="none"
          stroke="currentColor"
          className="text-primary"
          strokeWidth="1.5"
          vectorEffect="non-scaling-stroke"
          initial={{ pathLength: 0 }}
          animate={inView ? { pathLength: 1 } : undefined}
          transition={{ duration: 1.4, ease: EASE, delay: 0.2 }}
        />
      </svg>
      <div className="relative -mt-24 h-24">
        {CURVE.slice(1, -1).map((p, i) => (
          <motion.span
            key={i}
            className="absolute h-2 w-2 rounded-full bg-primary ring-2 ring-card"
            style={{
              left: `${p.x}%`,
              top: `${(p.y / 48) * 100}%`,
              marginLeft: -4,
              marginTop: -4,
            }}
            initial={{ opacity: 0, scale: 0 }}
            animate={inView ? { opacity: 1, scale: 1 } : undefined}
            transition={{ duration: 0.3, delay: 0.5 + i * 0.22, ease: EASE }}
          />
        ))}
      </div>
    </div>
  );
}

// ─── Clip strip ───────────────────────────────────────────────────────────────

function ClipStripPanel() {
  const ref = useRef<HTMLDivElement>(null);
  const inView = useInView(ref, { once: true, margin: "-60px" });

  return (
    <div ref={ref} className="space-y-2">
      {/* Chips: one cut into two, the second sliding later. */}
      <div className="relative h-6">
        <motion.div
          className="absolute top-0 h-6 rounded border border-primary/50 bg-primary/15 px-2 font-mono text-[9px] leading-6 text-primary"
          style={{ left: "0%", width: "34%" }}
          initial={{ opacity: 0 }}
          animate={inView ? { opacity: 1 } : undefined}
          transition={{ duration: 0.4 }}
        >
          take.wav
        </motion.div>
        <motion.div
          className="absolute top-0 h-6 overflow-hidden rounded border border-primary/50 bg-primary/15 px-2 font-mono text-[9px] leading-6 text-primary"
          style={{ width: "44%" }}
          initial={{ left: "36%", opacity: 0 }}
          animate={inView ? { left: "56%", opacity: 1 } : undefined}
          transition={{ duration: 1.2, delay: 0.6, ease: EASE }}
        >
          take.wav
        </motion.div>
      </div>
      {/* Waveform underneath, with a gap where the cut is. */}
      <div className="flex h-16 items-center gap-[2px]">
        {Array.from({ length: 64 }).map((_, i) => {
          const inGap = i > 22 && i < 36;
          const h = inGap ? 2 : 8 + seeded(i, 5) * 46;
          return (
            <motion.span
              key={i}
              className={
                inGap ? "flex-1 rounded-sm bg-border" : "flex-1 rounded-sm bg-primary/45"
              }
              initial={{ height: 2 }}
              animate={inView ? { height: h } : undefined}
              transition={{ duration: 0.5, delay: i * 0.008, ease: EASE }}
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
        <FadeIn className="mx-auto mb-12 max-w-2xl text-center">
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
        </FadeIn>
        <div className="mx-auto grid max-w-6xl gap-5 lg:grid-cols-3">
          {PANELS.map((p, i) => (
            <FadeIn key={p.title} delay={i * 0.1}>
              <Panel title={p.title} caption={p.caption}>
                {p.render}
              </Panel>
            </FadeIn>
          ))}
        </div>
      </div>
    </section>
  );
}
