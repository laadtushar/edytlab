import { octoberPosts } from "./blog-posts-october";
import { stripInline } from "./inline";
import { siteConfig } from "./site";

/**
 * `p`, `ul`, `ol` and `callout` text takes the inline markup in
 * `lib/inline.ts` (links, `code`, **bold**). A `prompt` is a line to
 * type into edytlab's chat, shown as typed: verbatim, never parsed.
 */
export type Block =
  | { type: "h2"; text: string }
  | { type: "h3"; text: string }
  | { type: "p"; text: string }
  | { type: "ul"; items: string[] }
  | { type: "ol"; items: string[] }
  | { type: "prompt"; text: string }
  | { type: "callout"; text: string };

export interface BlogPost {
  /** Kebab-case. Names the URL, the cover and the social image. */
  slug: string;
  title: string;
  /** ISO date (YYYY-MM-DD) the post was first published. */
  date: string;
  /** ISO date of the last substantive edit. Defaults to `date`. */
  dateModified?: string;
  /**
   * One or two sentences for search results, social cards, the blog
   * index and the feed. At most `MAX_DESCRIPTION` characters, about
   * what a search result shows before it cuts the line.
   */
  description: string;
  /** The longer lede shown under the title on the post itself. */
  excerpt: string;
  /** What the cover shows, as the alt text for the cover and the social image. */
  coverAlt: string;
  readTime: number;
  tags: string[];
  body: Block[];
}

export const MAX_DESCRIPTION = 160;

/** Who the posts are by: the project, as in every byline so far. */
export const BLOG_AUTHOR = { name: "edytlab", url: siteConfig.url } as const;

