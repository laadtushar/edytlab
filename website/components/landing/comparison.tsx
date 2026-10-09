"use client";

import { useRef } from "react";
import { Check, Minus, X } from "lucide-react";

import { AgentLoop } from "@/components/landing/agent-loop";
import { Reveal, Stagger } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, DRAWABLE } from "@/lib/gsap";

type CellValue = "yes" | "no" | "partial" | string;

interface Row {
  label: string;
  daw: CellValue;
  aiTool: CellValue;
  edytlab: CellValue;
}

const rows: Row[] = [
  { label: "Learning curve",         daw: "100+ hours",  aiTool: "Minutes",   edytlab: "Minutes" },
  { label: "Audio quality",          daw: "yes",         aiTool: "no",        edytlab: "yes"     },
  { label: "Privacy — local audio",  daw: "yes",         aiTool: "no",        edytlab: "yes"     },
  { label: "Natural language input",  daw: "no",          aiTool: "partial",   edytlab: "yes"     },
  { label: "Session branching",       daw: "no",          aiTool: "no",        edytlab: "yes"     },
  { label: "BYO LLM key",            daw: "no",          aiTool: "no",        edytlab: "yes"     },
  { label: "Stem separation",         daw: "partial",     aiTool: "partial",   edytlab: "Not yet" },
  { label: "MCP extensibility",       daw: "no",          aiTool: "no",        edytlab: "yes"     },
];

function Cell({ value, highlight }: { value: CellValue; highlight?: boolean }) {
  const base = "flex items-center justify-center py-3.5 text-sm";
  const hl = highlight ? "text-foreground font-medium" : "text-muted-foreground";

  if (value === "yes")
    return (
      <div className={`${base} ${hl}`}>
        <Check
          data-tick={highlight ? "" : undefined}
          className={`size-4 ${highlight ? "text-primary" : "text-muted-foreground/70"}`}
        />
      </div>
    );
  if (value === "no")
    return (
      <div className={`${base} ${hl}`}>
        <X className="size-4 text-muted-foreground/40" />
      </div>
    );
  if (value === "partial")
    return (
      <div className={`${base} ${hl}`}>
        <Minus className="size-4 text-muted-foreground/60" />
      </div>
    );

  return <div className={`${base} ${hl}`}>{value}</div>;
}

/**
 * The gap edytlab sits in, and the evidence for it, in one section.
 *
 * This was two: a "Problem" band that said DAWs are hard and AI tools
 * are shallow, and then this table, which says the same thing again
 * row by row. The claim now heads the table it is a summary of, with
 * the agent loop drawn between them — "the agent layer in between" as
 * a picture.
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
            clearProps: "strokeDasharray,strokeDashoffset",
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
          eyebrow="The gap"
          title="Pro DAWs are powerful but slow to learn. AI tools are easy but shallow."
          lead="edytlab is the agent layer in between: it plans, executes and iterates over a real audio engine, in a session you can trust and steer."
        />

        <AgentLoop />

        <Reveal>
          <div
            ref={table}
            className="mx-auto max-w-4xl overflow-x-auto rounded-xl border border-border/60"
          >
            <table className="w-full min-w-[480px] border-collapse text-sm">
              <thead>
                <tr className="border-b border-border/60">
                  <th className="w-[40%] py-4 pl-6 text-left text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                    Feature
                  </th>
                  <th className="w-[20%] py-4 text-center text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                    Traditional DAW
                  </th>
                  <th className="w-[20%] py-4 text-center text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                    AI Audio Tools
                  </th>
                  <th className="relative w-[20%] py-4 text-center text-xs font-semibold uppercase tracking-wider text-primary">
                    <span className="relative">
                      edytlab
                      <span className="absolute -top-1 -right-2 flex size-1.5 rounded-full bg-primary" />
                    </span>
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
                      <Cell value={row.daw} />
                    </td>
                    <td className="text-center">
                      <Cell value={row.aiTool} />
                    </td>
                    <td className="text-center">
                      <Cell value={row.edytlab} highlight />
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
