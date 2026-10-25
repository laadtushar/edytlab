import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, SRC_V090, UPDATED, source, sourceUpdated } from "./shared";

const RELEASES = siteConfig.releases;

export const bpmAndKey: BlogPost = {
  slug: "how-to-find-the-bpm-and-key-of-a-song",
  title: "How to Find a Song's BPM and Key",
  date: "2026-10-24",
  dateModified: UPDATED,
  description:
    "How tempo and key detection work, where they go wrong (double time, the wrong key), and what edytlab's analyze_track returns, with a run on synthetic audio.",
  coverAlt:
    "A row of vertical beat marks with a bracket over one beat, a half-spaced row of marks below it, and a circle of twelve points with one point joined to the centre",
  excerpt:
    "A BPM and key finder gives one number and one name, and both can be wrong in known ways. This is how the detection works, what edytlab's analyze_track returns and what it leaves as a placeholder, and what happened when it was run on audio with a known answer.",
  readTime: 11,
  tags: ["BPM", "key detection", "analyze_track", "beat grid"],
  body: [
    {
      type: "p",
      text: "To find a song's BPM and key, run it through a detector and then check the answer by ear. In edytlab you ask for the tempo and key of a track and the assistant calls `analyze_track`, which reads the audio on your computer and returns one tempo, one key and a beat grid for the whole file. A detector saves the listening time and is wrong in known ways: a tempo reported at double or half its real value, or a key that is close to the right one but not it. This post says how detection works, what edytlab returns (including two fields that are placeholders), and what happened when it was run on synthetic audio whose answer was known.",
    },
    {
      type: "callout",
      text:
        "Reading this needs only a browser. For the edytlab steps you need [edytlab](" +
        RELEASES +
        ") and an audio file. Its installers are unsigned, so macOS and Windows warn on first launch; [Getting Started](/docs/getting-started) walks through it. To see a tempo and bar length asked for in the app, watch the [extended intro demo](/videos/dj-extended-club-intro).",
    },
    {
      type: "p",
      text: "**Updated 2026-10-25 for v0.9.0.** v0.8.1 read a 90 BPM groove as 179 and a 70 BPM one as 141, put the beat grid on the hi-hats, and read a bare B♭ minor triad as F major. v0.9.0 reworked the tempo, beat grid and key detection ([issue 696](https://github.com/laadtushar/edytlab/issues/696)), and this post describes it.",
    },
    { type: "h2", text: "What BPM and key mean" },
    {
      type: "p",
      text: "**BPM** is beats per minute: how many steady pulses fit in a minute, which is the speed at which you would tap your foot. At 124 BPM a beat is 60 ÷ 124 = 0.484 s long and a bar of four beats is 1.935 s.",
    },
    {
      type: "p",
      text: "A **key** names the home note, the tonic, and the scale built on it, major or minor. Two keys are relative when they share a key signature: Open Music Theory gives C major and A minor as the example, neither of which has a sharp or a flat, and says the tonic of the minor key is three half steps below the tonic of its relative major. So a song in A minor and one in C major use the same seven notes, and what tells them apart is which note the music rests on. The same text points at the first and last notes of a piece as clues, because pieces often start and end on the tonic. edytlab's detector adds up the notes over the whole file, so it does not use where a piece starts and ends.",
    },
    { type: "h2", text: "How BPM and key detection work" },
    {
      type: "p",
      text: "Most tempo detectors work from onsets, the moments where the sound changes suddenly. The librosa documentation describes its beat tracker, which follows a 2007 paper by Daniel Ellis, in three stages: measure onset strength, estimate tempo from onset correlation, and pick peaks in the onset strength that are consistent with that tempo. It takes an initial guess for the tempo, `start_bpm`, which is 120 by default, and says that if no onset strength can be detected it reports 0 BPM and an empty list.",
    },
    {
      type: "p",
      text: "Key detection usually works from chroma: the energy in the audio folded into the twelve pitch classes, C, C♯, D and so on, regardless of octave. That twelve-number profile is compared with a template for each of the 24 major and minor keys, and the closest match wins. The classic templates are Carol Krumhansl and Edward Kessler's, from a 1982 paper in Psychological Review, which edytlab used before v0.9.0. Its code now uses David Temperley's, because the Krumhansl and Kessler set tends to name the dominant as the tonic (F for B♭), as its comments quote from the Humdrum `keycor` manual.",
    },
    { type: "h2", text: "Where detection goes wrong" },
    {
      type: "ul",
      items: [
        "**Double or half time.** The DJ program Mixxx has a menu entry that changes the detected BPM to 50%, 66%, 75%, 133.33%, 150% or 200% of the value it found, and its manual says to try the analyzer settings if many of the detected values are incorrect. That such a menu exists says a tempo off by a simple ratio is a known failure. edytlab lists the half and the double, with confidences, so a close call shows.",
        "**A wrong key.** Relative keys share their notes, as above, and edytlab's key comes from one profile summed over the whole file, so a piece built on a few chords gives it little to separate them with. The Humdrum manual, as edytlab's source quotes it, says the profile set it now uses tends to name the relative major as the tonic of a minor key, so the result carries the runner-up key and both scores.",
        "**One answer for the whole song.** A song that changes tempo or key has no single BPM or key. edytlab reports one of each.",
        "**Dynamics and style.** Audacity's manual calls its Beat Finder \"a fairly rough and ready tool\" that will not necessarily work well on a typical modern pop track with compressed dynamic range. Any onset-based method needs the beat to stand out from what is around it.",
      ],
    },
    { type: "h2", text: "What analyze_track returns" },
    {
      type: "p",
      text: "`analyze_track` takes a `track`, an index in the session, or a `path` to a file that is not in it, and not both. With `track` it analyses the track as it sits on the timeline, before its gain and effects. It only reads, so it is never held for approval, and the analysis is plain Rust on your computer, with no model to download. Its description lists BPM, key, beat grid, downbeats, sections, an RMS curve and integrated loudness. What each one is, read from the source:",
    },
    {
      type: "ul",
      items: [
        "**`bpm`** comes from two onset-strength curves, the new energy in each frame of 2,048 samples taken every 512, over the whole spectrum and over 30 to 150 Hz, where a kick sits. For every tempo from 50 to 220 BPM and every phase, it lays a comb on the curve, with teeth on the beats and probes halfway between, and scores the mean under the teeth minus half the mean under the probes. A real beat has onsets on it and nothing between; the double has half its teeth in silence, and the half has its probes on the missing beats. It reads the kick curve when there is a kick, so hi-hats are not beats. A weak preference for tempos near 120 only breaks ties. The answer is to 0.1 BPM, with **`bpm_confidence`** (0 to 1; below about 0.5 it is a coin toss) and **`bpm_alternatives`** (the half and the double, with confidences). `bpm` is null, with no beats, when there is no steady pulse, as in a pad or a drone.",
        "**`key`** is a name such as \"A minor\". The chroma counts only spectral peaks, in windows of about 0.37 s between 50 Hz and 5,000 Hz, after the recording's tuning is estimated and removed. A second chroma of the 40 to 250 Hz peaks is added at half weight, since the lowest note is the likeliest root, and the sum is correlated against the 24 Temperley templates. **`key_score`** is that correlation, and **`key_runner_up`** and **`key_runner_up_score`** are the next key and its score. Names use sharps (\"A# minor\", never \"B♭ minor\"), and the whole file gets one.",
        "**`beats`** is a grid at the comb's tempo and phase on the curve the tempo came from, so with a kick it falls on the kicks. Each beat is pulled to the strongest onset within 60 ms or a fifth of a beat, whichever is less, and the grid follows the pull a little, so a drummer who pushes the beat is tracked. Frames are 11.6 ms apart at 44.1 kHz, with each peak placed to a fraction of a frame; the tests hold the grid within 6 ms of synthetic kicks,.",
        "**`downbeats`** is every fourth beat starting from the first one. The source calls it a stub that assumes 4/4, and the result says so (`downbeats_source` is `assumed_4_4`). If the first beat is not the first beat of a bar, every downbeat is off by the same amount.",
        "**`sections`** is one section named \"track\" that spans the whole file. It is a placeholder: the source says real intro, verse and chorus segmentation is not done, and the result says so (`sections_source` is `whole_track`). Do not expect it to find a drop.",
        "**`rms_curve`** is one value for every 100 ms, and **`lufs_integrated`** is the file's integrated loudness, measured on the original channels.",
      ],
    },
    { type: "h2", text: "What a run on synthetic audio showed" },
    {
      type: "p",
      text: "Synthetic tracks were made with known answers and analysed with the code `analyze_track` runs. Each is 24 s at 44.1 kHz: a pad of three sine tones with two overtones each, a low kick thud on the beat and, in some, a short noise burst for a hi-hat between beats. The run was first made on v0.8.1 and repeated on v0.9.0 with regenerated tracks, and the numbers below are from the second run. Nothing here is a measure of accuracy on real recordings, which are messier in every way.",
    },
    {
      type: "ul",
      items: [
        "124 BPM, A minor triad, kick plus hi-hats on the off-beats: **124.0 BPM** (confidence 0.75), **A minor** (0.60).",
        "The same at 90 BPM with a D major triad: **90.0 BPM** (0.69, with 180.0 listed as the double), **D major** (0.81). v0.8.1 read this track as 179.0.",
        "The same at 70 BPM, A minor: **70.0 BPM** (0.60, with 139.9 listed as the double), **A minor** (0.66). v0.8.1 read this track as 141.0.",
        "Kick only, no hi-hats, at 124 and at 90 BPM: **124.0** and **90.0**, the same as with the hi-hats.",
        "A 120 BPM kick over a B♭ minor triad: **120.1 BPM, A# minor** (0.68, next F# major 0.53). v0.8.1 read this one as F major.",
        "The pad alone, no drums: **no tempo** (`bpm` is null) and no beats, key A minor.",
      ],
    },
    {
      type: "p",
      text: "On v0.8.1 the beat times of the 124 BPM track with hi-hats fell on the hi-hats, half a beat from the kicks, although the tempo was right. On v0.9.0, 49 of its 50 beats fall within 35 ms of a kick and none within 35 ms of a hi-hat; at 90 and 70 BPM all 36 and all 28 fall on a kick. If you line tracks up by the grid, still listen to where it lands: a real kick is not a synthetic one.",
    },
    { type: "h2", text: "Find BPM and key in edytlab" },
    { type: "prompt", text: "What are the tempo and key of track 1 and track 2? Don't change anything yet." },
    {
      type: "p",
      text: "The reply gives the numbers from the tool. Two habits are worth having. First, tap along to the reported BPM, or compare with half and twice it. A sparse song may come back at half or double its tempo; the result lists both in `bpm_alternatives`. Second, play the root note on a keyboard, or hum it, against the key you were given. If the assistant is asked to act on the result, such as stretching one track to the other's tempo, it is acting on this estimate.",
    },
    { type: "prompt", text: "Track 2 is really 90 BPM, not 180. Time-stretch it to match track 1's tempo using 90." },
    {
      type: "p",
      text: "That gets the correction into the next step. The factor for `time_stretch` is the tempo you want divided by the tempo you have; [the tempo post](/blog/change-tempo-without-changing-pitch) explains the stretch and its limits, and [the beatmatch walkthrough](/blog/beatmatch-and-blend-two-tracks) puts the numbers to use. The tool reference lists [every tool](/docs/tools) the assistant can call.",
    },
    {
      type: "p",
      text: "Mixxx, which corrects a wrong BPM by a ratio, Audacity's Beat Finder and the librosa library quoted above do this job too. None of them removes the check by ear.",
    },
    { type: "h2", text: "Questions about finding a song's BPM and key" },
    { type: "h3", text: "Why is the BPM double or half what I expected?" },
    {
      type: "p",
      text: "Because music often has hits between the beats, such as hi-hats, and a detector has to choose whether the main pulse is the slower or the faster one. edytlab penalises onsets halfway between its beats and reads the kick curve when there is a kick. In the synthetic run, hi-hats between the beats moved a 90 BPM track to 179 in v0.8.1; v0.9.0 reads 90.0 and lists 180.0 as the double. A kick on beats 1 and 3 only genuinely reads as the slower tempo, with the double in `bpm_alternatives`. Halve or double the answer until it matches what you tap.",
    },
    { type: "h3", text: "How accurate is the key?" },
    {
      type: "p",
      text: "No accuracy figure for real recordings is given here, because none was measured. The bare triad that v0.8.1 read as F major is read correctly in v0.9.0, and on the synthetic corpus used to choose the profile (24 keys per case) the setting that shipped was right in every case but an ambiguous four-chord loop. That corpus is clean synthetic audio, so it says nothing about records. Read the runner-up, then confirm by ear or on an instrument.",
    },
    { type: "h3", text: "Is BPM and key detection free in edytlab?" },
    {
      type: "p",
      text: "edytlab is free and open source (MIT), and the analysis itself runs on your computer with no model. The cost of asking for it in chat is the AI provider's tokens, unless you run a local Ollama model, and your audio stays on your computer either way; [the local-first page](/use-cases/local-ai-audio-editor) has the table.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-24. The run used synthetic audio generated for this post, against v0.8.1 and again, for the update, against v0.9.0. edytlab's own behaviour is stated from its source at the commit linked (re-read on 2026-10-25), not from the run alone.",
    },
    {
      type: "ul",
      items: [
        source("librosa documentation: librosa.beat.beat_track (version 0.11.0)", "https://librosa.org/doc/0.11.0/generated/librosa.beat.beat_track.html"),
        source("D. P. W. Ellis, \"Beat Tracking by Dynamic Programming\", Journal of New Music Research 36(1), 51-60, 2007 (DOI; reference details checked against Crossref)", "https://doi.org/10.1080/09298210701653344"),
        source("C. L. Krumhansl and E. J. Kessler, \"Tracing the dynamic changes in perceived tonal organization in a spatial representation of musical keys\", Psychological Review 89(4), 334-368, 1982 (DOI; reference details checked against Crossref)", "https://doi.org/10.1037/0033-295X.89.4.334"),
        source("Open Music Theory: Minor Scales, Scale Degrees, and Key Signatures (the relative relationship)", "https://viva.pressbooks.pub/openmusictheory/chapter/minor-scales/"),
        source("Mixxx manual, The Mixxx Library (Change BPM)", "https://manual.mixxx.org/2.5/en/chapters/library.html"),
        source("Audacity manual: Beat Finder", "https://manual.audacityteam.org/man/beat_finder.html"),
        sourceUpdated("edytlab source: analyze_track tool", SRC_V090("crates/tools/src/tool/analyze_track.rs"), "at commit aa9f6cd0"),
        sourceUpdated("edytlab source: audio-analysis bpm.rs (the comb, the prior), onset.rs, beats.rs and key.rs (the profile choice and the Humdrum manual it quotes)", SRC_V090("crates/audio-analysis/src/bpm.rs"), "at commit aa9f6cd0"),
        sourceUpdated("edytlab issue 696: analyze_track tempo, beat grid and key", "https://github.com/laadtushar/edytlab/issues/696"),
        sourceUpdated("edytlab test: key_corpus.rs (the profile comparison)", SRC_V090("crates/audio-analysis/tests/key_corpus.rs"), "at commit aa9f6cd0"),
        sourceUpdated("edytlab tool reference (analyze_track, time_stretch)", SRC_V090("docs/tools-reference.md"), "at commit aa9f6cd0"),
        source("edytlab demo captions: extended intro", SRC("website/lib/demos.ts"), "at commit 8bf752ff"),
      ],
    },
  ],
};
