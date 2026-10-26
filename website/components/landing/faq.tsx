import Link from "next/link";

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
    a: "Six API providers plus your ChatGPT plan: Anthropic, OpenAI, OpenRouter, Groq, Gemini and Ollama, which runs on your machine and needs no API key. Keys and the plan sign-in are described under Models above.",
  },
  {
    q: "Does my audio leave my machine?",
    a: "No. Decode, editing, mixing, rendering and transcription (with a speech model you download once) all run on your device. Besides your chat with the LLM provider, the app contacts only hosts you tell it to (a plugin URL, an MCP server) and Hugging Face when you click Download for the speech model. Raw audio is never uploaded; transcribed words go to the provider as chat text, and stay local with Ollama.",
    more: { href: "/use-cases/local-ai-audio-editor", label: "See what stays on your machine and what leaves" },
  },
  {
    q: "What are the system requirements?",
    a: "macOS 11 Big Sur or later (the .dmg is universal: Apple Silicon and Intel), Windows 10/11, or Linux (.deb / AppImage, x86_64). No GPU is needed.",
  },
  {
    q: "Is it free?",
    a: "The app is free. You pay your LLM provider directly for tokens, or use your ChatGPT plan's usage. A hosted subscription that bundles AI inference is on the roadmap; pricing isn't finalized.",
  },
  {
    q: "Is it open source?",
    a: "Yes. The desktop app and audio engine are in a public GitHub repository under the MIT License, free to use, modify and redistribute, including commercially.",
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
                  <AccordionContent>
                    {f.a}
                    {f.more ? (
                      <>
                        {" "}
                        <Link href={f.more.href} className="text-primary underline underline-offset-4">
                          {f.more.label}
                        </Link>
                        .
                      </>
                    ) : null}
                  </AccordionContent>
                </AccordionItem>
              ))}
            </Accordion>
          </Stagger>
        </LineDraw>
      </div>
    </section>
  );
}
