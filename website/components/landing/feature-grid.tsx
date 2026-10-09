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
 * The phrases `website_tool_docs.rs` pins to registered tools ("Edit
 * the words, not the waveform", "Demucs stem separation", "Warp a
 * performance onto a beat grid", "Loudness-normalise", "Fork, A/B
 * compare, and revert" …) are load-bearing: reword one and update that
 * test in the same change.
 */
const features = [
  {
    icon: FileText,
    title: "Edit the words, not the waveform",
    body: "Give a take a transcript and the transcript becomes the editor. Select a sentence to select the audio; delete it and the audio is cut and the gap closed. The remaining word timings shift to match, so the transcript keeps lining up with what you can hear. It is an ordinary edit underneath — undoable, branchable, and recorded like any other. The editing is built; the on-device transcription that produces the transcript has not shipped yet.",
  },
  {
    icon: MessageSquare,
    title: "Conversational multi-track",
    body: "Mash A's vocals over B's drums, key-match, give me three takes. The agent plans, shows you the plan, runs the multi-track mix and renders branches you can A/B — all from a single prompt.",
  },
  {
    icon: Waves,
    title: "Pro-grade DSP",
    body: "A pure-Rust audio graph (cpal · symphonia · rubato · realfft) with no C dependency in the audio path. Time-stretch and pitch-shift run on a phase vocoder written for this project, with formant preservation so a shifted voice still sounds like the same person. Warp a performance onto a beat grid in a single pass — no seam at the beats. Demucs stem separation and Whisper transcription are wired in as tools; their model inference has not shipped yet.",
  },
  {
    icon: GitBranch,
    title: "Undo, branch & compare",
    body: "Every state is a DAG node. Ctrl+Z/Y traverse the branch history. Fork, A/B compare, and revert are first-class — not hidden behind a linear undo stack.",
  },
  {
    icon: FileDown,
    title: "Export that plays anywhere",
    body: "WAV when you want the samples, FLAC for lossless at about half the size, MP3 when it has to open on anything. Loudness-normalise to a LUFS target — the number streaming platforms actually use — with a true-peak ceiling so it never clips getting there. On the way in, it opens WAV, MP3, FLAC and OGG.",
  },
  {
    icon: Zap,
    title: "MCP extensibility",
    body: "Register Model Context Protocol servers from Settings to give the agent new tools. Wire in stdio JSON-RPC servers and extend what edytlab can do without touching core code.",
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
        </LineDraw>
      </div>
    </section>
  );
}
