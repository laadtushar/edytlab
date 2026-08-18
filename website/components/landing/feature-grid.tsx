"use client";

import {
  AudioWaveform,
  FileDown,
  GitBranch,
  KeyRound,
  MessageSquare,
  ShieldCheck,
  Waves,
  Zap,
} from "lucide-react";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { Reveal, Stagger, TiltCard } from "@/components/motion";

const features = [
  {
    icon: MessageSquare,
    title: "Conversational multi-track",
    body: "Mash A's vocals over B's drums, key-match, give me three takes. The agent plans, executes multi-track mixing, and renders branches — all from a single prompt.",
  },
  {
    icon: Waves,
    title: "Pro-grade DSP",
    body: "Pure Rust audio graph (cpal · symphonia · rubato · realfft) with Demucs stem separation and Whisper transcription. Time-stretch, pitch-shift and formant preservation run on a phase vocoder written for this project — no C dependency in the audio path.",
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
    body: "Anthropic, OpenAI, Gemini, Groq, OpenRouter or a local Ollama daemon. Keys live in your OS keychain. Per-model agent profiles tune tools and behaviour. Swap providers without reinstalling.",
  },
  {
    icon: AudioWaveform,
    title: "Time, pitch and timing",
    body: "Stretch without moving the pitch, shift pitch without moving the clock, and preserve formants so a shifted voice still sounds like the same person. Warp a performance onto a beat grid in a single pass — no seam at the beats.",
  },
  {
    icon: FileDown,
    title: "Export that plays anywhere",
    body: "WAV when you want the samples, FLAC for lossless at about half the size, MP3 when it has to open on anything. Loudness-normalise to a LUFS target — the number streaming platforms actually use — with a true-peak ceiling so it never clips getting there.",
  },
  {
    icon: Zap,
    title: "MCP extensibility",
    body: "Register Model Context Protocol servers from Settings to give the agent new tools. Wire in stdio JSON-RPC servers and extend what edytlab can do without touching core code.",
  },
];

export function FeatureGrid() {
  return (
    <section
      id="features"
      className="border-y border-border/50 bg-secondary/20 py-20 md:py-28"
    >
      <div className="container">
        <Reveal className="mx-auto mb-14 max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Built for producers who want help — not handcuffs.
          </h2>
          <p className="mt-3 text-muted-foreground">
            What makes edytlab different from cleanup tools, preset chains, and
            shallow AI wrappers.
          </p>
        </Reveal>
        <Stagger
          className="mx-auto grid max-w-6xl gap-5 sm:grid-cols-2 lg:grid-cols-3"
          each={0.06}
          distance={28}
          scale
        >
          {features.map((f) => (
            <div key={f.title} className="group">
              <TiltCard className="h-full">
                <Card className="surface h-full border-border/60 backdrop-blur transition-colors group-hover:border-primary/40">
                  <CardHeader>
                    <div className="mb-3 flex size-11 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20 transition-transform duration-300 group-hover:rotate-6 group-hover:scale-110">
                      <f.icon className="size-5" />
                    </div>
                    <CardTitle>{f.title}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <CardDescription className="text-[0.95rem] leading-relaxed">
                      {f.body}
                    </CardDescription>
                  </CardContent>
                </Card>
              </TiltCard>
            </div>
          ))}
        </Stagger>
      </div>
    </section>
  );
}