const earlierPosts: BlogPost[] = [
  {
    slug: "ai-audio-editing-local-first",
    title: "AI Audio Editing in 2026: Why Local-First Is the Only Approach That Matters",
    date: "2026-05-10",
    description:
      "Cloud AI audio tools upload your files and depend on someone else's servers. Here is what local-first audio editing changes, and where an LLM still comes in.",
    coverAlt:
      "A laptop holding a waveform, with a dashed line to a small cloud that carries only a short text message",
    excerpt:
      "Cloud AI audio tools upload your stems to third-party servers, lock you into subscriptions, and go offline when the API is down. Local-first AI audio editing changes all of that.",
    readTime: 7,
    tags: ["local-first", "AI audio editor", "privacy", "offline"],
    body: [
      {
        type: "p",
        text: "AI audio editing has exploded. Tools that once required a full-time engineer — stem separation, automatic transcription, pitch correction, noise removal — now live inside consumer apps. The catch? Almost every one of them routes your audio through a cloud server.",
      },
      {
        type: "h2",
        text: "The Hidden Cost of Cloud Audio Processing",
      },
      {
        type: "p",
        text: "When you drag a file into a cloud-based AI audio tool, that file travels to a data center, gets processed by shared compute, and the result is streamed back to you. For a short voice memo this is fine. For a 48-track session with stems, stems, and pre-master busses, this is a privacy, latency, and reliability problem.",
      },
      {
        type: "ul",
        items: [
          "Your unreleased music is now on someone else's server — often with vague retention policies.",
          "Every file makes the round trip, so latency grows with file size.",
          "If the service has an outage, your session is blocked regardless of your deadline.",
          "Subscriptions fund the compute. Cancel the sub, lose the feature.",
        ],
      },
      {
        type: "h2",
        text: "What Local-First Actually Means for Audio",
      },
      {
        type: "p",
        text: "Local-first means the DSP engine — the code that actually processes audio samples — runs entirely on your machine. Your stems never leave. The waveform analysis happens on your CPU. The only bytes that leave your machine are the text tokens you send to your chosen LLM provider to describe the edit you want.",
      },
      {
        type: "callout",
        text: "edytlab uses a pure-Rust audio graph (cpal · symphonia · rubato · realfft). Every cut, gain adjustment and pitch shift runs on-device, and stem separation is built to run there too once its model ships. Only the chat conversation hits the network — and you choose which LLM provider that goes to.",
      },
      {
        type: "h2",
        text: "The Practical Difference: A Studio Workflow Example",
      },
      {
        type: "p",
        text: 'Imagine you are mastering a 10-track album. In a cloud workflow, you upload each stem set, wait for processing, download results, repeat. With a local-first editor, you open the session, type "trim the silence off the start of track 3, boost the low end by 4 dB, and export a WAV", and the agent runs that chain on files that are already on your disk.',
      },
      {
        type: "h3",
        text: "Nothing to Round-Trip",
      },
      {
        type: "p",
        text: "In a cloud workflow every operation pays for an upload, a queue and a download. In edytlab the audio is already on your disk, so an edit is a function call on files you already have. The one network request is the chat message to your LLM provider, and with Ollama even that stays on your machine.",
      },
      {
        type: "h2",
        text: "Bring Your Own LLM Key",
      },
      {
        type: "p",
        text: "Local-first audio processing does not mean you cannot use AI language models. edytlab connects to Anthropic, OpenAI, Google Gemini, Groq or OpenRouter using API keys you store in your own OS keychain — or to a local model through Ollama, with no key at all. The conversation that translates your plain-English instructions into tool calls runs through your chosen provider — you own the API contract, you see the usage, you can switch models without reinstalling anything.",
      },
      {
        type: "h2",
        text: "The Future of Professional Audio Tooling",
      },
      {
        type: "p",
        text: "Professional audio engineers have always been suspicious of cloud lock-in, and rightly so. Pro Tools famously pivoted to subscription, alienating a generation of studios. The next wave of AI audio tools is better served by a model where AI accelerates the workflow without owning the session data. Local-first is not a niche constraint — it is the architecture that actually respects professional requirements.",
      },
      {
        type: "p",
        text: "As on-device AI inference improves — Apple Silicon Neural Engine, AMD XDNA, NVIDIA DLSS-equivalent for audio — the gap between local and cloud audio AI should keep narrowing. Tools built local-first today will not need to be rearchitected. Tools built cloud-first will.",
      },
    ],
  },
  {
    slug: "stem-separation-explained-demucs",
    title: "Stem Separation Explained: How AI Isolates Vocals and Instruments",
    date: "2026-05-12",
    description:
      "How Demucs splits a mix into vocals, drums, bass and other, what each stem is good for, and where stem separation stands in edytlab (it has not shipped yet).",
    coverAlt:
      "One wide waveform on the left splitting into four stacked lanes on the right, one for each stem",
    excerpt:
      "Stem separation used to need the original multitrack session. Demucs can split a stereo mix into vocals, drums, bass and other, and it is built to run on your own machine. In edytlab the tool has not shipped yet, so this post explains the idea.",
    readTime: 6,
    tags: ["stem separation", "Demucs", "vocal isolation", "music production"],
    body: [
      {
        type: "p",
        text: "Stem separation — also called source separation — is the process of decomposing a mixed audio track into individual instrument stems. If you have a finished stereo mix and want just the vocal line, or the drum pattern, or the bass groove, stem separation is how you get there without the original multitrack session.",
      },
      {
        type: "h2",
        text: "How Demucs Works",
      },
      {
        type: "p",
        text: "Demucs is an open-source deep neural network model developed by Meta Research that uses a U-Net architecture operating on both the raw waveform and the spectrogram simultaneously. Unlike earlier FFT-based approaches that created audible artifacts (the classic 'phasey' sound), Demucs processes temporal dependencies in the audio signal, which dramatically reduces the musical noise floor in separated stems.",
      },
      {
        type: "ul",
        items: [
          "Waveform encoder: compresses the raw audio into a learned latent representation.",
          "Spectrogram encoder: simultaneously processes the frequency-domain view of the same signal.",
          "Dual-path transformer: models long-range dependencies across both representations.",
          "Decoder: reconstructs each stem from the shared latent space.",
        ],
      },
      {
        type: "h2",
        text: "The Four Standard Stems",
      },
      {
        type: "p",
        text: "Demucs v4 (htdemucs) separates a stereo mix into four stems by default: vocals, drums, bass, and other (everything else — guitars, keys, synths, orchestral elements). Each stem is output as a separate stereo file at the original sample rate.",
      },
      {
        type: "h3",
        text: "Vocals",
      },
      {
        type: "p",
        text: "The vocals stem isolates lead and backing vocals. Quality degrades when the vocal sits very close in frequency to a sustained synthesizer pad — the model cannot always distinguish sustained harmonic content from voice formants. For most commercial pop, R&B, and hip-hop material, vocal isolation quality is production-usable.",
      },
      {
        type: "h3",
        text: "Drums",
      },
      {
        type: "p",
        text: "Drums are the most reliably separated stem because percussion has a distinctive transient profile that is easy for the model to identify. Kick, snare, hi-hats, and cymbals all separate well unless the mix has heavy reverb smearing the transients.",
      },
      {
        type: "h2",
        text: "Practical Uses in Music Production",
      },
      {
        type: "ul",
        items: [
          "Isolate the vocal from a reference track to study the performance style.",
          "Extract the vocal stem to make an acapella for a DJ edit.",
          "Remove the bass from a full mix to re-record it with a different instrument.",
          "Create an instrumental version of a track where the original multitrack no longer exists.",
          "Transcribe a melody by separating the lead instrument and running it through pitch detection.",
        ],
      },
      {
        type: "h2",
        text: "Running Demucs Locally Without Uploading Anything",
      },
      {
        type: "p",
        text: "Cloud-based stem separation tools (Lalal.ai, LALAL.AI, Moises) all upload your audio. For unreleased material — demos, client work, sync licensing tracks — this is a non-starter. edytlab is built to run Demucs as a local tool call: the model runs on your machine, the stems are written to your local session, and nothing is uploaded. That inference has not shipped yet — today the separate_stems tool returns an error.",
      },
      {
        type: "callout",
        text: 'Once it ships, the workflow in edytlab is one sentence: "separate the vocals from track 1". The agent calls the stem separation tool, Demucs runs on-device, and the separated stems appear as new tracks in your session timeline.',
      },
      {
        type: "h2",
        text: "Model Selection and Quality Trade-offs",
      },
      {
        type: "p",
        text: "Demucs offers several model variants. htdemucs is the recommended default — it offers the best quality-to-speed ratio on modern hardware. mdx_extra gives slightly better vocal quality at the cost of more VRAM. htdemucs_6s adds guitar and piano as separate stems, which is useful for complex arrangements but takes longer to run.",
      },
      {
        type: "p",
        text: "How long separation takes depends on your hardware and on the variant, and a GPU helps a great deal. edytlab has no separation speed to quote, because the tool has not shipped.",
      },
    ],
  },
  {
    slug: "podcast-production-ai-workflow",
    title: "AI Podcast Production: Record, Edit, Mix and Export in One Session",
    date: "2026-05-13",
    dateModified: "2026-10-10",
    description:
      "A podcast edit is a list of repeatable steps. Here is the whole list, which of them edytlab can run today, and which wait on transcription.",
    coverAlt:
      "A speech waveform in bursts, with the gaps between them marked as trimmed silence",
    excerpt:
      "Podcast post-production is the same list of steps every episode. Here is the list, which steps edytlab can run from a sentence today, and which ones wait for on-device transcription, which has not shipped yet.",
    readTime: 8,
    tags: ["podcast editor", "AI podcast", "transcription", "Whisper", "audio editing"],
    body: [
      {
        type: "p",
        text: "Podcast post-production is one of the most repetitive audio editing tasks that exists. Every episode involves the same operations: noise removal, silence trimming, level normalization, music bed mixing, chapter markers, and export. AI audio editors can automate each of these without requiring you to learn a DAW.",
      },
      {
        type: "h2",
        text: "The Traditional Podcast Workflow (and Why It Takes So Long)",
      },
      {
        type: "p",
        text: "A typical solo-host podcast episode runs 30–60 minutes. Editing a raw recording to a publishable episode in a traditional DAW involves:",
      },
      {
        type: "ul",
        items: [
          "Manual review of the waveform to find and cut long pauses.",
          "Noise gate or spectral repair to remove room noise and HVAC hum.",
          "Loudness normalization to a delivery target (-14 LUFS for Spotify and YouTube, -16 LUFS for Apple Podcasts, -23 LUFS for broadcast).",
          "Music intro/outro mixing with level automation.",
          "Export to MP3 at appropriate bitrate with ID3 tags.",
          "Show notes generation from timestamps.",
        ],
      },
      {
        type: "p",
        text: "Each of these steps requires different tools, different knowledge, and careful listening. Together they make editing an episode a long job, and most of it is the same moves every time.",
      },
      {
        type: "h2",
        text: "The AI-Augmented Workflow",
      },
      {
        type: "p",
        text: "An AI audio editor with natural language control can replace most of this with a single session. Here is a realistic workflow using edytlab:",
      },
      {
        type: "h3",
        text: "Step 1: Load the Raw Recording",
      },
      {
        type: "p",
        text: 'Drag your WAV file into the session or type "load episode-045-raw.wav". The agent adds it as the first track. If you have a separate music bed file, load that too.',
      },
      {
        type: "h3",
        text: "Step 2: Transcribe and Review (Not Available Yet)",
      },
      {
        type: "p",
        text: 'This step is not available yet: the on-device Whisper decoder has not shipped, so today the transcribe tool returns an error, and ducking the music under the speech in step 4, which is keyed on the transcript, waits on it too. Once it ships, the step is one sentence, "transcribe track 1": the agent runs Whisper locally, with no upload and no API key for transcription, and returns a word-level transcript with timestamps, so you can see where filler words, long silences and retakes are without scrubbing the waveform. Until then, find them by listening.',
      },
      {
        type: "callout",
        text: "In edytlab, Whisper is designed to run entirely on-device, with the word-level transcript stored in the session. The decoder has not shipped yet, so there is no transcription speed to quote.",
      },
      {
        type: "h3",
        text: "Step 3: Describe the Edits",
      },
      {
        type: "p",
        text: 'Describe what you want, in plain English: "Cut all silences longer than 1.5 seconds. Remove the section between 12:30 and 13:45 — that was an off-topic tangent. Normalize to -16 LUFS." The agent executes each operation as a tool call against the session DAG.',
      },
      {
        type: "h3",
        text: "Step 4: Mix Music Beds",
      },
      {
        type: "p",
        text: 'Load your intro/outro music: "Add intro.wav to track 2, crossfade into the speech at 0:08, and duck the music under the speech to -18 dB". The agent handles the volume automation and crossfade geometry. You can preview immediately.',
      },
      {
        type: "h3",
        text: "Step 5: Export",
      },
      {
        type: "p",
        text: 'Type "export as MP3 192kbps with title Episode 45, author My Podcast". Done. The session state is saved as a DAG, so you can branch it, revert any edit, or export different versions (clean edit vs. explicit version) without re-doing work.',
      },
      {
        type: "h2",
        text: "What AI Cannot Replace (Yet)",
      },
      {
        type: "p",
        text: "Automated workflows do not replace critical listening. AI can normalize to a target LUFS, but it does not know if your interview guest had an unusually nasally recording environment that day. Ums and filler words will be removable automatically once transcription ships, but rhythm editing — making the conversation flow more naturally — still benefits from a human ear. Use AI to handle the mechanical steps and spend your time on the creative ones.",
      },
      {
        type: "h2",
        text: "Multi-Guest Podcast Editing",
      },
      {
        type: "p",
        text: "For interviews with multiple speakers, load each recording as a separate track. When you only have a mixed recording, split_by_speaker splits it into one track per speaker from speaker segments you give it, so each voice can be normalized and treated on its own, then re-mixed. This is not a perfect substitute for separate track recording, but it is production-viable for remote interviews recorded on a single channel.",
      },
    ],
  },
  {
    slug: "conversational-daw-prompt-to-mix",
    title: "From Prompt to Mix: How Conversational Audio Editing Works",
    date: "2026-05-15",
    description:
      "What happens between a sentence you type and a changed waveform: tools, session context, the session graph, and how to write prompts that work.",
    coverAlt:
      "A chat bubble with a text cursor, an arrow from it fanning out to three track lanes with waveforms",
    excerpt:
      "You type what you want. The AI figures out which audio operations to run, in which order, and executes them against your session. Here is exactly how that translation happens.",
    readTime: 6,
    tags: ["conversational DAW", "AI mixing", "LLM audio", "audio agent"],
    body: [
      {
        type: "p",
        text: 'When you type "boost the vocals 3 dB and add a subtle reverb" into an AI audio editor, a lot happens between that sentence and the changed waveform. Understanding the architecture makes you a better user — you learn what kinds of prompts work well, what the agent cannot do, and how to recover when it misinterprets your intent.',
      },
      {
        type: "h2",
        text: "The Tool-Use Model",
      },
      {
        type: "p",
        text: "Modern AI audio editors work by giving a large language model a set of tools — functions it can call to manipulate the audio session. These tools correspond to discrete audio operations: load a file, cut a region, adjust gain, apply a plugin, normalize loudness, render to disk.",
      },
      {
        type: "p",
        text: "When you send a message, the LLM reads your instruction, the current session state (what tracks exist, what the timeline looks like, what operations have already been applied), and decides which tool calls to make and with which arguments.",
      },
      {
        type: "callout",
        text: 'edytlab exposes tools like load, cut_range, gain, eq, normalize_loudness, time_stretch and render_final. The LLM plan for "remove the silence at the beginning and boost the bass" might be: cut_range on track 0 over the first 1.2 s → eq on track 0 with a +4 dB peak around 100 Hz.',
      },
      {
        type: "h2",
        text: "Session State as Context",
      },
      {
        type: "p",
        text: "The LLM does not just receive your text — it receives a structured representation of the current session: which tracks exist, their durations, current gain levels, any applied effects, the playback cursor position, and the undo history. This context window allows the model to make edits that reference previous operations ('undo the last normalization and try -14 LUFS instead').",
      },
      {
        type: "h2",
        text: "The Role of the DAG (Directed Acyclic Graph)",
      },
      {
        type: "p",
        text: "Each operation the agent performs creates a new node in a session graph. Nodes point to their parent state. This means every edit is non-destructive: the original audio data is never modified. Asking the agent to 'revert to before the reverb' just moves the session pointer back up the graph.",
      },
      {
        type: "ul",
        items: [
          "Branch: create a fork of the session to try a different arrangement without losing the current one.",
          "Compare: A/B between two branch nodes to decide which mix sounds better.",
          "Revert: jump to any earlier state by navigating the graph.",
        ],
      },
      {
        type: "h2",
        text: "Multi-Step Planning",
      },
      {
        type: "p",
        text: 'Complex requests like "make this sound like a 1970s soul record" require the LLM to plan a sequence of operations: warming the high frequencies (low-pass above 12 kHz), adding vinyl noise (a noise generator at -40 dB), compressing with slow attack (warm transient feel), and reducing the stereo width. A capable model will decompose this into the correct tool chain and execute each step in order.',
      },
      {
        type: "h2",
        text: "When Prompts Are Ambiguous",
      },
      {
        type: "p",
        text: '"Boost the bass" is ambiguous: which track? How much? What frequency? The agent either asks or picks a reasonable default and tells you what it did, and it can see which tracks exist, so naming the track in the prompt saves a round trip. If the result is not what you wanted, you can correct it in natural language: "not that track — the second one, and just +2 dB".',
      },
      {
        type: "h2",
        text: "Choosing Your LLM for Audio Agent Tasks",
      },
      {
        type: "p",
        text: "Not all LLMs perform equally well at multi-step audio planning. Models with strong function-calling support tend to decompose complex audio instructions into correct tool chains. Smaller models may execute the first tool correctly but lose track of the plan on longer chains. edytlab lets you swap providers without reinstalling, so you can test which model works best for your workflow. The post [Which LLM should drive your audio editor?](/blog/which-llm-should-drive-your-audio-editor) goes through the six providers.",
      },
      {
        type: "h2",
        text: "The Feedback Loop",
      },
      {
        type: "p",
        text: "The most effective conversational editing workflow is iterative. Make a rough cut with a broad prompt, listen back, then refine with specific corrections. The session graph captures every iteration, so you are never locked into a direction. Treat the AI agent like a skilled but literal engineer: it executes exactly what you describe, so precision in language produces precision in the edit.",
      },
    ],
  },
  {
    slug: "open-source-audio-editor-byo-llm",
    title: "Why the Best AI Audio Tools Let You Bring Your Own LLM Key",
    date: "2026-05-17",
    description:
      "AI audio tools that hardcode one model decide for you. What bring-your-own-key means in practice: where the key lives, what you can switch, and what it costs.",
    coverAlt:
      "A key plugging into a socket on a device, with the socket wired to a single line out",
    excerpt:
      "Vendor lock-in is the oldest trick in enterprise software. AI audio tools that hardcode a single provider are not tools — they are subscriptions. Here is what BYO-key architecture means in practice.",
    readTime: 5,
    tags: ["open source audio editor", "bring your own API key", "Anthropic", "OpenAI", "OpenRouter"],
    body: [
      {
        type: "p",
        text: "Every AI tool that buries its LLM provider in the backend is making a bet on your behalf: that the model they chose today will remain the best choice for your workflow forever. Software history gives little reason to take it.",
      },
      {
        type: "h2",
        text: "The Vendor Lock-In Pattern",
      },
      {
        type: "p",
        text: 'An AI audio tool that uses GPT-4 under the hood today may switch to a cheaper model to protect margins next quarter. You will not be told. The quality changes, the tool changes, and you have no lever to pull because the API key is theirs, not yours.',
      },
      {
        type: "ul",
        items: [
          "You cannot compare models for your specific workflow.",
          "You cannot use a model from a provider with better data privacy terms.",
          "You cannot route through OpenRouter to access smaller, faster, cheaper alternatives.",
          "You pay a markup on top of whatever the provider charges.",
        ],
      },
      {
        type: "h2",
        text: "What BYO-Key Architecture Looks Like",
      },
      {
        type: "p",
        text: "In a bring-your-own-key setup, the tool stores your API key in your OS keychain — not on any server. When you initiate a chat with the audio agent, the tool signs the request with your key and sends it directly to the provider endpoint. The tool developer never sees your key, never sees your conversation, never routes through a proxy that could log your prompts.",
      },
      {
        type: "callout",
        text: "edytlab stores API keys in your native OS keychain (macOS Keychain, Windows Credential Manager, the Secret Service on Linux). The desktop app reads the key at runtime, signs the LLM request locally, and sends it directly to the provider. No intermediary server, no usage logging by edytlab.",
      },
      {
        type: "h2",
        text: "Multi-Provider Support in Practice",
      },
      {
        type: "p",
        text: "Different providers have different strengths for audio agent tasks. Anthropic's Claude models are built for multi-step tool use, which suits complex arrangements. OpenAI's models are a drop-in if you already have access. OpenRouter puts many models, including open-weight ones, behind one key. Groq serves open models, Google Gemini offers long context, and Ollama runs models on your own machine with no key at all. The follow-up post [Which LLM should drive your audio editor?](/blog/which-llm-should-drive-your-audio-editor) compares all six.",
      },
      {
        type: "h3",
        text: "When to Switch Models",
      },
      {
        type: "ul",
        items: [
          "Complex multi-track arrangements with many interdependencies: use Claude or GPT-4o.",
          "Simple edits (normalize, cut, export): use a fast, cheap model like Haiku or GPT-4o mini.",
          "Budget-sensitive production: route through OpenRouter to reach cheaper open-weight models.",
          "Privacy-critical sessions: choose a provider with zero data retention commitments.",
        ],
      },
      {
        type: "h2",
        text: "The Open-Source Angle",
      },
      {
        type: "p",
        text: "edytlab is open source. The audio graph, the tool implementations, the Tauri bridge code — all public on GitHub. This matters for AI audio tools specifically because you can audit exactly how your audio is processed, confirm that stems are not uploaded, and even modify the tool definitions to add your own custom operations. Closed-source AI audio tools make trust claims you cannot verify.",
      },
      {
        type: "h2",
        text: "The Economics",
      },
      {
        type: "p",
        text: "What you spend follows the conversation: the messages you send, the session description the model reads each turn, and the tool results it gets back. Your audio is not part of it. Provider prices change, so look at your provider's pricing page rather than at a number in a blog post. When you own the key, you see the cost directly on your provider dashboard and can choose a cheaper model for simple edits. When the tool owns the key, that cost is buried in your subscription.",
      },
      {
        type: "p",
        text: "The future of AI tooling belongs to applications that treat the LLM as a commodity component — interchangeable, price-competitive, and user-selected — not as a proprietary moat. BYO-key is not a feature. It is a design philosophy.",
      },
    ],
  },
];

/** Every post, oldest first. Where two share a date, the order here is the order shown. */
export const posts: BlogPost[] = [...earlierPosts, ...octoberPosts];

export function getPost(slug: string): BlogPost | undefined {
  return posts.find((p) => p.slug === slug);
}

export function getAllSlugs(): string[] {
  return posts.map((p) => p.slug);
}

/** Newest first. A stable sort, so posts on one date keep their order above. */
export function postsNewestFirst(list: readonly BlogPost[] = posts): BlogPost[] {
  return [...list].sort((a, b) => b.date.localeCompare(a.date));
}

/** The last time a post changed: `dateModified`, else the day it went up. */
export function modifiedOf(post: BlogPost): string {
  return post.dateModified ?? post.date;
}

/** The words a reader reads, markup and prompts included. */
export function wordCount(post: BlogPost): number {
  const text = post.body
    .map((b) => {
      switch (b.type) {
        case "ul":
        case "ol":
          return b.items.map(stripInline).join(" ");
        case "prompt":
          return b.text;
        default:
          return stripInline(b.text);
      }
    })
    .join(" ");
  return text.split(/\s+/).filter(Boolean).length;
}
