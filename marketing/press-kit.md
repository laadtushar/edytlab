# edytlab press kit

The same content is published at <https://edytlab.com/press>. If you edit one,
edit the other.

**Website:** <https://edytlab.com>
**Download (newest versioned release):** <https://github.com/laadtushar/edytlab/releases/latest>
**Source:** <https://github.com/laadtushar/edytlab>
**Contact:** open an issue at <https://github.com/laadtushar/edytlab/issues>. There is no press email.

## One-sentence description

edytlab is a free, open-source desktop audio editor that you drive by telling an
AI what you want done.

## Boilerplate

edytlab is a free, open-source desktop audio editor that you drive by telling an
AI what you want done. A request such as "time-stretch the second track to match
the first, crossfade between them, and export a WAV" becomes calls to 99
deterministic audio tools. The tools run in a Rust engine on the user's own
computer, so audio files are never uploaded. Only the chat goes to the AI
provider the user chooses, and with a local model through Ollama it stays on the
computer too. Every edit is saved as a step in a history the user can branch,
compare and undo. edytlab runs on macOS, Windows and Linux, is released under
the MIT license, and its source is at github.com/laadtushar/edytlab.

## Key points

Four sentences that can be lifted as they are.

- You describe an edit in plain English, and edytlab makes it and shows each
  step in the chat.
- The AI never hears the audio. It reads a summary of the session and chooses
  from 99 editing tools, and the tools run on the user's own computer.
- Nothing is lost: every edit can be undone, and the session is a history the
  user can branch and compare.
- It is free and open source under the MIT license, for macOS, Windows and
  Linux, with the user's choice of AI, including a local model.

## Fact sheet

Checked against the repository and the GitHub releases, most recently for
v0.9.3, published 2026-10-27. The rows on AI providers, network requests and
keys include the ChatGPT sign-in that arrived in v0.9.0.

| | |
|---|---|
| Name | edytlab |
| What it is | A desktop audio editor you drive by chatting with an AI |
| Latest version | 0.9.3, published 2026-10-27 |
| License | MIT |
| Repository created | 2026-05-05 |
| Platforms | macOS (one universal build for Apple Silicon and Intel), Windows 10 and 11, and Linux |
| Installers | A `.dmg` for macOS, an `.msi` and an NSIS `.exe` for Windows, and a `.deb` and an AppImage for Linux |
| Signing | The installers are not signed or notarized yet, so macOS and Windows warn on first launch. The release notes give the steps. |
| Built with | Tauri 2, Rust and React |
| Audio engine | Written in Rust. Decoding uses `symphonia`, resampling uses `rubato`, and the effects are in a dependency-free `audio-dsp` crate made for the project. |
| Time-stretch and pitch-shift | A phase vocoder written for the project |
| Tools | 99 deterministic tools the AI can call. The generated list is `docs/tools-reference.md`. |
| AI providers | Six API providers plus the user's ChatGPT plan. The six are Anthropic, OpenAI, OpenRouter, Groq, Gemini and Ollama, which runs locally and needs no key. Since 0.9.0 the user can instead continue with ChatGPT, which uses the included usage of a ChatGPT Plus or Pro plan in place of a key. That is OpenAI's Sign in with ChatGPT for open-source apps, and it is a preview. Available for ChatGPT Plus and Pro. May not be available on workspace plans; that depends on OpenAI and your admin. |
| Where audio goes | Nowhere. Processing is local. Only chat text goes to the chosen provider. That includes the words a transcription finds, which are text, a limited amount per call and never audio, and which stay on the computer with Ollama. |
| Telemetry | None in the desktop app. The website uses anonymous Vercel analytics (page views and page performance, no cookies). When it is enabled, the website also uses Google Analytics 4, only after a visitor accepts it in the cookie banner. That one sets cookies, and the Cookie settings link in the footer withdraws it. |
| Other network requests | Besides the AI provider and any hosts the user sets up, such as plugins and MCP servers, the app makes the downloads the user starts. From 0.8.0, clicking Download in Settings fetches the speech model, Whisper base, about 139 MB, from Hugging Face (huggingface.co). Version 0.7.0 has no Download button. The speaker model, two files and about 33 MB, is a second, separate Download from the same host on the same terms. Hugging Face sees the IP address, the file requested, and that edytlab is asking and which version. It never sees audio. Nothing downloads at launch or from the AI, and transcribing or finding speakers makes no request. If the user clicks Continue with ChatGPT (0.9.0 and later), the browser goes to auth.openai.com to sign in, and the app renews the sign-in and, on sign-out, revokes it with that host. The assistant's requests then go to api.openai.com with the user's ChatGPT access token. They carry the same text as for any provider and never audio. OpenAI can tie them to the user's ChatGPT account, to edytlab's registration and to the installation's random host ID. Signing out does not disconnect edytlab in ChatGPT. The user does that in ChatGPT settings under Security and login. |
| Keys | Stored in the operating system's credential store. All the keys and the ChatGPT sign-in's token share one entry there, so macOS asks for the login password at most once per launch. The provider, model and base URL live in a settings file in the app's config folder, with no secret in it. The ChatGPT plan has no key. Its sign-in keeps one refresh token in that same entry, and a small `registration.json` in the config folder holds a host ID, OpenAI's client ID for edytlab and the account's email as a label. |
| History | A branchable graph of edits, with fork, A/B compare and undo |
| Optional safety | "Plan first" mode shows the AI's steps for approval before they run |
| Export | WAV, FLAC and MP3 |
| Maintainer | `@laadtushar` on GitHub |

