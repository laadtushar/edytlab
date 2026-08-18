"use client";

import { Counter, Stagger } from "@/components/motion";

const stats = [
  { value: "0", unit: "bytes", label: "audio uploaded to any server" },
  { value: "6", unit: "providers", label: "Anthropic · OpenAI · Gemini · Groq · OpenRouter · Ollama" },
  { value: "89", unit: "tools", label: "the agent can call, from fade to beat-warp" },
  { value: "100%", unit: "on-device", label: "DSP runs locally, always" },
];

export function StatsStrip() {
  return (
    <section className="relative py-14 md:py-16">
      <div className="container">
        <Stagger
          as="dl"
          className="mx-auto grid max-w-5xl grid-cols-2 gap-x-8 gap-y-10 md:grid-cols-4"
          each={0.09}
          scale
        >
          {stats.map((s) => (
            <div key={s.label} className="flex flex-col items-center gap-1 text-center">
              <div className="flex items-baseline gap-1.5">
                <dt className="text-4xl font-bold tracking-tight text-foreground sm:text-5xl">
                  <Counter value={s.value} />
                </dt>
                <span className="text-sm font-semibold text-primary">{s.unit}</span>
              </div>
              <dd className="text-sm text-muted-foreground">{s.label}</dd>
            </div>
          ))}
        </Stagger>
      </div>
      <div className="rule-fade container mt-14 max-w-5xl" />
    </section>
  );
}
