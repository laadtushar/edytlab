export type Block =
  | { type: "h2"; text: string }
  | { type: "h3"; text: string }
  | { type: "p"; text: string }
  | { type: "ul"; items: string[] }
  | { type: "callout"; text: string };

export interface BlogPost {
  slug: string;
  title: string;
  date: string;
  excerpt: string;
  readTime: number;
  tags: string[];
  body: Block[];
}

export const posts: BlogPost[] = [
  {
    slug: "ai-audio-editing-local-first",
    title: "AI Audio Editing in 2026: Why Local-First Is the Only Approach That Matters",
    date: "2026-05-10",
    excerpt:
      "Cloud AI audio tools upload your stems to third-party servers, lock you into subscriptions, and go offline when the API is down. Local-first AI audio editing changes all of that.",
    readTime: 7,
    tags: ["local-first", "AI audio editor", "privacy", "offline"],
    body: [
      {
        type: "p",
        text: "In the last three years, AI audio editing has exploded. Tools that once required a full-time engineer — stem separation, automatic transcription, pitch correction, noise removal — now live inside consumer apps. The catch? Almost every one of them routes your audio through a cloud server.",
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
          "Processing latency scales with file size; a 200 MB session can take minutes to round-trip.",
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
        text: "Local-first means the DSP engine — the code that actually processes audio samples — runs entirely on your machine. Your stems never leave. The AI model weights live locally. The waveform analysis happens on your CPU or GPU. The only bytes that leave your machine are the text tokens you send to your chosen LLM provider to describe the edit you want.",
      },
      {
        type: "callout",
        text: "edytlab uses a pure-Rust audio graph (cpal · symphonia · rubato · realfft). Every cut, gain adjustment, pitch shift, and stem separation call runs on-device. Only the chat conversation hits the network — and you choose which LLM provider that goes to.",
      },
      {
        type: "h2",
        text: "The Practical Difference: A Studio Workflow Example",
      },
      {
        type: "p",
        text: 'Imagine you are mastering a 10-track album. In a cloud workflow, you upload each stem set, wait for processing, download results, repeat. With a local-first editor, you open the session, type "separate the drums on track 3, boost the low end by 4 dB, and export a 24-bit WAV", and the agent executes that chain locally in seconds.',
      },
      {
        type: "h3",
        text: "Latency Numbers That Matter",
      },
      {
        type: "p",
        text: "A 96 kHz stereo file demucs stem separation on a modern M-series Mac or Ryzen 7 desktop takes roughly 8–15 seconds per minute of audio — fully offline. The equivalent cloud round-trip (upload + queue + process + download) is typically 45–120 seconds for the same file, and that assumes the API is healthy.",
      },
      {
        type: "h2",
        text: "Bring Your Own LLM Key",
      },
      {
        type: "p",
        text: "Local-first audio processing does not mean you cannot use AI language models. edytlab connects to Anthropic, OpenAI, or OpenRouter using API keys you store in your own OS keychain. The conversation that translates your plain-English instructions into tool calls runs through your chosen provider — you own the API contract, you see the usage, you can switch models without reinstalling anything.",
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
        text: "As on-device AI inference improves — Apple Silicon Neural Engine, AMD XDNA, NVIDIA DLSS-equivalent for audio — the gap between local and cloud audio AI will narrow to zero. Tools built local-first today will not need to be rearchitected. Tools built cloud-first will.",
      },
    ],
  },
  {
    slug: "stem-separation-explained-demucs",
    title: "Stem Separation Explained: How AI Isolates Vocals and Instruments",
    date: "2026-05-12",
    excerpt:
      "Stem separation used to require a mixing console and a human engineer. Now Demucs can split a stereo mix into vocals, drums, bass, and other in seconds — and it runs entirely on your laptop.",
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
          "Extract the drum stem to create an acapella version for a DJ edit.",
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
        text: "Cloud-based stem separation tools (Lalal.ai, LALAL.AI, Moises) all upload your audio. For unreleased material — demos, client work, sync licensing tracks — this is a non-starter. edytlab integrates Demucs as a local tool call: the model runs on your machine, the stems are written to your local session, and nothing is uploaded.",
      },
      {
        type: "callout",
        text: 'In edytlab, just type: "separate the vocals from track 1". The agent calls the stem separation tool, Demucs runs on-device, and the separated stems appear as new tracks in your session timeline.',
      },
      {
        type: "h2",
        text: "Model Selection and Quality Trade-offs",
      },
      {
        type: "p",
        text: "Demucs offers several model variants. htdemucs is the recommended default — it offers the best quality-to-speed ratio on modern hardware. mdx_extra gives slightly better vocal quality at the cost of more VRAM. htdemucs_6s adds guitar and piano as separate stems, which is useful for complex arrangements but takes roughly 2× the inference time.",
      },
      {
        type: "p",
        text: "On an Apple M3 MacBook Pro, a 4-minute track separates into 4 stems in approximately 45 seconds with htdemucs. On a Windows machine with a mid-range NVIDIA GPU (RTX 3060), the same track takes 18–25 seconds with CUDA acceleration.",
      },
    ],
  },
  {
    slug: "podcast-production-ai-workflow",
    title: "AI Podcast Production: Record, Edit, Transcribe, and Export in One Session",
    date: "2026-05-13",
    excerpt:
      "The typical podcast post-production workflow takes 4–6 hours per episode. Here is how to use an AI audio editor to collapse that to under 30 minutes without sacrificing quality.",
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
          "Loudness normalization to LUFS broadcast targets (-16 LUFS for Spotify, -19 LUFS for Apple Podcasts).",
          "Music intro/outro mixing with level automation.",
          "Export to MP3 at appropriate bitrate with ID3 tags.",
          "Show notes generation from timestamps.",
        ],
      },
      {
        type: "p",
        text: "Each of these steps requires different tools, different knowledge, and careful listening. It adds up to 4–6 hours of editing for a 1-hour episode for most solo producers.",
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
        text: "Step 2: Transcribe and Review",
      },
      {
        type: "p",
        text: 'Type "transcribe track 1". The agent calls Whisper locally — no upload, no API key for transcription needed — and returns a word-level transcript with timestamps. You can now see exactly where filler words, long silences, and retakes are without scrubbing the waveform.',
      },
      {
        type: "callout",
        text: "Whisper large-v3 runs entirely on-device in edytlab. A 60-minute audio file transcribes in approximately 4–8 minutes on a modern laptop, depending on hardware. The transcript is word-level timestamped and stored in the session.",
      },
      {
        type: "h3",
        text: "Step 3: Describe the Edits",
      },
      {
        type: "p",
        text: 'With the transcript in hand, describe what you want: "Cut all silences longer than 1.5 seconds. Remove the section between 12:30 and 13:45 — that was an off-topic tangent. Normalize to -16 LUFS." The agent executes each operation as a tool call against the session DAG.',
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
        text: "Automated workflows do not replace critical listening. AI can normalize to a target LUFS, but it does not know if your interview guest had an unusually nasally recording environment that day. Ums and filler words can be removed automatically, but rhythm editing — making the conversation flow more naturally — still benefits from a human ear. Use AI to handle the mechanical 80% and spend your time on the creative 20%.",
      },
      {
        type: "h2",
        text: "Multi-Guest Podcast Editing",
      },
      {
        type: "p",
        text: "For interviews with multiple speakers, load each recording as a separate track. edytlab's stem separation can help when you only have a mixed recording — separate the louder and quieter voices, normalize each independently, then re-mix. This is not a perfect substitute for separate track recording, but it is production-viable for remote interviews recorded on a single channel.",
      },
    ],
  },
  {
    slug: "conversational-daw-prompt-to-mix",
    title: "From Prompt to Mix: How Conversational Audio Editing Works",
    date: "2026-05-15",
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
        text: "Modern AI audio editors work by giving a large language model a set of tools — functions it can call to manipulate the audio session. These tools correspond to discrete audio operations: load a file, cut a region, adjust gain, apply a plugin, normalize loudness, run stem separation, render to disk.",
      },
      {
        type: "p",
        text: "When you send a message, the LLM reads your instruction, the current session state (what tracks exist, what the timeline looks like, what operations have already been applied), and decides which tool calls to make and with which arguments.",
      },
      {
        type: "callout",
        text: 'edytlab exposes tools like load_audio, cut_region, set_gain, normalize, stem_separate, transcribe, render_range. The LLM plan for "remove the silence at the beginning and boost the bass" might be: cut_region(track=1, start=0, end=1.2) → set_gain(track=1, region=bass_frequency_band, db=+4).',
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
          "Merge: take the best elements of two branches into a new node.",
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
        text: '"Boost the bass" is ambiguous: which track? How much? What frequency? The agent will make a reasonable default (the first track with audio, +3 dB, shelf below 200 Hz) and tell you what it did. If that is not what you wanted, you can correct it in natural language: "not that track — the second one, and just +2 dB".',
      },
      {
        type: "h2",
        text: "Choosing Your LLM for Audio Agent Tasks",
      },
      {
        type: "p",
        text: "Not all LLMs perform equally well at multi-step audio planning. Models with strong function-calling support (Claude 3.7 Sonnet, GPT-4o, Mistral Large via OpenRouter) reliably decompose complex audio instructions into correct tool chains. Smaller models may execute the first tool correctly but lose track of the plan on longer chains. edytlab lets you swap providers without reinstalling — you can test which model works best for your workflow.",
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
    excerpt:
      "Vendor lock-in is the oldest trick in enterprise software. AI audio tools that hardcode a single provider are not tools — they are subscriptions. Here is what BYO-key architecture means in practice.",
    readTime: 5,
    tags: ["open source audio editor", "bring your own API key", "Anthropic", "OpenAI", "OpenRouter"],
    body: [
      {
        type: "p",
        text: "Every AI tool that buries its LLM provider in the backend is making a bet on your behalf: that the model they chose today will remain the best choice for your workflow forever. That bet has a 0% historical success rate in software.",
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
        text: "edytlab stores API keys in your native OS keychain (macOS Keychain, Windows Credential Manager). The desktop app reads the key at runtime, signs the LLM request locally, and sends it directly to the provider. No intermediary server, no usage logging by edytlab.",
      },
      {
        type: "h2",
        text: "Multi-Provider Support in Practice",
      },
      {
        type: "p",
        text: "Different providers have different strengths for audio agent tasks. Anthropic Claude models have strong long-context reasoning and reliable multi-step tool use — good for complex arrangements. OpenAI GPT-4o has excellent speed and broad tool-call support — good for quick edits. OpenRouter gives you access to 50+ models including open-weight options like Llama 3.3, Mistral Large, and Qwen 2.5.",
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
          "Budget-sensitive production: route through OpenRouter to access open-weight models at 10× lower cost.",
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
        text: "A typical 30-minute podcast edit might consume 50,000–200,000 tokens of LLM context (session state + conversation history). At Claude Sonnet pricing (~$3/M input tokens), that is $0.15–0.60 per session. At Haiku pricing (~$0.25/M input), it is $0.01–0.05. When you own the key, you see these costs directly on your provider dashboard and can optimize accordingly. When the tool owns the key, those costs are buried in your subscription.",
      },
      {
        type: "p",
        text: "The future of AI tooling belongs to applications that treat the LLM as a commodity component — interchangeable, price-competitive, and user-selected — not as a proprietary moat. BYO-key is not a feature. It is a design philosophy.",
      },
    ],
  },
];

export function getPost(slug: string): BlogPost | undefined {
  return posts.find((p) => p.slug === slug);
}

export function getAllSlugs(): string[] {
  return posts.map((p) => p.slug);
}
