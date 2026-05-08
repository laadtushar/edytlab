"use client";

import { motion } from "framer-motion";
import { useMemo } from "react";

/**
 * Animated, decorative waveform-like SVG background.
 * Pure SVG paths — no canvas, no audio. Subtle motion only.
 */
export function WaveformBackground() {
  const lines = useMemo(() => {
    // Deterministic pseudo-random layout so SSR/CSR match.
    return Array.from({ length: 64 }, (_, i) => {
      const seed = (i * 9301 + 49297) % 233280;
      const rand = seed / 233280;
      const height = 8 + rand * 70;
      return { i, height };
    });
  }, []);

  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0 -z-10 overflow-hidden"
    >
      <div className="absolute inset-x-0 bottom-0 flex h-72 items-end justify-center gap-[3px] px-4 opacity-30">
        {lines.map(({ i, height }) => (
          <motion.div
            key={i}
            className="w-[3px] rounded-full bg-gradient-to-t from-primary/0 via-primary/60 to-primary"
            initial={{ height: 4 }}
            animate={{ height: [height * 0.4, height, height * 0.5, height * 0.8, height * 0.4] }}
            transition={{
              duration: 4 + (i % 7) * 0.3,
              repeat: Infinity,
              ease: "easeInOut",
              delay: (i % 13) * 0.07,
            }}
            style={{ height }}
          />
        ))}
      </div>
      <div className="absolute inset-0 bg-gradient-to-b from-background via-background/60 to-background" />
      <div className="absolute -top-40 left-1/2 h-[500px] w-[500px] -translate-x-1/2 rounded-full bg-primary/15 blur-3xl" />
    </div>
  );
}
