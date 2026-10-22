import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const lufsTargets: BlogPost = {
  slug: "lufs-for-spotify-youtube-apple-music-podcasts",
  title: "LUFS for Spotify, YouTube, Apple Music, Podcasts",
  date: "2026-10-22",
  description:
    "Spotify adjusts to -14 LUFS, Apple Podcasts recommends about -16, EBU R 128 is -23. None found for YouTube or Apple Music. Sources, and how to hit one.",
  coverAlt:
    "A waveform under a dashed horizontal line that marks a loudness target, with a solid ceiling line above it and one narrow peak that pokes through the ceiling",
  excerpt:
    "A loudness target is only worth using if you can point to who set it. These are the ones with a primary source, the ones without, what LUFS, dB and true peak mean, and how to reach a target in edytlab, including what its limiter cannot promise.",
  readTime: 11,
  tags: ["LUFS", "loudness", "podcast loudness", "true peak"],
  body: [
    {
      type: "p",
      text: "**Spotify** says it adjusts tracks to **-14 LUFS**. **Apple Podcasts** recommends about **-16 LKFS** (the same unit as LUFS) with a tolerance of 1 dB. The **EBU's R 128** broadcast recommendation is **-23 LUFS**, and **SoundCloud's** help page gives -14 like Spotify's. For **YouTube**, **Apple Music** and **Instagram**, no official page that could be found states a figure, so this post gives none. Every number below links the page it came from, and each page was read on 2026-10-22.",
    },
    {
      type: "callout",
      text:
        "Reading this needs only a browser. For the edytlab steps you need [edytlab](" +
        RELEASES +
        ") and an audio file. The installers are unsigned, so macOS and Windows warn on first launch; [Getting Started](/docs/getting-started) walks through it.",
    },
    { type: "h2", text: "LUFS for Spotify" },
    {
      type: "p",
      text: "Spotify's help page says tracks are adjusted to -14 LUFS \"according to the ITU 1770 standard\". It does this during playback: loudness is measured at upload, and the audio is not changed before it plays. A louder master is turned down. A softer one is turned up, but only as far as leaves 1 dB of headroom: the page's own example is a track at -20 LUFS with a true peak of -5 dB FS, which is lifted to -16 LUFS and not to -14.",
    },
    {
      type: "p",
      text: "Premium listeners can choose Loud (-11), Normal (-14) or Quiet (-19), and the page says the web player and third-party devices such as TVs and speakers do not use normalization. For masters it advises -14 integrated LUFS with a true peak below -1 dB TP, and a true peak below -2 dB if the master is louder than -14. A second Spotify page makes the point that loud does not win: a very dynamic track mastered to -14 LUFS keeps its peaks, while a loudly mastered one (it uses -6 LUFS as the example) is reduced in volume, and both play at the same perceived loudness.",
    },
    { type: "h2", text: "LUFS for Apple Music" },
    {
      type: "p",
      text: "Apple's Apple Digital Masters document describes Sound Check, which \"lets listeners hear all their songs at approximately the same loudness\" by storing a track's loudness in its metadata and raising or lowering playback from it. It gives no LUFS target. What it does ask for is a master that does not clip, with at least 1 dB of headroom, because oversampling during playback can clip a signal that sits at 0 dBFS. So the only figure Apple states there is headroom, not loudness.",
    },
    { type: "h2", text: "LUFS for YouTube and Instagram" },
    {
      type: "p",
      text: "No figure is given for either, because none from the platform itself was found. On 2026-10-22, searches of YouTube Help and Google support pages, and of Meta's help and business pages, turned up community threads, group posts and videos, and no help page that states a figure. A number copied from a forum is a guess. If you need one, find the platform's page that states it and note its date. In edytlab you can still ask for any number with `target_lufs`.",
    },
    { type: "h2", text: "LUFS for SoundCloud" },
    {
      type: "p",
      text: "SoundCloud's help centre says it applies loudness normalization to tracks as they are played. Its mastering tips are the same figures as Spotify's: -14 dB integrated LUFS with a true peak below -1 dB TP, and below -2 dB TP if the master is louder than -14.",
    },
    { type: "h2", text: "LUFS for podcasts" },
    {
      type: "p",
      text: "Apple Podcasts' audio requirements recommend preconditioning audio so that overall loudness stays \"around -16 dB LKFS, with a +/- 1 dB tolerance\" and the true-peak value does not exceed -1 dB FS, both calculated to ITU-R BS.1770-5, before encoding. Apple's reason is that \"audio compression algorithms typically don't modify the loudness and might clip the signal\". It adds that when loudness metadata is embedded in the file, Sound Check playback in the iOS Music app is at -16 dB.",
    },
    {
      type: "p",
      text: "Spotify's loudness page is written for music tracks and says -14. The Spotify support pages read for this post give no separate podcast figure. The EBU's streaming note, R 128 s2, lists podcasts among the services that \"typically\" use a higher target than the broadcast standard's -23.",
    },
    { type: "h2", text: "Broadcast: EBU R 128 is -23 LUFS" },
    {
      type: "p",
      text: "R 128 (version 5, November 2023) recommends normalizing programme loudness to -23.0 LUFS. A tolerance of ±1.0 LU is allowed where the target cannot practically be reached, with live programmes as its example, and the true peak of a programme should not exceed -1 dBTP in production. Its streaming supplement, R 128 s2, recommends streaming programmes unchanged at -23.0 LUFS.",
    },
    { type: "h2", text: "LUFS, dB and true peak in plain words" },
    {
      type: "p",
      text: "A **decibel (dB)** compares two levels. **dBFS** compares a level with full scale, the loudest value a digital file can hold, which is why digital levels are zero or negative. **LUFS** is a different kind of number: a measure of loudness, frequency-weighted and averaged over time, defined in ITU-R BS.1770. The EBU's R 128 notes that LUFS is equivalent to the LKFS that BS.1770 uses, and its Tech 3341 says 1 LU (one loudness unit) is equivalent to 1 dB, so 2 dB more gain is 2 LU more loudness.",
    },
    {
      type: "p",
      text: "**Integrated** loudness is that figure over a whole programme. Tech 3341 describes the gate that keeps silence out of it: 400 ms blocks, dropping any below -70 LUFS and then any more than 10 LU below the average of the rest. **Peak** normalization is a different thing. R 128 starts from the observation that peak normalisation \"has led to considerable loudness differences between programmes\".",
    },
    {
      type: "p",
      text: "A **sample peak** is the largest sample value in the file. A **true peak** is the largest value of the waveform between the samples, estimated by oversampling. FFmpeg's ebur128 filter documents both: sample peak is \"looking for the higher sample value\", and true peak is done \"on an over-sampled version of the input stream\". R 128 defines the Maximum True Peak Level as the waveform's maximum \"in the continuous time domain\".",
    },
    {
      type: "p",
      text: "The gap can be 3 dB. Tech 3341 lists a test signal for true-peak meters: a sine at a quarter of the sample rate (12 kHz in a 48 kHz file), amplitude 0.5 of full scale, phase 45 degrees, with a true peak of -6.0 dBTP. At that phase every sample is 0.5 × sin 45°, about 0.354, which is -9.0 dBFS. Made as a 3-second file with a 10 ms fade at each end and measured with `ffmpeg -af ebur128=peak=sample+true`, it read -9.0 dBFS sample peak and -6.0 dBFS true peak.",
    },
    { type: "h2", text: "Normalize audio to what dB?" },
    {
      type: "p",
      text: "If the question is which peak to normalize to, the pages above answer in true peak: -1 dB for Spotify, SoundCloud, Apple Podcasts and EBU R 128, and -2 dB on Spotify and SoundCloud for a master louder than -14 LUFS. A peak level does not say how loud a file sounds, though. For that, normalize to a loudness, and the number depends on where the file is going. In edytlab, `normalize` sets a peak (`target_dbfs`) and `normalize_loudness` sets integrated loudness; the tool text tells the agent to prefer the second for delivery because \"peak normalisation cannot match perceived loudness between files\".",
    },
    { type: "h2", text: "How to hit a loudness target in edytlab" },
    {
      type: "p",
      text: "`normalize_loudness` measures a track's integrated loudness (EBU R 128, through the pure-Rust `ebur128` crate) and sets the track's gain to reach the target. It does not rewrite the source file; the engine applies the gain when it renders. Start by asking what you have:",
    },
    { type: "prompt", text: "What is the integrated loudness of track 1?" },
    {
      type: "p",
      text: "That is `analyze_track`, which is read-only and returns the loudness in LUFS along with tempo, key and a beat grid. Then ask for the target:",
    },
    { type: "prompt", text: "Normalize track 1 to the apple_podcasts preset." },
    {
      type: "p",
      text: "The presets are `spotify` and `youtube` (-14), `apple_podcasts` (-16) and `broadcast` (-23); `target_lufs` takes any other number. The `youtube` preset is the same -14 as `spotify`, and since no official YouTube figure was found, treat it as -14 by another name. The reply reports `measured_lufs`, `applied_gain_db`, `achieved_lufs` and `shortfall_db`. Calling the tool directly on a synthetic 11.25-second file, it said: \"Track 0 measured -18.2 LUFS; applied +2.2 dB to reach -16.0 LUFS.\"",
    },
    {
      type: "p",
      text: "By default the gain is capped so the track's peak stays at or below -1 dBFS (`true_peak_ceiling_db`). Asking the same file for -8 LUFS needed +10.2 dB, which would have peaked above the ceiling. The tool applied +9.1 dB instead and said so: \"-9.1 LUFS, 1.1 dB short. Run limiter first to close the gap.\" Limiting first trades peaks for loudness, so listen to what it does.",
    },
    {
      type: "p",
      text: "The tool works on one track, measured as it sits on the timeline, before that track's effects, pan, sends and the master chain. For a mix, mix down to a new track with `mix_to_new_track` and normalize that. The [beatmatch demo](/videos/dj-beatmatched-transition) does it that way: it mixes the two tracks to a new one, compresses it, limits at -1 dB, raises it to about -14 LUFS and exports a 50-second WAV at -13.9 LUFS peaking at -1 dBFS. The [mini-mix demo](/videos/dj-mini-mix) brings each track to about -14 LUFS with a limiter at -1 dB on the master, and the assistant notes that the fades were counted in the measurement.",
    },
    {
      type: "prompt",
      text: "Export the result as a WAV to /Users/me/Desktop/podcast.wav, then analyze the exported file.",
    },
    {
      type: "p",
      text: "`render_final` writes the file and `analyze_track` with a `path` measures it, so the number you check is the file's and not the session's. The tools reference lists [every tool](/docs/tools), and [Undo is a graph](/blog/undo-is-a-graph-branches-and-ab-compare) covers going back to a version before the normalize.",
    },
    { type: "h2", text: "What the limiter and the ceiling cannot promise" },
    {
      type: "p",
      text: "`limiter` turns the gain down so that no sample exceeds `ceiling_db`, then lets it recover over `release_ms`. It has zero latency, and its description is plain that the ceiling is \"a sample-peak ceiling, not an inter-sample true-peak one\". `normalize_loudness` is the same. Its parameter is called `true_peak_ceiling_db`, but the check in the source compares the largest absolute sample value of the track.",
    },
    {
      type: "p",
      text: "So neither guarantees a true-peak limit. On the 12 kHz test tone above (samples at -9.0 dBFS, true peak -6.0), `limiter` with a -7 dB ceiling changed nothing, because no sample was above -7, and ffmpeg still measured a true peak of -6.0 dBFS, 1 dB over the ceiling. `normalize_loudness` with a -7 dB ceiling and a -0.5 LUFS target applied +2.0 dB and stopped when the largest sample reached -7.0 dBFS; ffmpeg measured the true peak at -4.0 dBFS. That tone is a test signal built to show the gap, not typical audio. The point is that the tools cannot tell you how large the gap is for your file.",
    },
    {
      type: "p",
      text: "So set the ceiling lower than the limit you have to meet (Spotify's own page asks for -2 dB when a master is louder than -14), export, and check the file with a true-peak meter. FFmpeg's ebur128 filter prints both peaks in its summary:",
    },
    {
      type: "code",
      text: "ffmpeg -nostats -i podcast.wav -af ebur128=peak=sample+true -f null -",
    },
    { type: "h2", text: "Questions about LUFS targets" },
    { type: "h3", text: "Is LKFS the same as LUFS?" },
    {
      type: "p",
      text: "Yes. A footnote in R 128 says LUFS is equivalent to LKFS, the name BS.1770 uses, and that the EBU uses LUFS because it complies with international naming conventions.",
    },
    { type: "h3", text: "Does Spotify turn loud music down?" },
    {
      type: "p",
      text: "Its page says negative gain is applied to louder masters so the loudness is -14 LUFS, and that this \"lowers the volume in comparison to the master - no additional distortion occurs\". It also says the web player and third-party devices do not use normalization, so playback there can differ.",
    },
    { type: "h3", text: "Why does another meter read differently from edytlab?" },
    {
      type: "p",
      text: "Two reasons from the sources above. A mono file is measured as one channel in edytlab (the `ebur128` crate's default channel map gives a single channel the left role), while Audacity's manual says a two-channel \"dual mono\" file reads 3 dB higher than the one-channel version, and ffmpeg's filter has a `dualmono` option for it. And `normalize_loudness` measures the track before its effects and the master chain, so a figure taken after them can differ.",
    },
    { type: "h3", text: "Does edytlab upload audio to measure it?" },
    {
      type: "p",
      text: "No. The tool text describes the analysis as pure Rust with no model weights. Your chat messages and the tools' results go to the AI provider you chose, and your audio does not; [the local-first page](/use-cases/local-ai-audio-editor) has the table.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-22. Figures are quoted as the pages print them; check the page before you rely on one, because platforms change them.",
    },
    {
      type: "ul",
      items: [
        source("Spotify for Artists: Loudness normalization on Spotify", "https://support.spotify.com/us/artists/article/loudness-normalization/"),
        source("Spotify for Artists: Track not as loud as others?", "https://support.spotify.com/us/artists/article/track-not-as-loud-as-others/"),
        source("Apple Podcasts for Creators: Audio requirements", "https://podcasters.apple.com/support/893-audio-requirements"),
        source("Apple: Apple Digital Masters (PDF; its file title reads 2.4.1)", "https://www.apple.com/apple-music/apple-digital-masters/docs/apple-digital-masters.pdf"),
        source("SoundCloud Help Center: Will SoundCloud play my track at the level it's mastered?", "https://help.soundcloud.com/hc/en-us/articles/360053660014-Will-SoundCloud-play-my-track-at-the-level-it-s-mastered"),
        source("EBU R 128, version 5, November 2023 (PDF)", "https://tech.ebu.ch/docs/r/r128.pdf"),
        source("EBU R 128 s2, Loudness in Streaming, November 2023 (PDF)", "https://tech.ebu.ch/docs/r/r128s2.pdf"),
        source("EBU Tech 3341, 'EBU Mode' metering, November 2023 (PDF)", "https://tech.ebu.ch/docs/tech/tech3341.pdf"),
        source("FFmpeg Filters Documentation: ebur128", "https://ffmpeg.org/ffmpeg-filters.html#ebur128-1"),
        source("Audacity Manual: Loudness Normalization", "https://manual.audacityteam.org/man/loudness_normalization.html"),
        source("ebur128 crate 0.1.10, default_channel_map", "https://docs.rs/ebur128/0.1.10/src/ebur128/ebur128.rs.html"),
        source("edytlab tool reference (normalize_loudness, limiter, normalize, analyze_track, mix_to_new_track, render_final)", SRC("docs/tools-reference.md"), "at commit b1678feb"),
        source("edytlab source: normalize_loudness.rs", SRC("crates/tools/src/tool/normalize_loudness.rs"), "at commit b1678feb"),
        source("edytlab demo captions: beatmatch and mini-mix", SRC("website/lib/demos.ts"), "at commit b1678feb"),
      ],
    },
    {
      type: "p",
      text: "Not sources, but searched: YouTube Help and Google support pages, and Meta's help and business pages, for an official loudness figure. None was found.",
    },
  ],
};
