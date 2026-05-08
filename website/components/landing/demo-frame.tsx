import { Play } from "lucide-react";

import { FadeIn } from "./fade-in";

export function DemoFrame() {
  return (
    <section className="py-20 md:py-28">
      <div className="container">
        <FadeIn className="mx-auto max-w-5xl">
          <div className="mb-10 text-center">
            <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
              See it in action
            </h2>
            <p className="mt-3 text-muted-foreground">
              A walkthrough of the conversational mashup workflow.
            </p>
          </div>
          <div className="group relative aspect-video w-full overflow-hidden rounded-2xl border bg-card shadow-2xl">
            <div className="absolute inset-0 bg-gradient-to-br from-primary/20 via-transparent to-primary/10" />
            <div className="absolute inset-0 flex flex-col items-center justify-center gap-4">
              <div className="flex size-20 items-center justify-center rounded-full bg-primary/20 ring-1 ring-primary/40 backdrop-blur transition-transform group-hover:scale-110">
                <Play className="size-8 fill-primary text-primary" />
              </div>
              <p className="text-sm font-medium text-muted-foreground">
                Demo video coming soon
              </p>
            </div>
            {/* Future: replace placeholder with <video autoPlay muted loop playsInline poster="/demo-poster.jpg" src="/demo.mp4" /> */}
          </div>
        </FadeIn>
      </div>
    </section>
  );
}
