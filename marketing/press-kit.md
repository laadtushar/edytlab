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
track to match the first, crossfade, and export a WAV" into calls to 93
deterministic audio tools. The tools run locally in a Rust engine on the user's
own computer, so audio files are never uploaded; only the chat goes to the
language-model provider the user selects (Anthropic, OpenAI, OpenRouter, Groq,
Gemini, or Ollama for a fully local setup). Every edit is saved as a node in a
branchable history, with undo and A/B comparison. edytlab runs on macOS,
Windows and Linux, is released under the MIT license, and its source is at
github.com/laadtushar/edytlab.

## Fact sheet

Every line below was checked against the repository or the GitHub release on
2026-10-09.

| | |
|---|---|
| Name | edytlab |
| What it is | Desktop audio editor driven by an AI agent |
| Latest version | 0.3.0, published 2026-10-09 |
| License | MIT |
| Repository created | 2026-05-05 |
| Platforms | macOS (one universal build for Apple Silicon and Intel), Windows 10 and 11, Linux |
| Installers | `.dmg` (macOS), `.msi` and NSIS `.exe` (Windows), `.deb` and AppImage (Linux) |
| Signing | Not signed or notarized yet. macOS and Windows warn on first launch; the release notes give the steps. |
| Built with | Tauri 2, Rust, React |
| Audio engine | Pure-Rust DSP. Decode with `symphonia`, resample with `rubato`, effects in an in-house dependency-free `audio-dsp` crate. |
| Time-stretch / pitch-shift | A phase vocoder written for the project |
| Tools | 93 deterministic tools the agent can call (generated list: `docs/tools-reference.md`) |
| AI providers | Anthropic, OpenAI, OpenRouter, Groq, Gemini and Ollama (local, no key) |
| Where audio goes | Nowhere. Processing is local. Only chat text goes to the chosen provider. |
| Telemetry | None in the app; no analytics on the website |
| Keys | Stored in the operating system's credential store |
| History | Branchable graph of edits with fork, A/B compare and undo |
| Optional safety | "Plan first" mode shows the agent's steps for approval before they run |
| Export | WAV, FLAC, MP3 |
| Maintainer | `@laadtushar` on GitHub |

### What is not shipped

State this plainly if asked.

- Stem separation (Demucs) and speech transcription (Whisper) are registered as
  tools but not implemented in any build. Editing by transcript depends on
  transcription and therefore does not work either.
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
| Demo video: beatmatch and blend two tracks (MP4, 2:58, shown at 1.6x speed) | `website/public/demos/dj-beatmatched-transition.mp4` | <https://edytlab.com/demos/dj-beatmatched-transition.mp4> |
| Demo video poster (JPG, 1280x776) | `website/public/demos/dj-beatmatched-transition.jpg` | <https://edytlab.com/demos/dj-beatmatched-transition.jpg> |
| Demo video: extended intro, with sound | `https://edytlab.com/demos/dj-extended-club-intro.mp4` | Add once PR #432 is merged |

Screenshots of the interface are not in the repository. Take your own from the
release build, or ask via an issue.

There are no brand guidelines. Use the logo as-is on a dark or light
background, and write the name in lowercase: edytlab.

## Approved descriptions of the demo

Quote only these. They are the captions the website shows next to the
recording.

> A DJ asks for both tracks' tempos, and the incoming track is time-stretched to
> match. It is started 8 bars before the outgoing track ends, the two are
> crossfaded, and a low-pass filter is put on the outgoing track over the
> overlap. The mix is compressed, limited at -1 dB, brought to -14 LUFS and
> exported as a WAV. Along the way Claude points out what it would change: at the
> 16 s start the bars land a beat apart. Shown at 1.6x speed.

## Short bios by length

**20 words:** edytlab is a free, open-source desktop audio editor you drive by chatting with an AI agent. Audio is processed locally.

**50 words:** edytlab is a free, open-source desktop audio editor for macOS, Windows and Linux. You describe an edit in chat and an AI agent runs deterministic audio tools to do it. The processing is local, so audio is never uploaded; only the chat goes to the AI provider you choose, or none if you use Ollama.
