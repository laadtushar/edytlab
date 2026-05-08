import {
  GitBranch,
  KeyRound,
  Laptop,
  MessageSquare,
  ShieldCheck,
  Waves,
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
    body: "Mash A's vocals over B's drums, key-match, give me three takes — say what you want, the agent plans and executes.",
  },
  {
    icon: Waves,
    title: "Pro-grade DSP",
    body: "Pure Rust audio graph (cpal · symphonia · dasp · fundsp · rubato) with Demucs, Whisper, and Rubber Band integrated end-to-end.",
  },
  {
    icon: ShieldCheck,
    title: "Local-first",
    body: "Your audio never leaves your machine. The DSP engine runs on-device; only chat tokens hit your chosen LLM provider.",
  },
  {
    icon: GitBranch,
    title: "Branchable sessions",
    body: "Every state is a DAG node. Fork, A/B compare, merge, and revert are first-class — not buried behind an undo stack.",
  },
  {
    icon: KeyRound,
    title: "Bring your own LLM",
    body: "Anthropic, OpenRouter, or OpenAI keys. Stored in your OS keychain. Swap providers any time without reinstalling.",
  },
  {
    icon: Laptop,
    title: "Mac and Windows",
    body: "Signed installers for macOS (Apple Silicon + Intel) and Windows 10/11. Auto-update friendly.",
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
            Six pillars that make edytlab feel different from cleanup tools and
            preset chains.
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
