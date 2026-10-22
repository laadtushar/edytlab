# v0.7.0 promo footage: nine clips of the real frontend, with a scripted assistant

Phase 1 of the v0.7.0 promo: the screen recordings the Remotion edit is cut from. Nothing is edited here: no titles, no
callouts, no sound design.

## Honesty note (binding for the edit)

- **The UI is real.** Every frame of app UI is the frontend of commit `23bbfe2e` (origin/main after v0.7.0 and #638; no frontend
  source file has changed since, only two of its tests, in #642), built for production and run in Chromium with only the IPC boundary to the Rust
  backend replaced, exactly as `apps/desktop/e2e/` runs it. The status bar says **V0.7.0**.
- **The assistant is scripted.** There is no API key, so the replies and the tool calls are raised by the recording
  (`record/lib/studio.ts`), not produced by a model. The tool names and arguments are the real ones
  (`docs/tools-reference.md`); the events are raised in the order the app's backend raises them. Put this note under every
  app shot: **v0.7.0 · real app · scripted assistant**.
- **The audio is synthetic**, written from arithmetic (`record/lib/music.ts`): a short A-minor loop (drums, bass, pad, keys,
  lead and a formant "vocal"). The footage never says it is anyone's music. The sound in S1 and S7 is a model of the mix for
  the edits shown (the linear fade and the -3 dB gain applied to those samples), not the engine's own render.
- **The pointer and the click ring are drawn by the recording** on top of the page (a capture of a page has no OS pointer).
  They are not part of the app.
- **Time is stepped.** A 3200x2000 capture takes 0.3 to 0.4 s, so the page's clock, its animations and its media position are
  advanced 1/30 s per frame and a picture is taken each time (`record/lib/kit.ts`, `lib/stage.ts`). The footage is 30 fps by
  construction and what moves, moves at the speed it would on screen. The app's own code, styles and IPC calls are unchanged.
- Not shown, so not to be claimed from this footage: stem separation or transcription (not shipped), signed installers (they
  are unsigned), live or DJ use (it is an offline editor), a model's or provider's name, a tool count. `../claims.md` has a
  row for every claim the footage does make, and where it is true.

## Format

| | |
|---|---|
| Picture | 3200x2000 (a 1600x1000 CSS-pixel window at device scale 2), 30 fps constant, H.264 High, yuv420p, CRF 16 (x264 `veryfast`), no audio track |
| Size | 38 MB for all nine clips |
| Sound | `s1-describe-it-happens.wav` and `s7-mix-stays-playable.wav`: mono, 44.1 kHz, 16-bit, **exactly as long as the clip and lined up with it** (silence until the mix started, silence where it stopped). Sample peak -0.8 dBFS: the edit sets loudness and true peak |
| Sidecars | `<shot>.marks.json` (moments, in seconds into the clip) and `<shot>.notes.json` (what was recorded: calls the app made, counts, where the sound plays) |
| Theme | The app's dark theme, `prefers-reduced-motion: no-preference` (the app's fades, spinners and settles are on) |

Useful regions, in CSS pixels of the 1600x1000 window (double them for the file): the chat pane x 1240 to 1600, y 52 to
972; the composer x 1252 to 1588, y 846 to 924; the left pane (timeline, graph) x 0 to 1240, y 52 to 972; the lane
sidebar x 0 to 132; a Settings dialog about x 560 to 1040. The assistant pane is 360 px wide whatever the window.

## The clips

