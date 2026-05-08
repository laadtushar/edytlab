import { FadeIn } from "./fade-in";

export function Problem() {
  return (
    <section className="border-y border-border/50 bg-secondary/30 py-20">
      <div className="container">
        <FadeIn className="mx-auto max-w-3xl text-center">
          <p className="text-sm font-semibold uppercase tracking-wider text-primary">
            The gap
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Pro DAWs are powerful but take 100+ hours to learn. AI tools are
            easy but shallow.
          </h2>
          <p className="mt-6 text-pretty text-lg text-muted-foreground">
            Nobody offers conversational, multi-track production at professional
            DSP quality. edytlab is the agent layer that plans, executes, and
            iterates over Demucs, Whisper, and Rubber Band — in a session you
            can actually trust and steer.
          </p>
        </FadeIn>
      </div>
    </section>
  );
}
