# Native E2E: handoff notes (delete before merging #391)

Written by the session that built this harness, for whichever session continues it. No secrets are in this file or should ever be.

## Run it
- Build: `cd apps/desktop && CARGO_PROFILE_DEV_DEBUG=0 pnpm tauri build --debug --no-bundle`
- Needs: xdotool, openbox, x11-utils, imagemagick, keyutils (apt).
- Run ONE suite at a time, in the background, output to a file: `OUT=/tmp/edytlab-native node suite.mjs <story-id-substring...> > /tmp/edytlab-native/suite-run.log 2>&1`.
- Never `pkill -f`/`pgrep -f` with a pattern that appears in your own command line: it kills your shell (exit 144).

## Last clean run (scripted model), 6 of 9
- PASS: 1-first-launch, 1-settings-panel, 1-provider-without-key, 2-open-several, 2-cancel-dialog, 3-template.
- FAIL 2-open-audio: the assertion `/^[0-9a-f]{7}$/` got `F9FAF82`. Cause: WebDriver returns rendered text and the status bar uppercases it with CSS. Test bug; `head()` now lower-cases.
- FAIL 2-not-audio: no `render-error` appeared after choosing `notes.wav` (not audio). Not yet diagnosed: could be a real app bug (a refused file should be named in an error, see the story's assertion) or a test timing/selector problem. Reproduce by hand, look at the failure screenshot, then decide; file an issue only if the app is at fault.
- FAIL 3-new-project: "native window did not close" on the directory chooser. Hypothesis, unverified: Return in a GTK folder chooser navigates into the folder instead of selecting it. Try the chooser's Select/Open button, or Ctrl+L, the path, then Alt+O.

## Groq
- A Groq call from the sandbox answered HTTP 502 with body `injection failed ("groq")`, even with an Authorization header supplied. That text is the sandbox egress proxy's, so the proxy's credential-injection rule for *.groq.com is failing; it is not the key and not Groq. Other hosts work (api.openai.com 401, example.com 200).
- `GROQ_API_KEY` is reportedly set as an environment variable in sessions of this environment (a sibling confirmed set and length only). Read it by name only; never print it or put it in any message, commit, issue or screenshot.
- First step: `curl` https://api.groq.com/openai/v1/models with no key. 401 or 200: proceed (Settings > Groq, a tool-calling model, Test, then the agent stories). Still 502 `injection failed`: the environment's Groq network secret or connector needs fixing by the user in the environment settings; report the exact message, and carry on with the scripted-model stories.

## Still to build
Timeline, track controls, clips and automation, markers/labels, undo/redo, graph and A/B, export selection (native save dialog), chat/agent stories (tool cards, plan approve/edit/reject, slash commands, palette, capabilities), settings editors, recording (PulseAudio null sink, plus the no-device error), shortcuts overlay, error banners. Then an HTML report with the screenshots; one GitHub issue per real bug (search first). Known: #392 (Settings Plugins tab clipped).
