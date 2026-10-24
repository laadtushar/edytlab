import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const bpmAndKey: BlogPost = {
  slug: "how-to-find-the-bpm-and-key-of-a-song",
  title: "How to Find a Song's BPM and Key",
  date: "2026-10-24",
  description:
    "How tempo and key detection work, where they go wrong (double time, the wrong key), and what edytlab's analyze_track returns, with a run on synthetic audio.",
  coverAlt:
    "A row of vertical beat marks with a bracket over one beat, a half-spaced row of marks below it, and a circle of twelve points with one point joined to the centre",
  excerpt:
    "A BPM and key finder gives one number and one name, and both can be wrong in known ways. This is how the detection works, what edytlab's analyze_track returns and what it leaves as a placeholder, and what happened when it was run on audio with a known answer.",
  readTime: 10,
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
      text: "Key detection usually works from chroma: the energy in the audio folded into the twelve pitch classes, C, C♯, D and so on, regardless of octave. That twelve-number profile is compared with a template for each of the 24 major and minor keys, and the closest match wins. The templates edytlab uses are credited in its code to Carol Krumhansl and Edward Kessler's 1982 paper in Psychological Review.",
    },
    { type: "h2", text: "Where detection goes wrong" },
    {
      type: "ul",
      items: [
        "**Double or half time.** The DJ program Mixxx has a menu entry that changes the detected BPM to 50%, 66%, 75%, 133.33%, 150% or 200% of the value it found, and its manual says to try the analyzer settings if many of the detected values are incorrect. That such a menu exists says a tempo off by a simple ratio is a known failure.",
        "**A wrong key.** Relative keys share their notes, as above, and edytlab's key comes from one profile summed over the whole file, so a piece built on a few chords gives it little to separate them with. The run below shows a bare triad read as the wrong key.",
        "**One answer for the whole song.** A song that changes tempo or key has no single BPM or key. edytlab reports one of each.",
        "**Dynamics and style.** Audacity's manual calls its Beat Finder \"a fairly rough and ready tool\" that will not necessarily work well on a typical modern pop track with compressed dynamic range. That tool places labels on beats that are much louder than their surroundings, and the same condition applies to any onset-based method: the beat has to stand out from what is around it.",
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
        "**`bpm`** comes from the audio's spectral flux, the amount of new energy in each analysis frame of 1,024 samples taken every 512. It keeps local peaks at least 300 ms apart, counts the intervals between them in a histogram, and looks only between 60 and 200 BPM, in steps of one. When the double or the half of its first pick also has at least half the support of the strongest bin, it takes whichever is closest to 120. So the answer has no meaningful decimals, and the bias toward 120 is deliberate.",
        "**`key`** is a name such as \"A minor\": 12-bin chroma from frames of 4,096 samples, between about 27.5 Hz and 4,186 Hz, correlated against the 24 templates. Names use sharps (\"A# minor\", never \"B♭ minor\"), and the whole file gets one.",
        "**`beats`** is a metronome grid at the estimated tempo. It starts from the strongest onset in the first four seconds, steps by 60 ÷ BPM, and moves each beat to the nearest detected onset within 50 ms. Positions come from analysis frames 512 samples apart, 11.6 ms at 44.1 kHz, so a beat time is good to about that, not to the sample.",
        "**`downbeats`** is every fourth beat starting from the first one. The source calls it a stub that assumes 4/4. If the first beat is not the first beat of a bar, every downbeat is off by the same amount.",
        "**`sections`** is one section named \"track\" that spans the whole file. It is a placeholder: the source says real intro, verse and chorus segmentation is not done. Do not expect it to find a drop.",
        "**`rms_curve`** is one value for every 100 ms, and **`lufs_integrated`** is the file's integrated loudness, measured on the original channels.",
      ],
    },
    { type: "h2", text: "What a run on synthetic audio showed" },
    {
      type: "p",
      text: "Synthetic tracks were made with known answers and passed to `analyze_track` through the tool dispatcher, without a model in between. Each is 24 s at 44.1 kHz: a pad of three sine tones with two overtones each, a low kick thud on the beat and, in some, a short noise burst for a hi-hat between beats. Nothing here is a measure of accuracy on real recordings, which are messier in every way.",
    },
    {
      type: "ul",
      items: [
        "124 BPM, A minor triad, kick plus hi-hats on the off-beats: **124.0 BPM, A minor**.",
        "The same at 90 BPM with a D major triad: **179.0 BPM** (double), D major.",
        "The same at 70 BPM, A minor: **141.0 BPM** (double), A minor.",
        "Kick only, no hi-hats, at 124 and at 90 BPM: **124.0** and **90.0**. Without the off-beat hits there was nothing at twice the rate to count.",
        "A 120 BPM kick over a B♭ minor triad: **120.0 BPM, F major**. The key was wrong. F is a note of the chord, but a bare triad is thin evidence.",
      ],
    },
    {
      type: "p",
      text: "In the 124 BPM run with hi-hats, the beat times fell on the hi-hats, half a beat from the kicks, and the tempo was still right. The anchor is the strongest onset by spectral flux in the first four seconds, and a noise burst raises flux in every frequency band at once, which may be why it outweighed the kick here. If you line tracks up by the grid, listen to where it lands before you trust it.",
    },
    { type: "h2", text: "Find BPM and key in edytlab" },
    { type: "prompt", text: "What are the tempo and key of track 1 and track 2? Don't change anything yet." },
    {
      type: "p",
      text: "The reply gives the numbers from the tool. Two habits are worth having. First, tap along to the reported BPM, or compare with half and twice it. A slow, sparse song at 70 BPM may come back as 140. Second, play the root note on a keyboard, or hum it, against the key you were given. If the assistant is asked to act on the result, such as stretching one track to the other's tempo, it is acting on this estimate.",
    },
    { type: "prompt", text: "Track 2 is really 90 BPM, not 179. Time-stretch it to match track 1's tempo using 90." },
    {
      type: "p",
      text: "That gets the correction into the next step. The factor for `time_stretch` is the tempo you want divided by the tempo you have; [the tempo post](/blog/change-tempo-without-changing-pitch) explains the stretch and its limits, and [the beatmatch walkthrough](/blog/beatmatch-and-blend-two-tracks) puts the numbers to use. The tool reference lists [every tool](/docs/tools) the assistant can call.",
    },
    {
      type: "p",
      text: "Other free programs do this job too. Mixxx analyses a library ahead of a set and lets you correct a wrong BPM by a ratio. Audacity's Beat Finder places labels on loud beats. librosa is a Python library whose beat tracker is quoted above. None of them removes the check by ear.",
    },
    { type: "h2", text: "Questions about finding a song's BPM and key" },
    { type: "h3", text: "Why is the BPM double or half what I expected?" },
    {
      type: "p",
      text: "Because music often has hits between the beats, such as hi-hats, and a detector has to choose whether the main pulse is the slower or the faster one. edytlab's source says it prefers the candidate closest to 120 when the evidence for it is within half of the strongest. In the synthetic run, hi-hats between the beats were enough to move a 90 BPM track to 179. Halve or double the answer until it matches what you tap.",
    },
    { type: "h3", text: "How accurate is the key?" },
    {
      type: "p",
      text: "No accuracy figure for real recordings is given here, because none was measured. The source describes the target as a correlation between the track's chroma and the 24 templates, and the run above shows a wrong answer on a bare triad. Use it to narrow the choices, then confirm by ear or on an instrument.",
    },
    { type: "h3", text: "Is BPM and key detection free in edytlab?" },
    {
      type: "p",
      text: "edytlab is free and open source (MIT), and the analysis itself runs on your computer with no model. The cost of asking for it in chat is the AI provider's tokens, unless you run a local Ollama model, and your audio stays on your computer either way; [the local-first page](/use-cases/local-ai-audio-editor) has the table.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-24. The run described above used synthetic audio generated for this post, with the edytlab tools called directly. edytlab's own behaviour is stated from its source at the commit linked, not from the run alone.",
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
        source("edytlab source: analyze_track tool", SRC("crates/tools/src/tool/analyze_track.rs"), "at commit 8bf752ff"),
        source("edytlab source: audio-analysis bpm.rs, beats.rs, key.rs and sections.rs", SRC("crates/audio-analysis/src/bpm.rs"), "at commit 8bf752ff"),
        source("edytlab tool reference (analyze_track, time_stretch)", SRC("docs/tools-reference.md"), "at commit 8bf752ff"),
        source("edytlab demo captions: extended intro", SRC("website/lib/demos.ts"), "at commit 8bf752ff"),
      ],
    },
  ],
};
