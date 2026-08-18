"use client";

import { useRef } from "react";
import { FileAudio, MessagesSquare, FileDown } from "lucide-react";

import { Reveal, Stagger } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE } from "@/lib/gsap";

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

export function HowItWorks() {
  const ref = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        gsap.from("[data-connector]", {
          scaleX: 0,
          opacity: 0,
          duration: 1,
          delay: 0.25,
          scrollTrigger: { trigger: ref.current, start: "top 80%", once: true },
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <section id="how-it-works" className="py-20 md:py-28">
      <div className="container">
        <Reveal className="mx-auto mb-14 max-w-2xl text-center">
          <p className="text-sm font-semibold uppercase tracking-wider text-primary">
            How it works
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Three steps. No DAW manual required.
          </h2>
        </Reveal>
        <div ref={ref} className="relative mx-auto max-w-5xl">
          {/* The connector draws itself left to right as the steps
              arrive, so the eye is led along the sequence rather than
              landing on three cards at once. */}
          <div className="absolute left-0 right-0 top-14 hidden md:block">
            <div
              data-connector
              className="mx-auto h-px origin-left bg-gradient-to-r from-transparent via-primary/40 to-transparent"
            />
          </div>

          <Stagger className="grid gap-5 md:grid-cols-3" each={0.12} distance={28} scale>
            {steps.map((s) => (
              <div
                key={s.title}
                className="ring-hover surface group relative h-full rounded-xl border border-border/60 p-6 backdrop-blur"
              >
                <div className="mb-4 flex size-12 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20 transition-transform duration-300 group-hover:-rotate-6 group-hover:scale-110">
                  <s.icon className="size-5" />
                </div>
                <h3 className="text-lg font-semibold">{s.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                  {s.body}
                </p>
              </div>
            ))}
          </Stagger>
        </div>
      </div>
    </section>
  );
}
