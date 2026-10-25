import type { Metadata } from "next";
import { Fragment } from "react";

import { LegalShell } from "@/components/landing/legal-shell";
import { pageSocial } from "@/lib/seo";

const title = "Changelog: release notes for every version";
const description =
  "What changed in each edytlab release, newest first: new audio tools, fixes and the limits that remain. Installers for every version are on GitHub Releases.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/changelog" },
  ...pageSocial("/changelog", `${title} · edytlab`, description),
};

interface Entry {
  version: string;
  date: string;
  summary: string;
  bullets: string[];
}

// Static, updated by hand as releases land. One line per change; the full
// list of binaries and tags is on GitHub Releases.
const entries: Entry[] = [
  {
    version: "v0.9.1",
    date: "2026-10",
    summary: "Fixes the first message after signing in with ChatGPT.",
    bullets: [
      "Fixed: in v0.9.0 the first message after Sign in with ChatGPT (preview) failed with “Unsupported content type”.",
      "Changed: a similar OpenAI refusal now shows a ChatGPT plan card with a copyable request ID.",
    ],
  },
  {
    version: "v0.9.0",
    date: "2026-10",
    summary: "Sign in with ChatGPT (preview), plus fixes to noise reduction, tempo and key.",
    bullets: [
      "New: Sign in with ChatGPT (preview). The assistant uses your ChatGPT plan's usage instead of an API key.",
      "In Settings, choose ChatGPT plan, then Continue with ChatGPT. Your browser opens OpenAI's sign-in; edytlab never sees your password.",
      "Pick a model and, where offered, a reasoning effort. Sign out, Use another account, Start over and Manage usage are on the same card.",
      "At your plan's limit for apps, the chat says so and stops: no retry, no switch to another provider.",
      "If your session ends mid-reply, Continue signs you in again and resends your message.",
      "Available for ChatGPT Plus and Pro; it may not be available on workspace plans, which depends on OpenAI and your admin.",
      "Your audio stays on your computer; prompts and tool results go to OpenAI as with any provider. A preview of OpenAI's own preview, so expect rough edges.",
      "Fixed: noise reduction put a loud click, up to full scale, at the start and end of a track.",
      "Fixed: hi-hats between beats doubled the tempo (90 BPM read as 179); the beat grid now sits on the kick.",
      "Fixed: a B-flat minor chord read as F major; minor keys are now told from their relative major more reliably.",
      "New: analysis reports its confidence, the half and double tempo and the runner-up key. A track with no beat reports no tempo instead of failing. Sections and downbeats are still estimates.",
      "Fixed: downloading the speech model again left about 136 MB of the old one in memory.",
      "Changed: transcription copies far less data out of the speech engine per word; on our machine that changed memory traffic, not speed.",
    ],
  },
  {
    version: "v0.8.1",
    date: "2026-10",
    summary: "Fixes editing a voice recorded at a different sample rate from the project.",
    bullets: [
      "Fixed: with a track at another rate than the project (16 kHz voice under a 48 kHz bed), nine tools turned seconds into the wrong number of frames.",
      "time_shift moved the track too far (3x for 16 kHz under 48 kHz); split_clip refused valid split points; split_by_speaker dropped later turns.",
      "punch_in refused a take at the track's own rate and spliced in one at another rate.",
      "cut_range, trim and truncate_silence put labels and transcript words in the wrong place.",
      "With sync-lock on, insert_silence and paste_region opened gaps of the wrong length in other tracks.",
      "Changed: tools count in each track's own frames, so the same seconds land in the same place on every track. A track whose clips are at two rates is refused where one number is needed.",
      "Fixed: on such clips a volume envelope, such as a duck under speech, landed at the wrong time, also after a cut or split. Earlier projects play as before.",
      "Fixed: with mixed rates the session length could be a third of the real length, or too long. It is now the length of a full render.",
    ],
  },
  {
    version: "v0.8.0",
    date: "2026-10",
    summary: "Transcription, editing by transcript, and a fix that stops renders overwriting your audio.",
    bullets: [
      "New: transcription. Ask for “transcribe track 1” to get every word with its time, as a new session version.",
      "It runs on your CPU with the Whisper base model (about 139 MB), downloaded once from Settings → Account (Download, Cancel, resume, Remove).",
      "Nothing downloads unless you press Download: not at launch, not when Settings opens, not when the assistant asks. Without the model, the chat shows a card that opens Settings.",
      "Name the language with a two-letter code (en, de, fr, es) or let it detect. A progress strip counts 30-second windows; Cancel stops it.",
      "Speed: seven seconds of audio took about three seconds on our machine; yours depends on its CPU.",
      "Checked on one short synthetic English recording of twelve words: none wrong, every start within 0.23 s. That is one recording, not a range of voices, so listen before you cut.",
      "New: edit by transcript. In the Transcript tab, select words and press Delete to cut that audio and close the gap.",
      "Or ask the assistant to cut a passage, remove filler words (it reports first and changes nothing until you say so), or find a phrase (“select where he talks about latency”).",
      "A transcript remembers its track: cuts land there and another track is refused. Ollama's small request offers these tools too.",
      "Transcribing sends no audio and makes no network request. The words go to your AI provider as a tool result (a bounded amount per call), and stay local with Ollama. Settings says so; the privacy page has details.",
      "Stem separation is still not available.",
      "Fixed: a render could overwrite audio the project reads. batch_apply with the default output_dir and an input's own format left three 12-second WAVs as 44-byte headers; render_final, export_recipe, export_multiple and Export Selection could overwrite a loaded recording.",
      "They now refuse and ask for another name or folder; batch_apply refuses the whole batch first. The check covers every version in the history.",
      "If you are on v0.7.0 or earlier, set output_dir to another folder and keep copies of originals.",
      "Fixed: batch_apply rendered inputs that differ after a dot (take.1.wav, take.2.wav) to one name and reported success. Each keeps its own name; inputs that would share a project folder (take.wav, take.flac) are refused first.",
      "Fixed: after Accept B in A/B compare, or the assistant's edits, lanes kept side A's tracks and a fader you were holding stayed over the assistant's value.",
      "Fixed: the missing-runtime error wrongly said edytlab does not ship ONNX Runtime. Release installers include it; a build from source does not.",
    ],
  },
  {
    version: "v0.7.0",
    date: "2026-10",
    summary: "Installers carry the ONNX Runtime, plus fixes to controls, the graph and large sessions.",
    bullets: [
      "New: installers ship ONNX Runtime 1.23.2 and its notices (plus the Visual C++ runtime on Windows); Settings → Account says bundled. No speech or stem model ships with it, so stem separation and transcription are still unavailable.",
      "Fixed: a fader, pan, mute or solo you set could be put back, or lost, when an earlier change's read-back arrived late.",
      "Fixed: the History graph kept removed nodes until the next edit.",
      "Fixed: switching to the Graph or Transcript tab and back could fail with “Could not load the mix”. It is now kept and re-rendered once if lost.",
      "Changed: fixes and new bundled skills reach existing installs. A skill you edited or deleted is left alone; an edited one gets the new text beside it as <name>.bundled.md.",
      "New: the assistant lists every track, bus and marker of a big session page by page, including on Ollama's small context; the session summary is capped so a big project fits.",
      "Fixed: the assistant guessed effect setting names. It is now told them (a gain's level is db; a limiter has ceiling_db and release_ms).",
      "Changed: audio the app writes is named by sample rate and channel count as well as samples, so different layouts no longer share a file. Earlier projects keep working.",
      "Fixed: a batch run could write into an open project.",
      "Fixed: a project folder restored from backup (or trashed and recreated) opened while the assistant still worked in the old one. It now waits for the turn to end.",
      "Security: updated rustls-webpki and webpki-roots past two published advisories about distrusted certificates.",
    ],
  },
  {
    version: "v0.6.1",
    date: "2026-10",
    summary: "Fixes Save As copies, A/B compare renders and speech analysis stalls.",
    bullets: [
      "Fixed: opening a Save As copy cleaned out its own audio, so deleting or moving the original left the copy silent. Copies now play from their own folder, generated tones and mixdowns included.",
      "A v0.6.0 copy is repaired the first time v0.6.1 opens it while the original is still there. Open it once before deleting the original.",
      "The repair copies needed audio back, overwrites nothing and stops before the disk is nearly full.",
      "On exFAT, FAT and some network drives the repair copies nothing; open those copies from a local disk.",
      "Fixed: an edit or cleanup during Save As could leave the copy naming audio it lacked. The copy is now the project as it was at your click, without cached previews and auditions (rebuilt when needed).",
      "Fixed: A/B compare wrote two full-length WAVs to the system temp folder and never removed them. They now go in the project's preview cache.",
      "Fixed: selecting a spoken passage or ducking music under speech held up your edits. Faders, mute and undo now answer meanwhile.",
      "Changed: selecting a passage can join speech separated by short gaps. When a phrase occurs more than once, the assistant is told which occurrence it found and how many there are.",
      "New: Settings → Account shows whether ONNX Runtime is installed; this build ships none. Stem separation and transcription are still not shipped.",
      "Security: updated the zip library for plugin archives past three published advisories. The app never used the affected functions.",
    ],
  },
  {
    version: "v0.6.0",
    date: "2026-10",
    summary: "Master and bus effects, many undo and project-opening fixes, and a safer history compaction.",
    bullets: [
      "Fixed: compacting history could delete a WAV you loaded from your own folder. It now deletes only audio the project made in its own folder. Earlier releases, v0.5.0 included, have this bug: update before you compact a project.",
      "New: effects on the master bus and on buses. A limiter on the master keeps working as tracks change; it used to render the mix into a new “Master” track.",
      "The assistant can put an effect on a bus, let you hear one before adding it, and knows buses by name.",
      "A bus effect is heard only when a track sends to that bus.",
      "Fixed: a saved effect the app cannot run (such as a reverb in a chain) failed every preview, playback and export. It is now left out, the rest plays, and the window names it.",
      "Only gain, limiter and the low-pass, high-pass and notch filters run in a chain. Reverb, EQ and echo still work as edits to a track's audio.",
      "Changed: the assistant no longer puts an unrunnable effect in a chain, and refuses a setting an effect lacks (“gain_db” for “db”). The command palette no longer suggests a reverb bus.",
      "Fixed: in the installed app, deleting an agent profile, skill or MCP server in Settings ignored Cancel.",
      "Fixed: after Save As, the assistant kept editing the original while the window showed the copy.",
      "Fixed: opening a project from Recent projects or Open project could land a step behind and drop the newest edits.",
      "Fixed: opening a project while the assistant worked on it, including the open one, split it in two, and the cleanup at open could delete audio a tool had just written.",
      "Fixed: undo retraces your steps after a restart (it could skip one, such as a mute you turned off again). Plan first stays on if you left it on; it was off at every launch.",
      "Fixed: after the assistant named a version, made alternative takes in one step or applied a recipe, the window and project disagreed: the next undo went wrong or skipped the new version.",
      "Fixed: compaction could remove a version undo or redo pointed at, so every press failed until a restart. They now step past a missing version.",
      "Fixed: clicking a version in the Graph view moved the window but not the project, so the next undo went to the previewed version. A click now only previews; Set as head still moves you.",
      "Fixed: adding a label on macOS. The name is now typed in a box in the label lane, from the lane or the ruler.",
      "Fixed: dragging a region turned it into a marker. It now moves whole and shows its full length, a click on a label no longer moves it, and labels move with pasted audio.",
      "New: select a spoken passage without a transcript: “the last thing said” on a voice track is found from its audio.",
      "It finds where there is speech, not what is said; finding a phrase still needs a transcript, which this build cannot make.",
      "Fixed: with sync-lock on, a paste lengthened one track and left the rest, so later audio drifted. Other tracks now open the same gap, and one undo restores them.",
      "Changed: the assistant's previews stay in the project, in a size-limited cache. They used to be left in the system temp folder (about 55 MB for five minutes of stereo).",
      "A track loaded from a preview keeps its audio when the cache makes room.",
      "Changed: with Plan first on, previews and effect auditions no longer wait for approval, and a held recipe or batch lists each step in order (it showed only the file path).",
      "Fixed: keychain problems were hidden. On Linux, a key saved after the Secret Service went away lasts only until restart, and Settings says so before it closes. A later-returning service no longer replaces it with an older copy.",
      "If you decline to unlock the keyring at launch, the app asks nothing more until you save in Settings. Save no longer overwrites your chosen model with the default, and the chat says why there is no assistant.",
      "Changed: the app matches the website: violet on zinc instead of amber, the edytlab wordmark, small motions (off when your system asks for reduced motion).",
      "Security: updated rustls past four published advisories.",
      "Stem separation and transcription are still not shipped.",
    ],
  },
  {
    version: "v0.5.0",
    date: "2026-10",
    summary: "Current models by default, plus fixes to chats, plan cards, Ollama and Linux keychain handling.",
    bullets: [
      "Changed: Anthropic's chat model is Claude Sonnet 5.5, its classifier Claude Haiku 5.5, and Settings fetches the model list live. OpenAI, OpenRouter and Gemini defaults move to current models (Gemini's old default was shut down in June).",
      "A saved model its provider has retired now says so and points to Settings, instead of a raw 404.",
      "Fixed: the second message of a chat on a 5.x model could fail with “Invalid signature in thinking block”. Each model's output is also capped at its own limit.",
      "Fixed: edits you make while the assistant works are kept, in order. Faders, mute, markers and undo answer during a long tool such as a time-stretch.",
      "The tool stops if you change a track it is working on.",
      "Fixed: twelve track commands (gain, mute, rename, remove and more) froze the window.",
      "Fixed: plan steps showed only a tool name and could name a tool that does not exist. They now read as sentences with values, unknown tools are dropped, and unshipped tools say “not available in this build”.",
      "Fixed: the request classifier could read a question as an edit and hold it at a plan gate.",
      "Fixed: a turn ending on a tool-argument error made the provider refuse every later message in the chat.",
      "Changed: local models (Ollama) get a core set of tools plus those the message asks for, instead of about 15,000 tokens of descriptions before the first word.",
      "New: ducking music under speech works without a transcript; name the voice track.",
      "Fixed: on Linux, API keys survive a reboot (Secret Service: GNOME Keyring, KWallet or KeePassXC). With none running, the app says the key lasts only until restart.",
      "An unreadable keychain says so, Settings no longer erases what it could not read, and the window opens while the keyring asks to be unlocked.",
      "Fixed: Settings fits small windows (the tab strip scrolls; the dialog scrolls with Save in view).",
      "Fixed: signed macOS builds can record from the microphone.",
      "Fixed: a missing ONNX Runtime is an error instead of a hang. Model download code is in the app, but nothing calls it and no model is pinned. Stem separation and transcription are still not shipped.",
      "Changed: Set as head in the graph is a step undo can return from, and the label lane follows undo and redo.",
    ],
  },
  {
    version: "v0.4.0",
    date: "2026-10",
    summary: "Play works straight away, undo follows your path, and the limiter limits instead of clipping.",
    bullets: [
      "Fixed: Space did nothing and Play stayed disabled until you made a preview. Play now renders one when there is none, and after an edit plays the new version.",
      "New: a Play button on the toolbar; a click on the ruler or a lane places the playhead.",
      "Changed: replies render lists, bold, headings, code and tables. HTML stays text, and a link shows its address rather than opening in the app window.",
      "Fixed: requests were capped at 10 tool calls without the model knowing, and one mastering pass takes seven to ten. The cap is now 20, the model knows it, and a request reaching it ends with a summary of what is left.",
      "Fixed: the limiter clipped (23% distortion on a 1 kHz sine 6 dB over a −6 dBFS ceiling).",
      "It now turns gain down at once and recovers over a settable release (80 ms default): 0.19% on the same sine. It works on sample peaks, with no look-ahead.",
      "Fixed: with Plan first on, a request answered with an edit and no plan ran at once. The first changing step is now held and shown as the plan.",
      "Fixed: undo went to a version's first parent when an edit returned to an earlier state (load, mute, unmute, undo removed the track).",
      "Undo and redo now follow your path, a new edit after undo clears redo, and a new project starts fresh history.",
      "New: a reasoning-effort setting for Anthropic models, low to max, in Settings. Only Anthropic receives it; unset sends what was sent before.",
      "Fixed: on Linux the window could freeze after the assistant's edits. Lanes now draw waveforms without an audio player each.",
      "Fixed: a request taken for a mashup printed a plan as raw text and made no edit.",
      "Fixed: changing zoom while an edited track loaded broke the timeline.",
    ],
  },
  {
    version: "v0.3.0",
    date: "2026-10",
    summary: "Playback on Linux and macOS, and an assistant that knows the session it is editing.",
    bullets: [
      "Fixed: Space played nothing and showed “403 Forbidden” on Linux and macOS, after the file scope narrowed in 0.2.0. Windows was not affected.",
      "New: the assistant knows every track's name, position, level, pan and effects, and the current version, so “fade out the last two seconds” and “export the current mix” work.",
      "Fixed: ask for a track's tempo, key or loudness by name; the assistant used to ask you for a file path.",
      "Fixed: Plan first works with Ollama, Groq and Gemini, and after the first edit of a conversation. Before, the edit ran unapproved.",
      "Fixed: a failed request stops showing “Thinking”, errors name the failing provider, and a rejected key offers Open Settings instead of raw JSON.",
      "Fixed: a tool call with no arguments ended the turn with an error.",
      "Fixed: the window froze during a long edit.",
      "Fixed: after an edit, the clip and status bar showed a 64-character hash. Edited audio is named after its track.",
      "Changed: history stays under 2 GiB. Audio only older versions use is removed and rebuilt on demand when you undo to it, preview it or export it.",
      "Changed: switching between A and B crossfades instead of cutting, and a volume curve the assistant writes draws itself.",
      "Fixed: undo works with a gain or pan slider focused, and no longer writes the slider's old value back.",
      "Fixed: opening an unreadable file says which and why, instead of adding an empty track.",
    ],
  },
  {
    version: "v0.2.0",
    date: "2026-09",
    summary: "Transcript editing, one track per speaker, region descriptions, a shared timeline axis and security hardening.",
    bullets: [
      "New: edit audio by editing its transcript; cut a word and the audio goes with it. On-device transcription had not shipped, so in this release the pane stays empty.",
      "New: split an interview into one track per speaker, each with its own gain, EQ and noise treatment. Played together they are sample-identical to the original.",
      "New: describe a region (“select where he talks about latency”, “the last thirty seconds of speech”, “bars 9 to 16”).",
      "You check the selection before anything acts; an unresolvable description says so. Phrases and speech need a transcript, so only bar ranges resolve for now.",
      "New: timer record runs a recording unattended; punch-in replaces a selected region with a new take.",
      "Fixed: every timeline row shares one time axis, so a shorter track or moved clip sits where it plays. Zoomed in, the selection, playhead and drag follow the screen, and one scrollbar or swipe pans everything.",
      "New: a label lane you type into. Chapter marks are added, renamed, moved and deleted beside the audio, survive time-shifting edits and round-trip through label export.",
      "Fixed: opening a file loads it straight into the session. It was sent to the agent as a sentence, so with no model set up every edit failed.",
      "Changed: a returning user opens into their project where they left it (zoom, selection, playhead). An edit made just before quitting is no longer undone by the saved view.",
      "Fixed: undo and redo work on macOS, and redo works at all. Labels, template applications and batch loads are undone as themselves.",
      "Fixed: destructive edits keep every clip's volume automation. Labels and the transcript move with cuts, silences, speed changes and stretches, and a whole track shifts together.",
      "Fixed: resampler latency compensated, leveler gain applied continuously, a clip's frames read at its own sample rate, effect chain built per track so it no longer restarts at every clip.",
      "Fixed: a drawn track of several clips streams its sources instead of decoding a whole recording into memory (over a gigabyte for an hour of stereo).",
      "Changed: derived audio no version refers to is removed when the project opens; audio undo history needs is kept.",
      "Security: the app ships a Content Security Policy, the webview reads only the project's own audio rather than any file on disk, and the capability whitelist is enforced when a tool runs.",
      "Changed: typefaces are bundled, so the app no longer contacts Google Fonts on launch.",
      "Fixed: auditions play instead of printing a file path, the spectrogram toggle draws a spectrogram, an undecodable file says so, and A/B comparison keeps the playhead when you switch sides.",
      "New: project metadata (title, artist, album and more) carries into the tags of exported MP3 and FLAC files.",
    ],
  },
  {
    version: "v0.1.0",
    date: "2026-08",
    summary: "Effect chains, mixer controls, bus routing, MP3 and FLAC export, and vocoder and time-stretch fixes.",
    bullets: [
      "New: per-track effect chains reach the audio, with five tools to operate them and no click at chunk boundaries.",
      "Fixed: vocoder phase locking. A steady 2 kHz tone stretched 2x swung 2.09x in level (6 dB of warble); it now measures 1.003x.",
      "Fixed: time_stretch put features up to 41 ms off their mark.",
      "Fixed: a time-stretch bug could produce full-scale output (a 2 kHz sine stretched 2x peaked at 740x the input). Stretching never creates energy now.",
      "Fixed: timing drift in long stretches. On a 120-to-100 BPM warp the fifth beat came 10.5 ms early.",
      "New: storage_report splits disk use three ways: what the current version needs, what only undo holds, and what nothing references.",
      "Changed: every history node records the tool, parameters and engine version that made it, across all 81 tools.",
      "Changed: a real logo replaces the stock Tauri icons.",
      "New: a base URL field per provider, for a local server on another port, a gateway or a proxy (LM Studio, llama.cpp and vLLM included). The model list follows the endpoint chat uses.",
      "New: gain, pan, mute and solo are editable per track without asking the agent.",
      "New: volume automation is visible and editable: draw points on the lane, drag them, nudge with arrows. One gesture is one undo step.",
      "New: clips appear as chips you can select, move and delete, with move_clip and remove_clip for the agent too. A track split by an interior cut no longer looks continuous.",
      "Fixed: align_to_beat recorded a beat grid and changed nothing. It now stretches each inter-beat segment by its own ratio in one vocoder pass, with no seam.",
      "Fixed: preserve_formants was accepted and ignored on two tools. Shifted voices now keep their vocal-tract resonances.",
      "New: MP3 export on a pure-Rust encoder (no LAME, no C dependency); bitrate settable, default 192 kbps CBR.",
      "New: normalize_loudness targets LUFS, not peak (two peak-normalised files can differ by 10 LUFS). Gain is capped at a true-peak ceiling and the shortfall reported.",
      "New: bus routing: sends, per-bus effect chains and tools to reach them, so one reverb can serve several tracks.",
      "New: FLAC export, lossless, about half the size of WAV and sample-identical to it.",
      "New: Ollama joins Anthropic, OpenAI, Gemini, Groq and OpenRouter: six providers, and the local one needs no key.",
    ],
  },
  {
    version: "v0.1.0-dev (earlier)",
    date: "2026-08",
    summary: "69 built-in tools, MCP, skills, profiles, memory and recording, with many audio fixes.",
    bullets: [
      "Fixed: fade, reverse, insert_silence, copy_region and paste_region ignored the interleave stride: on stereo every span halved, and an odd sample count swapped left and right. They count in frames now.",
      "Fixed: stereo_to_mono and mono_to_stereo wrote the wrong channel count into the WAV header, so results played at double speed an octave high, or half speed an octave low.",
      "Fixed: low_pass_filter(cutoff_hz: 30000) on a 44.1 kHz track saturated into a full-scale square wave. Frequencies are now held just below Nyquist in every filter and EQ band.",
      "Fixed: seven tools panicked on a reversed time range (end before start) instead of returning an error.",
      "Fixed: renders dropped everything after an interior cut (cutting 10,000 frames from a one-second track rendered 12,000 frames instead of 38,000). Clips mix like tracks, gaps as silence.",
      "Fixed: destructive edits cover every clip of a split track; a reverb after an interior cut used to leave the second half dry.",
      "Fixed: split_clip did not re-base a clip's volume automation (a fade-out restarted at full volume) and cut_range discarded it. Both keep it now.",
      "New: MCP servers start automatically at launch and contribute their tools without a restart. Requests have deadlines and server stderr appears in errors.",
      "Fixed: agent profiles that pin a model on another provider use that provider's key, not the active provider's.",
      "Fixed: the MCP server editor's args, env and headers fields accept multi-line input.",
      "New: install skills and agent profiles from a GitHub repo (github:org/repo) or a local path.",
      "New: eight audio skills ship pre-installed: podcast cleanup, vocal chain, loudness mastering, noise reduction, dialog enhancement, music mixing, silence cleanup and an export guide.",
      "New: agent profiles save a model override, a tool whitelist and a system-prompt addition.",
      "New: skills are markdown files with always, keyword or regex triggers, editable in Settings.",
      "New: global and per-project memory notes are spliced into the system prompt.",
      "New: microphone recording, captured straight to WAV.",
      "New: a spectrogram toggle in the timeline.",
      "New: plan steps can be edited inline before you approve them.",
      "New: individual tools can be disabled from the capabilities menu.",
      "New: 69 built-in audio tools, including reverb, echo, noise gate, limiter, de-esser, tone and noise generators, Audacity-format label import/export and per-track export.",
      "New: OpenAI joins Anthropic and OpenRouter, with a catalogue-backed model picker.",
      "Changed: per-provider keychain slots; a legacy unsuffixed Anthropic key is still read.",
    ],
  },
];

