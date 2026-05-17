"use client";

import { Check, Minus, X } from "lucide-react";
import { motion } from "framer-motion";

import { FadeIn } from "./fade-in";

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
  { label: "Stem separation",         daw: "partial",     aiTool: "partial",   edytlab: "yes"     },
  { label: "MCP extensibility",       daw: "no",          aiTool: "no",        edytlab: "yes"     },
];

function Cell({ value, highlight }: { value: CellValue; highlight?: boolean }) {
  const base = "flex items-center justify-center py-3.5 text-sm";
  const hl = highlight ? "text-foreground font-medium" : "text-muted-foreground";

  if (value === "yes")
    return (
      <div className={`${base} ${hl}`}>
        <Check className={`size-4 ${highlight ? "text-primary" : "text-muted-foreground/70"}`} />
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

const rowVariants = {
  hidden: { opacity: 0, x: -16 },
  visible: (i: number) => ({
    opacity: 1,
    x: 0,
    transition: { delay: i * 0.06, duration: 0.45, ease: [0.21, 0.47, 0.32, 0.98] },
  }),
};

export function Comparison() {
  return (
    <section className="py-20 md:py-28">
      <div className="container">
        <FadeIn className="mx-auto mb-12 max-w-2xl text-center">
          <p className="text-sm font-semibold uppercase tracking-wider text-primary">
            How it stacks up
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            Pro quality. Zero friction.
          </h2>
          <p className="mt-3 text-muted-foreground">
            The column that matters is the one with every row filled.
          </p>
        </FadeIn>

        <FadeIn>
          <div className="mx-auto max-w-4xl overflow-x-auto rounded-xl border border-border/60">
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
              <motion.tbody
                initial="hidden"
                whileInView="visible"
                viewport={{ once: true, amount: 0.2 }}
              >
                {rows.map((row, i) => (
                  <motion.tr
                    key={row.label}
                    custom={i}
                    variants={rowVariants}
                    className={`border-b border-border/40 last:border-0 ${i % 2 === 0 ? "" : "bg-secondary/10"}`}
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
                  </motion.tr>
                ))}
              </motion.tbody>
            </table>
          </div>
        </FadeIn>
      </div>
    </section>
  );
}
