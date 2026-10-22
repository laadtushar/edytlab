# v0.7.0 promo footage: nine clips of the released app, filmed live with real Claude

The screen recordings the Remotion edit is cut from. Nothing is edited here: no titles, no callouts, no sound design. This
replaces the first set (#644), which was Playwright against the e2e fake backend with a scripted assistant; that set, and
the script that made it, are in `../../archive/promo-v0.7.0/2026-10-22-scripted/`.

## Honesty note (binding for the edit)

- **The app is the released v0.7.0.** The binary is the one in the release's `.deb` (`edytlab_0.7.0_amd64.deb`, built from tag
  `v0.7.0`; its sha256 starts `bc1e11c8`). The release's AppImage carries the same binary with its paths patched for the
  bundle. It runs on Linux under Xvfb with WebKitGTK, driven by tauri-driver and X input events. The status bar says
  **V0.7.0**. Nothing in the app is replaced: the Rust backend, the store, the agent loop and the file chooser are all real.
- **The assistant is real Claude** in S1, S3 and S4 (nowhere else is an assistant used). The app is set to its Anthropic
  provider, model `claude-sonnet-5-5` (Claude Sonnet 5.5), reasoning effort `low`, with its base URL pointed at a local
  metering proxy (`apps/desktop/e2e-native/claude-proxy.mjs`) that counts what the run spends and forwards to
  api.anthropic.com. The request, the replies and the tool calls are what the model returned, unedited. Settings, where the
  key is typed, is never on film. A turn is not retried: where Claude's first answer was a question (S3), the next thing
  typed is the natural answer, and it is on film.
- **Everything the user does is real input.** The pointer is the X pointer (the recorder draws it), clicks are X clicks, and
  text is typed with real key events (`xdotool type`, a word at a time at a human pace). Nothing is clicked through the
  page's script, except where a clip's setup says so below (before filming begins).
- **Time is real.** Each clip is the screen at 15 frames a second, in real time. Waits for the model are in the clips; the edit
  speeds them up only with its "N× speed" badge.
- **The audio is synthetic**, written from arithmetic (`apps/desktop/e2e-native/stories/promo-audio.mjs`): a 16-second A-minor
  loop at 120 BPM in six stems (drums, bass, pad, keys, lead, a formant "vocal"). Nothing sampled, recorded or downloaded;
  the footage never says it is anyone's music. The sound in S1 and S7 is the app's own playback, captured from the audio
  server's monitor of the sink the app plays into.
- **The project contents are prepared, off film.** Track files are the stems above; S3's history (20 nodes, two abandoned tries
  and a line of twelve edits) was made by editing lanes with the keyboard and Ctrl+Z before filming; S4's 200 tracks were
  loaded through the app's Open Audio chooser in three batches (the names are invented, the files are short copies of the
  stems at 22.05 kHz).
- Not shown, so not to be claimed from this footage: stem separation or transcription (not shipped), signed installers (they
  are unsigned), a use other than editing, a user count or a price. `../claims.md` has a row for every claim the footage makes,
  and where it is true.

## Format

