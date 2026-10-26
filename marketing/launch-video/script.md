# edytlab launch video: script and claim ledger

The scene scripts are code: `tools/launch.py` (read it for exact times and callout boxes). This file lists what each cut shows and where each claim is true. The old script, written for the earlier recordings (it named a tool count, a version-era build and specific readings), is in `marketing/archive/launch-video/2026-10-26-evergreen/script.md`.

**Rule for every line below and in `launch.py`: no claim that changes with a release.** No version, tool count, model or provider name, price, date or signing status, and no analysis reading (key, beat time, confidence) in a title or callout. See `marketing/README.md`, "Evergreen media rules". A callout says what the shot shows, nothing more.

## Cuts

- **LinkedIn 1:1, 63.1 s:** hook ("I built an audio editor you talk to.") -> the chores -> "Or just describe it." -> six steps on real footage -> two more jobs -> how it works -> end card.
- **Reddit 16:9, 57.5 s:** plain first line -> the same six steps -> two more jobs -> how it works -> end card (GitHub address).
- **Instagram 9:16, 32.3 s:** hook -> five steps incl. real audio -> end card.

## Footage (all from `website/public/demos/`, recorded with the real model)

| Step | Source | Title | Callouts (what the shot shows) |
|---|---|---|---|
| 1 | BT 23.4-28.4 | Describe it in plain English | "You type the edit": the composer, the request typed in |
| 2 | BT 40.0-43.5 | The agent calls a tool | "A real tool call": the green "Time stretch" chip; "Tempo matched, pitch kept": the reply (the reply says the tempo was matched and the pitch unchanged) |
| 3 | BT 50.6-54.5 | It reports what changed | "It says what it did": the opening of the reply |
| 4 | BT 63.0-71.0 | Describe the blend | the typed request; "Three edits, one request": the reply's list of three edits; "The crossfade": the overlapped waveforms |
| 5 | BT 74.67-83.87 | Hear it | "Press play: real audio"; "Heard in real time": the playhead over the waveform. The app's own sound plays (audible from 74.87 s) |
| 6 | BT 117.0-123.0 | Master and export | "It reports what it exported": the export reply |
| also | EI 49.0-53.0, MM 33.5-37.5 | Also: hold the low end until the drop; And a three-track mini-mix | "A high-pass until the drop"; "4-bar overlaps" |

`BT`, `EI`, `MM` are the beatmatch, extended-intro and mini-mix demos. Every shot is asserted by `launch.py` to lie inside the burned-in caption it is cut from (the windows are in `marketing/reels/tools/spec.py`).

## Claims on screen and where each is true

| Claim | Where it is true |
|---|---|
| Real screen recording, waits sped up | the note under every shot; the demos are live recordings and `make-demo-videos.mjs` plays waits faster |
| Deterministic tools run on your machine; only the chat goes to the provider you choose | the site's privacy page and `docs/architecture.md`; the diagram is labelled and names no provider and no count |
| Audio files stay on your computer | same sources |
| Free, open source; MIT licence; macOS, Windows, Linux | the repository's `LICENSE` and the release installers |
| Synthetic demo tracks (Instagram end card) | `apps/desktop/e2e-native/fixtures.mjs` writes the tracks |
| "Which prep job would you hand off first?" | a question to the viewer; no claim |

Nothing shows or says stem separation, a model name, a version or a count.