| Clip | Length | What it shows | Moments worth using (seconds into the clip) |
|---|---|---|---|
| `s1-describe-it-happens.mp4` + `.wav` | 18.9 s | **S1. Describe it, it happens.** Two tracks (Backing, Vocal). The request is typed into the composer: "Fade the intro in over 4 seconds and bring the vocal down 3 dB". The scripted assistant makes the two calls the agent would, `fade` and `set_track_gain`; the timeline updates; Play | 1.5 the pointer clicks the box, then rests off the words; 2.4 to 5.9 typed at human pace. 6.4 Enter: the bubble, then "Thinking…" to about 7.7. About 7.7 the **Fade** chip appears running; 8.3 it resolves with a node divider and the Backing lane redraws with the 4 s ramp. 9.5 **Track volume** resolves; the Vocal readout reads -3.0 dB. 9.6 to 10.1 the reply streams in. 12.4 Play: the playhead runs to 18.4; the sound is the `.wav`, 12.43 to 18.37 |
| `s2-faders-stay-put.mp4` | 6.7 s | **S2. Faders that stay put (#596).** Three tracks; the pointer takes hold of the first lane's gain slider, then four ArrowRight taps. The readout steps **+0.5, +1.0, +1.5, +2.0 dB** and never goes back. Each tap's read-back is held and let go in the middle of the next tap (see "S2 checked against the code before the fix") | 2.0 slider focused (purple ring). Key-downs at 2.71, 3.24, 3.77, 4.31, each held 0.16 s; the readout is +0.5 from 2.71, +1.0 from 3.24, +1.5 from 3.77, +2.0 from 4.31. The held read-backs land at 3.34, 3.87, 4.41. 4.5 to 6.7 it holds at +2.0 |
| `s3-history-graph.mp4` | 19.2 s | **S3. The History graph keeps up (#602).** The Graph tab open: 17 nodes, two abandoned tries and a line of edits. "Name this version chorus" (chip **Rename version**): the head's bubble reads **chorus** the moment the turn ends. Then "Compact the history, keep the last 8 edits" (two **Compact session** chips, a dry run and the apply): the removed nodes leave the graph, 17 to 8 | 2.5 to 3.9 typing; 4.3 sent; 6.33 the turn ends and **chorus** appears on the head bubble (nothing else done). About 10 to 12.2 the second request typed; 12.6 sent; 13.9 the first Compact chip (the dry run) and 15.2 the second (the apply); **16.0 the 9 removed nodes disappear** at once; 16.8 the reply. 17 to 19.2 holds on the 8 left |
| `s4-200-tracks.mp4` | 21.7 s | **S4. Big sessions (#618).** A 200-track orchestral session ("200 TRACKS" in the toolbar). "make all the strings quieter": two **List session** chips (paging past the cut session block), then twelve **Track volume** chips; the string lanes read -4.0 dB | 0.8 to 3.7 a fast look down the list and back (the size of it). 7.5 typed, 8.0 sent. About 9.0 to 10.6 the two List session chips; 10.9 to 14.9 the twelve Track volume chips, each with its node divider; 15.9 the reply is done. 17.2 to 19.6 the timeline scrolls to the strings; 19.6 Violin I a, b, c read -4.0 dB |
| `s5-runtime-bundled.mp4` | 7.2 s | **S5. The ONNX Runtime is bundled.** Settings, Account tab; the last line reads **ONNX Runtime: 1.23.2 (bundled)** with the library's path under it | 2.0 the pointer clicks the gear; the dialog opens at 2.1 with the line already in view at the foot of the Account tab; 4.0 the pointer rests beside it; hold to 7.2. Groundwork only: no model, no inference |
| `s6-skills-note.mp4` | 11.2 s | **S6. Skills update (#574).** Settings, Skills tab: the eight bundled skills, and in the empty pane "Skills that come with edytlab are updated when a release changes them, unless you have edited them." Then `loudness-master` is opened | 2.1 gear clicked and the dialog opens; about 3.9 Skills tab clicked; 4.0 to 8.9 the note is on screen (the pointer hovers it from about 5); 8.97 a skill is clicked: its fields and body open (the note goes with the empty pane); hold to 11.2 |
| `s7-mix-stays-playable.mp4` + `.wav` | 15.6 s | **S7. The mix stays playable (#607).** Play; to the Graph tab; back to the Timeline; Play again. While the Graph tab is open the mix's file is removed from the preview cache; on the way back the app asks for the mix once more and plays it, with no error | 1.9 Play (sound 1.9 to 6.0). 6.0 Graph tab (the player goes with the timeline, so the sound stops); 6.0 to 8.4 the graph, 5 nodes. 8.4 back to the Timeline; the mix is asked for again and loaded (about 0.5 s). 10.5 Play again (sound 10.53 to 15.03); 15.0 paused |
| `s8a-broll-timeline.mp4` | 15.1 s | **S8a. B-roll.** Six tracks and five labels (intro, verse, chorus, bridge, outro). Zoomed in twice, panned slowly from the start of the piece to its end, then down the lanes. No chat is used (the assistant pane shows its first-run hints): crop to the left pane | 0.9 to 3.1 the two zoom clicks; 3.13 to 10.33 the slow pan (7.2 s) along the piece, the labels sliding with the audio; 10.8 to 14.4 the lanes scroll slowly down to the label lane; a 0.7 s hold at the end |
| `s8b-broll-graph.mp4` | 13.1 s | **S8b. B-roll.** The History graph with its branches (17 nodes, head named **chorus**): zoomed in a little with the wheel, then dragged along the line of edits to the head. No chat in use: crop to the left pane | 1.7 to 2.4 the pointer moves onto the graph; 2.6 to 3.3 the wheel zooms in a little; 4.2 to 11.07 the drag along the line of edits, ending at the head bubble **chorus**; a 2 s hold at the end |

S8a and S8b are backgrounds: slow, nothing to read, no text in them to quote.

## S2 checked against the code before the fix

S2 sets up the order that broke before #626 (each read-back lands between the next tap's key-down and key-up). To be sure it
is the failing order and not just a calm one, the same script was run with #626's source change taken out
(`git apply -R` of `e516da7e` over `apps/desktop/src`, not committed, restored afterwards):

| | before #626 | v0.7.0 (the footage) |
|---|---|---|
| Readout while tap 2's key is held, after tap 1's read-back lands | **+0.5 dB** (the slider jumped back) | +1.0 dB |
| Readout after the fourth tap | **+1.5 dB** | +2.0 dB |
| `set_track_gain` calls sent | **2** (`0.5`, `1`; taps 2 and 4 committed nothing) | 4 (`0.5`, `1`, `1.5`, `2`) |

The footage is the v0.7.0 column. `notes.json` for S2 records its four calls, and the shot fails if they are not those four.

## Re-taking a clip

`record/README.md`. In short, from `apps/desktop`:
`pnpm exec playwright test -c ../../marketing/promo-v0.7.0/record/playwright.config.ts s3`
(a clip takes 1 to 5 minutes; S4's 200 lanes take the longest). A clip is rewritten in place, and its raw frames are
deleted when it ends.
