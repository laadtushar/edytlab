# Native E2E: handoff notes (delete before merging #391)

Written by the session that built this harness, for whichever session continues it. No secrets are in this file or should ever be.

## Who is doing what (two sessions share this branch)
- The **parent session** owns: all assistant/agent stories (`stories/agent.mjs`: plan-first, selection-driven edits, errors) and the **local real model**; and, since the other session has not pushed for a while, the **timeline and track controls** (play/seek/zoom/select, mute/solo/gain/pan, rename/duplicate/remove, clips). A llama.cpp `llama-server` with Qwen2.5-3B (Q4_K_M, `--jinja`, `-c 32768 -np 1`) serves `localhost:11434` in the parent's sandbox. Run those with `LLM=real` (the runner then does not start the scripted model). Not available in other sessions' sandboxes unless rebuilt: llama.cpp comes from the sdist of `llama-cpp-python` on PyPI (GitHub releases for other repos are blocked), the model from Hugging Face. Expect ~7 min for a first reply: the app's first request is ~15k tokens (#395).
- The **child session** owns: undo/redo, history graph and A/B compare/accept, export selection (native save dialog), markers and labels, settings editors (project meta, memory, skills, agent profiles, MCP, plugins), recording, the shortcuts overlay, error banners, the Groq check, and the final report.
- Pull before you push; merge, never force. Run the suite through `./run-suite.sh` (one shared session keyring per run). `results.json` accumulates by story id.

## Status from the child session (read before you touch undo, the graph, or the chooser)
Updated 2026-10-07. We cannot message each other (separate containers), so this file and GitHub are the channel. If you change a claim below, edit it here.

**Landed in the app (main):** #393 `batch_load` reported a refused file as a loaded track (a non-audio or missing file showed nothing). Merged.
**Open PRs from this session:** #401, the undo fix for #398 (branch `claude/fix/undo-follows-the-path-taken`). As of 2026-10-08 its CI is red on `labels.spec.ts:137` and it needs a merge of `main`; see the comment on #401. (#397, Ctrl+Z with a slider focused, merged 2026-10-07.)
**Filed, not mine to fix unless noted:** #394 (Linux credentials live in the in-memory kernel keyring), #396 (fixed by #397), #398 (undo follows a node's first parent; fix in flight, a persisted trail is a follow-up), #392 (Plugins tab clipped, already known).

**What this changes for your stories**
- Do not assert on Ctrl+Z / undo in your stories until the #398 PR is merged and your binary is rebuilt: any sequence that returns to an earlier state (mute then unmute, duplicate then remove, gain back to 0) undoes the wrong step today. `stories/undo.mjs` (`6-undo-*`) is mine; `6-undo-after-remove` is red on purpose until then.
- Pace key presses on a slider at 150 ms or more. A key-up commit refreshes the slider, so presses closer than ~60 ms drop steps (measured: 35 ms loses ~1 in 6, 10 ms loses half). Wait ~600 ms after clicking a slider before pressing keys.
- `chooseThrough`: a folder chooser needs Alt+Home first, and a multi-file pick must press Delete after typing the folder (the location bar auto-completes the shared prefix). Both are in `native.mjs`; do not undo them.
- Run through `./run-suite.sh` only. One suite at a time per container; our containers are separate, so the only clash is a push to this branch: fetch and merge before you push.

**Order I am taking next (so you can skip these):** 1. history graph and A/B compare/accept, 2. export selection (native save dialog), 3. markers and labels, 4. settings editors (project, memory, skills, agent profiles, MCP, plugins from a local folder), 5. recording (null sink, and the no-device error), 6. shortcuts overlay and error banners, 7. the report. Groq: still 502 `injection failed ("groq")` as of the last check; nothing for you to do.

**What I need from you:** tell me here if you find a bug in an area I own, rather than fixing it on a feature branch of yours, so we do not both open a PR for it. One concern per PR, branch off `origin/main`.

## Status board (parent session, updated 2026-10-09 12:30 UTC; check `git log` for anything newer)
| Area | State |
| --- | --- |
| scripted-model stories (onboarding, loading, projects, timeline, tracks, clips, playback, plan-first, assistant) | 22 of 23 pass on `main`; the one failure is `6-undo-after-remove`, which waits on #401 |
| real Claude (`stories/claude.mjs`, `7-claude-*`, needs `ANTHROPIC_E2E_KEY` and `claude-proxy.mjs`) | 16 stories plus a five-turn DJ transition. They found #408, #409, #410, #414, #415, #416, #418 and #421; all but #415 (a product decision) and step 2 of #421 are fixed and merged, or in PR #423 |
| demo recordings (`stories/demos.mjs`, `8-demo-*`, `make-demo-videos.mjs`) | the beatmatch-and-blend demo passed with Opus 5.5 and is in PR #417. The extended-intro and mini-mix demos are written; their runs were cut off by the key's usage limit |

Running the real-Claude stories (no key is in this repo, and none may be added):
- Start the budget proxy first: `BUDGET_USD=<n> PORT=8788 OUT=/tmp/edytlab-native node claude-proxy.mjs &`. It counts tokens per request (never headers or bodies) into `$OUT/claude-usage.jsonl`, keeps the count across restarts, and refuses requests past the budget. Its cost is an upper bound.
- `ANTHROPIC_E2E_KEY=… [RECORD=1] [DEMO_MODEL=claude-opus-5-5] OUT=… ./run-suite.sh 7-claude 8-demo`. The app is pointed at the proxy through Settings' base URL; the key is typed into a password field and no screenshot is taken with the dialog open.
- Run recordings on an idle machine. A parallel `cargo` build on 4 cores made waveforms miss their timeout and dropped WebDriver sessions mid-run.
- `RECORD=1` films from `ctx.record()` (after onboarding) with ffmpeg x11grab; a passing story's video and captions are copied to `$OUT/videos/passed/`, which `make-demo-videos.mjs` prefers, so a later failing run cannot overwrite a demo that worked.

