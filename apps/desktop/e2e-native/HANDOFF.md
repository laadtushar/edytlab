# Native E2E: handoff notes (delete before merging #391)

Written by the session that built this harness, for whichever session continues it. No secrets are in this file or should ever be.

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

## Groq
- `curl https://api.groq.com/openai/v1/models` with no key returns HTTP 502 with body `injection failed ("groq")` (checked twice, in two sessions). That text is the sandbox egress proxy's: its credential-injection rule for *.groq.com is failing, so the network secret is not provisioned. Other hosts work.
- `GROQ_API_KEY` is also set as an environment variable in these sessions. The proxy rewrites requests to the host, so it is not a way around the 502; do not print it or put it in any message, commit, issue or screenshot.
- If a session finds 401 or 200 instead: proceed (Settings > Groq, a tool-calling model, Test, then the agent stories). If still 502: report the exact message and carry on with the scripted-model stories.

## Still to build
Timeline, track controls, clips and automation, markers/labels, undo/redo, graph and A/B, export selection (native save dialog), chat/agent stories (tool cards, plan approve/edit/reject, slash commands, palette, capabilities), settings editors, recording (PulseAudio null sink, plus the no-device error), shortcuts overlay, error banners. Then an HTML report with the screenshots; one GitHub issue per real bug (search first). Known: #392 (Settings Plugins tab clipped).
