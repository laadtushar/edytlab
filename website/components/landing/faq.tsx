import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";

import { Reveal } from "@/components/motion";

const faqs = [
  {
    q: "Does my audio leave my machine?",
    a: "No. The DSP engine runs entirely on your device — decode, stem separation, transcription, mixing, and rendering all happen locally. The only network traffic is your chat with the LLM provider you've configured. Your raw audio is never uploaded.",
  },
  {
    q: "Which LLM models work?",
    a: "Anthropic (Sonnet 4.6 / Haiku 4.5), OpenAI's tool-use-capable models, Google Gemini, Groq, and any model on OpenRouter. You can switch providers — and swap per-model agent profiles — from the settings panel without reinstalling.",
  },
  {
    q: "What are the system requirements?",
    a: "macOS 11 Big Sur or later — the .dmg is a universal binary, so Apple Silicon and Intel both run it — Windows 10/11, or Linux (.deb / AppImage). 8 GB RAM minimum, 16 GB recommended for stem separation. A GPU helps but isn't required — Metal on Mac and CUDA on Windows accelerate ML when available.",
  },
  {
    q: "Is it free?",
    a: "The app is free in BYO-key mode — you pay your LLM provider directly for tokens. A hosted subscription that bundles AI inference is on the roadmap; pricing isn't finalized.",
  },
  {
    q: "Can I use a local LLM?",
    a: "Local LLMs via Ollama (Qwen, Llama 3, etc.) are planned for v1 phase 3. Tool-calling reliability on local models is weaker, so we ship a simplified tool surface in that mode.",
  },
  {
    q: "Is it open source?",
    a: "Yes. The desktop app and audio engine live in a public GitHub repository under the MIT License, so you can use, modify and redistribute them, including commercially.",
  },
];

export function FAQ() {
  return (
    <section id="faq" className="py-20 md:py-28">
      <div className="container">
        <Reveal className="mx-auto mb-12 max-w-2xl text-center">
          <h2 className="text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Frequently asked
          </h2>
        </Reveal>
        <Reveal className="mx-auto max-w-2xl">
          <Accordion type="single" collapsible className="w-full">
            {faqs.map((f, i) => (
              <AccordionItem key={f.q} value={`item-${i}`}>
                <AccordionTrigger>{f.q}</AccordionTrigger>
                <AccordionContent>{f.a}</AccordionContent>
              </AccordionItem>
            ))}
          </Accordion>
        </Reveal>
      </div>
    </section>
  );
}
