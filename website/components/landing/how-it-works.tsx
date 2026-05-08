import { FileAudio, MessagesSquare, FileDown } from "lucide-react";

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
    body: "“Mashup A over B, key-match, give me 3 takes on the drop.” The agent plans, shows the plan, and renders branches you can A/B.",
  },
  {
    icon: FileDown,
    title: "3. Export",
    body: "Pick the branch you like. Export to WAV, MP3, FLAC, or OGG with LUFS targeting. The session graph keeps every alternative.",
  },
];

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
        <div className="mx-auto grid max-w-5xl gap-5 md:grid-cols-3">
          {steps.map((s, i) => (
            <FadeIn key={s.title} delay={i * 0.08}>
              <div className="relative h-full rounded-xl border border-border/60 bg-card/40 p-6 backdrop-blur">
                <div className="mb-4 flex size-12 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20">
                  <s.icon className="size-5" />
                </div>
                <h3 className="text-lg font-semibold">{s.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                  {s.body}
                </p>
              </div>
            </FadeIn>
          ))}
        </div>
      </div>
    </section>
  );
}
