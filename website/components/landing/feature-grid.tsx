import {
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

import { FadeIn } from "./fade-in";

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
    body: "Anthropic, OpenRouter, or OpenAI keys stored in your OS keychain. Per-model agent profiles let you tune tools and behavior. Swap providers without reinstalling.",
  },
  {
    icon: Zap,
    title: "MCP extensibility",
    body: "Register Model Context Protocol servers from Settings to give the agent new tools. Wire in stdio JSON-RPC servers and extend what edytlab can do without touching core code.",
  },
];

export function FeatureGrid() {
  return (
    <section id="features" className="border-y border-border/50 bg-secondary/20 py-20 md:py-28">
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
        <div className="mx-auto grid max-w-6xl gap-5 sm:grid-cols-2 lg:grid-cols-3">
          {features.map((f, i) => (
            <FadeIn key={f.title} delay={i * 0.05}>
              <Card className="h-full border-border/60 bg-card/60 backdrop-blur transition-colors hover:border-primary/40 hover:bg-card">
                <CardHeader>
                  <div className="mb-3 flex size-11 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20">
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
            </FadeIn>
          ))}
        </div>
      </div>
    </section>
  );
}