/** The newest releases are shown in full; older ones start collapsed. */
const OPEN_RELEASES = 3;

function Bullets({ items }: { items: string[] }) {
  return (
    <ul>
      {items.map((b, i) => (
        <li key={i}>{b}</li>
      ))}
    </ul>
  );
}

export default function ChangelogPage() {
  const recent = entries.slice(0, OPEN_RELEASES);
  const older = entries.slice(OPEN_RELEASES);
  return (
    <LegalShell title="Changelog" path="/changelog">
      <p>
        Newest first. Installers and tags are on{" "}
        <a href="https://github.com/laadtushar/edytlab/releases">
          GitHub Releases
        </a>
        .
      </p>
      {/* Flat — heading, summary, list — so the shell's cascade reveals
          each part as it arrives instead of a whole release at once. */}
      {recent.map((e) => (
        <Fragment key={e.version}>
          <h2>
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
          </h2>
          <p>{e.summary}</p>
          <Bullets items={e.bullets} />
        </Fragment>
      ))}
      <h2>Older releases</h2>
      {older.map((e) => (
        <details key={e.version} className="mt-3 rounded-lg border border-border/50 px-4 py-2">
          <summary className="cursor-pointer font-semibold">
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.summary}
            </span>
          </summary>
          <Bullets items={e.bullets} />
        </details>
      ))}
    </LegalShell>
  );
}
