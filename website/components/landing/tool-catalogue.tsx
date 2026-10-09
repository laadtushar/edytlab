/**
 * What the agent can actually call.
 *
 * A tool count is a number nobody believes and nobody can check. Naming
 * the tools is the difference between a claim and a fact. The count
 * itself is quoted once, in the stats strip; this section is where it
 * is backed up, so it does not repeat the number — it shows the names.
 *
 * A highlight reel, not the whole registry: the tools reference at
 * `/docs/tools` is the exhaustive list, and the copy here says so
 * rather than implying that anything missing from this page is missing
 * from the app.
 *
 * Kept in step with `crates/tools/src/dispatcher.rs`, and guarded:
 * `website_tool_docs.rs` fails if this list names a tool the agent
 * cannot call, and if any page on the site quotes a count the registry
 * disagrees with. Treat the dispatcher as the source of truth.
 *
 * There used to be a marquee of these same names scrolling above the
 * grid — the list twice, one copy unreadable. The grid is the list.
 *
 * Motion: the groups and their chips cascade in as they scroll up, and
 * the filter above them narrows the grid to one group with Flip — the
 * groups that stay glide to their new places, the others fade out, so
 * the reader can see where things went instead of the grid jumping.
 * Without a script the filter is inert and every group is shown.
 */

"use client";

import Link from "next/link";
import { useRef, useState } from "react";
import { ArrowRight } from "lucide-react";

import { Cascade, Reveal } from "@/components/motion";
import { SectionHeading } from "@/components/motion/section-heading";
import { gsap, useGSAP, NO_PREFERENCE } from "@/lib/gsap";
import { Flip } from "@/lib/gsap-flip";

const GROUPS = [
  {
    name: "Level",
    tools: [
      "gain",
      "normalize",
      "normalize_loudness",
      "leveler",
      "compressor",
      "limiter",
      "noise_gate",
    ],
  },
  {
    name: "Tone",
    tools: ["eq", "low_pass_filter", "high_pass_filter", "notch_filter"],
  },
  {
    name: "Repair",
    tools: [
      "noise_reduction",
      "click_removal",
      "de_esser",
      "vocal_reduction",
      "truncate_silence",
    ],
  },
  {
    name: "Effects",
    tools: [
      "reverb",
      "echo",
      "distortion",
      "phaser",
      "tremolo",
      "stereo_widener",
    ],
  },
  {
    name: "Effect chains",
    tools: [
      "add_effect",
      "set_effect_params",
      "set_effect_bypassed",
      "reorder_effects",
      "remove_effect",
    ],
  },
  {
    name: "Time & pitch",
    tools: ["time_stretch", "pitch_shift", "change_speed", "align_to_beat"],
  },
  {
    name: "Arrangement",
    tools: [
      "cut_range",
      "trim",
      "split_clip",
      "move_clip",
      "remove_clip",
      "copy_region",
      "paste_region",
      "insert_silence",
      "repeat_selection",
      "time_shift",
      "set_clip_envelope",
    ],
  },
  {
    name: "Mixing",
    tools: [
      "set_track_gain",
      "set_pan",
      "mute_track",
      "solo_track",
      "create_bus",
      "set_send",
      "mix_to_new_track",
    ],
  },
  {
    name: "Analysis",
    tools: [
      "analyze_track",
      "storage_report",
      "plot_spectrum",
      "silence_finder",
      "transcribe",
      "separate_stems",
    ],
  },
  {
    name: "History",
    tools: ["fork_node", "compare_nodes", "revert_to", "apply_diff", "name_node"],
  },
  {
    name: "Export",
    tools: ["render_preview", "render_final", "export_multiple", "export_labels"],
  },
];

export function ToolCatalogue() {
  const [only, setOnly] = useState<string | null>(null);
  const grid = useRef<HTMLDivElement>(null);
  const before = useRef<Flip.FlipState | null>(null);

  const choose = (name: string | null) => {
    // Record where every group is *before* React changes the grid, so
    // Flip can animate from there. Skipped with reduced motion: the
    // filter then simply applies.
    if (grid.current && window.matchMedia(NO_PREFERENCE).matches) {
      before.current = Flip.getState(grid.current.querySelectorAll("[data-group]"));
    }
    setOnly((current) => (current === name ? null : name));
  };

  useGSAP(
    () => {
      const state = before.current;
      if (!state) return;
      before.current = null;
      Flip.from(state, {
        duration: 0.55,
        ease: "power3.inOut",
        absolute: true,
        onEnter: (els) =>
          gsap.fromTo(els, { opacity: 0, scale: 0.96 }, { opacity: 1, scale: 1, duration: 0.4 }),
        onLeave: (els) => gsap.to(els, { opacity: 0, scale: 0.96, duration: 0.3 }),
      });
    },
    { dependencies: [only], scope: grid },
  );

  return (
    <section
      id="tools"
      className="border-y border-border/50 bg-secondary/20 py-20 md:py-28"
    >
      <div className="container">
        <SectionHeading
          eyebrow="The toolbox"
          title="Named, not counted."
          lead="Ask in plain language and the agent picks the tool. These are the ones you will reach for most; the tools reference lists every one, and if something is not there the agent cannot do it — and will tell you so rather than pretend."
        />

        <Cascade
          className="mx-auto mb-10 flex max-w-4xl flex-wrap justify-center gap-1.5"
          each={0.02}
          distance={8}
        >
          {[null, ...GROUPS.map((g) => g.name)].map((name) => {
            const pressed = name === null ? only === null : only === name;
            return (
              <button
                key={name ?? "all"}
                type="button"
                data-press
                aria-pressed={pressed}
                onClick={() => choose(name)}
                className={`rounded-full border px-3 py-1 font-mono text-[11px] uppercase tracking-wider transition-colors ${
                  pressed
                    ? "border-primary/60 bg-primary/15 text-primary"
                    : "border-border/60 text-muted-foreground hover:border-primary/40 hover:text-foreground"
                }`}
              >
                {name ?? "All"}
              </button>
            );
          })}
        </Cascade>

        <div ref={grid}>
          <Cascade
            className="mx-auto grid max-w-6xl gap-x-8 gap-y-8 sm:grid-cols-2 lg:grid-cols-3"
            selector=":scope > [data-group] > h3, :scope > [data-group] li"
            each={0.012}
            distance={8}
          >
            {GROUPS.map((g) => (
              <div
                key={g.name}
                data-group
                data-flip-id={g.name}
                className={only && only !== g.name ? "hidden" : undefined}
              >
                <h3 className="mb-3 font-mono text-[11px] uppercase tracking-widest text-muted-foreground">
                  {g.name}
                </h3>
                <ul className="flex flex-wrap gap-1.5">
                  {g.tools.map((t) => (
                    <li
                      key={t}
                      className="rounded border border-border/60 bg-card px-2 py-1 font-mono text-[11px] text-foreground/80 transition-colors hover:border-primary/50 hover:text-foreground"
                    >
                      {t}
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </Cascade>
        </div>

        <Reveal className="mx-auto mt-12 max-w-2xl text-center text-sm text-muted-foreground" distance={12}>
          <p>
            Plus track and format management — add, remove, rename and duplicate
            tracks, resample, mono/stereo conversion, tone and noise
            generators, markers and label import/export.
          </p>
          <Link
            href="/docs/tools"
            data-nudge="4"
            className="group mt-4 inline-flex items-center gap-1.5 font-medium text-primary"
          >
            The full tools reference
            <ArrowRight className="size-3.5" />
          </Link>
        </Reveal>
      </div>
    </section>
  );
}
