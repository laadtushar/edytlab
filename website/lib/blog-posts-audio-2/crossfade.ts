import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const crossfadeTwoSongs: BlogPost = {
  slug: "crossfade-two-songs",
  title: "Crossfade Two Songs: Equal Power vs Linear",
  date: "2026-10-24",
  description:
    "Equal-power against linear crossfades with measured levels, matching tempo and beats first, and making each curve with edytlab's fade and set_clip_envelope.",
  coverAlt:
    "Two crossing curves, one falling and one rising, shown twice: straight lines that cross low in the middle, and bowed curves that cross higher",
  excerpt:
    "A crossfade is two volume curves that cross. Whether they are straight lines or bowed ones decides whether the middle gets quieter or louder. This is the arithmetic, the levels measured on synthetic audio, the beatmatching that comes first, and the tools in edytlab that make each curve.",
  readTime: 8,
  tags: ["crossfade", "DJ transition", "equal power", "beatmatching"],
  body: [
    {
      type: "p",
      text: "To crossfade two songs, bring them to the same tempo, line their beats up, overlap the end of one with the start of the next, and fade one out while the other fades in. The choice that matters is the curve. An equal-power crossfade keeps the loudness steady when the two songs are unrelated, while a linear one dips by about 3 dB in the middle; and with identical audio the two swap places, so a linear fade stays level and an equal-power one rises by 3 dB. In edytlab the tempo comes from `time_stretch`, the position from `time_shift`, a linear crossfade from `fade`, and an equal-power one from `set_clip_envelope`. This post shows the arithmetic, the measured levels, and the steps.",
    },
    {
      type: "callout",
      text:
        "To watch it done, the [mini-mix demo](/videos/dj-mini-mix) joins three tracks with overlapping crossfades, and the [beatmatch demo](/videos/dj-beatmatched-transition) blends two with a low-pass on the outgoing one. The [beatmatch post](/blog/beatmatch-and-blend-two-tracks) gives the sentences to type. You need [edytlab](" +
        RELEASES +
        "); its installers are unsigned, so macOS and Windows warn on first launch ([Getting Started](/docs/getting-started)).",
    },
    { type: "h2", text: "Linear and equal-power crossfades" },
    {
      type: "p",
      text: "Call the outgoing gain a and the incoming gain b, both running across the overlap from x = 0 to x = 1. A **linear** crossfade has a = 1 − x and b = x. An **equal-power** one has a = cos(πx/2) and b = sin(πx/2). The Web Audio specification uses the same cosine and sine pair for its equal-power panning algorithm.",
    },
    {
      type: "p",
      text: "What the listener hears depends on how the two signals relate. If they are unrelated, their powers add, so the total is a² + b². Linear gives 0.5 at x = 0.5, which is 3 dB down. Equal-power gives cos² + sin² = 1 everywhere, so the level stays put. If they are the same audio in phase, their amplitudes add, so the total is a + b. Linear gives exactly 1, constant. Equal-power gives 1.414 at x = 0.5, which is 3 dB up.",
    },
    {
      type: "p",
      text: "Audacity's manual describes both effects. Its Crossfade Tracks effect has \"Constant Gain\", the default, which fades with linear ramps, and says it \"may cause the overall volume to dip slightly during the fade\". \"Constant Power\" fades \"combat the tendency of the volume dipping\", with the caveat that \"the peak level may rise during the course of the fade\". It describes its second constant-power variant as one that starts to fade in a little more rapidly, which it says may be preferred when crossfading songs that are beat-matched.",
    },
    {
      type: "p",
      text: "Two different songs are more like the unrelated case than the identical one, apart from material that lines up on purpose, such as a kick on the same beat after beatmatching. The practical answer is a curve between the two. Audacity's manual calls its first constant-power fade \"generally a good choice as a general purpose crossfade effect\".",
    },
    { type: "h2", text: "Match the tempo and line up the beats first" },
    {
      type: "p",
      text: "Two songs at different speeds drift apart through the overlap, so match them first. Find both tempos with `analyze_track`, then stretch the slower or faster one with `time_stretch`, which keeps pitch ([how a time-stretch works](/blog/change-tempo-without-changing-pitch) and [how tempo detection can fail](/blog/how-to-find-the-bpm-and-key-of-a-song)). The factor is the tempo you want divided by the tempo you have: 124 ÷ 120 = 1.0333.",
    },
    {
      type: "p",
      text: "Then place the second song so one of its beats lands on a beat of the first. The `beats` list from `analyze_track` gives the times. On the synthetic pair used below, the first song had a beat at 16.15 s, 33 beats after its first. After the stretch, the second song's first beat was at 0.279 s, so `time_shift` by 15.875 s put the two grids on top of each other. Check the beat times by ear before you rely on them, because detection can put its grid on a hi-hat instead of a kick.",
    },
    {
      type: "p",
      text: "In the demos, the assistant pointed out that after the stretch the second track's bars ran a beat behind the first's and offered a half-second correction, and in the mini-mix it started one track on the other's bar line, which made that overlap 8.77 s instead of exactly four bars. How long to overlap is a choice with no standard figure. The demos use about four bars, or eight in the first demo, and their crossfades are straight-line ones.",
    },
    { type: "h2", text: "What a run on synthetic audio showed" },
    {
      type: "p",
      text: "To see the levels, two kinds of pair were made: two unrelated pads (an A minor triad and a B♭ minor triad, each three tones with two overtones, so they share no frequency) and the same 220 Hz tone twice, in phase. Each is 24 s at 44.1 kHz. The second was shifted to start at 16 s, so the two overlap for 8 s. The tools were called directly, the mix rendered to WAV and measured in 0.2 s windows. The numbers are relative to the level before the overlap.",
    },
    {
      type: "ul",
      items: [
        "**Unrelated pads, linear (`fade`):** -2.1 dB a quarter of the way in, **-3.1 dB at the middle**.",
        "**Unrelated pads, equal-power (`set_clip_envelope`):** within 0.12 dB of level throughout.",
        "**Identical tones, linear:** 0.0 dB throughout.",
        "**Identical tones, equal-power:** +2.3 dB a quarter in and **+3.0 dB at the middle**.",
        "**Unrelated pads with no fades at all:** about +3 dB through the overlap, which is why you fade.",
        "**ffmpeg's `acrossfade`, same pads, 8 s:** `c1=tri:c2=tri` (the `afade` default, a linear slope) measured -3.1 dB at the middle; `c1=qsin:c2=qsin` (quarter of a sine) stayed within 0.11 dB.",
      ],
    },
    {
      type: "p",
      text: "A separate run followed the whole procedure on two synthetic songs, a 124 BPM one and a 120 BPM one, each a pad with a kick on the beat. The second was stretched by 1.0333, shifted 15.875 s, and both were crossfaded linearly over 16 to 24 s. `analyze_track` on the rendered mix then reported 124.0 BPM and 81 beats across its 39 s, one steady grid. That agrees with the beats being aligned, and is not proof.",
    },
    { type: "h2", text: "Crossfade two songs in edytlab" },
    {
      type: "prompt",
      text: "What is the tempo of track 1 and of track 2? Then time-stretch track 2 to match track 1, keeping its pitch.",
    },
    {
      type: "prompt",
      text: "Move track 2 so its first beat lands on one of track 1's beats near 16 seconds, then crossfade them linearly over the overlap.",
    },
    {
      type: "p",
      text: "`fade` applies a linear fade-in or fade-out to a range of one track, so a linear crossfade is two calls, an `out` on the first track and an `in` on the second, over the same seconds. It rewrites the track's audio as a new file and adds a session node, so [undo](/blog/undo-is-a-graph-branches-and-ab-compare) works.",
    },
    {
      type: "prompt",
      text: "Undo the fades. Crossfade the same overlap with equal power instead, using volume envelopes on both clips.",
    },
    {
      type: "p",
      text: "`set_clip_envelope` replaces the volume curve of one clip with points of time and gain in dB, and the engine interpolates between them at render time, so it rewrites no audio. The times are seconds from the start of that clip. Interpolation is a straight line in dB, so the curve needs enough points: for the outgoing clip, gain_db = 20·log₁₀(cos(πx/2)), and for the incoming one 20·log₁₀(sin(πx/2)), with x running from 0 to 1 across the overlap. A dB value cannot be minus infinity, so the end points are a very low number, -80 in the run. The run above used a point every half second over 8 s and held the level within 0.12 dB. Fewer points give a rougher curve, which was not measured. As a guide, at a quarter of the way the outgoing gain is -0.69 dB and the incoming -8.34 dB, and at the middle both are -3.01 dB.",
    },
    {
      type: "p",
      text: "Then mix and master. `mix_to_new_track` renders the selected tracks to a baked track, and the loudness side is covered in [the LUFS post](/blog/lufs-for-spotify-youtube-apple-music-podcasts). An equal-power crossfade of correlated material can raise the peak, as Audacity warns, so check the peak of the overlap, and read [the true-peak post](/blog/true-peak-vs-sample-peak) for what edytlab's limiter does and does not hold. The [DJ page](/use-cases/dj) collects the rest, and the [tools reference](/docs/tools) lists every tool named here.",
    },
    {
      type: "p",
      text: "Other programs do the same job. Audacity's Crossfade Tracks effect offers the constant-gain and constant-power fades described above, with a custom curve between them. ffmpeg's `acrossfade` filter takes `curve1` and `curve2`, and its documentation points to `afade` for the curve names, such as `qsin`. For example, `ffmpeg -i first.flac -i second.flac -filter_complex acrossfade=d=10:c1=qsin:c2=qsin output.flac` crossfades over 10 s with a quarter-sine curve on both.",
    },
    { type: "h2", text: "Questions about crossfading two songs" },
    { type: "h3", text: "What is the difference between a crossfade and a fade-out then a fade-in?" },
    {
      type: "p",
      text: "A crossfade overlaps the two songs, so for a while both are heard. A fade-out then a fade-in plays them one after the other, and the gap between is silence or the tail of the first. The ffmpeg `acrossfade` filter has an `overlap` option, on by default, that makes this explicit: with it off, the second stream starts after the first ends.",
    },
    { type: "h3", text: "How long should a crossfade be?" },
    {
      type: "p",
      text: "There is no standard figure. The mini-mix demo overlaps by about four bars at 124 BPM, which is a little under eight seconds, and the beatmatch demo starts the second track under the last eight bars of the first.",
    },
    { type: "h3", text: "Does edytlab beatmatch automatically?" },
    {
      type: "p",
      text: "Not by itself. It analyses both tracks, and the assistant stretches and positions them with the tools above because you asked. The demos show the assistant flagging when the bars do not line up. The detector can be wrong, and the grid is a single steady tempo, so a song with a drifting tempo needs `align_to_beat`, which warps it onto a grid you give it.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-24. The levels came from synthetic audio generated for this post, with the edytlab tools called directly and the renders measured with a script and with ffmpeg 6.1.1. The arithmetic above is derived from the curves given, not taken from a source. edytlab's own behaviour is stated from its tool reference and source at the commit linked.",
    },
    {
      type: "ul",
      items: [
        source("Audacity manual: Crossfade Tracks", "https://manual.audacityteam.org/man/crossfade_tracks.html"),
        source("FFmpeg documentation: acrossfade and afade filters", "https://ffmpeg.org/ffmpeg-filters.html#acrossfade"),
        source("W3C Web Audio API (the equal-power panning algorithm)", "https://webaudio.github.io/web-audio-api/"),
        source("edytlab tool reference (fade, set_clip_envelope, time_shift, time_stretch, analyze_track, mix_to_new_track, align_to_beat)", SRC("docs/tools-reference.md"), "at commit 8bf752ff"),
        source("edytlab source: fade tool (linear ramp)", SRC("crates/tools/src/tool/fade.rs"), "at commit 8bf752ff"),
        source("edytlab source: audio-engine render.rs (envelope interpolation in dB)", SRC("crates/audio-engine/src/render.rs"), "at commit 8bf752ff"),
        source("edytlab demo captions: beatmatch and mini-mix", SRC("website/lib/demos.ts"), "at commit 8bf752ff"),
      ],
    },
  ],
};