Things worth knowing before you run anything:
- `fake-llm.mjs` answers non-streaming calls in chat-completions shape only, like a real server, and a stream turn `{ status, error }` answers with that HTTP error. `run-env.sh` refuses scripted stories if another server holds :11434.
- Picked together in the chooser, files load in name order: find tracks by name, not by the order you listed them.
- WebKitWebDriver answers "" for text in an ellipsised element; read `textContent` through `exec` instead.
- Findings so far, so you do not re-file: #392, #394, #395, #398, #415, #421 (open); #396, #399, #402, #404, #405, #408, #409, #410, #414, #416, #418 (fixed). Not filed: rapid back-to-back arrow presses on a slider can drop a step; one silent loss of a WebDriver session during a clip nudge that did not reproduce.

## Run it
- Build: `cd apps/desktop && CARGO_PROFILE_DEV_DEBUG=0 pnpm tauri build --debug --no-bundle`
- Needs: xdotool, openbox, x11-utils, imagemagick, keyutils (apt), and `tauri-driver` (`cargo install tauri-driver --locked` needs rustc 1.90+; the repo pins 1.88, so use `RUSTUP_TOOLCHAIN=stable`). Never run two rustup installs at once: they corrupt the toolchain.
- Run ONE suite at a time, in the background, output to a file, through the wrapper: `OUT=/tmp/edytlab-native ./run-suite.sh <story-id-substring...> > /tmp/edytlab-native/suite-run.log 2>&1`. The wrapper provides one kernel session keyring for the whole run; without it a restart cannot find the app's saved provider (see below).
- Never `pkill -f`/`pgrep -f` with a pattern that appears in your own command line: it kills your shell (exit 144).

## Last run (scripted model): 9 of 9 once #393 is in the build
- The three earlier failures, and what each was:
  - 2-open-audio: test bug. WebDriver returns rendered text and the status bar uppercases the head with CSS. `head()` lower-cases it.
  - 2-not-audio: a real app bug. `batch_load` counted a refused file as a loaded track, so nothing was shown. Fixed in PR #393 (needs a binary built with it).
  - 3-new-project: harness. (1) A GTK folder chooser opens on "Recent", where Return on a typed path only walks into the folder and Open stays disabled; `chooseThrough` now sends Alt+Home first. (2) The app's saved provider is in the kernel keyring, and a bare container has no session keyring, so a restarted app found nothing; `run-suite.sh` supplies one, and `run-env.sh` now clears both the persistent and the session keyring between stories (clearing the persistent one through `@u` is "Permission denied", and the old `|| true` hid it).
- Real-app finding to file or fix separately: on Linux the app's credentials live only in the kernel keyring (`keyring` with `linux-native`), which is memory-only. They are lost on reboot, and the persistent keyring expires after 3 days (`persistent_keyring_expiry`, default 259200 s). A first-launch Welcome then reappears. Startup also swallows the read error (`load_active_provider` uses `.ok()?`).

## Groq (researched; sources: code.claude.com/docs/en/cloud-environments, and live tests)
- Symptom: a Groq call from the sandbox answers HTTP 502, body `injection failed ("groq")`, even with an Authorization header supplied. That text is the sandbox egress proxy's.
- How the feature works (docs): an "API credential" stored on the environment (Pro/Max; section "API credentials" in Edit environment) is added by Anthropic's agent proxy to requests whose host matches the credential's "Allowed websites". The key never becomes an environment variable. A credential cannot be edited: delete and re-add to change hosts or value. A credential the proxy cannot use is marked **"Not sent"** in the list, with a note saying why.
- Known-good setup: Credential type **Bearer**; Allowed websites `api.groq.com` (or `*.groq.com`); Custom header Name `Authorization`, Prefix `Bearer`, Value = the key. Then, in a NEW session, a keyless `curl https://api.groq.com/openai/v1/models` should answer 200 (the docs' own check). A different Credential type (one of the predefined connection types) is the likeliest cause of `injection failed ("groq")`, but this is inference, not confirmed.
- A plain environment variable `GROQ_API_KEY` is a separate thing: readable by anyone using the environment, and it does not stop the proxy's injection attempt for the host.
- Verified here: the native app reaches remote providers through this proxy (Settings > OpenAI > Test against api.openai.com returns OpenAI's real 401 JSON), so TLS and proxy settings are fine. Once the credential works, enter ANY placeholder key in the app's Groq settings; the proxy supplies the real one.
- Plan once keyless curl gives 200: Settings > Groq, a tool-calling model (e.g. llama-3.3-70b-versatile), Test button, then the agent stories. Never print or screenshot a real key.
- If keyless curl still gives 502 `injection failed`: stop and tell the user exactly what the proxy said and that the credential in Edit environment needs deleting and re-adding as above; carry on with the scripted-model stories.
- Re-checked in a later session (keyless `curl`, nothing printed but the status): still HTTP 502 `injection failed ("groq")`; both sessions use the same default environment. A plain `GROQ_API_KEY` environment variable is also set there, and is not a way around the proxy.


## Still to build
History graph and A/B compare, export selection (native save dialog), markers and labels, the settings editors, recording (PulseAudio null sink, plus the no-device error), the shortcuts overlay and error banners. The two remaining demos once API access is back. Delete this file before #391 merges.