### What is not shipped

State this plainly if asked.

- Stem separation is not available. The tool is registered, but it is not
  implemented in any build.
- Transcription (Whisper base) works from 0.8.0, on the user's computer, once
  the user downloads the speech model (about 139 MB) in Settings. Version 0.7.0
  and earlier cannot transcribe. Its accuracy has been checked on one short
  synthetic English recording and about 16 minutes of English audiobook speech,
  where 4% to 7% of the words were wrong. It has not been checked on a range of
  voices or languages.
- edytlab edits files, offline. It is not a live DJ tool, and it is not a DAW:
  there is no MIDI and no VST or CLAP hosting.
- There is no web or mobile version.

### Not available from the project

There are no user counts, download statistics, benchmarks, testimonials, awards
or press quotes to cite, and none should be invented.

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
| Demo video: beatmatch and blend two tracks (MP4, with sound) | `website/public/demos/dj-beatmatched-transition.mp4` | <https://edytlab.com/demos/dj-beatmatched-transition.mp4> |
| Demo video poster (JPG, 1280x776) | `website/public/demos/dj-beatmatched-transition.jpg` | <https://edytlab.com/demos/dj-beatmatched-transition.jpg> |
| Demo video: extend an intro for mixing (MP4, with sound) | `website/public/demos/dj-extended-club-intro.mp4` | <https://edytlab.com/demos/dj-extended-club-intro.mp4> |
| Demo video: a three-track mini-mix (MP4, with sound) | `website/public/demos/dj-mini-mix.mp4` | <https://edytlab.com/demos/dj-mini-mix.mp4> |

Screenshots of the interface are not in the repository. Take your own from the
release build, or ask via an issue.

There are no brand guidelines. Use the logo as-is on a dark or light
background, and write the name in lowercase: edytlab.

## Approved descriptions of the demo

Quote only these. They are the captions the website shows next to the
recording.

> A DJ asks for the two tracks' tempos, then Claude time-stretches Neon Rush to
> Midnight Drive's tempo, keeping its pitch, starts it under Midnight Drive's
> last 8 bars, and blends the two with a crossfade and a low-pass filter on the
> outgoing track. The mix is mastered for streaming and exported as a WAV.
> Playbacks are in real time.

## Short bios by length

**20 words:** edytlab is a free, open-source desktop audio editor you drive by chatting with an AI. Audio never leaves your computer.

**50 words:** edytlab is a free, open-source audio editor for macOS, Windows and Linux. You tell it what you want in chat and an AI runs the editing tools. The work happens on your computer. Audio is never uploaded. Only the chat goes to the AI you choose, or nowhere with Ollama.
