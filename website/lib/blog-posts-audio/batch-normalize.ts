import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

/** The script printed in the post, as it was run (ffmpeg 6.1.1, 2026-10-22). */
const FFMPEG_SCRIPT = `#!/usr/bin/env bash
set -euo pipefail
I=-16 TP=-1 LRA=11
mkdir -p out
for f in in/*.wav; do
  # Pass 1: measure only. Nothing is written (-f null -).
  json=$(ffmpeg -hide_banner -nostats -i "$f" \\
    -af "loudnorm=I=$I:TP=$TP:LRA=$LRA:print_format=json" -f null - 2>&1 \\
    | sed -n '/^{/,/^}/p')
  m() { jq -r ".$1" <<<"$json"; }
  # Pass 2: apply a linear gain from the measured values.
  ffmpeg -hide_banner -loglevel error -y -i "$f" -ar 48000 \\
    -af "loudnorm=I=$I:TP=$TP:LRA=$LRA:measured_I=$(m input_i):measured_LRA=$(m input_lra):measured_TP=$(m input_tp):measured_thresh=$(m input_thresh):offset=$(m target_offset):linear=true" \\
    "out/$(basename "$f")"
done`;

export const batchNormalize: BlogPost = {
  slug: "batch-normalize-audio-files-to-a-lufs-target",
  title: "Batch Normalize Audio Files to a LUFS Target",
  date: "2026-10-22",
  description:
    "Bring a folder of audio to one LUFS target three ways: ffmpeg loudnorm, an Audacity macro, and an edytlab recipe with batch_apply. Steps and caveats.",
  coverAlt:
    "Four stacked audio files whose waveforms have very different heights on the left, an arrow, and the same four files on the right with waveforms of one height",
  excerpt:
    "Bulk normalizing is easy to get almost right. Here is the ffmpeg two-pass method, the Audacity macro route and the edytlab recipe route, with what each one cannot tell you, and one default in edytlab v0.7.0 and earlier that can overwrite your originals.",
  readTime: 9,
  tags: ["batch normalize", "loudnorm", "LUFS", "recipes"],
  body: [
    {
      type: "p",
      text: "To batch normalize audio files, measure each file's integrated loudness, apply the gain that brings it to one target such as -16 LUFS, and export every file. Normalizing each file to the same peak does not do that, because files with the same peak can differ in loudness. Three routes follow: ffmpeg's `loudnorm` filter in two passes, an Audacity macro applied to a folder of files, and an edytlab recipe run with `batch_apply`. The ffmpeg script and the edytlab tool calls were run on three synthetic 12-second stereo files. The Audacity steps come from its manual and were not run.",
    },
    {
      type: "callout",
      text:
        "Work on copies. Batch tools write files, and one default of `batch_apply` in edytlab v0.7.0 and earlier can overwrite the originals ([issue #653](https://github.com/laadtushar/edytlab/issues/653), fixed in v0.8.0; explained below). Only the third route needs [edytlab](" +
        RELEASES +
        "), whose installers are unsigned, so macOS and Windows warn on first launch; [Getting Started](/docs/getting-started) walks through it.",
    },
    { type: "h2", text: "Normalize to a loudness target, not a peak" },
    {
      type: "p",
      text: "Peak normalization scales a file until its loudest sample reaches a level. The EBU's R 128 starts from the observation that peak normalisation \"has led to considerable loudness differences between programmes\" and recommends normalizing programme loudness instead. Normalizing to a loudness gives every file the same integrated loudness figure, which is the measure that platform targets use. Which number to pick is in [LUFS for Spotify, YouTube, Apple Music and podcasts](/blog/lufs-for-spotify-youtube-apple-music-podcasts).",
    },
    { type: "h2", text: "ffmpeg batch normalize with loudnorm, in two passes" },
    {
      type: "p",
      text: "FFmpeg's documentation describes `loudnorm` as EBU R128 loudness normalization with single-pass and double-pass modes. The options that matter are `I`, the integrated loudness target (default -24), `TP`, the maximum true peak (default -2), and `LRA`, the loudness range target (default 7). The two-pass method measures first, then corrects using the measured values.",
    },
    {
      type: "ol",
      items: [
        "Pass 1 runs the filter with `print_format=json` and writes nothing (`-f null -`). The filter prints the file's measured values as JSON.",
        "Pass 2 repeats the filter with `measured_I`, `measured_LRA`, `measured_TP`, `measured_thresh` and `offset` filled in from pass 1, and `linear=true`, which normalizes by scaling the source linearly.",
      ],
    },
    {
      type: "p",
      text: "The docs set conditions on `linear`. All four measured values must be given, the target LRA should not be lower than the source's, and the change in loudness should not push the true peak past the target `TP`. If a condition fails, the filter reverts to dynamic mode. In dynamic mode the stream is upsampled to 192 kHz to find true peaks, so the docs advise setting the output rate with `-ar` or `aresample`. This script does both passes for every WAV in `in/` and writes `out/`. It needs the `jq` tool to read the JSON, and `-ar 48000` should be your files' rate.",
    },
    { type: "code", text: FFMPEG_SCRIPT },
    {
      type: "p",
      text: "Run on three synthetic 12-second stereo WAVs with ffmpeg 6.1.1, it produced files that ffmpeg's own `ebur128` filter measured at -16.0, -16.0 and -15.9 LUFS. That shows the script runs and lands near the target. It says nothing about your files, so measure yours:",
    },
    {
      type: "code",
      text: "ffmpeg -nostats -i out/take1.wav -af ebur128=peak=sample+true -f null -",
    },
    { type: "h2", text: "Batch normalize audio files in Audacity with a macro" },
    {
      type: "p",
      text: "Audacity's manual describes a macro as a sequence of pre-configured commands that can be applied to projects or to audio files, and names batch processing, where many files are processed unattended and exported, as one of its three uses. The steps follow the manual and were not run for this post.",
    },
    {
      type: "ol",
      items: [
        "Open Tools > Macro Manager and create a macro.",
        "Insert Loudness Normalization (Effect > Volume and Compression) and set its parameters. The manual strongly recommends setting parameters for every command in a macro, because otherwise Audacity uses the settings from the last time you ran the effect by hand. The effect's default is perceived loudness at -23 LUFS, the EBU figure, so change it to your target.",
        "Insert an Export command so each processed file is written out. The manual says exports from a macro go into a folder called macro-output, in the place set under Directories preferences.",
        "Choose Apply Macro to Files and select the files, which must be in a single directory. The manual recommends not processing more than 500 files at a time.",
      ],
    },
    {
      type: "p",
      text: "Two notes from the same manual. Loudness Normalization has a \"Treat mono as dual-mono\" box, on by default, which the manual says differs from many other LUFS meters that read a mono track 3 LU lower. And it says this effect is normally applied as the final editing step before export.",
    },
    { type: "h2", text: "Batch normalize a folder in edytlab with a recipe" },
    {
      type: "p",
      text: "`normalize_loudness` sets one track to a target, as [the LUFS post](/blog/lufs-for-spotify-youtube-apple-music-podcasts) describes. To run it over a folder you record the step once as a recipe and replay it. `export_recipe` writes the session's edit chain, every tool and its parameters in order, to a JSON file with no audio in it. `batch_apply` runs a recipe across every audio file in a folder, and each file becomes its own project with its own history.",
    },
    { type: "prompt", text: "Load /Users/me/raw/take1.wav." },
    {
      type: "p",
      text: "One representative file is enough. The recipe's first step is this `load`, and `batch_apply` swaps in each file of the folder in turn.",
    },
    { type: "prompt", text: "Normalize track 1 to the apple_podcasts preset." },
    {
      type: "prompt",
      text: "Export this session's edit chain as a recipe to /Users/me/podcast-level.recipe.json.",
    },
    {
      type: "p",
      text: "In a run, this wrote a two-step recipe: `load` with the file's path, and `normalize_loudness` with `{\"preset\": \"apple_podcasts\", \"track\": 0}`. You can open it and read it before anything replays. Then run it:",
    },
    {
      type: "prompt",
      text: "Run /Users/me/podcast-level.recipe.json over every file in /Users/me/raw. Put the projects in /Users/me/normalized and render each one as a WAV.",
    },
    {
      type: "p",
      text: "That is `batch_apply` with `recipe_path`, `input_dir`, `output_dir` and `render_format`. Every file is attempted even if an earlier one fails, and the report says which succeeded and which were refused, with the reason. In a run with a separate `output_dir`, three renders came out, and ffmpeg's `ebur128` filter measured each at -16.0 LUFS.",
    },
    {
      type: "callout",
      text:
        "Always give `batch_apply` an `output_dir` that is not the input folder. It defaults to the input folder, puts each file's project in a folder named after the file, and renders to `<output_dir>/<file name>.<format>`. With the default and a render format that matches an input's extension, the render's target is the original file itself. In a run on the code of 2026-10-22, three WAV files batch-processed with the default `output_dir` and `render_format: wav` were all left as 44-byte headers, and all three were reported as refused. With a separate `output_dir` the same batch worked and the inputs were unchanged. This is [issue #653](https://github.com/laadtushar/edytlab/issues/653). It is fixed in v0.8.0: from that version `batch_apply` refuses, before it starts any file, a batch in which a render would land on one of the inputs, and the message tells you to set `output_dir` to another folder. On v0.7.0 and earlier, set `output_dir` and keep copies of the originals.",
    },
    {
      type: "p",
      text: "To watch loudness matching in a recording, the [mini-mix demo](/videos/dj-mini-mix) brings three tracks to about -14 LUFS. That is one session with three tracks, not a batch of separate files, but the measuring and the gain are the same tool.",
    },
    { type: "h3", text: "What the report tells you, and what it does not" },
    {
      type: "p",
      text: "For each file the report gives whether it succeeded, the project folder, how many steps were applied and the path of the render. It does not repeat each step's own output. A file whose gain `normalize_loudness` capped at its -1 dBFS ceiling therefore still counts as succeeded, and nothing in the report says it fell short of the target. Measure the renders:",
    },
    {
      type: "prompt",
      text: "Analyze the loudness of each WAV in /Users/me/normalized.",
    },
    {
      type: "p",
      text: "`batch_apply` picks up `.wav`, `.mp3`, `.flac` and `.ogg` files directly in the folder, sorted by name. Other files are skipped without comment, and subfolders are not entered. A file that does not open as a WAV is transcoded to a WAV inside its project when it loads, and a render to MP3 is lossy. [Undo is a graph](/blog/undo-is-a-graph-branches-and-ab-compare) explains why each file's own history is useful: you can go back on one file without touching the others.",
    },
    { type: "h2", text: "Which route to use" },
    {
      type: "ul",
      items: [
        "**ffmpeg** runs without a window and is scriptable, and the two passes are visible in the script. You parse its JSON yourself.",
        "**Audacity** works from its window: one macro for the whole folder, up to 500 files at a time by the manual's recommendation. The manual also points to Python scripting for more flexibility.",
        "**edytlab** takes the step as a sentence, gives you a recipe file you can read, and keeps a project with history per file. It needs an AI provider, billed by tokens unless you use a local Ollama model, and, on v0.7.0 and earlier, it needs the care with `output_dir` above.",
      ],
    },
    { type: "h2", text: "Questions about batch normalizing" },
    { type: "h3", text: "What is the difference between normalizing to a peak and to LUFS?" },
    {
      type: "p",
      text: "A peak target sets the level of the loudest sample. A LUFS target sets the integrated loudness of the whole file. Two files with the same peak can sound different in loudness, which is the EBU's reason for recommending loudness.",
    },
    { type: "h3", text: "Which LUFS target should I choose?" },
    {
      type: "p",
      text: "It depends on where the files are going. [The LUFS post](/blog/lufs-for-spotify-youtube-apple-music-podcasts) lists the figures that platforms and the EBU publish, each with its source, and says where no figure was found.",
    },
    { type: "h3", text: "Can I batch normalize MP3 files on a Mac?" },
    {
      type: "p",
      text: "edytlab's batch reads MP3 files, and there are macOS (universal), Windows and Linux builds. The installers are unsigned, so macOS warns on first launch. For ffmpeg and Audacity, see their own sites for the platforms they support.",
    },
    { type: "h3", text: "Does batch_apply change my original files?" },
    {
      type: "p",
      text: "It reads them, and writes projects and renders into `output_dir`. On v0.7.0 and earlier, the default `output_dir` and a matching render format could write over them; v0.8.0 refuses that batch instead (see the box above and issue #653). See [the local-first page](/use-cases/local-ai-audio-editor) for what stays on your computer and what goes to the AI provider.",
    },
    { type: "h3", text: "Do I need an AI key to run the edytlab route?" },
    {
      type: "p",
      text: "The agent that turns your sentences into tool calls needs a provider. A hosted provider bills tokens; Ollama runs locally and needs no key. The audio is processed on your computer either way. The tools are listed in the [tools reference](/docs/tools).",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-22. The ffmpeg script and the edytlab tool calls were run once, on synthetic audio generated for this post, with ffmpeg 6.1.1 and the edytlab tools called directly.",
    },
    {
      type: "ul",
      items: [
        source("FFmpeg Filters Documentation: loudnorm", "https://ffmpeg.org/ffmpeg-filters.html#loudnorm"),
        source("FFmpeg Filters Documentation: ebur128", "https://ffmpeg.org/ffmpeg-filters.html#ebur128-1"),
        source("Audacity Manual: Macros", "https://manual.audacityteam.org/man/macros.html"),
        source("Audacity Manual: Loudness Normalization", "https://manual.audacityteam.org/man/loudness_normalization.html"),
        source("EBU R 128, version 5, November 2023 (PDF)", "https://tech.ebu.ch/docs/r/r128.pdf"),
        source("edytlab tool reference (normalize_loudness, export_recipe, batch_apply, analyze_track, render_final)", SRC("docs/tools-reference.md"), "at commit b1678feb"),
        source("edytlab source: batch.rs, the code of batch_apply", SRC("crates/tools/src/tool/batch.rs"), "at commit b1678feb"),
        source("edytlab source: recipe.rs, what a recipe contains", SRC("crates/tools/src/recipe.rs"), "at commit b1678feb"),
        source("edytlab source: load.rs, how non-WAV files are loaded", SRC("crates/tools/src/tool/load.rs"), "at commit b1678feb"),
        source("edytlab issue #653: batch_apply with the default output_dir (fixed in v0.8.0)", "https://github.com/laadtushar/edytlab/issues/653"),
      ],
    },
  ],
};