| | |
|---|---|
| Picture | 3200x2000 (a 1600x1000 CSS-pixel page at the display's scale of 2, `GDK_SCALE=2`), 15 fps constant, H.264 High, yuv420p, CRF 18, no audio track |
| Sound | `s1-describe-it-happens.wav` and `s7-mix-stays-playable.wav`: mono, 44.1 kHz, 16-bit, **exactly as long as the clip and lined up with it**, peak -0.8 dBFS (the edit sets loudness and true peak) |
| Sidecars | `<clip>.marks.json` (named moments, seconds into the clip) and `<clip>.notes.json` (the request, the reply, the tool calls Claude made and what they returned, the capture's own log, and for S1 and S7 how far the sound was moved) |
| Theme | The app's dark theme as it ships |

Useful regions, in CSS pixels of the 1600x1000 page (double them for the file): the chat pane x 1240 to 1600, y 52 to 972; the
composer x 1252 to 1588, y 846 to 924; the left pane (timeline, graph) x 0 to 1240, y 52 to 972; the lane sidebar x 0 to 132; a
Settings dialog about x 560 to 1040.

How the clips were made, and why they can be trusted for sync and completeness, is in `../record/README.md`.

## The clips

Times are seconds into the clip. "Chip" is the tool badge the app puts in the chat.

| Clip | Length | What it shows, and what real Claude did | Moments |
|---|---|---|---|
| `s1-describe-it-happens.mp4` + `.wav` | 37.2 s | **S1. Describe it, it happens.** Two tracks, Backing and Vocal. The request is typed: "Fade the backing in over the first 4 seconds and bring the vocal down 3 dB". Claude made two tool calls, `fade { track: 0, kind: "in", range: 0 to 4 s }` and `gain { track: 1, db: -3 }` (chips **Fade** and **Gain**), and replied: "I made both changes: Backing (track 0): it now fades in over the first 4 seconds. Vocal (track 1): its gain is now -3 dB. I haven't rendered or exported anything yet. Tell me if you want that as the next step." The Backing lane redraws with the ramp and the Vocal readout reads -3.0 dB. Then Play | typing 2.1 to 9.6; sent about 10.1; Fade chip running 12.3, done 15.5; Gain chip 16.0; lanes redraw 16.3 to 16.7; reply done 18.5; Play pressed about 24.9 (the app renders the mix first), sound 26.3 to 34.1 |
| `s2-faders-stay-put.mp4` | 12.1 s | **S2. Faders that stay put (#596).** Three tracks; the pointer takes hold of the first lane's gain slider with a click on its thumb, then four ArrowRight key taps (real key events, about 140 ms down, about 400 ms apart). The readout steps +0.5, +1.0, +1.5, +2.0 dB and never goes back; the session holds +2.0 dB. Nothing is arranged: no backend answer is held or delayed. The story records every change of the readout and fails if one goes down | slider taken 3.9; readout +0.5 at 5.07, +1.0 at 5.60, +1.5 at 6.13, +2.0 at 6.67; holds to the end |
| `s3-history-graph.mp4` | 49.7 s | **S3. The History graph keeps up (#602).** The Graph tab open beside the chat, 20 nodes. "Name this version chorus": Claude called `name_node` on the head (chip **Rename version**), replied "I labelled the current version (node f5206c71…605e) "chorus". I only changed the label, not the audio.", and the head's bubble reads **chorus** with no other change. Then "Compact the history, keep the last 8 edits": Claude called `compact_session { keep_last: 8 }` (a dry run: "Would remove 12 node(s)…"), then `compact_session { apply: true, keep_last: 8 }` in the same turn (two **Compact session** chips), and the nodes leave the graph, 20 to 8. Its reply: "I compacted the history. It removed 12 old nodes and kept the last 8 edits on the current chain, so recent undo still works. It freed no disk space (0.0 MB), because none of those nodes held audio files of their own. The removal is permanent, so you can't undo back to those 12 steps. I didn't check whether the "chorus" node was one of them. If it was, the label is gone with it. I can check and re-label the current head if you like." (The "chorus" bubble is still there: it is the head.) | typing 3.1 to 12.4; sent 12.3; Rename version chip done 14.3; "chorus" on the head and the reply done 17.8; second request typed 24.1 to 35.4; sent 35.4; nodes leave 40.9; reply done 43.3 |
| `s4-200-tracks.mp4` | 33.7 s | **S4. Big sessions (#618).** 200 tracks ("200 TRACKS" in the toolbar). A look down the lanes, then "make all the strings quieter". Claude called `list_session { what: "tracks", offset: 140, limit: 60 }` once (chip **List session**; its first words: "Strings are probably in the 60 tracks I can't see, so I'll list those first."), then `gain { db: -6 }` on tracks 147 to 158, twelve calls (chips **Gain**, each with a node divider), and replied that it had lowered all 12 string tracks (Cello a/b, Contrabass a/b, Viola a/b, Violin I a to c, Violin II a to c) by 6 dB, assumed those were the strings and changed no other track. The story asserts the twelve strings are at -6 dB and no other track moved | look down the lanes 1.7 to 5.1; typed 6.1 to 11.8; sent 11.8; List session chip 14.1; twelve Gain chips 16.1 to 20.3; reply done 22.5; scrolls to the strings 26.9 to 29.5, where the lanes read -6.0 dB |
| `s5-runtime-bundled.mp4` | 12.3 s | **S5. The ONNX Runtime is bundled.** Settings, Account tab; the last line reads **ONNX Runtime: 1.23.2 (bundled)** with the library's path under it (`/tmp/edytlab-v0.7.0/usr/lib/edytlab/libonnxruntime.so`, where the `.deb` was unpacked). The provider on this machine is Ollama (no key is involved); the dialog also shows the machine's "no Secret Service" note above the provider list, which the edit keeps out of frame. Groundwork only: no model, no inference | gear pressed 1.5; dialog opens 3.9 to 4.3; the line is in view throughout |
| `s6-skills-note.mp4` | 17.7 s | **S6. Skills update (#574).** Settings, Skills tab: the eight bundled skills, installed at launch into the (fresh) home folder, and in the empty pane "Skills that come with edytlab are updated when a release changes them, unless you have edited them." Then `loudness-master` is opened | pointer travels to the Skills tab from 5.3; its content (the note) shows from 7.2; the skill opens at 13.0 |
| `s7-mix-stays-playable.mp4` + `.wav` | 21.5 s | **S7. The mix stays playable (#607).** Play; to the Graph tab; back to the Timeline; Play again. While the Graph tab is open the mix's file is removed from the project's preview cache by hand (`.audiograph/previews/<hash>.wav`); on the way back the app renders the mix again, writes the same file, loads it and plays it with no error (the story asserts the file is gone, is written again and `timeline-mix-error` never shows) | Play pressed 2.8; sound 4.7 to 9.3; Graph tab 9.3 (the player goes with the timeline, so the sound stops); back on the timeline 11.7; second Play sound 13.55 to 18.85 |
| `s8a-broll-timeline.mp4` | 22.6 s | **S8a. B-roll.** Six tracks and five labels (intro, verse, chorus, bridge, outro, added with the label lane's double-click). Two zoom presses, a slow pan along the piece (the page's own smooth scroll of the timeline's scrollbar), then down the lanes | zoom 1.5 to 3; pan 8.0 to 15.2; lanes 17.7 to 21.3 |
| `s8b-broll-graph.mp4` | 20.4 s | **S8b. B-roll.** The History graph with its branches (20 nodes): zoomed a little with the wheel, then dragged | wheel 1.5 to 3; drag 5.6 to 12.5 |

S8a and S8b are backgrounds: slow, nothing to read.

## Re-taking a clip

`../record/README.md`. In short: `apps/desktop/e2e-native/stories/promo.mjs` records, `../record/make-footage.mjs` cuts.
A clip whose capture fell behind, or whose story failed, is not used; it is recorded again.
