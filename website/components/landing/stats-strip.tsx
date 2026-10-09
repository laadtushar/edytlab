"use client";

import { useRef } from "react";

import { Counter, Stagger } from "@/components/motion";
import { gsap, useGSAP, motionOk, NO_PREFERENCE, REVEAL_START } from "@/lib/gsap";
import "@/lib/gsap-text";

/**
 * The numbers, said once.
 *
 * Each figure is the headline of a section further down — the toolbox,
 * the provider cards, the first FAQ answer — and those sections show
 * the detail rather than quoting the figure again. There were four:
 * "0 bytes uploaded" and "100% on-device" were the same fact twice, so
 * it is three.
 *
 * Motion: the three stats settle in a stagger, each figure counts up to
 * its value, and its unit resolves out of noise beside it like a
 * readout settling (ScrambleText). The markup holds the final text, so
 * none of this is needed to read them.
 *
 * `website_tool_docs.rs` checks that the tool count here is the
 * registry's, as a quoted `"N"`.
 */
const stats = [
  { value: "0", unit: "bytes", label: "of your audio uploaded — the DSP runs on your machine" },
  { value: "6", unit: "providers", label: "bring your own key, or run a local model" },
  { value: "93", unit: "tools", label: "the agent can call, from fade to beat-warp" },
];

export function StatsStrip() {
  const ref = useRef<HTMLElement>(null);

  useGSAP(
    () => {
      const mm = motionOk();
      mm.add(NO_PREFERENCE, () => {
        gsap.to("[data-unit]", {
          scrambleText: { text: "{original}", chars: "lowerCase", speed: 0.5 },
          duration: 1.1,
          delay: 0.2,
          stagger: 0.12,
          ease: "none",
          scrollTrigger: { trigger: ref.current, start: REVEAL_START, once: true },
        });
      });
      return () => mm.revert();
    },
    { scope: ref },
  );

  return (
    <section ref={ref} className="relative py-14 md:py-16">
      <div className="container">
        <Stagger
          as="dl"
          className="mx-auto grid max-w-4xl gap-x-8 gap-y-10 sm:grid-cols-3"
          each={0.09}
          scale
        >
          {stats.map((s) => (
            <div key={s.unit} className="flex flex-col items-center gap-1 text-center">
              <div className="flex items-baseline gap-1.5">
                <dt className="text-4xl font-bold tracking-tight text-foreground sm:text-5xl">
                  <Counter value={s.value} />
                </dt>
                {/* An invisible copy holds the width, and the visible
                    unit sits over it — so the scramble, whose letters
                    are wider and narrower than the real ones, never
                    reflows the figure beside it. */}
                <span className="relative inline-block whitespace-nowrap text-sm font-semibold text-primary">
                  <span aria-hidden className="invisible">
                    {s.unit}
                  </span>
                  <span data-unit className="absolute left-0 top-0">
                    {s.unit}
                  </span>
                </span>
              </div>
              <dd className="max-w-[16rem] text-sm text-muted-foreground">{s.label}</dd>
            </div>
          ))}
        </Stagger>
      </div>
      <div className="rule-fade container mt-14 max-w-5xl" />
    </section>
  );
}
