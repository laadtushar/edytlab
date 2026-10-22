import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const tempoWithoutPitch: BlogPost = {
  slug: "change-tempo-without-changing-pitch",
  title: "Change Tempo Without Changing Pitch: How It Works",
  date: "2026-10-22",
  description:
    "A speed change moves pitch with tempo; a phase vocoder keeps it. How time-stretching works, its artifacts, and edytlab's time_stretch and pitch_shift.",
  coverAlt:
    "A row of overlapping bell-shaped windows, and below it the same windows placed further apart, with the spacing between the first two marked on each row",
  excerpt:
    "Time-stretching sounds like one trick and is a small pipeline: windows, spectra, phases and a rule for putting them back. This is that pipeline in plain words, from edytlab's own implementation, with the artifacts it fights and the limits its code admits to.",
  readTime: 9,
  tags: ["time stretch", "pitch shift", "phase vocoder", "tempo"],
  body: [
    {
      type: "p",
      text: "To change tempo without changing pitch, use a time-stretch and not a speed change. A speed change resamples: it plays the same samples faster or slower, so pitch moves with tempo. A time-stretch rebuilds the audio at a new length with the same frequencies, and the usual way to do that is a **phase vocoder**. In edytlab, `time_stretch` takes a factor (2.0 is twice as fast, 0.5 half as fast) and `pitch_shift` does the reverse job, moving pitch in semitones while the length stays put. This post explains how edytlab's phase vocoder works, where its artifacts come from, and why its own tool notes say that factors far from 1.0 make them worse.",
    },
    {
      type: "callout",
      text:
        "To hear it, not read it: in the [mini-mix demo](/videos/dj-mini-mix) two tracks are time-stretched to 124 BPM, keeping their pitch, and the assistant checks that their beats line up with a third's. The [beatmatch post](/blog/beatmatch-and-blend-two-tracks) gives the sentences to type, and the [DJ page](/use-cases/dj) shows the rest. You need [edytlab](" +
        RELEASES +
        "); its installers are unsigned, so macOS and Windows warn on first launch ([Getting Started](/docs/getting-started)).",
    },
    { type: "h2", text: "Speed change, time-stretch and pitch shift" },
    {
      type: "ul",
      items: [
        "**`change_speed`** resamples a track \"without pitch preservation\". Faster is shorter and higher; slower is longer and lower.",
        "**`time_stretch`** changes a track's length \"without changing its pitch\". A factor of 0.5 is twice as long, 2.0 half as long, and the output is the input length divided by the factor.",
        "**`pitch_shift`** moves pitch in semitones \"without changing its duration\". +12 is an octave up, -12 an octave down, and the range is ±48.",
      ],
    },
    {
      type: "p",
      text: "`pitch_shift` is built from the other two. It stretches the audio by the pitch ratio (2 to the power of semitones ÷ 12), then reads the result back that many times faster. The stretch cancels the change in length that the faster read-back would cause, and the read-back multiplies every frequency by the ratio.",
    },
    { type: "h2", text: "How a phase vocoder changes tempo without changing pitch" },
    {
      type: "p",
      text: "The phase vocoder was described in 1966 by Flanagan and Golden, in the Bell System Technical Journal. edytlab's version is plain Rust on an FFT library, with no model in it. Its module documentation explains the method, and this is that explanation in ordinary words.",
    },
    {
      type: "ol",
      items: [
        "**Cut the audio into overlapping windows.** edytlab uses windows of 2048 samples with a bell-shaped (Hann) fade at each edge, each starting 512 samples after the one before, so every sample sits in four windows. At 44.1 kHz a window is about 46 ms, which a code comment calls long enough to resolve bass notes and short enough that attacks do not smear too much.",
        "**Take each window's spectrum.** A Fourier transform gives every frequency bin a magnitude (how much energy) and a phase (where it is in its cycle).",
        "**Put the windows back with a different spacing.** To slow down, the output windows are placed further apart than the input ones were; to speed up, closer together. The output spacing is the input spacing times the ratio, and adding the overlapping windows gives audio that many times as long.",
        "**Repair the phases.** Moving windows alone would leave the sine waves in neighbouring windows out of step where they overlap, and they would partly cancel. Between two input windows, a bin's phase advances by what its centre frequency predicts plus a deviation for how far the real partial sits from the centre. Measuring that deviation gives the partial's true frequency, and the output phase is advanced by that true frequency across the new spacing. The documentation calls recovering the true frequency \"the whole trick\".",
        "**Pitch is untouched because the frequencies are; the length changes because the spacing did.**",
      ],
    },
    {
      type: "p",
      text: "One assumption sits under step 4: the deviation is only known modulo a full turn, so the code takes it that no partial drifts more than half a bin per hop.",
    },
    { type: "h2", text: "Where the artifacts come from" },
    {
      type: "p",
      text: "A 1999 paper by Laroche and Dolson, which edytlab's code credits for one of its fixes, names the two most prominent artifacts of a phase vocoder. **Transient smearing** is heard as \"a slight loss of percussiveness\": piano attacks, for example, can have less \"bite\". **Phasiness** is a characteristic coloration, and \"time-expanded speech often sounds as if the speaker is much further from the microphone\". The paper says both occur even with factors close to one.",
    },
    {
      type: "p",
      text: "edytlab's code answers each. For smearing it **resets the phase on attacks**. It measures spectral flux, how much new energy appears in the spectrum from one window to the next. When a window's flux rises more than 1.5 standard deviations above the recent average, and by at least a tenth of the window's total magnitude, every bin's output phase is set back to its input phase, so the attack is rebuilt as recorded. A reset in the middle of a steady note would put a click in it, so the code is deliberately cautious. It needs 16 windows of history first and waits at least four windows between resets, and its documentation says a false positive is worse than a miss.",
    },
    {
      type: "p",
      text: "For phasiness it uses **identity phase locking**, from the same paper. It finds the spectral peaks, where a bin counts if it beats the two bins on each side and stands more than three times above the window's average magnitude. Then the bins around each peak follow that peak's phase rotation instead of drifting on their own, so the bins of one partial stay in step. The module documentation reports what that does to a synthetic steady 2 kHz sine stretched to twice its length: without locking the level swings by a factor of 2.09, which it calls 6 dB of warble on a flat input, and with locking by 1.003. On broadband material such as noise or a cymbal no peak stands out, so the code falls back to plain per-bin phase advance.",
    },
    {
      type: "p",
      text: "What it still does not do, in the documentation's own words: peaks are not tracked from one window to the next, so a fast glissando can waver, and large factors make that worse. The more refined \"scaled\" phase locking is not implemented. And each channel is stretched on its own, so a hard-panned source \"can drift slightly in image at large factors\".",
    },
    { type: "h2", text: "Why small changes sound cleaner" },
    {
      type: "p",
      text: "The evidence is two sentences in edytlab's tool descriptions. `time_stretch` says that sustained material is clean and attacks are preserved, \"but dense material can sound slightly phasey and factors far from 1.0 make that worse\". `pitch_shift` adds that it \"is most reliable within about a fifth; larger shifts are limited by the vocoder rather than by the correction\". Neither gives a number beyond that. The hard limits are only that a stretch factor must be positive and finite and a pitch shift must be within ±48 semitones.",
    },
    {
      type: "p",
      text: "So the safe habit is a small change on sustained material. A stretch of a few BPM is a factor near 1.0, the easy case. The paper's caution still applies: near-1.0 stretches are not artifact-free in a plain phase vocoder, which is why the resets and the locking exist. If a stretch sounds phasey, a smaller factor is the first thing to try, and a different section of the same track is the second.",
    },
    { type: "h2", text: "Change tempo without changing pitch in edytlab" },
    { type: "prompt", text: "What are the tempo and key of track 1 and track 2?" },
    {
      type: "p",
      text: "That is `analyze_track`. Check the BPM it reports by ear before you rely on it. The factor is the tempo you want divided by the tempo you have: 128 ÷ 124 is about 1.032 (example numbers).",
    },
    {
      type: "prompt",
      text: "Time-stretch track 2 so its tempo matches track 1, keeping the pitch.",
    },
    {
      type: "p",
      text: "`time_stretch` writes the stretched audio as a new file and adds a session node, so undo works ([Undo is a graph](/blog/undo-is-a-graph-branches-and-ab-compare)), and a second stretch applies to audio the first already stretched. Use `change_speed` if the pitch should move too. If a performance drifts, rather than running steadily at a different tempo, `align_to_beat` warps each stretch between beats onto a grid you give it. It does that in one pass of the vocoder, so there is no seam at the beats, and it limits any one segment's ratio to between 0.1 and 10.",
    },
    {
      type: "prompt",
      text: "Pitch-shift the vocal track up 2 semitones and keep its formants.",
    },
    {
      type: "p",
      text: "Set `preserve_formants` for voices. It measures the original's spectral envelope and puts it back on the shifted signal, so the tool text says a shifted voice \"sounds like the same person singing higher rather than the classic chipmunk or giant\". Leave it off for instruments. `time_stretch` accepts the flag and ignores it, because a time-stretch moves no frequency.",
    },
    {
      type: "p",
      text: "A run on a 3-second synthetic 220 Hz tone, calling the tools directly, shows the contract. Factor 2.0 gave 1.5 s still at 220 Hz, factor 0.5 gave 6 s at 220 Hz, and `pitch_shift` of +12 kept 3 s and gave 440 Hz (the frequency from a zero-crossing count of the rendered file). The [tools reference](/docs/tools) has the full list of what the agent can call.",
    },
    { type: "h2", text: "Questions about time stretching" },
    { type: "h3", text: "What is the difference between time stretching and pitch shifting?" },
    {
      type: "p",
      text: "Time stretching changes the length and keeps the pitch. Pitch shifting changes the pitch and keeps the length. In edytlab they are `time_stretch` and `pitch_shift`, and the second is made from the first plus a resample.",
    },
    { type: "h3", text: "Why does stretched speech sound far away?" },
    {
      type: "p",
      text: "That is the phasiness artifact. Laroche and Dolson say time-expanded speech often sounds as if the speaker is much further from the microphone than in the recording. edytlab's phase locking targets it, and the tool description still warns that dense material can sound slightly phasey.",
    },
    { type: "h3", text: "Is a phase vocoder the only way to time-stretch?" },
    {
      type: "p",
      text: "No. The 1999 paper contrasts it with time-domain methods that overlap-add excerpts of the signal, which it describes as cheap and good in some cases, such as factors close to one or monophonic sounds. edytlab uses a phase vocoder.",
    },
    { type: "h3", text: "How do I change tempo without changing pitch for free?" },
    {
      type: "p",
      text: "edytlab is free and open source (MIT). The only cost is the AI provider's tokens, unless you run a local Ollama model, and your audio stays on your computer either way; [the local-first page](/use-cases/local-ai-audio-editor) has the table.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-22. The run on the synthetic tone called the edytlab tools directly. The two papers are cited for what they say about the method; edytlab's own behaviour is stated from its source.",
    },
    {
      type: "ul",
      items: [
        source("J. L. Flanagan and R. M. Golden, \"Phase Vocoder\", Bell System Technical Journal 45(9), 1493-1509, 1966 (DOI; reference details checked against Crossref)", "https://doi.org/10.1002/j.1538-7305.1966.tb01706.x"),
        source("J. Laroche and M. Dolson, \"Improved phase vocoder time-scale modification of audio\", IEEE Transactions on Speech and Audio Processing 7(3), 323-332, 1999 (DOI; abstract and introduction read in a full-text copy on SciSpace)", "https://doi.org/10.1109/89.759041"),
        source("edytlab source: audio-time vocoder.rs, the method and its limits", SRC("crates/audio-time/src/vocoder.rs"), "at commit b1678feb"),
        source("edytlab source: audio-time lib.rs, stretch.rs and shift.rs", SRC("crates/audio-time/src/lib.rs"), "at commit b1678feb"),
        source("edytlab source: audio-time formant.rs and warp.rs", SRC("crates/audio-time/src/formant.rs"), "at commit b1678feb"),
        source("edytlab tool reference (time_stretch, pitch_shift, change_speed, align_to_beat, analyze_track)", SRC("docs/tools-reference.md"), "at commit b1678feb"),
        source("edytlab demo captions: beatmatch and mini-mix", SRC("website/lib/demos.ts"), "at commit b1678feb"),
      ],
    },
  ],
};
