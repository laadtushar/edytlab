import Link from "next/link";
import { ArrowUpRight, Cloud, Laptop } from "lucide-react";

import { LineDraw, Stagger } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";

/**
 * The one place the page talks about models.
 *
 * Three other spots used to: a "Bring your own LLM" feature card, and
 * two FAQ answers ("Which LLM models work?", "Can I use a local LLM?")
 * that listed these same six providers again. Their detail — keychain,
 * per-model agent profiles, switching without a reinstall, what a local
 * model needs — is folded in here, next to the providers it is about.
 */
const providers = [
  {
    name: "Anthropic",
    body: "Sonnet & Haiku. Best tool-use quality.",
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
  {
    name: "Google Gemini",
    body: "Long context, generous free tier.",
    href: "https://aistudio.google.com/apikey",
  },
  {
    name: "Groq",
    body: "Open models at very low latency.",
    href: "https://console.groq.com/keys",
  },
  {
    name: "Ollama",
    body: "Runs on your own machine. No key, no account — not even the chat leaves the computer. It gets a compact tool set sized for small contexts, and every other tool is still there when you name it; pick a model trained for tool use to drive them reliably.",
    href: "https://ollama.com",
    cta: "Get Ollama",
    local: true,
  },
];

export function ProviderCards() {
  return (
    <section id="providers" className="border-y border-border/50 bg-secondary/20 py-20 md:py-28">
      <div className="container">
        <SectionHeading
          eyebrow="Models"
          title="Bring your own key. Switch any time."
          lead="Keys live in your OS keychain, and we never proxy your audio or your tokens. Change provider, model or per-model agent profile from Settings, without reinstalling."
        />
        {/* Cards stagger in and draw their icons — a cloud where the
            model is hosted, a laptop where it runs on yours — then lift
            and glow under the pointer. */}
        <LineDraw>
          <Stagger
            className="mx-auto grid max-w-5xl gap-4 sm:grid-cols-2 lg:grid-cols-3"
            each={0.06}
            distance={20}
          >
            {providers.map((p) => {
              const Where = p.local ? Laptop : Cloud;
              return (
                <Link
                  key={p.name}
                  href={p.href}
                  target="_blank"
                  rel="noopener noreferrer"
                  data-lift
                  className="ring-hover group flex h-full flex-col gap-2 rounded-xl border border-border/60 bg-card/60 p-6 hover:bg-card"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="text-lg font-semibold">{p.name}</div>
                    <Where
                      className="size-5 shrink-0 text-muted-foreground transition-colors group-hover:text-primary"
                    />
                  </div>
                  <p className="text-sm text-muted-foreground">{p.body}</p>
                  <div className="mt-auto inline-flex items-center gap-1 pt-4 text-xs font-medium text-primary">
                    {p.cta ?? "Get a key"}
                    <ArrowUpRight className="size-3.5 transition-transform duration-300 group-hover:-translate-y-0.5 group-hover:translate-x-0.5" />
                  </div>
                </Link>
              );
            })}
          </Stagger>
        </LineDraw>
      </div>
    </section>
  );
}
