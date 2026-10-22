import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const removeSilence: BlogPost = {
  slug: "remove-silence-from-audio-without-chopping-breaths",
  title: "Remove Silence From Audio Without Chopping Breaths",
  date: "2026-10-22",
  description:
    "Set a threshold, a minimum duration and a short pause to keep. How truncate, cut and split differ, in Audacity, ffmpeg silenceremove and edytlab.",
  coverAlt:
    "Two bursts of speech with a wide gap between them above, an arrow, and the same two bursts below with the gap cut down to a short pause that is still there",
  excerpt:
    "Deleting every gap leaves speech with no air in it. This is how threshold, minimum duration and the pause you keep interact, what Audacity, ffmpeg and edytlab each do about the pause, and a way to keep one in edytlab.",
  readTime: 8,
  tags: ["remove silence", "truncate silence", "split audio", "podcast editing"],
  body: [
    {
      type: "p",
      text: "To remove silence from audio you set three things: a **threshold** (how quiet counts as silence), a **minimum duration** (how long it has to last to count) and **what stays** (nothing, or a short pause). The third decides whether the result sounds chopped. Delete every gap completely and one sentence runs into the next with no air. Audacity's Truncate Silence, ffmpeg's `silenceremove` filter and edytlab's `silence_finder` and `truncate_silence` all use these settings, and they differ on the third, which is what this post is about.",
    },
    {
      type: "callout",
      text:
        "The values below (-45 dB, 0.7 s, 0.3 s) are illustrations, not rules: breaths and room noise differ from one recording to the next, so find the silences and listen to them before you cut. The edytlab part needs [edytlab](" +
        RELEASES +
        "), whose installers are unsigned, so macOS and Windows warn on first launch ([Getting Started](/docs/getting-started)).",
    },
    { type: "h2", text: "Threshold, minimum duration and what stays" },
    {
      type: "ul",
      items: [
        "**Threshold:** the level below which audio counts as silence, in dBFS. Too low and background noise is never silent, so nothing is found. Too high and quiet speech, breaths and the ends of words count as silence and are cut. Audacity's Truncate Silence takes -20 to -80 dB, ffmpeg calls it `stop_threshold`, and edytlab's tools call it `threshold_db`.",
        "**Minimum duration:** how long the audio must stay under the threshold. Audacity calls it Duration (as short as 0.001 s), ffmpeg `stop_duration`, and edytlab `min_silence_ms`, which is 500 ms if you leave it out.",
        "**What stays:** Audacity's \"Truncate to\" keeps a length you choose, and \"Compress to\" keeps a percentage of the excess. ffmpeg's `stop_silence` keeps up to that long. edytlab's `truncate_silence` has no such setting and removes the whole silent stretch.",
      ],
    },
    { type: "h2", text: "Truncate, cut or split: what is the difference?" },
    {
      type: "p",
      text: "**Truncate** shortens each silence and leaves some of it. **Cut** removes the silent stretch altogether. **Split** divides the audio at the silence into separate pieces and removes nothing. Audacity's manual draws the same line. Truncate Silence makes silences shorter by \"deleting a section from the middle of the silent region\", while Silence Finder, which Label Sounds has since superseded, is for when you \"just want to split the recording into tracks at a specific point without removing the silences\".",
    },
    { type: "h2", text: "Truncate silence in Audacity" },
    {
      type: "p",
      text: "Audacity's manual describes Truncate Silence as reducing the length of passages whose volume stays below the threshold for at least the duration, by deleting a section from the middle of each, so some silence stays at both ends. These steps follow the manual and were not run for this post.",
    },
    {
      type: "ol",
      items: [
        "Select the audio and open Effect > Special > Truncate Silence.",
        "Set Threshold (-20 to -80 dB) and Duration, the shortest silence to act on.",
        "Choose the action. \"Truncate Detected Silence\" shortens every silence to the \"Truncate to\" length. \"Compress Excess Silence\" keeps the Duration and shrinks what is beyond it to the percentage you set, so long pauses stay longer than short ones.",
        "Click OK.",
      ],
    },
    {
      type: "p",
      text: "Two limits from the manual. Truncate Silence only removes audio and does not reduce noise in the silences it keeps. And on audio with fades it can remove the quietest part of a fade, so apply it before adding fades.",
    },
    { type: "h2", text: "Remove silence from audio with ffmpeg" },
    {
      type: "p",
      text: "FFmpeg's documentation says `silenceremove` removes silence from the beginning, middle or end of the audio. For the middle, a negative `stop_periods` makes it start over after each silence, and its examples use -1 to trim silence from beginning to end. `stop_threshold` is the level. `stop_duration` is the \"duration of silence that must exist before audio is not copied any more\". `stop_silence` is the \"max duration of silence at end that will be kept after trimming\". Detection defaults to the RMS over a 20 ms window, so a very short blip is averaged over that window and not judged sample by sample.",
    },
    {
      type: "code",
      text: "ffmpeg -i in.wav -af \"silenceremove=stop_periods=-1:stop_threshold=-45dB:stop_duration=0.1:stop_silence=0.2\" out.wav",
    },
    {
      type: "p",
      text: "Measured with `silencedetect` on a synthetic 11.25-second, voice-like file with pauses of 0.93, 1.42, 0.60 and 0.92 s, ffmpeg 6.1.1 left pauses of 0.29, 0.29, 0.32 and 0.29 s. The pause left in a long silence came out as about `stop_duration` plus `stop_silence` (0.1 + 0.2), not `stop_silence` alone: with `stop_duration=0.7` and `stop_silence=0.25`, the 0.93 s pause stayed 0.93 s. The docs' own example reads as keeping at most `stop_silence`, so check what your version leaves by running `silencedetect` on the output.",
    },
    {
      type: "p",
      text: "The threshold matters more than the rest. With `stop_threshold=-30dB` instead of -45, the same file came out about a second shorter, because the synthetic breaths in it (noise bursts about 24 dB under the voice) then counted as silence. Real breaths differ, so measure before you cut. To list silences without changing anything, `silencedetect` prints each one's start, end and duration:",
    },
    {
      type: "code",
      text: "ffmpeg -i in.wav -af silencedetect=noise=-45dB:d=0.7 -f null -",
    },
    { type: "h2", text: "Remove silence from audio in edytlab" },
    {
      type: "p",
      text: "edytlab has a finder and a cutter that use one rule. `silence_finder` is read-only and returns the silent ranges as `start_sec` and `end_sec`. `truncate_silence` removes them. Both take `threshold_db` and `min_silence_ms`. Both judge each sample frame on its own: a frame is silent when its loudest channel is under the threshold, and a run counts when it lasts at least the minimum. A single sample above the threshold ends the run, with no averaging window as in ffmpeg's default. So find first, which changes nothing:",
    },
    {
      type: "prompt",
      text: "Find the silences longer than 700 ms below -45 dB on track 1 and list them.",
    },
    {
      type: "p",
      text: "On the synthetic file above, calling the tool directly returned three ranges: 2.42 to 3.35 s, 5.03 to 6.45 s and 10.33 to 11.25 s. At -30 dB the first range started at 2.09 s instead: a 0.35-second synthetic breath, peaking near -32 dBFS, had become part of the silence. That is the whole breath problem in one number, and it is why the finder comes before the cutter.",
    },
    { type: "h3", text: "Remove the silences completely" },
    { type: "prompt", text: "Remove them." },
    {
      type: "p",
      text: "`truncate_silence` deletes each qualifying range whole. It has no setting for a pause to keep, and it joins the two sides with no fade. On the synthetic file it took the length from 11.25 s to 7.975 s, which is exactly the three ranges; the 0.6 s pause, under the 700 ms minimum, was left alone. Labels placed on the track follow the audio.",
    },
    { type: "h3", text: "Shorten the silences and keep a pause" },
    {
      type: "prompt",
      text: "Find the silences longer than 700 ms below -45 dB on track 1. Cut each one down to 0.3 s by removing the middle of it, working from the last silence to the first.",
    },
    {
      type: "p",
      text: "The assistant has what it needs in `silence_finder` and `cut_range`, which removes a range of samples and shifts the rest left. For each silence it keeps 0.15 s at each end: it cuts from the start plus 0.15 s to the end minus 0.15 s, turning seconds into samples with the sample rate. It works from the last silence to the first because each cut moves everything after it, which is how the code of `truncate_silence` orders its own cuts. That sentence has not been run through a model here. Calling `silence_finder` and `cut_range` directly with those numbers on the synthetic file left pauses of 0.30 s in all three places, and the 0.6 s pause untouched.",
    },
    {
      type: "p",
      text: "This is the \"truncate\" of Audacity's manual, assembled from two tools. Breaths are not in the ranges when the threshold is under them, so they stay. [Podcast production with an AI workflow](/blog/podcast-production-ai-workflow) puts it in the context of a whole episode.",
    },
    {
      type: "p",
      text: "No demo video removes silence. To see how a typed sentence becomes tool calls, and what the assistant reports back, the [launch video](/videos/launch-video) follows a DJ job from request to export.",
    },
    { type: "h2", text: "Split audio by silence" },
    {
      type: "p",
      text: "To split rather than remove, Audacity's route is Analyze > Label Sounds, which labels the sounds or the silences between them. Its threshold defaults to -30 dB, measured as peak, average or RMS level in 10 ms chunks, and a silence has to last 1 s by default. Then File > Export Audio with Multiple Files writes one file per label. FFmpeg's `silencedetect` gives you the times to cut at, and you cut the file yourself. In edytlab, `silence_finder` gives the ranges and `split_clip` splits a clip at a time you give, while `label` marks regions and `export_labels` writes them as Audacity-format label text. There is no step that exports each piece as its own file: `export_multiple` writes whole tracks as WAV files, one per track.",
    },
    { type: "h2", text: "Questions about removing silence" },
    { type: "h3", text: "How do I remove silence from audio for free?" },
    {
      type: "p",
      text: "Audacity's home page calls it a free, open-source audio editor, and FFmpeg's legal page says FFmpeg is licensed under the LGPL. edytlab is MIT-licensed; the only cost is the AI provider's tokens, unless you run a local Ollama model.",
    },
    { type: "h3", text: "What does truncate silence mean?" },
    {
      type: "p",
      text: "Shortening silent stretches instead of deleting them. Audacity's effect of that name has a \"Truncate to\" length. edytlab's `truncate_silence` has the same name but removes the stretch completely, which is the cut in this post's terms.",
    },
    { type: "h3", text: "Will removing silence cut my breaths?" },
    {
      type: "p",
      text: "It can, if the threshold is above the breath's level. Find the silences first with `silence_finder`, `silencedetect` or Audacity's Label Sounds, look at what falls inside them, and lower the threshold until the breaths are outside.",
    },
    { type: "h3", text: "Does edytlab upload my audio to find silence?" },
    {
      type: "p",
      text: "No. The detection is local Rust. Your chat and the tools' results go to the AI provider you chose, and your audio does not; [the local-first page](/use-cases/local-ai-audio-editor) has the table, and the [tools reference](/docs/tools) lists every tool.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-22. The ffmpeg measurements and the edytlab tool calls were run once, on synthetic audio generated for this post, with ffmpeg 6.1.1 and the edytlab tools called directly.",
    },
    {
      type: "ul",
      items: [
        source("FFmpeg Filters Documentation: silenceremove", "https://ffmpeg.org/ffmpeg-filters.html#silenceremove"),
        source("FFmpeg Filters Documentation: silencedetect", "https://ffmpeg.org/ffmpeg-filters.html#silencedetect"),
        source("FFmpeg: Legal", "https://ffmpeg.org/legal.html"),
        source("Audacity Manual: Truncate Silence", "https://manual.audacityteam.org/man/truncate_silence.html"),
        source("Audacity Manual: Label Sounds", "https://manual.audacityteam.org/man/label_sounds.html"),
        source("Audacity Manual: Silence Finder", "https://manual.audacityteam.org/man/silence_finder.html"),
        source("Audacity Manual: Exporting multiple audio files", "https://manual.audacityteam.org/man/export_multiple.html"),
        source("Audacity: home page", "https://www.audacityteam.org/"),
        source("edytlab tool reference (silence_finder, truncate_silence, cut_range, split_clip, label, export_labels, export_multiple)", SRC("docs/tools-reference.md"), "at commit b1678feb"),
        source("edytlab source: truncate_silence.rs", SRC("crates/tools/src/tool/truncate_silence.rs"), "at commit b1678feb"),
        source("edytlab source: silence_finder.rs", SRC("crates/tools/src/tool/silence_finder.rs"), "at commit b1678feb"),
      ],
    },
  ],
};
