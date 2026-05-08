import Link from "next/link";

import { Card, CardContent } from "@/components/ui/card";

import { FadeIn } from "./fade-in";

const providers = [
  {
    name: "Anthropic",
    body: "Claude Sonnet & Haiku. Best tool-use quality.",
    href: "https://console.anthropic.com/settings/keys",
  },
  {
    name: "OpenRouter",
    body: "One key, dozens of models. Route by cost or capability.",
    href: "https://openrouter.ai/keys",
  },
  {
    name: "OpenAI",
    body: "GPT-class models. Drop-in if your team already has access.",
    href: "https://platform.openai.com/api-keys",
  },
];

export function ProviderCards() {
  return (
    <section id="providers" className="border-y border-border/50 bg-secondary/20 py-20 md:py-28">
      <div className="container">
        <FadeIn className="mx-auto mb-12 max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Bring your own key. Switch any time.
          </h2>
          <p className="mt-3 text-muted-foreground">
            Keys live in your OS keychain. We never proxy your audio or your
            tokens.
          </p>
        </FadeIn>
        <div className="mx-auto grid max-w-4xl gap-4 sm:grid-cols-3">
          {providers.map((p, i) => (
            <FadeIn key={p.name} delay={i * 0.05}>
              <Link
                href={p.href}
                target="_blank"
                rel="noopener noreferrer"
                className="block h-full"
              >
                <Card className="h-full border-border/60 bg-card/60 transition-colors hover:border-primary/40 hover:bg-card">
                  <CardContent className="flex h-full flex-col gap-2 p-6">
                    <div className="text-lg font-semibold">{p.name}</div>
                    <p className="text-sm text-muted-foreground">{p.body}</p>
                    <div className="mt-auto pt-4 text-xs font-medium text-primary">
                      Get a key →
                    </div>
                  </CardContent>
                </Card>
              </Link>
            </FadeIn>
          ))}
        </div>
      </div>
    </section>
  );
}
