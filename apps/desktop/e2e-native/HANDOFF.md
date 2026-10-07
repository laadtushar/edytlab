# Native E2E: handoff notes (delete before merging #391)

Written by the session that built this harness, for whichever session continues it. No secrets are in this file or should ever be.

## Who is doing what (two sessions share this branch)
- The **parent session** owns: all assistant/agent stories (`stories/agent.mjs`: plan-first, selection-driven edits, errors) and the **local real model**; and, since the other session has not pushed for a while, the **timeline and track controls** (play/seek/zoom/select, mute/solo/gain/pan, rename/duplicate/remove, clips). A llama.cpp `llama-server` with Qwen2.5-3B (Q4_K_M, `--jinja`, `-c 32768 -np 1`) serves `localhost:11434` in the parent's sandbox. Run those with `LLM=real` (the runner then does not start the scripted model). Not available in other sessions' sandboxes unless rebuilt: llama.cpp comes from the sdist of `llama-cpp-python` on PyPI (GitHub releases for other repos are blocked), the model from Hugging Face. Expect ~7 min for a first reply: the app's first request is ~15k tokens (#395).
- The **child session** owns: undo/redo, history graph and A/B compare/accept, export selection (native save dialog), markers and labels, settings editors (project meta, memory, skills, agent profiles, MCP, plugins), recording, the shortcuts overlay, error banners, the Groq check, and the final report.
- Pull before you push; merge, never force. Run the suite through `./run-suite.sh` (one shared session keyring per run). `results.json` accumulates by story id.

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
Timeline, track controls, clips and automation, markers/labels, undo/redo, graph and A/B, export selection (native save dialog), chat/agent stories (tool cards, plan approve/edit/reject, slash commands, palette, capabilities), settings editors, recording (PulseAudio null sink, plus the no-device error), shortcuts overlay, error banners. Then an HTML report with the screenshots; one GitHub issue per real bug (search first). Known: #392 (Settings Plugins tab clipped).
