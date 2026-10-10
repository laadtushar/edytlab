# tools

Python 3 scripts that built everything in this folder (they need `ffmpeg`/`ffprobe` and Pillow; `narrate.py` needs `requests`).

- `spec.py`: the single source of truth: caption start times on the final demo videos, the audible "Listen" windows, the narration lines, the edit lists of the reels.
- `cues.py`: edit list to cue timeline (cut lengths follow the real speech length).
- `narrate.py`: ElevenLabs text-to-speech with timestamps and sound effects. Reads the key from `ELEVENLABS_API_KEY` in the environment only. Run as
  `set -a; . <env file>; set +a; python3 narrate.py gen reel-highlights ...`. Keeps a 8,000-character budget ledger.
- `render.py`, `build.py`: the 1080x1920 renderer (PIL + one ffmpeg filter graph) and the orchestrator; `docs.py`, `readme.py`: write transcripts.md and README.md.
- `detect_captions.py`, `map-captions.mjs`, `captions-analytic-mapping.json`: how the caption times were found. Mapping the raw recording times through `make-demo-videos.mjs`'s
  segment logic drifts by up to 3.4 s (every concat segment boundary adds about 0.1 s), so the times in `spec.py` are measured on the final mp4 (frame where the lower third changes).
- `rms_listen_regions.py`: RMS profile used to find where the drop lands inside the Listen moments.

Paths inside the scripts point at the original working directory; adjust `ROOT` in `spec.py` to re-run elsewhere.
