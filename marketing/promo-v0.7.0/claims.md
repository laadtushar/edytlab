# Claim ledger: what the v0.7.0 promo footage shows, and where each claim is true

Every claim the footage in `footage/` makes gets a row here. Anything the edit puts on screen over this footage (titles, chips,
callouts, the end card) needs a row of its own, and the edit may claim no more than the row allows.

The footage is the **released v0.7.0 app** (the `.deb`'s binary, built from tag `v0.7.0`), filmed live, in real time, with
real input. Where an assistant is in the shot (S1, S3, S4) it is **real Claude** (`claude-sonnet-5-5`, effort `low`) through the
app's own Anthropic provider; the replies and tool calls below are quoted from what was recorded
(`footage/<clip>.notes.json`). Nothing is scripted. The first version of this promo (#644) was scripted; it is in
`../archive/promo-v0.7.0/2026-10-22-scripted/`. The note under every app shot reads **v0.7.0 · real app · real Claude
(Sonnet 5.5)** where the assistant is in the shot, and **v0.7.0 · real app · recorded live** where it is not (S2, S5 to S8).

## What is prepared, synthetic or added by the recording

| Thing | What it is | Where it is written |
|---|---|---|
| The app | The released v0.7.0 build, unchanged, run under Xvfb and WebKitGTK and driven by tauri-driver (WebDriver) and X input events | `apps/desktop/e2e-native/` |
| The assistant | Real Claude Sonnet 5.5 at effort `low`, through the app's Anthropic provider, whose base URL is a local metering proxy that forwards to api.anthropic.com. The tool calls and replies are the model's, unedited | `claude-proxy.mjs`; `footage/README.md` |
| All audio | Synthesised in code: drums, bass, pad, keys, lead and a formant "vocal", a 16-second A-minor loop at 120 BPM. Nothing sampled, recorded or downloaded. The footage never says it is anyone's music | `apps/desktop/e2e-native/stories/promo-audio.mjs` |
| The sound in S1 and S7 | The app's own playback of its rendered mix, captured from the audio server's monitor of the sink the app plays into, and lined up with the picture by a flash-and-tone beacon at the start of each recording | `record/make-footage.mjs`, `record/README.md` |
| Project contents | Track names and files (the synthetic stems), the 200 track names of S4 (invented), the labels of S8a. S3's history (20 nodes) was made before filming with the app's own lane controls and Ctrl+Z, not by the assistant | `stories/promo.mjs` |
| The pointer | The real X pointer, drawn into the recording by x11grab | |
| Time | Real time, at 15 fps. The edit speeds up some stretches (typing, and waits for the model) and says so with an "N× speed" badge whenever the rate is 1.45 or more | `remotion/src/cuts.ts` |

## S1. Describe it, it happens (`s1-describe-it-happens.mp4`, with `.wav`)

| On screen | Where it is true |
|---|---|
| A plain-English request typed into the composer: "Fade the backing in over the first 4 seconds and bring the vocal down 3 dB" | The composer is the real one (`Chat.tsx`); the text is typed with real key events and sent with Enter. Claude's answer to it is what follows |
| Two tool chips, **Fade** then **Gain** | Claude called `fade { track: 0, kind: "in", range: { start_sec: 0, end_sec: 4 } }` and `gain { track: 1, db: -3 }` (recorded in `notes.json`); the labels are the app's (`ToolBadge.tsx` `TOOL_LABELS`, held to the registered tools by `crates/tools/tests/tool_badge_labels.rs`). Claude chose `gain` (additive; the track was at 0 dB) rather than `set_track_gain`; the tool answered "Applied -3 dB gain to track 1 (now -3 dB)" |
| A node divider under each chip | `agent://node-created` before `tool-call-end` (`events.rs`) |
| The backing track fades in over the first 4 seconds | `fade` is a linear ramp, destructive, written to `.audiograph/derived/` (`crates/tools/src/tool/fade.rs`). The lane is the app's own redraw of the new file, labelled "Backing" as for any derived audio |
| The vocal lane reads **-3.0 dB** | The lane's own readout (`Timeline.tsx`) after `gain`; the story asserts the session holds -3 dB on track 1 |
| The assistant's reply: "I made both changes: Backing (track 0): it now fades in over the first 4 seconds. Vocal (track 1): its gain is now -3 dB. I haven't rendered or exported anything yet. Tell me if you want that as the next step." | What Claude wrote, as recorded |
| Play; the playhead runs while the mix plays | The transport plays `render_preview`'s file in an `<audio>` element and the lanes follow it (CLAUDE.md "Playback runs in the webview"). Play renders the mix first (#448), which is the spinner. The sound is the app's own playback of the synthetic stems |
| **V0.7.0** in the status bar | `apps/desktop/src-tauri/tauri.conf.json` version 0.7.0, shown from `__APP_VERSION__` |

## S2. Faders that stay put (`s2-faders-stay-put.mp4`)

| On screen | Where it is true |
|---|---|
| A click on a lane's gain slider, then four ArrowRight taps; the readout steps +0.5, +1.0, +1.5, +2.0 dB and never goes back | #596, fixed by #626 (`lib/ownedControls.ts`; CLAUDE.md "A lane control the user has set ... is the user's until the session confirms it"). The slider and the key events are real, the backend's timing is the app's own: nothing is arranged, held or delayed. The story records every change of the readout (`notes.json`, `readouts`) and fails if one goes down, and asserts the session holds +2.0 dB |

What S2 does not claim: that the bug showed in every webview, or how often, or that this shot reproduces the failing order (the first version of this shot arranged that order on purpose; this one shows only that, in the real app, taps in sequence land where they were put). `e2e/lane-readback.spec.ts` is the test that holds the failing order.

## S3. The History graph keeps up (`s3-history-graph.mp4`)

| On screen | Where it is true |
|---|---|
| The Graph tab open beside the chat: 20 nodes, two abandoned tries and a line of edits | The real `GraphView` (react-flow, dagre layout) drawing a history made by real edits before filming: three loads, two abandoned tries (made, then undone with Ctrl+Z) and twelve edits on the line that stays. The tool names on the bubbles (`set_track_gain`, `set_pan`) are the ops the lane controls write |
| "Name this version chorus": the **Rename version** chip (`name_node`), then the head's bubble reads **chorus** with nothing else done | Claude called `name_node` with the head's id and the label "chorus", and said: "I labelled the current version (node f5206c71…605e) "chorus". I only changed the label, not the audio." #602, fixed by #625: `name_node` moves no head and makes no node, so no `node-created` follows; `useSession` counts `agent://done` as a reason to read the graph again (`graphVersion`; CLAUDE.md "The History graph reads again on ..."; `App.graphRefresh.test.tsx`). The story asserts the head did not move and a bubble reads "chorus" |
| "Compact the history, keep the last 8 edits": two **Compact session** chips (`compact_session { keep_last: 8 }`, a dry run, then `{ apply: true, keep_last: 8 }`), and the nodes leave the graph (20 become 8) | Claude made both calls in one turn (recorded). The dry run answered "Would remove 12 node(s) of history and 0.0 MB of audio, keeping the last 8 on the current chain", the second "Removed 12 node(s) of history". What a compaction removes is `plan_compaction` (`crates/tools/src/reclaim.rs`): everything off the head's chain, and the chain beyond the last `keep_last` nodes. #602/#625 again: `forgetRemoved` runs when `compact_session` ends and the graph is read again. The story asserts the bubble count went from 20 to 8 |
| Claude's reply: "I compacted the history. It removed 12 old nodes and kept the last 8 edits on the current chain, so recent undo still works. It freed no disk space (0.0 MB), because none of those nodes held audio files of their own. The removal is permanent ... I can check and re-label the current head if you like." | What Claude wrote, as recorded. The "chorus" bubble is still in the graph: it is the head |

Claude's behaviour here varied between takes (one answered the dry run with a question and waited for "Yes, go ahead."; one applied at once); the footage is a take where it did both calls in one turn. Nothing was typed to make it.

## S4. Big sessions (`s4-200-tracks.mp4`)

| On screen | Where it is true |
|---|---|
| **200 TRACKS** in the toolbar | 200 tracks loaded through the Open Audio chooser (names invented, files short copies of the synthetic stems) |
| "make all the strings quieter"; a **List session** chip, then twelve **Gain** chips | #618, PR #634: `list_session` pages through tracks, buses and markers the session block left out (`crates/tools/src/tool/list_session.rs`). The block has 8,000 bytes (`SESSION_BLOCK_MAX_BYTES`, `crates/ai/src/session_context.rs`) and a track line of these names is about 55 bytes (`crates/tools/src/session_lines.rs`), so the lines of the last tracks are cut; the cut block says how many it left out and names `list_session`. The strings are tracks 147 to 158 (0-based, as the app numbers them), past the cut. Claude's first words: "Strings are probably in the 60 tracks I can't see, so I'll list those first.", then one `list_session { what: "tracks", offset: 140, limit: 60 }`, then `gain { db: -6 }` on each of the twelve (recorded). One page, not two, as the recording shows |
| Thirteen calls in one turn | The agent allows 20 a turn (`MAX_TOOL_CALLS_PER_TURN`, `crates/ai/src/prompt.rs`) |
| The twelve string lanes read **-6.0 dB** | `gain { track: i, db: -6 }` for each of the twelve; the readouts are the lanes' own. The story asserts all twelve strings are below -0.5 dB and no other track moved |
| Claude's reply: "... I lowered all 12 string tracks by 6 dB. They are Cello a/b, Contrabass a/b, Viola a/b, Violin I a–c and Violin II a–c (tracks 147–158). I assumed those were the strings and didn't change any other tracks. If you want a different amount, tell me and I'll set it." | What Claude wrote, as recorded. It picked 6 dB itself ("quieter" named no amount) |

## S5. Runtime bundled (`s5-runtime-bundled.mp4`)

| On screen | Where it is true |
|---|---|
| Settings, Account tab, last line: **ONNX Runtime: 1.23.2 (bundled)**, and a path under it | `lib/mlRuntimeLine.ts` writes `ONNX Runtime: <version> (bundled)` for `state: "found", source: "bundled"`, the answer `ml_runtime_status` gave for the library beside the release's binary (`<unpacked .deb>/usr/lib/edytlab/libonnxruntime.so`). 1.23.2 is the pin in `src-tauri/ml-runtime.json`. Every release installer after v0.6.1 carries it (#383, PR #615; CLAUDE.md "Repo specifics") |
| The path `/tmp/edytlab-v0.7.0/usr/lib/edytlab/libonnxruntime.so` | Where the `.deb` was unpacked for this recording. It says the runtime was found in the bundle's resource directory, not that any installer unpacks there |

What S5 does not claim: stem separation or transcription. No model ships and no inference runs (CLAUDE.md, "Demucs and Whisper ... are not shipped"; #383-#385). The edit says: **groundwork only: no model yet**. The crop keeps the dialog's provider list and the machine's "no Secret Service" note out of frame; in the wider crops the model field below them (the provider on this machine is Ollama, model `llama3.2`) is visible, and nothing is claimed about it.

## S6. Skills update (`s6-skills-note.mp4`)

| On screen | Where it is true |
|---|---|
| Settings, Skills tab; the empty pane reads "Skills that come with edytlab are updated when a release changes them, unless you have edited them." | `SkillsEditor.tsx` (`skills-bundled-note`), added with #574 / PR #631 (`bundled_skills.rs`: a bundled skill is replaced in an existing install unless the user edited it, in which case the new text is written beside it as `<name>.bundled.md`). The story asserts the note's text |
| Eight skills in the list, with their triggers | The eight files in `src-tauri/resources/skills/`, installed at launch into the app's home (`dialog-enhancer`, `export-guide`, `loudness-master`, `music-mix`, `noise-reducer`, `podcast-cleanup`, `silence-cleaner`, `vocal-chain`) |
| `loudness-master` opened in the editor | `read_skill` for that file; the text is the bundled file's |

## S7. Mix stays playable (`s7-mix-stays-playable.mp4`, with `.wav`)

| On screen | Where it is true |
|---|---|
| Play; to the Graph tab and back; Play again, with no error | #607, PR #635: the window's own render is held in the preview cache (`window_mix.rs`), and a mix whose file is gone is rendered again once (`Timeline.tsx` `onMixMissing`, `lib/mixState.ts`). `e2e/mix-evicted.spec.ts` ("is rendered again on the way back to the timeline, and plays") holds the same |
| What the shot arranges | While the Graph tab is open the mix's file is deleted from the project's preview cache by the story (as a cache cleared by hand would), so the load on the way back finds nothing; the app renders the mix again and the file is back (the story asserts all three, and that `timeline-mix-error` never shows) |
| The sound | The app's own playback: the first Play stops when the Graph tab replaces the timeline and its player, the second runs until the pause |

What S7 does not claim: that the cache evicts the mix often. The real fix is the held entry; this shot shows the second line of defence. (In one recording of this shot, made while the machine was busy, the Play button stayed disabled after the file was removed and the story failed its wait; the next takes passed. The footage is a passing take. The failed take is not used, and is not evidence about #607 either way.)

## S8. B-roll (`s8a-broll-timeline.mp4`, `s8b-broll-graph.mp4`)

| On screen | Where it is true |
|---|---|
| Six tracks, zoomed in and panned from the start of the piece to its end, then down the lanes; labels **intro, verse, chorus, bridge, outro** | The real timeline and label lane: labels added with the lane's double-click and a typed name. The tracks are synthetic audio |
| The History graph with branches, zoomed and dragged | The real `GraphView` on the 20-node history made as for S3. Nothing is claimed about a particular project |

## The edit (phase 2): what Remotion draws over the footage, and where each claim is true

Everything below is drawn by the Remotion project (`remotion/src/cuts.ts` for the words, `remotion/src/shots.ts` for the callouts;
`remotion/scripts/cues.ts` turns the same data into the `.srt` files). Remotion draws only titles, chips, rings, labels, the
"speed" badge, the progress line and the end card. **No app UI is drawn**: every pixel of app is the footage above, cropped and
zoomed. The edit shows no issue or pull-request numbers: an earlier render of it put the number of the fix in a corner chip on
five shots (#596, #602, #618, #574, #607) and "(#596)" in a sub line, and viewers could not tell what they meant, so they were
removed (those numbers stay in this ledger as the sources of the claims). Every shot of app footage carries the note named at the top (`HonestyNote`), and the edit checks in code that the words
it adds lie inside Meta's 9:16 safe zone (`remotion/src/components/SafeCheck.tsx`). The edit says nothing about stem separation
or transcription.

| Words on screen | Where it is true |
|---|---|
| **v0.7.0 · real app · real Claude (Sonnet 5.5)** (shots of S1, S3, S4) and **v0.7.0 · real app · recorded live** (S2, S5 to S8) | The footage README's honesty note: the app is the released build; the assistant is real Claude Sonnet 5.5 through the app's provider (`claude-sonnet-5-5`); the other shots use no assistant |
| **N× speed** (R1 and V1's S3 and S4 stretches, R2's and V1's typing and waiting stretches) | The stretch of footage under it plays at that rate; the footage itself is real time. Shown whenever the rate is 1.45 or more. Everything else plays at 1× |
| **What's new in v0.7.0** / **Five changes you can see** | R1 shows five changes: faders (#596), the history graph (#602), big sessions (#618), the mix (#607), skills (#574) |
| **Faders stay where you put them** / **Four key taps, four steps**; labels **gain slider**, **+0.5 dB**, **+1.0 dB**, **+1.5 dB**, **+2.0 dB, no bounce back** | S2 above: #596, fixed by #626; the readout steps 0.5, 1.0, 1.5, 2.0 and never goes back. The labels are the readout's own values at those moments (5.07, 5.60, 6.13, 6.67 s). "No bounce back" is the behaviour in the real app, in order, not a claim about every webview |
| **The history graph keeps up** / **Names and clean-ups show at once**; labels **named at once**, **20 nodes became 8** | S3 above: #602, fixed by #625; the 20 and the 8 are this history's own counts (bubbles in the graph before and after) |
| **Every edit is a node in the history graph** (V1, over the B-roll graph) | README "Branchable session graph" and the app's first-run text, "saves each change as a node in the graph" (visible in the S8 footage) |
| **Name a version: it shows at once** / **Compact the history: nodes leave at once** | S3 above, the two requests the footage shows, each typed in plain words and carried out by Claude |
| **200 tracks? Just ask.** / **200 tracks?** / **Ask.** / **Just say it** / **“make all the strings quieter”** | S4: the toolbar reads 200 TRACKS; the request is the one typed in the footage and Claude carried it out |
| **list_session pages past the summary**, **It reads what the summary left out**, **list_session reads what the summary left out**, **list_session, then gain**, label **list_session** | S4 above: #618 / PR #634 (`list_session` lists what the session block left out; the block is cut at 8,000 bytes). Claude called it once, with `offset: 140` |
| **Twelve strings, one sentence**, **Twelve strings, 6 dB quieter**, **All twelve strings 6 dB quieter**, **Violins, viola, cello, contrabass**, label **-6.0 dB on the strings** | S4 above: twelve `gain { db: -6 }` calls, tracks 147 to 158 (Violin I and II, Viola, Cello, Contrabass); the lanes read -6.0 dB. Claude chose the 6 dB |
| **Each change is a node on the graph** | S4: each Gain chip has a node divider under it. Not said of `list_session`, which reads and makes no node |
| **Switch tabs. It still plays.** / **Switch tabs, press play: it still plays** / **Even when the mix is gone from the cache**; labels **back to the timeline**, **plays, no error** | S7 above: #607 / PR #635; the story deletes the mix's file from the preview cache while the Graph tab is open and asserts it is rendered again and plays with no error. It does not claim the cache evicts often |
| **Bundled skills stay current** / **Bundled skills update, unless you edited them** / **Updated when a release changes them**; label **unless you edited them** | S6 above: the app's own note (`SkillsEditor.tsx`, #574 / PR #631) |
| **The ONNX Runtime now ships in every installer**; chip **groundwork only: no model yet** | S5 above and CLAUDE.md "Repo specifics": every release installer after v0.6.1 carries ONNX Runtime 1.23.2 (#383, PR #615), so v0.7.0's do, and the app, run from the release's own `.deb`, found it in the bundle and said so. No model ships and no inference runs, which the chip says |
| **Describe the edit.** / **Describe the edit in plain English** / **Say it in plain English** / **Typed into the chat box** / **Describe the edit. It happens.** | S1 above: a plain-English request typed into the real composer, and real Claude carried it out |
| **The assistant calls the tools** / **The assistant calls the tools, the timeline updates** / **fade, then gain**; labels **fade**, **gain** | S1 above: the Fade and Gain chips are `fade` and `gain` (`ToolBadge.tsx` labels, `tool_badge_labels.rs`); Claude made the calls |
| **The timeline changes**; labels **fades in over 4 s**, **-3.0 dB** | S1 above: the linear 4 s ramp from `fade { kind: "in" }` and the Vocal lane's -3.0 dB readout after `gain { db: -3 }` |
| **Then hear it** / **Press play and hear it** / **A synthetic demo track**; chip **demo audio, made in code**; label **hear the result** | S1 above: the sound is the app's playback of synthetic stems written from arithmetic (`promo-audio.mjs`). Said so on screen |
| End card: **edytlab**, **v0.7.0**, **edytlab.com**, **Free and open source (MIT)**, **macOS · Windows · Linux**, **Unsigned installers** | `LICENSE` (MIT); README (macOS universal, Windows, Linux builds); the installers are unsigned (CLAUDE.md, release workflows); <https://edytlab.com> |

### Sound

| Sound | Where it comes from |
|---|---|
| The music under S1's Play and S7's plays | The app's own audio in the footage (`s1-describe-it-happens.wav`, `s7-mix-stays-playable.wav`), laid at the moments of the footage it belongs to, and silent where the footage is silent (`remotion/scripts/cues.ts` fails the build if a stretch that does not play the clip's sound has any). Synthetic: no sample, no recording, no one's music |
| Whoosh on a scene change, tick when a label lands or a tool call resolves, chime under the wordmark | Synthesised in code by `marketing/launch-video/tools/sfx.py`; the tick is played at several pitches for the run of twelve chips. Nothing else: no stock music, no voice, no ElevenLabs |

## Not shown, and not claimed

- Stem separation and transcription: not shown. They are not shipped.
- Signed installers: not shown. Installers are unsigned.
- Any use other than editing: not shown. It is an offline editor with an AI assistant.
- That Claude always answers this way: the shots show one take of each; its answers vary (S3 above says how).
- A provider's price, a user count: not shown. The model's name appears once, in the note, as the model that was used.
- The tool count: not shown on the footage. (There are 94 tools, `docs/tools-reference.md`.)
