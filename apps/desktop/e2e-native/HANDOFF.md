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

## Groq (researched; sources: code.claude.com/docs/en/cloud-environments, and live tests)
- Symptom: a Groq call from the sandbox answers HTTP 502, body `injection failed ("groq")`, even with an Authorization header supplied. That text is the sandbox egress proxy's.
- How the feature works (docs): an "API credential" stored on the environment (Pro/Max; section "API credentials" in Edit environment) is added by Anthropic's agent proxy to requests whose host matches the credential's "Allowed websites". The key never becomes an environment variable. A credential cannot be edited: delete and re-add to change hosts or value. A credential the proxy cannot use is marked **"Not sent"** in the list, with a note saying why.
- Known-good setup: Credential type **Bearer**; Allowed websites `api.groq.com` (or `*.groq.com`); Custom header Name `Authorization`, Prefix `Bearer`, Value = the key. Then, in a NEW session, a keyless `curl https://api.groq.com/openai/v1/models` should answer 200 (the docs' own check). A different Credential type (one of the predefined connection types) is the likeliest cause of `injection failed ("groq")`, but this is inference, not confirmed.
- A plain environment variable `GROQ_API_KEY` is a separate thing: readable by anyone using the environment, and it does not stop the proxy's injection attempt for the host.
- Verified here: the native app reaches remote providers through this proxy (Settings > OpenAI > Test against api.openai.com returns OpenAI's real 401 JSON), so TLS and proxy settings are fine. Once the credential works, enter ANY placeholder key in the app's Groq settings; the proxy supplies the real one.
- Plan once keyless curl gives 200: Settings > Groq, a tool-calling model (e.g. llama-3.3-70b-versatile), Test button, then the agent stories. Never print or screenshot a real key.
- If keyless curl still gives 502 `injection failed`: stop and tell the user exactly what the proxy said and that the credential in Edit environment needs deleting and re-adding as above; carry on with the scripted-model stories.

## Still to build
Timeline, track controls, clips and automation, markers/labels, undo/redo, graph and A/B, export selection (native save dialog), chat/agent stories (tool cards, plan approve/edit/reject, slash commands, palette, capabilities), settings editors, recording (PulseAudio null sink, plus the no-device error), shortcuts overlay, error banners. Then an HTML report with the screenshots; one GitHub issue per real bug (search first). Known: #392 (Settings Plugins tab clipped).
