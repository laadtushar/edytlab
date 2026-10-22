# Claim ledger: what the v0.7.0 promo footage shows, and where each claim is true

Every claim the footage in `footage/` makes gets a row here. Anything the edit puts on screen over this footage
(titles, chips, callouts, the end card) needs a row of its own, and the edit may claim no more than the row allows.
Started with the footage (phase 1); the edit adds its own rows.

The footage is the real frontend at commit `23bbfe2e` (origin/main after v0.7.0 and #638), with only the IPC
boundary replaced, as the e2e suite runs it (`apps/desktop/e2e/`). **The assistant is scripted**: there is no API key, so
the replies and the tool calls are raised by `record/lib/studio.ts`, not produced by a model. The note under every app
shot reads: **v0.7.0 · real app · scripted assistant**.

## What is scripted, synthetic or added by the recording

| Thing | What it is | Where it is written |
|---|---|---|
| The assistant's words and tool calls | Scripted. Tool names and arguments are the real ones (`docs/tools-reference.md`); the events are raised in the order `commands.rs` `emit_agent_event` emits them (`tool-call`, then `node-created`, then `tool-call-end`, then `done`) | `record/lib/studio.ts` (`agentTurn`), and each `record/sN-*.shot.ts` |
| The Rust store | Replaced by a `Studio` that keeps nodes, head and tracks and answers `list_tracks`, `get_graph`, `render_preview` and the rest in the shapes the real commands return (the way `e2e/backend.ts` already does) | `record/lib/studio.ts` |
| All audio | Synthesised in code: drums, bass, pad, keys, lead and a formant "vocal", a short A-minor loop at 120 BPM. Nothing sampled, recorded or downloaded. The footage never says it is anyone's music | `record/lib/music.ts` |
| The mix that plays (S1, S7) | The sum of the track audio with the edits applied (the linear fade-in from `fade`, the -3 dB track gain), computed in `music.ts`. It is a model of what the engine renders for those edits, not the engine's output | `record/lib/music.ts`, `s1-*.shot.ts` |
| The `.wav` beside S1 and S7 | The mix as the page played it, placed on the footage's clock (silence until Play, silence after it stops) | `record/lib/media.ts` |
| Project contents | Track names, marker names, the history of edits and the 200-track template are invented for the shots | `record/lib/history.ts`, `lib/orchestra.ts`, the shots |
| The pointer and the click ring | Drawn by the recording on top of the page (a capture of a page has no OS pointer). They are not part of the app | `record/lib/human.ts` |
| Time | The page's clock, animations and media position are stepped 1/30 s per frame so a 3200x2000 capture (0.3 to 0.4 s each) is exactly 30 fps. The app's code, styles and IPC calls are unchanged | `record/lib/kit.ts`, `lib/stage.ts` |

## S1. Describe it, it happens (`s1-describe-it-happens.mp4`, with `.wav`)

| On screen | Where it is true |
|---|---|
| A plain-English request typed into the composer: "Fade the intro in over 4 seconds and bring the vocal down 3 dB" | The composer is the real one (`Chat.tsx`); the recording types into it and presses Enter. That it can be typed is the claim; that a model would answer it this way is not (the answer is scripted) |
| Two tool chips, **Fade** then **Track volume** | The labels the app gives `fade` and `set_track_gain` (`ToolBadge.tsx` `TOOL_LABELS`, held to the registered tools by `crates/tools/tests/tool_badge_labels.rs`). The scripted calls are `fade { track: 0, kind: "in", range: { start_sec: 0, end_sec: 4 } }` and `set_track_gain { track: 1, db: -3 }` |
| A node divider under each chip | `agent://node-created` before `tool-call-end` (`events.rs`: "sent once per call that moved it, before that call's `tool-call-end`") |
| The backing track fades in over the first 4 seconds | `fade` is a linear ramp, destructive, written to `.audiograph/derived/` (`crates/tools/src/tool/fade.rs`, `apply_fade`, `Kind::In`: gain = t). The lane is drawn from a file made by the same ramp (`music.ts`, `fadeIn`) and labelled with the track's name, as `lib/derivedAudio.ts` does for derived audio |
| The vocal lane reads **-3.0 dB** | `set_track_gain` is absolute ("Set a track's gain in decibels (absolute, not additive)", `docs/tools-reference.md`); the readout is the lane's own (`Timeline.tsx`) |
| The assistant's reply: "Done. The backing now fades in over the first 4 seconds, and the vocal is 3 dB quieter." | Scripted. It says only what the two calls did |
| Play; the playhead runs while the mix plays | The transport plays `render_preview`'s file in an `<audio>` element and the lanes follow it (`Timeline.tsx`; CLAUDE.md "Playback runs in the webview"). Here the page's media position is stepped with the page clock (see above), and the sound is the `.wav`, laid on the same clock |
| **V0.7.0** in the status bar | `apps/desktop/src-tauri/tauri.conf.json` version 0.7.0, shown from `__APP_VERSION__` (`version.spec.ts`) |

## S2. Faders that stay put (`s2-faders-stay-put.mp4`)

| On screen | Where it is true |
|---|---|
| Four ArrowRight taps on a lane's gain slider; the readout steps +0.5, +1.0, +1.5, +2.0 dB and never goes back | #596, fixed by #626 (`lib/ownedControls.ts`; CLAUDE.md "A lane control the user has set ... is the user's until the session confirms it"). `e2e/lane-readback.spec.ts` ("a key tap is not lost when the read-back of the tap before lands before its key-up") holds the same order with real key events. The slider and the keys are real; the backend's timing is scripted |
| What the shot arranges | Each tap's read-back of the track list is held and let go 100 ms into the next tap, after its key-down and before its key-up: the order that put the slider back one step before #626. The same shot on the code before #626 shows the bounce (see `footage/README.md`, "S2 checked against the code before the fix") |
| `set_track_gain` is sent four times, once per tap | `notes.json` records the calls: `{ track: 0, gainDb: 0.5 }`, 1, 1.5, 2, and the shot fails if they differ |

What S2 does not claim: that the bug showed in every webview, or how often. It shows the fixed behaviour in the failing order.

## S3. The History graph keeps up (`s3-history-graph.mp4`)

| On screen | Where it is true |
|---|---|
| The Graph tab open beside the chat | The real `GraphView` (react-flow, dagre layout) drawing the scripted history: 17 nodes, two abandoned tries and a line of edits. Tool names on the bubbles are real tools; the history is invented |
| "Name this version chorus": the **Rename version** chip (`name_node`), then the head's bubble reads **chorus** with nothing else done | #602, fixed by #625: `name_node` moves no head and makes no node, so no `node-created` follows; `useSession` counts `agent://done` as a reason to read the graph again (`graphVersion`; CLAUDE.md, "The History graph reads again on ..."; `App.graphRefresh.test.tsx`) |
| "Compact the history, keep the last 8 edits": two **Compact session** chips (a dry run, then `apply: true`), and the removed nodes leave the graph (17 nodes become 8) | #602/#625 again: `forgetRemoved` runs when `compact_session` ends and the graph is read again. What a compaction removes is `plan_compaction` (`crates/tools/src/reclaim.rs`): everything off the head's chain, and the chain beyond the last `keep_last` nodes, whose oldest kept node becomes a root. `lib/history.ts` `compact` does the same to the scripted history, so 9 nodes go: 4 of the line's tail and 5 abandoned |
| "Removed 9 nodes of history. The last 8 on the current chain are untouched." | Scripted. The 9 and the 8 are this history's, from the `compact` above, not a store's |

## S4. Big sessions (`s4-200-tracks.mp4`)

| On screen | Where it is true |
|---|---|
| **200 TRACKS** in the toolbar | 200 tracks in the scripted session (`lib/orchestra.ts`; the names are invented) |
| "make all the strings quieter"; two **List session** chips, then twelve **Track volume** chips | #618, PR #634: `list_session` pages through tracks, buses and markers the session block left out (`crates/tools/src/tool/list_session.rs`; a page is at most 100 items or 4,096 bytes, `PAGE_MAX_ITEMS`, `PAGE_MAX_BYTES`). The block has 8,000 bytes (`SESSION_BLOCK_MAX_BYTES`, `crates/ai/src/session_context.rs`) and a track line of these names is about 55 bytes (`crates/tools/src/session_lines.rs`, `track_line`), so even if the fixed parts took no bytes at all only 146 of the 200 track lines fit (computed over this session's names); the strings are tracks 148 to 159, past that cut. The cut block says it, and names `list_session`. Which calls a model would make is scripted |
| Fourteen calls in one turn | The agent allows 20 a turn (`MAX_TOOL_CALLS_PER_TURN`, `crates/ai/src/prompt.rs`) |
| The twelve string lanes read **-4.0 dB** | `set_track_gain { track: i, db: -4 }` for each of the twelve; the readouts are the lanes' own |
| "Done. All 12 string tracks (Violin I and II, Viola, Cello, Contrabass) are now 4 dB quieter." | Scripted. It says what the calls did |

## S5. Runtime bundled (`s5-runtime-bundled.mp4`)

| On screen | Where it is true |
|---|---|
| Settings, Account tab, last line: **ONNX Runtime: 1.23.2 (bundled)**, and a path under it | `lib/mlRuntimeLine.ts` writes `ONNX Runtime: <version> (bundled)` for `state: "found", source: "bundled"`. The answer is the shape `ml_runtime_status` returns (`MlRuntimeStatusDto::Found`, `commands.rs`; the Rust test `a_found_runtime_is_reported_with_its_version_path_and_source` pins `{ state, version, path, source: "bundled" }`). 1.23.2 is the pin in `src-tauri/ml-runtime.json`. Every release installer after v0.6.1 carries it (#383, PR #615; CLAUDE.md "Repo specifics") |
| The path `/usr/lib/edytlab/libonnxruntime.so` | An example: the place a .deb install's resource directory puts the library (the first place `e2e/backend.ts` already lists for a Linux install). The footage does not say it is any particular installer's |
| The model list in the dialog | Scripted as the provider's own list (`source: "live"`), the same four ids `e2e/backend.ts` carries, because the user in the shot has a key saved |

What S5 does not claim: stem separation or transcription. No model ships and no inference runs (CLAUDE.md, "Demucs and Whisper ... are not shipped"; #383-#385). If the edit mentions the line at all it says: **groundwork: the ONNX Runtime now ships in every installer**.

## S6. Skills update (`s6-skills-note.mp4`)

| On screen | Where it is true |
|---|---|
| Settings, Skills tab; the empty pane reads "Skills that come with edytlab are updated when a release changes them, unless you have edited them." | `SkillsEditor.tsx` (`skills-bundled-note`), added with #574 / PR #631 (`bundled_skills.rs`: a bundled skill is replaced in an existing install unless the user edited it, in which case the new text is written beside it as `<name>.bundled.md`) |
| Eight skills in the list, with their triggers | The eight files in `src-tauri/resources/skills/` (`dialog-enhancer`, `export-guide`, `loudness-master`, `music-mix`, `noise-reducer`, `podcast-cleanup`, `silence-cleaner`, `vocal-chain`), answered as `list_skills` answers them (`lib/skills.ts`) |
| `loudness-master` opened in the editor | `read_skill` for that file; the text is the bundled file's |

## S7. Mix stays playable (`s7-mix-stays-playable.mp4`, with `.wav`)

| On screen | Where it is true |
|---|---|
| Play; to the Graph tab and back; Play again, with no error | #607, PR #635: the window's own render is held in the preview cache (`window_mix.rs`), and a mix whose file is gone is rendered again once (`Timeline.tsx` `onMixMissing`, `lib/mixState.ts`). `e2e/mix-evicted.spec.ts` ("is rendered again on the way back to the timeline, and plays") |
| What the shot arranges | While the Graph tab is open the mix's file is removed (as a cache cleared by hand would), so the load on the way back gets a 404; the app asks `render_preview` again, once, and the scripted backend writes the file and answers. The shot asserts exactly two `render_preview` calls (one for the first Play, one for the missing file) and no "Could not load the mix" |
| The sound | The `.wav`: the first Play stops when the Graph tab replaces the timeline and its player (the page's `pause`, as the HTML spec has it for an element taken out of the document), the second runs until the pause |

What S7 does not claim: that the cache evicts the mix often. The real fix is the held entry; this shot shows the second line of defence.

## S8. B-roll (`s8a-broll-timeline.mp4`, `s8b-broll-graph.mp4`)

| On screen | Where it is true |
|---|---|
| Six tracks, zoomed in and panned from the start of the piece to its end, then down the lanes; labels **intro, verse, chorus, bridge, outro** over them | The real timeline and label lane. The tracks are synthetic audio and the labels are scripted (`list_markers`) |
| The History graph with branches, zoomed and dragged | The real `GraphView` on the scripted 17-node history. Nothing is claimed about a particular project |

## Not shown, and not claimed

- Stem separation and transcription: not shown. They are not shipped.
- Signed installers: not shown. Installers are unsigned.
- A live or DJ use: not shown. It is an offline editor.
- A model's name, a provider's name, a user count, a price: not shown.
- The tool count: not shown on the footage. (There are 94 tools, `docs/tools-reference.md`.)
