import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";

import { LineDraw, Stagger } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";

/**
 * Questions the rest of the page does not already answer.
 *
 * "Can I use a local LLM?" listed the provider cards a second time,
 * directly above them; that detail now lives in the provider section.
 * "Which LLM models work?" stays as the one-line answer people search
 * for, and `crates/ai/tests/website_provider_claims.rs` holds it to the
 * provider registry. This is also the long answer to the stats strip's
 * "0 bytes uploaded", which is why the local-first feature card could go.
 */
const faqs = [
  {
    q: "Which LLM models work?",
    a: "Six providers: Anthropic, OpenAI, OpenRouter, Groq, Gemini and Ollama. Hosted providers use your own key, stored in the OS keychain; Ollama runs models on your machine and needs no API key. Switch provider and model in Settings at any time.",
  },
  {
    q: "Does my audio leave my machine?",
    a: "No. The DSP engine runs entirely on your device — decode, editing, mixing, and rendering all happen locally, and stem separation and transcription are built to run locally too once their models ship. The only network traffic is your chat with the LLM provider you've configured. Your raw audio is never uploaded.",
  },
  {
    q: "What are the system requirements?",
    a: "macOS 11 Big Sur or later — the .dmg is a universal binary, so Apple Silicon and Intel both run it — Windows 10/11, or Linux (.deb / AppImage, x86_64). No GPU is needed: the current build runs no ML models, because stem separation and transcription have not shipped yet.",
  },
  {
    q: "Is it free?",
    a: "The app is free in BYO-key mode — you pay your LLM provider directly for tokens. A hosted subscription that bundles AI inference is on the roadmap; pricing isn't finalized.",
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
        <SectionHeading title="Frequently asked" />
        {/* Questions arrive one by one, each chevron drawing itself as
            its row lands; an answer settles in as its panel opens. */}
        <LineDraw className="mx-auto max-w-2xl">
          <Stagger selector="[data-faq-item]" each={0.07} distance={14}>
            <Accordion type="single" collapsible className="w-full">
              {faqs.map((f, i) => (
                <AccordionItem key={f.q} value={`item-${i}`} data-faq-item>
                  <AccordionTrigger>{f.q}</AccordionTrigger>
                  <AccordionContent>{f.a}</AccordionContent>
                </AccordionItem>
              ))}
            </Accordion>
          </Stagger>
        </LineDraw>
      </div>
    </section>
  );
}
