"use client";

import { FileAudio, MessagesSquare, FileDown } from "lucide-react";
import { motion } from "framer-motion";

import { FadeIn } from "./fade-in";

const steps = [
  {
    icon: FileAudio,
    title: "1. Drop audio",
    body: "Drag in songs, stems, or a folder of takes. WAV, MP3, FLAC, OGG — decoded with symphonia, no upload, no waiting.",
  },
  {
    icon: MessagesSquare,
    title: "2. Talk to the agent",
    body: `“Mashup A over B, key-match, give me 3 takes on the drop.” The agent plans, shows the plan, and renders branches you can A/B.`,
  },
  {
    icon: FileDown,
    title: "3. Export",
    body: "Pick the branch you like. Export to WAV, FLAC for a lossless file about half the size, or MP3 when it has to play anywhere. The session graph keeps every alternative.",
  },
];

const cardVariants = {
  hidden: { opacity: 0, y: 48, scale: 0.94 },
  visible: (i: number) => ({
    opacity: 1,
    y: 0,
    scale: 1,
    transition: {
      delay: i * 0.14,
      duration: 0.65,
      ease: [0.21, 0.47, 0.32, 0.98],
    },
  }),
};

export function HowItWorks() {
  return (
    <section id="how-it-works" className="py-20 md:py-28">
      <div className="container">
        <FadeIn className="mx-auto mb-14 max-w-2xl text-center">
          <p className="text-sm font-semibold uppercase tracking-wider text-primary">
            How it works
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Three steps. No DAW manual required.
          </h2>
        </FadeIn>
        <div className="relative mx-auto max-w-5xl">
          {/* Animated connector line */}
          <div className="absolute left-0 right-0 top-14 hidden md:block">
            <motion.div
              className="mx-auto h-px bg-gradient-to-r from-transparent via-primary/40 to-transparent"
              initial={{ scaleX: 0, opacity: 0 }}
              whileInView={{ scaleX: 1, opacity: 1 }}
              viewport={{ once: true }}
              transition={{ delay: 0.3, duration: 1, ease: [0.21, 0.47, 0.32, 0.98] }}
              style={{ originX: 0 }}
            />
          </div>

          <motion.div
            className="grid gap-5 md:grid-cols-3"
            initial="hidden"
            whileInView="visible"
            viewport={{ once: true, amount: 0.2 }}
          >
            {steps.map((s, i) => (
              <motion.div
                key={s.title}
                custom={i}
                variants={cardVariants}
                whileHover={{ y: -4, transition: { type: "spring", stiffness: 300, damping: 20 } }}
                className="relative h-full rounded-xl border border-border/60 bg-card/40 p-6 backdrop-blur"
              >
                <motion.div
                  className="mb-4 flex size-12 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20"
                  whileHover={{ scale: 1.15, rotate: -6 }}
                  transition={{ type: "spring", stiffness: 400, damping: 15 }}
                >
                  <s.icon className="size-5" />
                </motion.div>
                <h3 className="text-lg font-semibold">{s.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                  {s.body}
                </p>
              </motion.div>
            ))}
          </motion.div>
        </div>
      </div>
    </section>
  );
}
