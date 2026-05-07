# Demo recordings

This directory holds the screen-capture demos linked from the top-level
`README.md`.

## `phase1-podcast-cleanup.mp4` (M16 acceptance)

A ~2 minute, no-edits screen capture of the Phase 1 podcast-cleanup
flow:

1. Open the desktop app on macOS.
2. Drop `tests/fixtures/raw_podcast_intro.wav` (the 15 s fixture, 2 s of
   room tone before speech).
3. Type: "Remove the silence at the start, then normalize to -1 dBFS."
4. Wait for the chat narration and the two tool badges (`cut_range`,
   `normalize`).
5. Click the preview button; speech starts at t=0.
6. Click "Export...", choose `.mp3`, save.

Per the M16 plan, the same flow must be demonstrated on Windows 11
before Phase 1 ships.

The `.mp4` itself is recorded by the dev manually (it captures the
actual desktop app, not the CLI smoke harness in `apps/cli/tests/`) and
is committed once Phase 1 ships. The deterministic E2E test in
`apps/cli/tests/deterministic_pipeline.rs` is the always-on
machine-checkable gate; this video is the human-checkable one.
