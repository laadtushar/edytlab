# Test fixtures

Binary fixtures (WAV, MP3, etc.) are NOT committed to this directory.
They're regenerated at test time by helpers in the test files
themselves so the repo stays text-only.

## `raw_podcast_intro.wav` (M16)

Generated in-process by `apps/cli/tests/deterministic_pipeline.rs` and
`apps/cli/tests/podcast_cleanup.rs` via `hound`. Specs:

- 15 s total, 44.1 kHz mono 16-bit PCM
- First 2 s: digital silence (stand-in for room tone)
- Next 13 s: 440 Hz sine at -6 dBFS (stand-in for speech)

If you want a real-audio variant for manual desktop testing, drop your
own `raw_podcast_intro.wav` here. It's `.gitignore`'d via the top-level
ignore on binary audio files.
