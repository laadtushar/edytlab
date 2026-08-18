"use client";

import { motion } from "framer-motion";
import { AnimatedCounter } from "./animated-counter";

const stats = [
  { value: "0", unit: "bytes", label: "audio uploaded to any server" },
  { value: "6", unit: "providers", label: "Anthropic · OpenAI · Gemini · Groq · OpenRouter · Ollama" },
  { value: "86", unit: "tools", label: "the agent can call, from fade to beat-warp" },
  { value: "100%", unit: "on-device", label: "DSP runs locally, always" },
];

const container = {
  hidden: {},
  visible: { transition: { staggerChildren: 0.12, delayChildren: 0.1 } },
};

const item = {
  hidden: { opacity: 0, y: 32, scale: 0.92 },
  visible: {
    opacity: 1,
    y: 0,
    scale: 1,
    transition: { duration: 0.6, ease: [0.21, 0.47, 0.32, 0.98] },
  },
};

export function StatsStrip() {
  return (
    <section className="py-14 md:py-16">
      <div className="container">
        <motion.dl
          className="mx-auto grid max-w-5xl grid-cols-2 gap-x-8 gap-y-10 md:grid-cols-4"
          variants={container}
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.3 }}
        >
          {stats.map((s) => (
            <motion.div
              key={s.label}
              variants={item}
              className="flex flex-col items-center gap-1 text-center"
            >
              <div className="flex items-baseline gap-1.5">
                <dt className="text-4xl font-bold tracking-tight text-foreground sm:text-5xl">
                  <AnimatedCounter value={s.value} />
                </dt>
                <motion.span
                  className="text-sm font-semibold text-primary"
                  initial={{ opacity: 0, x: -6 }}
                  whileInView={{ opacity: 1, x: 0 }}
                  viewport={{ once: true }}
                  transition={{ delay: 0.5, duration: 0.4 }}
                >
                  {s.unit}
                </motion.span>
              </div>
              <dd className="text-sm text-muted-foreground">{s.label}</dd>
            </motion.div>
          ))}
        </motion.dl>
      </div>
    </section>
  );
}
