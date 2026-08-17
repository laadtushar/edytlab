"use client";

/**
 * What the agent can actually call.
 *
 * "75 tools" is a number nobody believes and nobody can check. Naming
 * them is the difference between a claim and a fact, and it doubles as
 * the honest answer to "can it do X?" — if X is not on this list, it
 * cannot.
 *
 * Kept in step with `crates/tools/src/dispatcher.rs`. The repo has a
 * test (`tool_badge_labels.rs`) that fails when the desktop app's label
 * map drifts from the registry; this page has no such guard, so treat
 * the dispatcher as the source of truth when editing.
 */

import { motion } from "framer-motion";

import { FadeIn } from "./fade-in";

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
  return (
    <section
      id="tools"
      className="border-y border-border/50 bg-secondary/20 py-20 md:py-28"
    >
      <div className="container">
        <FadeIn className="mx-auto mb-12 max-w-2xl text-center">
          <p className="font-mono text-xs uppercase tracking-widest text-primary">
            The toolbox
          </p>
          <h2 className="mt-3 text-balance text-3xl font-semibold tracking-tight sm:text-4xl">
            81 tools the agent can reach for.
          </h2>
          <p className="mt-4 text-pretty text-lg text-muted-foreground">
            Named, not counted. Ask in plain language and the agent picks — but
            if something is not on this list, it cannot do it, and it will tell
            you so rather than pretend.
          </p>
        </FadeIn>

        <div className="mx-auto grid max-w-6xl gap-x-8 gap-y-8 sm:grid-cols-2 lg:grid-cols-3">
          {GROUPS.map((g, gi) => (
            <FadeIn key={g.name} delay={gi * 0.05}>
              <h3 className="mb-3 font-mono text-[11px] uppercase tracking-widest text-muted-foreground">
                {g.name}
              </h3>
              <ul className="flex flex-wrap gap-1.5">
                {g.tools.map((t, i) => (
                  <motion.li
                    key={t}
                    initial={{ opacity: 0, y: 8 }}
                    whileInView={{ opacity: 1, y: 0 }}
                    viewport={{ once: true, margin: "-40px" }}
                    transition={{
                      duration: 0.35,
                      delay: i * 0.02,
                      ease: [0.21, 0.47, 0.32, 0.98],
                    }}
                    className="rounded border border-border/60 bg-card px-2 py-1 font-mono text-[11px] text-foreground/80"
                  >
                    {t}
                  </motion.li>
                ))}
              </ul>
            </FadeIn>
          ))}
        </div>

        <FadeIn delay={0.2}>
          <p className="mx-auto mt-12 max-w-2xl text-center text-sm text-muted-foreground">
            Plus track and format management — add, remove, rename and duplicate
            tracks, resample, mono/stereo conversion, tone and noise
            generators, markers and label import/export.
          </p>
        </FadeIn>
      </div>
    </section>
  );
}
