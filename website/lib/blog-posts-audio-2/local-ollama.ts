import type { BlogPost } from "../blog";
import { siteConfig } from "../site";
import { SRC, source } from "./shared";

const RELEASES = siteConfig.releases;

export const localOllama: BlogPost = {
  slug: "edit-audio-with-a-local-model-ollama",
  title: "Edit Audio With a Local Model Using Ollama",
  date: "2026-10-24",
  description:
    "Run edytlab's assistant on Ollama with no account or API key. What stays on your computer, what a local model is sent, and the context and tool-calling limits.",
  coverAlt:
    "A laptop outline containing a small chip, a waveform and a speech bubble, with a dashed line around all three and nothing crossing it",
  excerpt:
    "With Ollama there is no key and no account, and the chat stays home as well as the audio. A local model usually runs with a small context, and edytlab changes what it sends to suit. This is the setup, what is sent, what it costs in capability, and where the limits are.",
  readTime: 8,
  tags: ["Ollama", "local AI", "privacy", "tool calling"],
  body: [
    {
      type: "p",
      text: "You can edit audio with a local model by running Ollama on your computer and choosing it in edytlab's Settings: there is no account and no API key, your audio never leaves the machine (as with any provider), and the chat stays on it too. What you give up is the strength of a large hosted model. Models differ in how reliably they call tools, which is why edytlab has a Test button, and a local model's context window is small, so edytlab sends Ollama a reduced tool list (`ToolSet::Slim`) instead of all of them. This post covers the setup, what is sent and what is kept, and the limits, using edytlab's source and Ollama's documentation.",
    },
    {
      type: "callout",
      text:
        "You need [edytlab](" +
        RELEASES +
        "), Ollama, and a model you have pulled. edytlab's installers are unsigned, so macOS and Windows warn on first launch ([Getting Started](/docs/getting-started)). The demo videos here, such as the [beatmatch walkthrough](/videos/dj-beatmatched-transition), were recorded with Claude, not with Ollama.",
    },
    { type: "h2", text: "What stays on your computer" },
    {
      type: "p",
      text: "edytlab decodes, edits, mixes and renders audio itself, in-process, with any provider; the model only chooses which tools to call and in what order ([the local-first page](/use-cases/local-ai-audio-editor) has the table). What leaves the machine is the chat: your messages, the session summary and the results of the tools. With a hosted provider that goes to the provider. With Ollama it goes to the address Ollama listens on. Ollama's own FAQ answers the question \"Does Ollama send my prompts and answers back to ollama.com?\": \"Ollama runs locally. We don't see your prompts or data when you run locally.\"",
    },
    {
      type: "p",
      text: "Three conditions apply. First, the address: edytlab's default is `http://localhost:11434/v1`, and its source notes that it can be changed because running Ollama on another machine on the network is a normal setup. If you change it, the chat goes to that machine. Second, Ollama's FAQ says that when you use its cloud-hosted models it processes your prompts and responses to provide the service, and its cloud documentation says cloud models do not need to be downloaded. For a fully local setup, pull a model and use it, and note that Ollama can run in local-only mode by setting `OLLAMA_NO_CLOUD=1`, which turns off its cloud models and web search. Third, a tool result can carry text taken from your audio. After you ask for a transcript, the words go to the model as text, and with Ollama on your own machine they stay there ([privacy page](/privacy)).",
    },
    { type: "h2", text: "Set it up" },
    {
      type: "ol",
      items: [
        "Install Ollama and pull a model that supports tool calling. Ollama lists such models under a Tools category, and its documentation says it supports tool calling, which lets a model invoke tools and use their results in its replies.",
        "Open Settings in edytlab and choose Ollama. No key is asked for. Ollama's OpenAI-compatible API wants a key value in the client and ignores it, and edytlab sends no Authorization header at all.",
        "The model list is read from Ollama at the address you gave. If Ollama is not running, the message says to start it with `ollama serve` and pull a model, for example `ollama pull llama3.2`, which is also the default edytlab guesses.",
        "Press Test. It offers the model a throwaway tool and checks that it actually calls it. The source's comment explains why: a model can connect and list fine and still fail the first real request, because tool support belongs to the model and not to the server. The check is one trivial call, so a pass does not show that the model can carry out a long job.",
      ],
    },
    {
      type: "p",
      text: "[The getting started guide](/docs/getting-started) walks through the same steps, and you can switch between Ollama and a hosted provider at any time.",
    },
    { type: "h2", text: "What edytlab sends a local model" },
    {
      type: "p",
      text: "Every provider but Ollama is sent all the tools the turn permits. Anthropic has to be, because its prompt cache depends on the whole list. edytlab's source says local models usually run with a 4k to 8k context, so Ollama gets `ToolSet::Slim`, built in `crates/ai/src/tool_selection.rs`:",
    },
    {
      type: "ul",
      items: [
        "A core set of the edits asked for most: `analyze_track`, `compressor`, `cut_range`, `eq`, `fade`, `gain`, `high_pass_filter`, `mute_track`, `noise_reduction`, `normalize`, `normalize_loudness`, `render_final`, `revert_to`, `set_pan` and `set_track_gain`.",
        "Any tool your message names, by its name or by a word of it (\"reverb\", \"time stretch\"), and the tools an approved plan names, then tools the assistant just proposed or that you named earlier, all within a byte budget.",
        "Descriptions cut to the first sentence and the parts of each schema the model can do without removed.",
        "The names of the tools left out, in one line of the system prompt, so the model knows they exist. If it calls one, the call still runs, checked against that tool's full schema.",
      ],
    },
    {
      type: "p",
      text: "The set is worked out once per turn, so every step of a turn sends the same tools, which lets a local server reuse the prompt it has already processed. The session summary in the prompt is capped at 2,300 bytes for Ollama, and a project too big for that has its track, bus and marker lists cut, with a line that names the read-only `list_session` tool to page through the rest.",
    },
    {
      type: "p",
      text: "In practice: ask for what you want in plain words. \"Reverse it\", \"add a limiter\" and \"time-stretch track 2 to 124 BPM\" each name the tool, and it comes along. A vague request can leave the model working from the core set and a line of names.",
    },
    { type: "h2", text: "What a run showed about the size" },
    {
      type: "p",
      text: "edytlab's own `request_size` test drives the real agent against a mock server with a synthetic one-track session and records the first request. Run at the commit linked, with the message \"Make track 0 louder by 6 dB.\", it printed this for Ollama: a system prompt of 2,382 bytes, tools of 6,252 bytes, 8,787 bytes in all. For a provider sent the whole list over the OpenAI-compatible wire the same test printed 66,897 bytes of tools and 68,258 in all. A message that names a dozen tools made the Ollama request 11,068 bytes.",
    },
    {
      type: "p",
      text: "The test's bound is half of an 8,192-token context, at an assumed three bytes per token. That ratio is an assumption, not a measurement: no tokenizer is run, so these are byte counts and not token counts. The code's comments say so and so does this post.",
    },
    { type: "h2", text: "The real limits" },
    {
      type: "ul",
      items: [
        "**Context length.** Ollama's documentation says it defaults to a 4k context on a machine with under 24 GiB of VRAM, 32k with 24 to 48 GiB and 256k at 48 GiB or more, and that agents and coding tools should be set to at least 64,000 tokens. By the test's assumed ratio, below, the first request alone is about 2,900 tokens, which leaves little of a 4,096-token context for the conversation and the reply, and a larger setting costs memory. Raise it with the Ollama app's slider or `OLLAMA_CONTEXT_LENGTH=64000 ollama serve`. edytlab's local-model page says that a long conversation can outgrow a small context, that a server such as llama.cpp may refuse an over-long request (the app then says how to raise the limit), and that Ollama trims instead, so replies that start to forget earlier messages are the sign.",
        "**Tool calling varies by model.** The Berkeley Function Calling Leaderboard exists because how accurately models call functions is measured, model by model, including over several turns. No ranking of local models is given here. An edit such as a transition is many calls in the right order, and a smaller model may do the first steps well and lose the thread. Split a big job into short requests.",
        "**Speed.** edytlab's page says that on a CPU a large request means a long wait for the first reply.",
        "**Plan first.** edytlab's page calls Plan first mode, which shows the assistant's steps for your approval before anything runs, a useful guard with smaller models.",
      ],
    },
    { type: "h2", text: "Try it" },
    { type: "prompt", text: "Make track 0 louder by 6 dB." },
    { type: "prompt", text: "Reduce the noise on track 1, using the first second as the noise profile." },
    { type: "prompt", text: "Time-stretch track 2 to 124 BPM, keeping its pitch." },
    {
      type: "p",
      text: "The first is the message edytlab's size test uses. The second is in the core set. The third names `time_stretch` by its words, which is what brings it into the request. See [how noise reduction works](/blog/remove-background-noise-from-audio) for the second, and [which model should drive your editor](/blog/which-llm-should-drive-your-audio-editor) for how the hosted providers compare. The [tools reference](/docs/tools) lists every tool the assistant can use.",
    },
    { type: "h2", text: "Questions about editing audio with Ollama" },
    { type: "h3", text: "Do I need a GPU to run Ollama with edytlab?" },
    {
      type: "p",
      text: "edytlab does not need one, because its DSP runs on the CPU in Rust, and the transcription model runs on the CPU too. The chat model is Ollama's job, and Ollama sets its default context by VRAM, as above. On a CPU, edytlab's page warns of a long wait for a first reply.",
    },
    { type: "h3", text: "Is it really offline?" },
    {
      type: "p",
      text: "The editing and the chat can be, once Ollama and a model are installed. edytlab sends no telemetry. The app connects only where you tell it to: the model list from the Ollama address, and, if you click Download in Settings, the speech model for transcription from Hugging Face. [The privacy page](/privacy) lists them.",
    },
    { type: "h3", text: "Which model should I use?" },
    {
      type: "p",
      text: "No recommendation is made here, because none was measured. Pick one Ollama lists under Tools, that fits in your memory with a larger context, and press Test. Ollama's documentation says a larger context needs more memory. Try the same request on two models and keep the one that carries it out.",
    },
    { type: "h2", text: "Sources" },
    {
      type: "p",
      text: "Outside pages were read on 2026-10-24. The sizes come from running edytlab's own request_size test at the commit named, with a mock server and a synthetic session; no Ollama model was run for this post, so nothing here says how any model performs. edytlab's behaviour is stated from its source.",
    },
    {
      type: "ul",
      items: [
        source("Ollama documentation: FAQ (prompts and data, cloud models, OLLAMA_NO_CLOUD)", "https://docs.ollama.com/faq"),
        source("Ollama documentation: Context length", "https://docs.ollama.com/context-length"),
        source("Ollama documentation: OpenAI compatibility", "https://docs.ollama.com/api/openai-compatibility"),
        source("Ollama documentation: Tool calling", "https://docs.ollama.com/capabilities/tool-calling"),
        source("Ollama documentation: Cloud", "https://docs.ollama.com/cloud"),
        source("Ollama: models with tool support", "https://ollama.com/search?c=tools"),
        source("Berkeley Function Calling Leaderboard (BFCL) V4", "https://gorilla.cs.berkeley.edu/leaderboard.html"),
        source("edytlab source: tool_selection.rs (the slim tool set)", SRC("crates/ai/src/tool_selection.rs"), "at commit 8bf752ff"),
        source("edytlab source: provider.rs (the Ollama provider)", SRC("crates/ai/src/provider.rs"), "at commit 8bf752ff"),
        source("edytlab source: validate.rs (the Test probe)", SRC("crates/ai/src/validate.rs"), "at commit 8bf752ff"),
        source("edytlab source: models.rs (the model list from Ollama)", SRC("crates/ai/src/models.rs"), "at commit 8bf752ff"),
        source("edytlab test: request_size.rs (the sizes quoted)", SRC("crates/ai/tests/request_size.rs"), "at commit 8bf752ff"),
        source("edytlab demo and video captions", SRC("website/lib/videos.ts"), "at commit 8bf752ff"),
      ],
    },
  ],
};
