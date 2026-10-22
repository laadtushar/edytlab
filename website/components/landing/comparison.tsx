"use client";

import { useRef } from "react";
import { Check } from "lucide-react";

import { AgentLoop } from "@/components/landing/agent-loop";
import { Reveal, Stagger } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, DRAWABLE, undash } from "@/lib/gsap";

type CellValue = "yes" | string;

interface Row {
  label: string;
  /** What edytlab does about it, as a tick or a short word. */
  edytlab: CellValue;
  /** One line a reader can check against the app, its docs or the issue it names. */
  detail: string;
}

/**
 * About edytlab alone. This used to be a three-way table (a "Traditional
 * DAW" column, an "AI Audio Tools" column and edytlab), and the two
 * competitor columns were category verdicts with no source — one of
 * them, "MCP extensibility: no" for AI tools, is contradicted by a
 * competitor's own help page. A claim about a product class is true or
 * false of each member, and nobody had checked each member, so those
 * cells are gone rather than hedged. Named, sourced, dated comparisons
 * belong on pages of their own, one competitor each.
 *
 * Every row here is something the app or its docs shows. The two "Not
 * yet" rows are the same limits the feature grid and the FAQ state.
 */
const rows: Row[] = [
  {
    label: "Audio stays on your machine",
    edytlab: "yes",
    detail:
      "Only the chat goes to the provider you pick. With Ollama that can stay on your machine too.",
  },
  {
    label: "Natural-language input",
    edytlab: "yes",
    detail: "Describe the edit in chat; the agent calls the audio tools to make it.",
  },
  {
    label: "Session branching",
    edytlab: "yes",
    detail: "Fork, A/B compare and revert. Undo follows the path you took.",
  },
  {
    label: "Bring your own LLM key",
    edytlab: "yes",
    detail: "Anthropic, OpenAI, OpenRouter, Groq or Gemini with your key; Ollama needs none.",
  },
  {
    label: "MCP client",
    edytlab: "yes",
    detail: "Register MCP servers in Settings to give the agent more tools.",
  },
  {
    label: "Stem separation",
    edytlab: "Not yet",
    detail: "Wired in as a tool; the model inference has not shipped (#385).",
  },
  {
    label: "Transcription and text-based editing",
    edytlab: "Not yet",
    detail: "The editing is built; the transcription it needs has not shipped (#384).",
  },
];

/** edytlab's cell: a tick, drawn in on scroll, or a short word. */
function Cell({ value }: { value: CellValue }) {
  const base = "flex items-center justify-center py-3.5 text-sm text-foreground font-medium";

  if (value === "yes")
    return (
      <div className={base}>
        <Check data-tick="" className="size-4 text-primary" />
      </div>
    );
  return <div className={base}>{value}</div>;
}

/**
 * What edytlab does today, and what it does not yet, in one section.
 *
 * This was two: a "Problem" band that said DAWs are hard and AI tools
 * are shallow, and then a table that said the same thing again row by
 * row. Both were verdicts on product classes with nothing behind them,
 * so the heading and the table now say only what edytlab does, with the
 * agent loop drawn between them.
 *
 * The rows arrive one by one; once they have, edytlab's ticks are
 * written in down the column (DrawSVG), so the eye runs down the one
 * column that matters.
 */
export function Comparison() {
  const table = useRef<HTMLDivElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        const ticks = table.current?.querySelectorAll(`[data-tick] :is(${DRAWABLE})`);
        if (!ticks?.length) return;
        gsap.fromTo(
          ticks,
          { drawSVG: "0%" },
          {
            drawSVG: "100%",
            duration: 0.35,
            stagger: 0.09,
            delay: 0.45,
            ease: "power2.out",
            onComplete: undash(ticks),
            scrollTrigger: { trigger: table.current, start: "top 75%", once: true },
          },
        );
      });
      return () => mm.revert();
    },
    { scope: table },
  );

  return (
    <section id="compare" className="border-y border-border/50 bg-secondary/20 py-20 md:py-28">
      <div className="container">
        <SectionHeading
          eyebrow="What it does"
          title="What edytlab does today, and what it does not yet."
          lead="edytlab is an agent layer over a real audio engine: it plans, executes and iterates, in a session you can steer. Everything in the table is about edytlab alone, and can be checked in the app and the docs."
        />

        <AgentLoop />

        <Reveal>
          <div
            ref={table}
            className="mx-auto max-w-4xl overflow-x-auto rounded-xl border border-border/60"
          >
            <table className="w-full min-w-[560px] border-collapse text-sm">
              <thead>
                <tr className="border-b border-border/60">
                  <th className="w-[30%] py-4 pl-6 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                    Feature
                  </th>
                  <th className="relative w-[18%] py-4 text-center text-xs font-semibold uppercase tracking-wider text-primary">
                    <span className="relative">
                      edytlab
                      <span className="absolute -top-1 -right-2 flex size-1.5 rounded-full bg-primary" />
                    </span>
                  </th>
                  <th className="w-[52%] py-4 pl-4 pr-6 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                    What that means
                  </th>
                </tr>
              </thead>
              <Stagger
                as="tbody"
                selector="tr"
                each={0.06}
                distance={12}
              >
                {rows.map((row, i) => (
                  <tr
                    key={row.label}
                    className={`border-b border-border/40 transition-colors last:border-0 hover:bg-primary/5 ${i % 2 === 0 ? "" : "bg-secondary/10"}`}
                  >
                    <td className="py-1 pl-6 pr-4 text-sm font-medium text-foreground/80">
                      {row.label}
                    </td>
                    <td className="text-center">
                      <Cell value={row.edytlab} />
                    </td>
                    <td className="py-3 pl-4 pr-6 text-sm text-muted-foreground">
                      {row.detail}
                    </td>
                  </tr>
                ))}
              </Stagger>
            </table>
          </div>
        </Reveal>
      </div>
    </section>
  );
}
