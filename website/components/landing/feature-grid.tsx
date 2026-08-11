"use client";

import {
  GitBranch,
  KeyRound,
  MessageSquare,
  ShieldCheck,
  Waves,
  Zap,
} from "lucide-react";
import { motion } from "framer-motion";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { FadeIn } from "./fade-in";
import { TiltCard } from "./tilt-card";

const features = [
  {
    icon: MessageSquare,
    title: "Conversational multi-track",
    body: "Mash A's vocals over B's drums, key-match, give me three takes. The agent plans, executes multi-track mixing, and renders branches — all from a single prompt.",
  },
  {
    icon: Waves,
    title: "Pro-grade DSP",
    body: "Pure Rust audio graph (cpal · symphonia · dasp · rubato) with Demucs stem separation, Whisper transcription, and Rubber Band pitch/time stretch. Zoom the waveform with Ctrl+scroll or +/−/0.",
  },
  {
    icon: ShieldCheck,
    title: "Local-first",
    body: "Your audio never leaves your machine. The DSP engine runs on-device; only chat tokens hit your chosen LLM provider.",
  },
  {
    icon: GitBranch,
    title: "Undo, branch & compare",
    body: "Every state is a DAG node. Ctrl+Z/Y traverse the branch history. Fork, A/B compare, and revert are first-class — not hidden behind a linear undo stack.",
  },
  {
    icon: KeyRound,
    title: "Bring your own LLM",
    body: "Anthropic, OpenAI, Gemini, Groq or OpenRouter keys stored in your OS keychain. Per-model agent profiles let you tune tools and behavior. Swap providers without reinstalling.",
  },
  {
    icon: Zap,
    title: "MCP extensibility",
    body: "Register Model Context Protocol servers from Settings to give the agent new tools. Wire in stdio JSON-RPC servers and extend what edytlab can do without touching core code.",
  },
];

const cardVariants = {
  hidden: { opacity: 0, y: 40, scale: 0.95 },
  visible: (i: number) => ({
    opacity: 1,
    y: 0,
    scale: 1,
    transition: {
      delay: i * 0.08,
      duration: 0.6,
      ease: [0.21, 0.47, 0.32, 0.98],
    },
  }),
};

export function FeatureGrid() {
  return (
    <section
      id="features"
      className="border-y border-border/50 bg-secondary/20 py-20 md:py-28"
    >
      <div className="container">
        <FadeIn className="mx-auto mb-14 max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Built for producers who want help — not handcuffs.
          </h2>
          <p className="mt-3 text-muted-foreground">
            What makes edytlab different from cleanup tools, preset chains, and
            shallow AI wrappers.
          </p>
        </FadeIn>
        <motion.div
          className="mx-auto grid max-w-6xl gap-5 sm:grid-cols-2 lg:grid-cols-3"
          initial="hidden"
          whileInView="visible"
          viewport={{ once: true, amount: 0.1 }}
        >
          {features.map((f, i) => (
            <motion.div key={f.title} custom={i} variants={cardVariants} className="group">
              <TiltCard className="h-full">
                <Card className="h-full border-border/60 bg-card/60 backdrop-blur transition-colors hover:border-primary/40 hover:bg-card">
                  <CardHeader>
                    <motion.div
                      className="mb-3 flex size-11 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20"
                      whileHover={{ scale: 1.18, rotate: 6 }}
                      transition={{ type: "spring", stiffness: 400, damping: 15 }}
                    >
                      <f.icon className="size-5" />
                    </motion.div>
                    <CardTitle>{f.title}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <CardDescription className="text-[0.95rem] leading-relaxed">
                      {f.body}
                    </CardDescription>
                  </CardContent>
                </Card>
              </TiltCard>
            </motion.div>
          ))}
        </motion.div>
      </div>
    </section>
  );
}
