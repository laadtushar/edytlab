import { FadeIn } from "./fade-in";

const stats = [
  { value: "0", unit: "bytes", label: "audio uploaded to any server" },
  { value: "3", unit: "providers", label: "Anthropic · OpenRouter · OpenAI" },
  { value: "∞", unit: "branches", label: "every state saved as a DAG node" },
  { value: "100%", unit: "on-device", label: "DSP runs locally, always" },
];

export function StatsStrip() {
  return (
    <section className="py-14 md:py-16">
      <div className="container">
        <FadeIn>
          <dl className="mx-auto grid max-w-5xl grid-cols-2 gap-x-8 gap-y-10 md:grid-cols-4">
            {stats.map((s) => (
              <div key={s.label} className="flex flex-col items-center gap-1 text-center">
                <div className="flex items-baseline gap-1.5">
                  <dt className="text-4xl font-bold tracking-tight text-foreground sm:text-5xl">
                    {s.value}
                  </dt>
                  <span className="text-sm font-semibold text-primary">{s.unit}</span>
                </div>
                <dd className="text-sm text-muted-foreground">{s.label}</dd>
              </div>
            ))}
          </dl>
        </FadeIn>
      </div>
    </section>
  );
}
