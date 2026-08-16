import type { Metadata } from "next";

import { LegalShell } from "@/components/landing/legal-shell";

export const metadata: Metadata = {
  title: "Changelog",
  description: "Recent updates to edytlab.",
};

interface Entry {
  version: string;
  date: string;
  bullets: string[];
}

// Static stub. Updated manually as releases land. The full git history lives on
// GitHub Releases — link below.
const entries: Entry[] = [
  {
    version: "v0.1.0",
    date: "2026-08",
    bullets: [
      "Mixer controls. Gain, pan, mute and solo are editable per track without asking the agent — none of them had a command before, so every mixing decision went through a sentence and a model round trip. Pan in particular had a balance law in the render engine and nothing in the product able to reach it.",
      "Volume automation is visible and editable. The curve, its persistence and the render integration all existed; there was no way to see or touch it. Draw points on the lane, drag them, nudge with arrows — one gesture is one undoable step, not one per pixel.",
      "Clip timeline. A track split by an interior cut used to render as one continuous lane, so the seam was invisible. Clips now appear as chips you can select, move and delete, with move_clip and remove_clip available to the agent too.",
      "align_to_beat warps audio. It recorded a beat grid that nothing read and reported success while changing nothing — the last tool in the repo doing that. It now stretches each inter-beat segment by its own ratio in a single vocoder pass, so there is no seam at the beats.",
      "Formant preservation. preserve_formants was a documented, accepted, ignored flag on two tools. Shifted voices now keep the resonances of the vocal tract where they were instead of travelling with the pitch.",
      "MP3 export, on a pure-Rust encoder — no LAME, no C dependency, no licence interaction. Bitrate is settable and defaults to 192 kbps CBR.",
      "normalize_loudness targets LUFS rather than a peak. Two files peak-normalised to the same value can differ by 10 LUFS, so peak normalisation cannot answer 'make this as loud as everything else'. Gain is capped at a true-peak ceiling and the shortfall is reported when the cap bites.",
      "Bus routing. Sends, per-bus effect chains, and the tools to reach them, so one reverb can serve several tracks.",
      "FLAC export — lossless, about half the size of WAV, sample-identical to the WAV of the same render.",
      "A time-stretch bug that could produce a full-scale output. A steady 2 kHz sine stretched 2x came back peaking at 740x the input: the overlap-add divided phase-rewritten frames by a window energy that ramps to zero at the edges. Fixed, with tests asserting that stretching redistributes energy in time and never creates any.",
      "A timing drift in every long stretch. Rounding each synthesis hop to an integer accumulated — measured on a 120-to-100 BPM warp, that put the fifth beat 10.5 ms early. Positions are now accumulated in floating point and rounded once.",
      "Ollama joins Anthropic, OpenAI, Gemini, Groq and OpenRouter — six providers, and the local one needs no key at all.",
    ],
  },
  {
    version: "v0.1.0-dev (earlier)",
    date: "2026-08",
    bullets: [
      "Audio fidelity: five tools — fade, reverse, insert_silence, copy_region and paste_region — converted seconds straight into a sample index with no interleave stride. On a stereo track that halved every span, and an odd sample count swapped left and right for the rest of the track. All five now count in frames.",
      "stereo_to_mono and mono_to_stereo wrote the source's channel count into the WAV header rather than the converted buffer's, so the result played at double speed an octave high, or half speed an octave low. The conversion maths was right all along; only the write-back was wrong.",
      "Filters no longer diverge above Nyquist. low_pass_filter(cutoff_hz: 30000) on a 44.1 kHz track put the biquad poles outside the unit circle and the render saturated into a full-scale square wave. Frequencies are now held just below Nyquist across every filter and every EQ band.",
      "Seven tools panicked outright on a reversed time range (end before start) instead of returning an error.",
      "Renders no longer drop everything after an interior cut. A track is a list of clips and the render graph only ever read the first one, so cutting ten thousand frames out of the middle of a one-second track rendered 12 000 frames instead of 38 000. Clips now mix the way tracks do, with gaps as silence.",
      "Destructive edits cover every clip of a split track. A reverb applied after an interior cut used to treat the first half and leave the second half dry, with a hard seam at the join.",
      "Cutting or splitting a clip keeps its volume automation. split_clip copied the curve across without re-basing it, so a fade-out restarted at full volume after the split; cut_range discarded the curve entirely.",
      "Extensibility: MCP servers now start automatically at launch, so a registered server contributes its tools without a manual restart. Requests are deadline-bounded and server stderr is surfaced in error messages.",
      "Agent profiles that pin a model on another provider now authenticate against that provider instead of reusing the active provider's key.",
      "MCP server editor: the args, env, and headers fields accept multi-line input correctly.",
      "Plugins: install skills and agent profiles from a GitHub repo (github:org/repo) or a local path.",
      "Eight audio skills ship pre-installed — podcast cleanup, vocal chain, loudness mastering, noise reduction, dialog enhancement, music mixing, silence cleanup, and an export guide.",
      "Agent profiles: save a model override, a tool whitelist, and a system-prompt addition, then switch between them.",
      "Skills: markdown files with always/keyword/regex triggers, editable in Settings.",
      "Memory: global and per-project notes spliced into the system prompt.",
      "Microphone recording via CPAL, captured straight to WAV.",
      "Spectrogram view toggle in the timeline.",
      "Plan steps can be edited inline before you approve them.",
      "Individual tools can be disabled from the capabilities menu.",
      "69 built-in audio tools, including reverb, echo, noise gate, limiter, de-esser, tone and noise generators, Audacity-format label import/export, and per-track export.",
      "OpenAI provider added alongside Anthropic and OpenRouter; model picker is catalogue-backed.",
      "Per-provider keychain slots; legacy unsuffixed Anthropic key still read for back-compat.",
    ],
  },
];

export default function ChangelogPage() {
  return (
    <LegalShell title="Changelog">
      <p>
        Recent landed work. The authoritative list of binaries and tags lives
        on{" "}
        <a href="https://github.com/laadtushar/edytlab/releases">
          GitHub Releases
        </a>
        .
      </p>
      {entries.map((e) => (
        <section key={e.version} className="mt-10">
          <h2>
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
          </h2>
          <ul>
            {e.bullets.map((b, i) => (
              <li key={i}>{b}</li>
            ))}
          </ul>
        </section>
      ))}
    </LegalShell>
  );
}
