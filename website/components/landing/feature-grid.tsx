"use client";

import { FileDown, FileText, GitBranch, MessageSquare, Waves, Zap } from "lucide-react";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { Stagger } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";

/**
 * Six cards, one idea each.
 *
 * There were nine. Three said something another section already says
 * better: "Local-first" is the stats strip and the first FAQ answer,
 * "Bring your own LLM" is the provider section, and "Time, pitch and
 * timing" repeated the DSP card's time-stretch and formant sentence —
 * so the two DSP cards are one card now, and the other two live where
 * their detail is.
 *
 * The phrases `website_tool_docs.rs` pins to registered tools ("one-time
 * model download", "Warp a performance onto a beat grid",
 * "Loudness-normalise", "Fork, A/B compare, and revert" …) are
 * load-bearing: reword one and update that test in the same change. The
 * text-editing card carries its limit on purpose (the model is a download,
 * not part of the app).
 */
const features = [
  {
    icon: FileText,
    title: "Text editing, one-time model download",
    body: "Delete words to cut the audio; the transcript becomes the editor.",
  },
  {
    icon: MessageSquare,
    title: "Conversational multi-track",
    body: "Describe the edit; the agent plans it and renders takes to compare.",
  },
  {
    icon: Waves,
    title: "Pro-grade DSP",
    body: "Time-stretch and pitch-shift keep formants. Warp a performance onto a beat grid.",
  },
  {
    icon: GitBranch,
    title: "Undo, branch & compare",
    body: "Fork, A/B compare, and revert any state.",
  },
  {
    icon: FileDown,
    title: "Export that plays anywhere",
    body: "WAV, FLAC or MP3. Loudness-normalise to a LUFS target.",
  },
  {
    icon: Zap,
    title: "MCP extensibility",
    body: "Add Model Context Protocol servers in Settings for new agent tools.",
  },
];

export function FeatureGrid() {
  return (
    <section id="features" className="py-20 md:py-28">
      <div className="container">
        <SectionHeading
          eyebrow="Features"
          title="Built for producers who want help — not handcuffs."
        />
        {/* Six equal cards: the grid stretches every card to the tallest
            in its row, and the text is short enough never to set it. They
            rise in a small stagger (the icons are plain, never drawn on scroll, so they are visible at rest); hover or focus lifts one, tints its
            border and gives its icon a small turn (`globals.css`). */}
          <Stagger
            className="mx-auto grid max-w-6xl grid-cols-1 gap-4 sm:grid-cols-2 sm:gap-5 lg:grid-cols-3"
            each={0.05}
            distance={8}
            duration={0.4}
          >
            {features.map((f) => (
              <div key={f.title} className="group card-hover h-full rounded-lg">
                <Card className="surface h-full border-border/60 group-hover:border-primary/50 group-focus-within:border-primary/50">
                  <CardHeader className="p-5 pb-2">
                    <div className="mb-3 flex size-11 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20 motion-safe:transition-transform motion-safe:duration-200 motion-safe:ease-out motion-safe:group-hover:rotate-6 motion-safe:group-hover:scale-[1.08] motion-safe:group-focus-within:rotate-6 motion-safe:group-focus-within:scale-[1.08]">
                      <f.icon className="size-5" />
                    </div>
                    <CardTitle>{f.title}</CardTitle>
                  </CardHeader>
                  <CardContent className="p-5 pt-0">
                    <CardDescription className="text-sm leading-relaxed">{f.body}</CardDescription>
                  </CardContent>
                </Card>
              </div>
            ))}
          </Stagger>
      </div>
    </section>
  );
}
