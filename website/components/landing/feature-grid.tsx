"use client";

import { FileDown, FileText, GitBranch, MessageSquare, Waves, Zap } from "lucide-react";

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

import { LineDraw, Stagger, TiltCard } from "@/components/motion";
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
 * The phrases `website_tool_docs.rs` pins to registered tools ("Text-based
 * editing, after a one-time model download", "Warp a performance onto a
 * beat grid", "Loudness-normalise", "Fork, A/B compare, and revert" …) are
 * load-bearing: reword one and update
 * that test in the same change. The text-editing headline carries its
 * limit on purpose (the model is a download, not part of the app): headings
 * are what snippets and assistants quote.
 */
const features = [
  {
    icon: FileText,
    title: "Text-based editing, after a one-time model download",
    body: "Transcribe a take on your computer and the transcript becomes the editor: delete a sentence and the audio is cut and the gap closed, as an ordinary undoable edit. It needs the speech model (about 139 MB), which you download once in Settings; the words it finds reach your AI provider as text, unless you use Ollama.",
  },
  {
    icon: MessageSquare,
    title: "Conversational multi-track",
    body: "Match B's tempo to A's, start it eight bars before A ends, crossfade, give me three takes. The agent plans (and can show you the plan first) and renders branches you can A/B.",
  },
  {
    icon: Waves,
    title: "Pro-grade DSP",
    body: "Time-stretch and pitch-shift run on a phase vocoder written for this project, with formant preservation. Warp a performance onto a beat grid in a single pass.",
  },
  {
    icon: GitBranch,
    title: "Undo, branch & compare",
    body: "Every state is a node in a graph. Fork, A/B compare, and revert are first-class, not hidden behind a linear undo stack.",
  },
  {
    icon: FileDown,
    title: "Export that plays anywhere",
    body: "WAV, FLAC or MP3. Loudness-normalise to a LUFS target with a true-peak ceiling. It opens WAV, MP3, FLAC and OGG.",
  },
  {
    icon: Zap,
    title: "MCP extensibility",
    body: "Register Model Context Protocol servers from Settings to give the agent new tools, without touching core code.",
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
        {/* Cards settle in a stagger, each icon is drawn stroke by
            stroke as its card lands, and a card tilts toward the cursor
            once it is there. */}
        <LineDraw>
          <Stagger
            className="mx-auto grid max-w-6xl gap-3 sm:grid-cols-2 sm:gap-5 lg:grid-cols-3"
            each={0.06}
            distance={28}
            scale
          >
            {features.map((f) => (
              <div key={f.title} className="group">
                <TiltCard className="h-full">
                  <Card className="surface h-full border-border/60 backdrop-blur transition-colors group-hover:border-primary/40">
                    <CardHeader className="max-sm:p-4 max-sm:pb-2">
                      <div className="mb-3 hidden size-11 items-center justify-center rounded-lg sm:flex bg-primary/10 text-primary ring-1 ring-primary/20 transition-transform duration-300 group-hover:rotate-6 group-hover:scale-110">
                        <f.icon className="size-5" />
                      </div>
                      <CardTitle>{f.title}</CardTitle>
                    </CardHeader>
                    <CardContent className="max-sm:p-4 max-sm:pt-0">
                      <CardDescription className="text-sm leading-relaxed sm:text-[0.95rem]">
                        {f.body}
                      </CardDescription>
                    </CardContent>
                  </Card>
                </TiltCard>
              </div>
            ))}
          </Stagger>
        </LineDraw>
      </div>
    </section>
  );
}
