# edytlab press kit

The same content is published at <https://edytlab.com/press>. If you edit one,
edit the other.

**Website:** <https://edytlab.com>
**Download (newest versioned release):** <https://github.com/laadtushar/edytlab/releases/latest>
**Source:** <https://github.com/laadtushar/edytlab>
**Contact:** open an issue at <https://github.com/laadtushar/edytlab/issues>. There is no press email.

## One-sentence description

edytlab is a free, open-source desktop audio editor where you edit by chatting
with an AI agent.

## Boilerplate

edytlab is a free, open-source desktop audio editor where you edit by chatting
with an AI agent. The agent turns a request such as "time-stretch the second
track to match the first, crossfade, and export a WAV" into calls to 94
deterministic audio tools. The tools run locally in a Rust engine on the user's
own computer, so audio files are never uploaded; only the chat goes to the
language-model provider the user selects (Anthropic, OpenAI, OpenRouter, Groq,
Gemini, or Ollama for a fully local setup, or the user's own ChatGPT Plus or Pro
plan through OpenAI's Sign in with ChatGPT, a preview). Every edit is saved as a node in a
branchable history, with undo and A/B comparison. edytlab runs on macOS,
Windows and Linux, is released under the MIT license, and its source is at
github.com/laadtushar/edytlab.

## Fact sheet

Every line below was checked against the repository or the GitHub release on
2026-10-22, for v0.7.0 (the Latest version row was updated on 2026-10-25 for
v0.9.0, for v0.9.1 and for v0.9.2 (the version and download lines), and the telemetry row on 2026-10-24 for the website's cookie banner; the
AI providers, network requests and keys rows describe the ChatGPT sign-in that
shipped in v0.9.0).

| | |
|---|---|
| Name | edytlab |
| What it is | Desktop audio editor driven by an AI agent |
| Latest version | 0.9.2, published 2026-10-26 |
| License | MIT |
| Repository created | 2026-05-05 |
| Platforms | macOS (one universal build for Apple Silicon and Intel), Windows 10 and 11, Linux |
| Installers | `.dmg` (macOS), `.msi` and NSIS `.exe` (Windows), `.deb` and AppImage (Linux) |
| Signing | Not signed or notarized yet. macOS and Windows warn on first launch; the release notes give the steps. |
| Built with | Tauri 2, Rust, React |
| Audio engine | Pure-Rust DSP. Decode with `symphonia`, resample with `rubato`, effects in an in-house dependency-free `audio-dsp` crate. |
| Time-stretch / pitch-shift | A phase vocoder written for the project |
| Tools | 94 deterministic tools the agent can call (generated list: `docs/tools-reference.md`) |
| AI providers | Six API providers plus the user's ChatGPT plan: Anthropic, OpenAI, OpenRouter, Groq, Gemini and Ollama (local, no key), and, from 0.9.0, Continue with ChatGPT, which uses a ChatGPT Plus or Pro plan's included usage instead of a key (OpenAI's Sign in with ChatGPT for open-source apps, a preview). Available for ChatGPT Plus and Pro. May not be available on workspace plans; that depends on OpenAI and your admin. |
| Where audio goes | Nowhere. Processing is local. Only chat text goes to the chosen provider; that includes the words a transcription finds (text, a bounded amount per call, never audio), which stay on the computer with Ollama. |
| Telemetry | None in the desktop app. The website uses anonymous Vercel analytics (page views and page performance, no cookies) and, when enabled, Google Analytics 4 only after a visitor accepts it in the cookie banner (it sets cookies; the footer's Cookie settings link withdraws it) |
| Other network requests | Besides the AI provider and the hosts you configure (plugins, MCP servers), one download you start: from 0.8.0 (0.7.0 has no Download button), the speech model (Whisper base, about 139 MB) is downloaded from Hugging Face (huggingface.co) when the user clicks Download in Settings. Hugging Face sees the IP address, the file requested and that edytlab (and which version) is asking, never audio. Nothing downloads at launch or from the agent. Transcribing makes no request. From 0.9.0, and only if the user clicks Continue with ChatGPT: the browser goes to auth.openai.com to sign in, and the app then exchanges, renews and (on sign-out) revokes the sign-in with that host and sends the assistant's requests (the same text as for any provider, never audio) to api.openai.com with the user's ChatGPT access token. OpenAI can tie them to the user's ChatGPT account, to edytlab's registration and to the installation's random host ID. Signing out does not disconnect edytlab in ChatGPT; the user does that in ChatGPT settings under Security and login. |
| Keys | Stored in the operating system's credential store. All keys, and the ChatGPT sign-in's token, share one entry of that store, so macOS asks for the login password at most once per launch; the provider, model and base URL are in a settings file in the app's config folder, with no secret in it. The ChatGPT plan has no key: its sign-in keeps one refresh token in that same entry (slot `chatgpt_refresh_token`) and a small `registration.json` (host ID, OpenAI's client ID for edytlab, the account's email as a label) in the app's config folder. |
| History | Branchable graph of edits with fork, A/B compare and undo |
| Optional safety | "Plan first" mode shows the agent's steps for approval before they run |
| Export | WAV, FLAC, MP3 |
| Maintainer | `@laadtushar` on GitHub |

### What is not shipped

State this plainly if asked.

- Stem separation (Demucs) is registered as a tool but not implemented in any
  build.
- Speech transcription (Whisper base) works from 0.8.0, on the user's computer,
  only after the user downloads the speech model (about 139 MB) in Settings.
  0.7.0 and earlier cannot transcribe. Its accuracy has been checked on one
  short synthetic English recording and about 16 minutes of English audiobook
  speech (4% to 7% of the words wrong), not on a range of voices or languages.
- It is an offline editor for files. It is not a live DJ tool and it is not a
  DAW (no MIDI, no VST/CLAP hosting).
- There is no web or mobile version.

### Not available from the project

There are no user counts, download statistics, benchmarks, testimonials,
awards or press quotes to cite, and none should be invented.

## Assets (all in the repository)

Paths are relative to the repo root; URLs resolve once these files are on
`main`.

| Asset | Path | URL |
|---|---|---|
| Logo (SVG) | `website/public/logo.svg` | <https://edytlab.com/logo.svg> |
| Logo (PNG) | `website/public/logo.png` | <https://edytlab.com/logo.png> |
| Icon 512x512 | `website/public/icon-512.png` | <https://edytlab.com/icon-512.png> |
| Icon 192x192 | `website/public/icon-192.png` | <https://edytlab.com/icon-192.png> |
| App icon set | `apps/desktop/src-tauri/icons/` | <https://github.com/laadtushar/edytlab/tree/main/apps/desktop/src-tauri/icons> |
| Demo video: beatmatch and blend two tracks (MP4, 2:45, with sound) | `website/public/demos/dj-beatmatched-transition.mp4` | <https://edytlab.com/demos/dj-beatmatched-transition.mp4> |
| Demo video poster (JPG, 1280x776) | `website/public/demos/dj-beatmatched-transition.jpg` | <https://edytlab.com/demos/dj-beatmatched-transition.jpg> |
| Demo video: extend an intro for mixing (MP4, 1:55, with sound) | `website/public/demos/dj-extended-club-intro.mp4` | <https://edytlab.com/demos/dj-extended-club-intro.mp4> |
| Demo video: a three-track mini-mix (MP4, 1:58, with sound) | `website/public/demos/dj-mini-mix.mp4` | <https://edytlab.com/demos/dj-mini-mix.mp4> |

Screenshots of the interface are not in the repository. Take your own from the
release build, or ask via an issue.

There are no brand guidelines. Use the logo as-is on a dark or light
background, and write the name in lowercase: edytlab.

## Approved descriptions of the demo

Quote only these. They are the captions the website shows next to the
recording.

> A DJ asks what they are working with: Neon Rush is about 6.7% faster. Neon
> Rush is time-stretched to Midnight Drive's 120 BPM, keeping its pitch, and
> started at 16 s, under Midnight Drive's last 8 bars; Claude points out that
> its bars then run a beat behind and offers to start it at 15.5 s instead.
> Midnight Drive fades out under a 1 kHz low-pass
> while Neon Rush fades in. For the master, Claude mixes the two to a new track,
> compresses it, limits at -1 dB and raises it to about -14 LUFS, then exports a
> 50-second WAV at -13.9 LUFS, peaking at -1 dBFS. Waits for Claude are sped up;
> every playback is in real time, with sound.

## Short bios by length

**20 words:** edytlab is a free, open-source desktop audio editor you drive by chatting with an AI agent. Audio is processed locally.

**50 words:** edytlab is a free, open-source desktop audio editor for macOS, Windows and Linux. You describe an edit in chat and an AI agent runs deterministic audio tools to do it. The processing is local, so audio is never uploaded; only the chat goes to the AI provider you choose, or none if you use Ollama.
