"use client";

import Link from "next/link";
import { Apple, Download } from "lucide-react";
import { motion } from "framer-motion";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { siteConfig } from "@/lib/site";
import { WaveformBackground } from "./waveform-bg";

const line1 = ["Describe", "it."];
const line2 = ["Get", "pro-grade", "audio", "edits."];

const word = {
  hidden: { opacity: 0, y: 48, rotateX: -25, filter: "blur(4px)" },
  visible: (i: number) => ({
    opacity: 1,
    y: 0,
    rotateX: 0,
    filter: "blur(0px)",
    transition: {
      delay: i * 0.07,
      duration: 0.65,
      ease: [0.21, 0.47, 0.32, 0.98],
    },
  }),
};

export function Hero() {
  return (
    <section className="relative overflow-hidden pb-24 pt-32 md:pb-32 md:pt-40">
      <WaveformBackground />
      <div className="container relative">
        <div className="mx-auto max-w-4xl text-center" style={{ perspective: "1200px" }}>
          <motion.div
            initial={{ opacity: 0, scale: 0.85, y: -12 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            transition={{ duration: 0.5, ease: [0.21, 0.47, 0.32, 0.98] }}
          >
            <Badge
              variant="outline"
              className="mb-6 border-primary/30 bg-primary/10 text-primary"
            >
              Local-first AI audio editor · {siteConfig.version}
            </Badge>
          </motion.div>

          <h1 className="text-balance text-5xl font-bold tracking-tight sm:text-6xl md:text-7xl">
            <div className="mb-1 overflow-hidden">
              <motion.div
                className="flex flex-wrap justify-center gap-x-[0.25em]"
                initial="hidden"
                animate="visible"
              >
                {line1.map((w, i) => (
                  <motion.span key={i} custom={i} variants={word} className="inline-block">
                    {w}
                  </motion.span>
                ))}
              </motion.div>
            </div>
            <div className="overflow-hidden">
              <motion.div
                className="flex flex-wrap justify-center gap-x-[0.25em]"
                initial="hidden"
                animate="visible"
              >
                {line2.map((w, i) => (
                  <motion.span
                    key={i}
                    custom={i + line1.length}
                    variants={word}
                    className="gradient-text inline-block"
                  >
                    {w}
                  </motion.span>
                ))}
              </motion.div>
            </div>
          </h1>

          <motion.p
            className="mx-auto mt-6 max-w-2xl text-pretty text-lg text-muted-foreground md:text-xl"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.65, duration: 0.55, ease: [0.21, 0.47, 0.32, 0.98] }}
          >
            Desktop audio editor where you chat with an AI to load, cut, mix,
            transcribe, and render. Pure-Rust DSP, local-first, BYO LLM key.
          </motion.p>

          <motion.div
            className="mt-10 flex flex-col items-center justify-center gap-3 sm:flex-row"
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.82, duration: 0.5, ease: [0.21, 0.47, 0.32, 0.98] }}
          >
            <motion.div whileHover={{ scale: 1.06 }} whileTap={{ scale: 0.96 }}>
              <Button asChild size="lg" className="glow w-full sm:w-auto">
                <Link href={siteConfig.releases}>
                  <Apple className="size-4" />
                  Download for Mac
                </Link>
              </Button>
            </motion.div>
            <motion.div whileHover={{ scale: 1.06 }} whileTap={{ scale: 0.96 }}>
              <Button
                asChild
                size="lg"
                variant="outline"
                className="w-full sm:w-auto"
              >
                <Link href={siteConfig.releases}>
                  <Download className="size-4" />
                  Download for Windows
                </Link>
              </Button>
            </motion.div>
          </motion.div>

          <motion.p
            className="mt-4 text-xs text-muted-foreground"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 1.1, duration: 0.5 }}
          >
            Signed installers · Universal Mac (Apple Silicon + Intel) · Windows 10/11
          </motion.p>
        </div>
      </div>
    </section>
  );
}
