import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const noiseReduction: BlogPost = {
  slug: "remove-background-noise-from-audio",
  title: "Remove Background Noise From Audio: How It Works",
  date: "2026-10-24",
  description:
    "Spectral noise reduction needs a noise-only stretch and can leave musical noise. How edytlab's noise_reduction works, with its limits and one known defect.",
  coverAlt:
    "A noisy waveform whose first stretch is bracketed as the noise profile, and below it the same waveform with the noise cut down and a few isolated spikes left over",
  excerpt:
    "Removing steady background noise is subtraction: measure the noise, take it away. It works only as well as the measurement and it leaves a particular kind of residue when pushed. This is how edytlab's noise_reduction does it, with a run on synthetic audio and one defect it found.",
  readTime: 9,
  tags: ["noise reduction", "spectral subtraction", "musical noise", "noise_reduction"],
  body: [
    {
      type: "p",
      text: "To remove background noise from audio, give the tool a stretch of the recording that holds only the noise, so it can measure what the noise looks like, and it subtracts that from the whole recording. In edytlab you tell the assistant how many seconds at the start of a track are noise-only, and it calls `noise_reduction` with that length as `noise_duration_sec`. The method works on steady noise such as hiss, fans and hum, and it has limits you can hear: it leaves a residue called musical noise when pushed hard, it damages speech if the profile includes speech, and it does little for noise that changes. This post explains the method, runs it on synthetic audio, and reports a click at the start of the output that the run turned up.",
    },
    {
      type: "callout",
      text:
        "To hear the workflow rather than read it, watch a [typed request become tool calls](/videos/reel-v070-describe-the-edit). For the steps here you need [edytlab](" +
        RELEASES +
        ") and a recording that starts with a second or so of just the noise. The installers are unsigned, so macOS and Windows warn on first launch ([Getting Started](/docs/getting-started)).",
    },
    { type: "h2", text: "What spectral subtraction does" },
    {
      type: "p",
      text: "The method was described in 1979 by S. Boll in IEEE Transactions on Acoustics, Speech, and Signal Processing. The abstract says it suppresses stationary noise from speech \"by subtracting the spectral noise bias calculated during nonspeech activity\", and that secondary procedures then attenuate the residual noise left after the subtraction. Stationary means the noise does not change: its spectrum at the start is its spectrum later.",
    },
    {
      type: "p",
      text: "edytlab's version, read from its source, goes like this:",
    },
    {
      type: "ol",
      items: [
        "**Cut the audio into overlapping windows** of 2,048 samples with a Hann (bell-shaped) fade, each starting 512 samples after the last. At 44.1 kHz a window is about 46 ms.",
        "**Measure the noise.** Each window inside the first `noise_duration_sec` seconds is turned into a spectrum, and the magnitude of each frequency bin is averaged across those windows. That average is the noise profile. If the stretch is shorter than one window, the first window is used.",
        "**Subtract.** For every window of the whole track, each bin's magnitude becomes its old magnitude minus `strength` times the profile's, but never less than `floor` times its old magnitude. The phase is kept.",
        "**Put it back together.** An inverse transform and an overlap-add rebuild the audio. Stereo is done one channel at a time, each with its own profile.",
      ],
    },
    {
      type: "p",
      text: "`strength` defaults to 0.85 and `floor` to 0.05, and the tool refuses values outside 0 to 1 for either. A `floor` of 0.05 means no bin keeps less than 5% of its magnitude, which is 26 dB down. A `strength` of 1 subtracts exactly the average noise and never more.",
    },
    { type: "h2", text: "Why it leaves musical noise" },
    {
      type: "p",
      text: "Noise is random, so some windows have more energy in a bin than the average that was subtracted and some less. Where a window has more, a little survives; where it has less, the result would go negative and is clamped. What is left is scattered, short-lived peaks. Berouti, Schwartz and Makhoul's 1979 paper describes it: the original method \"usually introduces an annoying 'musical noise'\", and their fix was to subtract an overestimate of the noise spectrum and to stop components falling below a preset minimum, the spectral floor. edytlab's `floor` is that floor. Its `strength` stops at 1, so it never subtracts an overestimate, and it subtracts magnitudes where their paper's wording is the power spectrum.",
    },
    {
      type: "p",
      text: "The Audacity manual describes what it sounds like: \"random bursts of very short tones at random frequencies, sometimes called 'musical noise', 'bird song' or 'tinkly-bells'\", and says it can happen when the noise floor is higher than was estimated, either because Sensitivity was set too low or because the noise profile was not representative. Audacity has a Frequency Smoothing setting that spreads the reduction over neighbouring bands to make such artifacts sound more acceptable, and always smooths over time (20 ms attack, 100 ms release). ffmpeg's `afftdn` filter has a `gain_smooth` option that its documentation describes as useful to reduce random music noise artefacts. edytlab's tool has no smoothing of either kind, and no way to listen to what was removed, which Audacity calls the Residue. Other methods are reported to avoid it: a 1994 paper by O. Cappé studies the Ephraim and Malah suppressor, which its abstract says was used for restoring degraded audio recordings because it is free of that artifact.",
    },
    { type: "h2", text: "What a run on synthetic audio showed" },
    {
      type: "p",
      text: "The input was an 8-second mono file at 44.1 kHz: white noise at -34 dBFS RMS throughout, a harmonic, voice-like tone with a changing pitch from 1.0 to 3.0 s and from 3.6 to 6.0 s at about -19 dBFS, and silence under the noise elsewhere. The tools were called directly, then the result rendered to WAV and measured with a script. These are one synthetic file and one machine. They show the direction of each effect and are not a measure of how it sounds on a real recording.",
    },
    {
      type: "ul",
      items: [
        "**Default settings, profile from the first 0.9 s:** the noise in the pauses fell from -34.1 dBFS to -42.0, about 8 dB. The voice region read -19.2 before and -19.4 after.",
        "**Strength 1.0, floor 0.0:** the pauses fell to -43.9, about 10 dB. **Strength 0.5:** -38.2, about 4 dB.",
        "**How spiky the residue is:** a rough measure made for this post, the share of a pause's energy that sits in the strongest 1% of its time-frequency cells (a 2,048-point spectrogram). White noise measured 0.056. After the default settings it measured 0.113, and after strength 1.0 and floor 0.0 it measured 0.137. The same energy sits in fewer places, which fits the isolated spikes that musical noise is. Strength 0.5 gave 0.079.",
        "**A profile that includes speech:** with the voice starting at 0.2 s and a 1.0 s profile, the noise in the pauses still fell about 8.5 dB, but the voice itself fell from -19.3 to -22.4 dBFS, 3.1 dB, against 0.3 dB with a clean profile.",
        "**Noise that changes:** with the noise made 6 dB louder after 4 s, the pause at the end fell only 3.4 dB, from -28.0 to -31.4, against 7.8 dB when the noise matched the profile.",
      ],
    },
    { type: "h2", text: "Where the noise profile comes from" },
    {
      type: "p",
      text: "Audacity's manual asks you to select a region \"which contains only noise\", says a minimum of 2,048 samples is needed and a longer profile is better, and says duplicating a very short sample does not help because the effect works from the noise's statistics. edytlab differs in one way that matters: it takes no selection. The profile is always the start of the track, counted from the track's first frame across all its clips, so a recording that does not begin with noise needs a noise-only second at the front before this tool can be used on it. If the noise-only stretch is in the middle, Audacity can use it, and ffmpeg's `afftdn` can capture a profile from the input with its `sample_noise` option.",
    },
    {
      type: "p",
      text: "When recording, hold still and record a second or two of the room before the first word. The two failures above, speech in the profile and noise that changes, both come from a profile that does not match the noise.",
    },
    { type: "h2", text: "A click at the start of the output (issue #692)" },
    {
      type: "p",
      text: "The run found a defect. In the synthetic file, whose samples never exceeded 0.40, 16 of the output's first samples were at full scale, and the level stayed above 0.1 for about 2.5 ms and above 0.03 for about 7 ms, where the noise itself is about 0.005. It was the same with other settings. The end of the track was clean. The likely cause is in the overlap-add: the first few samples are covered by one window whose value is almost zero, and dividing by it magnifies the error. It is filed as [issue 692](https://github.com/laadtushar/edytlab/issues/692) and is not fixed in this post's version. Until it is, trimming the first 512 samples after noise reduction removed it: the first 2,048 samples then peaked at 0.026, about the noise.",
    },
    { type: "h2", text: "Remove background noise in edytlab" },
    {
      type: "prompt",
      text: "The first 0.9 seconds of track 1 is only room noise. Reduce the background noise on track 1, using that as the noise profile.",
    },
    {
      type: "p",
      text: "The tool rewrites the track's audio as a new file and adds a session node, so undo works ([Undo is a graph](/blog/undo-is-a-graph-branches-and-ab-compare)). Pick a profile length shorter than the noise-only stretch, so no speech is in it. `strength` and `floor` have defaults; ask for other values by number.",
    },
    {
      type: "prompt",
      text: "That sounds watery. Undo it and try again with strength 0.5.",
    },
    {
      type: "p",
      text: "A lower `strength` leaves more noise and less residue, the same trade the Audacity manual describes for its Noise Reduction slider: the lowest value that reduces the noise to an acceptable level. Do hum and clicks first, as Audacity advises, with the tools aimed at them: `notch_filter` attenuates frequencies near a centre frequency, `click_removal` replaces sample spikes with interpolated values, and `noise_gate` silences what is below a threshold, which helps in the pauses and does nothing under speech. Traffic, a crowd or a changing noise are not what this method is for: the Audacity manual calls such irregular background noise unsuitable for its version. The [tools reference](/docs/tools) lists all of them, and [the local-first page](/use-cases/local-ai-audio-editor) says what leaves your computer when you ask for any of it.",
    },
    { type: "h2", text: "Questions about removing background noise" },
    { type: "h3", text: "Why does my voice sound watery after noise reduction?" },
    {
      type: "p",
      text: "Because the subtraction also removes some of the voice when the noise and the voice share frequencies, and what is left of the noise comes in short bursts. The Audacity manual says reducing noise usually results in some distortion, and that when the voice is not much louder than the noise the result may be too distorted. Lower the strength, and use a cleaner profile.",
    },
    { type: "h3", text: "How long should the noise sample be?" },
    {
      type: "p",
      text: "Audacity's manual says at least 2,048 samples and that longer is better. edytlab averages the 2,048-sample windows inside the stretch you name, so one second gives it about 80 windows to average. Stop before any speech starts.",
    },
    { type: "h3", text: "Is noise reduction in edytlab free?" },
    {
      type: "p",
      text: "edytlab is free and open source (MIT). The processing runs on your computer. Asking for it in chat costs the AI provider's tokens, unless you use a local Ollama model, and the audio stays on your computer either way.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-24. The run used synthetic audio generated for this post, with the edytlab tools called directly and the output measured with a script; the click was seen on that file and the numbers are from that run. edytlab's own behaviour is stated from its source at the commit linked.",
    },
    {
      type: "ul",
      items: [
        source("S. Boll, \"Suppression of acoustic noise in speech using spectral subtraction\", IEEE Transactions on Acoustics, Speech, and Signal Processing 27(2), 113-120, 1979 (DOI; abstract read on IEEE Xplore)", "https://doi.org/10.1109/TASSP.1979.1163209"),
        source("M. Berouti, R. Schwartz and J. Makhoul, \"Enhancement of speech corrupted by acoustic noise\", ICASSP '79, 208-211 (DOI; abstract read on IEEE Xplore)", "https://doi.org/10.1109/ICASSP.1979.1170788"),
        source("O. Cappé, \"Elimination of the musical noise phenomenon with the Ephraim and Malah noise suppressor\", IEEE Transactions on Speech and Audio Processing 2(2), 345-349, 1994 (DOI; abstract read on IEEE Xplore)", "https://doi.org/10.1109/89.279283"),
        source("Audacity manual: Noise Reduction", "https://manual.audacityteam.org/man/noise_reduction.html"),
        source("FFmpeg documentation: afftdn filter", "https://ffmpeg.org/ffmpeg-filters.html#afftdn"),
        source("edytlab source: noise_reduction tool", SRC("crates/tools/src/tool/noise_reduction.rs"), "at commit 8bf752ff"),
        source("edytlab tool reference (noise_reduction, notch_filter, click_removal, noise_gate)", SRC("docs/tools-reference.md"), "at commit 8bf752ff"),
        source("edytlab issue 692: noise_reduction puts a full-scale click in the first samples", "https://github.com/laadtushar/edytlab/issues/692"),
      ],
    },
  ],
};
