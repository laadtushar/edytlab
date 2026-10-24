import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const truePeak: BlogPost = {
  slug: "true-peak-vs-sample-peak",
  title: "True Peak vs Sample Peak: What -1 dBTP Means",
  date: "2026-10-24",
  description:
    "A sample peak is the largest stored value; a true peak is the waveform's largest value between samples. Why -1 dBTP, and what edytlab's ceiling does not do.",
  coverAlt:
    "Dots on a smooth wave that rises above a dashed ceiling line between two dots, with the dots themselves all sitting below the line",
  excerpt:
    "The loudest sample in a file is not always the loudest the waveform gets. That gap is what true peak measures and what platforms leave headroom for. This is the standard's own account, the numbers from a run, and a plain statement of what edytlab's limiter does not do.",
  readTime: 8,
  tags: ["true peak", "dBTP", "headroom", "limiter"],
  body: [
    {
      type: "p",
      text: "A **sample peak** is the largest sample value stored in a file. A **true peak** is the largest value the waveform reaches between the samples, found by oversampling, and it can be higher. Platforms ask for a true peak of about -1 dBTP (decibels relative to full scale, true-peak measurement) so that nothing clips after conversion to a lossy format. edytlab's limiter and its normalize tools work on samples, which makes their ceilings a sample-peak ceiling, not an inter-sample true-peak one: leave headroom under the figure you must meet, and check the file with a true-peak meter such as ffmpeg's `ebur128` filter. The rest of this post is the standard's account, the numbers from a run, and how to do that check.",
    },
    {
      type: "callout",
      text:
        "Reading this needs only a browser. For the edytlab steps you need [edytlab](" +
        RELEASES +
        ") and an audio file, and the check at the end needs ffmpeg. The installers are unsigned, so macOS and Windows warn on first launch ([Getting Started](/docs/getting-started)). In the [beatmatch demo](/videos/dj-beatmatched-transition) the assistant mixes two tracks, limits at -1 dB and exports a WAV, which is the step this post is about.",
    },
    { type: "h2", text: "Sample peak and true peak" },
    {
      type: "p",
      text: "A digital file is a list of numbers, one for each moment the sound was sampled. ITU-R BS.1770-5, the loudness standard, defines true-peak level as \"the maximum (positive or negative) value of the signal waveform in the continuous time domain\", and says it \"may be higher than the largest sample value\". Its informative annex explains why: the actual peaks of a sampled signal \"usually occur between the samples rather than precisely at a sampling instant\", so a meter that only looks at the stored values can read low.",
    },
    {
      type: "p",
      text: "How much lower depends on the signal. The annex says that for a pure tone at a quarter of the sampling frequency, in an unlucky phase, a sample-peak meter reads 3 dB low. For single transients it says the under-read \"can commonly be several dBs\", because real sounds have high-frequency content at their attacks. EBU Tech 3341 turns this into test signals: a sine at a quarter of the sample rate, half of full scale, starting at 45 degrees, has a true peak of -6.0 dBTP, while its largest sample is 0.5 × sin 45° = 0.354, which is -9.0 dBFS. The same signal at 1.41 times full scale has a true peak of +3.0 dBTP, although none of its samples is above 1.0.",
    },
    { type: "h2", text: "How true peak is measured" },
    {
      type: "p",
      text: "Annex 2 of BS.1770-5 gives the method. Attenuate by 12.04 dB to leave room for integer arithmetic (not needed in floating point), oversample four times, so 48 kHz becomes 192 kHz, with a low-pass interpolating filter (the annex gives a 48th-order, four-phase FIR as an example), take the absolute value, and convert to decibels, written dB TP. The oversampling \"more accurately indicates the actual waveform that is represented by the audio samples\". Four times is a compromise: the annex's table puts the largest remaining under-read at 0.554 dB for content up to 0.45 of the sample rate and 0.688 dB at half of it, and 16 times at 0.034 dB and 0.042 dB.",
    },
    {
      type: "p",
      text: "Recommendation 5 of the standard says the method should be based on that annex \"or on a method that gives similar or superior results\", so meters can differ. EBU R 128 allows the meter a tolerance of ±0.3 dB for signals limited to 20 kHz.",
    },
    { type: "h2", text: "Why platforms ask for about -1 dBTP" },
    {
      type: "p",
      text: "The standard's own reason is in its preamble: peak levels \"may increase due to commonly applied processes such as filtering or bit-rate reduction\". What each party asks for, from its own page:",
    },
    {
      type: "ul",
      items: [
        "**Spotify** advises masters at -14 integrated LUFS with true peak below -1 dB TP, which it says suits lossy formats (Ogg/Vorbis and AAC) and keeps extra distortion out of the transcoding, and below -2 dB if the master is louder than -14.",
        "**Apple Podcasts** recommends that the true-peak value does not exceed -1 dB FS, with loudness and true peak calculated to ITU-R BS.1770-5, before encoding, because \"audio compression algorithms typically don't modify the loudness and might clip the signal\" if it is not respected.",
        "**Apple Digital Masters** recommends leaving at least 1 dB of headroom, because oversampling during playback \"can result in undesirable clipping\" if the audio is at 0 dBFS.",
        "**EBU R 128** says a programme's true-peak level shall not exceed -1 dBTP during production, and that the permitted maximum may be lower for different distribution systems and data reduction rates.",
      ],
    },
    {
      type: "p",
      text: "The common figure is -1 dBTP. The lesson of the Spotify line is that louder masters may be asked to leave more.",
    },
    { type: "h2", text: "What edytlab's ceiling does" },
    {
      type: "p",
      text: "The `limiter` tool's description is exact about this: \"Zero latency; the ceiling is a sample-peak ceiling, not an inter-sample true-peak one.\" It turns the gain down so no sample goes over `ceiling_db`, then lets it recover over `release_ms`. `normalize` scans a track for its peak amplitude and sets gain so that peak equals `target_dbfs`: also a sample peak. `normalize_loudness` has a parameter named `true_peak_ceiling_db`; its own description calls it a \"Peak ceiling in dBFS\", and the source finds the largest absolute sample value on the track. `render_final` reports `peak_dbfs`, and that too is the largest absolute sample it wrote. `analyze_track` measures loudness and reports no peak at all.",
    },
    {
      type: "p",
      text: "So nothing in edytlab measures or limits true peak, and a master that reads -1.0 dBFS in its export report can read higher on a true-peak meter. For limiting the whole mix, the `limiter` description points to `add_effect` with scope \"master\", which keeps the mix limited as tracks change. The test file for the tool says it shares its implementation with the effect-chain limiter, so the same caveat applies there.",
    },
    { type: "h2", text: "What a run on synthetic audio showed" },
    {
      type: "p",
      text: "The signals were the EBU test tones above, generated as 16-bit mono WAV files at 48 kHz, three seconds long with 10 ms fades: a quarter-sample-rate sine at 45 degrees, at amplitude 0.5 and at 1.41. The edytlab tools were called directly, the result rendered to WAV, and the render measured with ffmpeg 6.1.1, `ebur128=peak=sample+true`.",
    },
    {
      type: "ul",
      items: [
        "**The tones as made:** amplitude 0.5 measured -9.0 dBFS sample peak and -6.0 dBTP; amplitude 1.41 measured 0.0 dBFS (ffmpeg printed -0.0) and +3.0 dBTP. Both match Tech 3341's table.",
        "**`limiter`, ceiling -1.0, on the 1.41 tone:** sample peak -1.0 dBFS, **true peak +2.0 dBTP**.",
        "**`normalize`, target -1.0, on the 0.5 tone:** sample peak -1.0 dBFS, **true peak +2.0 dBTP**.",
        "**`limiter`, ceiling -4.0, on the 1.41 tone:** sample peak -4.0 dBFS, true peak -1.0 dBTP.",
      ],
    },
    {
      type: "p",
      text: "These tones are the extreme case the standard describes, with a 3 dB gap. The annex says the potential error grows with the frequency of the signal, and 12 kHz is high. It gives no typical figure for music and nor does this post. The only way to know a file's gap is to measure it.",
    },
    { type: "h2", text: "Leave headroom and check it" },
    {
      type: "p",
      text: "Pick a ceiling a couple of dB under your target, render, measure, and lower the ceiling if the true peak is still over. In edytlab:",
    },
    {
      type: "prompt",
      text: "Put a limiter on the master with a ceiling of -3 dB, then export a WAV and tell me its peak.",
    },
    {
      type: "p",
      text: "Then measure the exported file with ffmpeg. FFmpeg's documentation describes the options: sample peak is \"looking for the higher sample value\", and true-peak mode does the lookup \"on an over-sampled version of the input stream\" and needs a build with libswresample. The summary at the end of the output has a \"True peak\" block:",
    },
    {
      type: "code",
      text: "ffmpeg -hide_banner -nostats -i master.wav -af ebur128=peak=sample+true -f null -",
    },
    {
      type: "p",
      text: "If it reads above your target, ask for a lower ceiling and render again. Each limiter is a session node, so [undo](/blog/undo-is-a-graph-branches-and-ab-compare) takes it back. [The loudness post](/blog/lufs-for-spotify-youtube-apple-music-podcasts) covers the LUFS side of the same delivery, and [the tools reference](/docs/tools) lists the limiter and the rest.",
    },
    { type: "h2", text: "Questions about true peak" },
    { type: "h3", text: "Is -1 dBTP the same as -1 dBFS?" },
    {
      type: "p",
      text: "No. dBFS compares a level with full scale and is usually read from the stored samples. dBTP is the same scale applied to the true peak. A file whose largest sample is -1 dBFS can have a true peak above that, as the run shows. Apple writes \"dB FS\" in its podcast requirement but says the value is calculated to BS.1770-5, which is the true-peak method.",
    },
    { type: "h3", text: "Does edytlab guarantee my master stays under -1 dBTP?" },
    {
      type: "p",
      text: "No. Its limiter and its normalize tools hold a sample peak. They are useful for staying out of digital clipping, and a headroom margin plus a measurement gives you the true-peak figure.",
    },
    { type: "h3", text: "How much headroom is enough?" },
    {
      type: "p",
      text: "There is no universal figure. The standard says transients commonly under-read by several dB on a sample-peak meter, and its extreme tone by 3 dB. The platform pages above ask for about -1 dBTP and, for Spotify's louder masters, -2. Measure the file you are delivering.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-24. The run used synthetic test tones generated for this post, with the edytlab tools called directly and the renders measured with ffmpeg. edytlab's own behaviour is stated from its tool reference and source at the commit linked.",
    },
    {
      type: "ul",
      items: [
        source("ITU-R BS.1770-5, Algorithms to measure audio programme loudness and true-peak audio level (11/2023), Annex 2 and its Attachment 1 (PDF)", "https://www.itu.int/dms_pubrec/itu-r/rec/bs/R-REC-BS.1770-5-202311-I!!PDF-E.pdf"),
        source("EBU R 128, Loudness normalisation and permitted maximum level of audio signals (November 2023, PDF)", "https://tech.ebu.ch/docs/r/r128.pdf"),
        source("EBU Tech 3341, Loudness metering: 'EBU Mode' metering to supplement EBU R 128 loudness normalisation (PDF; Table 1, signals 15 to 19)", "https://tech.ebu.ch/docs/tech/tech3341.pdf"),
        source("Spotify for Artists: Loudness normalization", "https://support.spotify.com/us/artists/article/loudness-normalization/"),
        source("Apple Podcasts for Creators: Audio requirements", "https://podcasters.apple.com/support/893-audio-requirements"),
        source("Apple: Apple Digital Masters (PDF; Loudness, Dynamic Range, and Clipping)", "https://www.apple.com/apple-music/apple-digital-masters/docs/apple-digital-masters.pdf"),
        source("FFmpeg documentation: ebur128 filter", "https://ffmpeg.org/ffmpeg-filters.html#ebur128-1"),
        source("edytlab tool reference (limiter, normalize, normalize_loudness, render_final, analyze_track)", SRC("docs/tools-reference.md"), "at commit 8bf752ff"),
        source("edytlab source: normalize_loudness tool", SRC("crates/tools/src/tool/normalize_loudness.rs"), "at commit 8bf752ff"),
        source("edytlab test: limiter_tool.rs (the tool shares its implementation with the effect-chain limiter)", SRC("crates/tools/tests/limiter_tool.rs"), "at commit 8bf752ff"),
        source("edytlab source: audio-engine render.rs (the peak that render_final reports)", SRC("crates/audio-engine/src/render.rs"), "at commit 8bf752ff"),
        source("edytlab demo captions: beatmatch and blend", SRC("website/lib/demos.ts"), "at commit 8bf752ff"),
      ],
    },
  ],
};
