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
  bullets: string[];
}

// Static stub. Updated manually as releases land. The full git history lives on
// GitHub Releases — link below.
const entries: Entry[] = [
  {
    version: "v0.6.0",
    date: "2026-10",
    bullets: [
      "Compacting history no longer deletes your own recordings. A WAV file you load stays where you keep it, and compacting the session (asking the assistant to reclaim disk space) could delete it from your disk once the versions that used it were pruned. Compaction now deletes only audio the project made itself, inside its own folder, and checks each file again before deleting it. Earlier releases, v0.5.0 included, have this bug, so update before you compact a project.",
      "Effects on the master bus and on buses. Ask for a limiter on the master and the assistant puts it on the master bus, where it keeps working as the tracks change; it used to render the mix into a new \"Master\" track that later edits did not reach. It can also put an effect on a bus, let you hear one on the master or a bus before adding it, and knows the session's buses by name. A bus effect is heard only when a track sends to that bus.",
      "A saved effect the app cannot run in a chain no longer breaks playback. So far only gain, the limiter and the low-pass, high-pass and notch filters run in a chain. A project holding another kind there, such as a reverb an earlier version let the assistant add to a track, failed every preview, playback and export until you found and removed it. That effect is now left out, the rest plays, and the window names what was left out: on Play, after an export and in A/B compare.",
      "The assistant no longer puts an effect in a chain that cannot run it. Reverb, EQ, echo and the others still work as edits to a track's audio. A setting the effect does not have, such as \"gain_db\" on a gain whose level is \"db\", is refused instead of being saved and ignored, and the command palette no longer suggests a reverb bus.",
      "Delete waits for your answer. In the installed app, deleting an agent profile, a skill or an MCP server in Settings went ahead at once, without waiting for you to confirm, so Cancel did nothing.",
      "After Save As, the assistant edits the copy. It went on editing the original project while the window showed the copy.",
      "Opening a project no longer takes it back to an older version. Opening one from Recent projects or Open project went to the version saved with its zoom and selection, which could be a step behind, and the newest edits dropped off the timeline.",
      "Opening a project while the assistant is working on it, including the one already open, no longer splits it in two. The window and the assistant could each go on with their own idea of the latest version, and the cleanup that runs when a project opens could delete audio a tool had just written.",
      "Undo and Plan first survive a restart. Undo retraces the steps you took after the app is closed and opened again; it used to fall back on each version's first parent and could skip a step, such as a mute you had turned off again. Plan first stays on if you left it on: it was off at every launch, so the first edits after a restart ran without asking.",
      "Undo follows the version the assistant leaves you on. When the assistant named a version, the window moved to that version although the project had not, so the next undo went to the wrong place. When it made alternative takes in one step or applied a recipe, the window was not told, so the track list stayed out of date and undo skipped the new version.",
      "Undo keeps working after the assistant compacts history. Compaction could remove a version that undo or redo still pointed at, such as a mute you had turned off again, and every press of undo then failed with the same error until a restart. Undo and redo now step past a version that is gone.",
      "A click on a version in the Graph view previews it and leaves your place alone. It used to move the window to that version without moving the project, so undo after the next edit went to the previewed version, and after a restart undo lost the steps you took. Set as head still moves you there.",
      "Adding a label works on macOS, and regions keep their shape. A new label is named in a box in the label lane, whether you add it there or from the ruler; the name used to be asked for in a dialog the macOS app cannot show, so no label was added. Dragging a region moves all of it instead of turning it into a single marker, a region shows its whole length, a click on a label no longer moves it, and labels move with pasted audio as they do with other edits that shift time.",
      "Select a spoken passage without a transcript. Ask for \"the last thing said\" on a voice track and the assistant finds it in the track's audio, as ducking already does. It finds where there is speech, not what is said, so finding a phrase still needs a transcript, which this build cannot make.",
      "With sync-lock on, a paste moves the other tracks too. Pasting made one track longer and left the rest where they were, so everything after the paste drifted out of line. The other tracks now open the same gap, and one undo puts them all back.",
      "The assistant's previews stay inside the project, in a cache with a size limit. A preview of part of the session was written to the system's temporary folder and left there, about 55 MB for five minutes of stereo. With Plan first on, previews and effect auditions no longer wait for approval, since they change nothing in the session. A track the assistant loads from a preview keeps its audio when the cache makes room.",
      "With Plan first on, a held recipe or batch shows the edits it will run. The card showed only the recipe's file path; it now lists each step in order, and says so when the recipe cannot be read.",
      "Keychain problems are reported instead of hidden. On Linux, a key saved after the Secret Service went away lasts only until restart, and Settings now says so before it closes; a key saved while the service was briefly away is no longer replaced by an older copy when it comes back. After you decline to unlock the keyring at launch, the app asks nothing more until you save in Settings, Save leaves your chosen model alone instead of writing the default over it, and the chat says why there is no assistant.",
      "The app looks like the website: violet on zinc instead of amber, the edytlab wordmark, and small motions on buttons, cards, banners and messages. Motion is off when your system asks for reduced motion.",
      "The library behind the app's secure connections (rustls) is updated past four published security advisories.",
      "Stem separation and transcription are still not shipped.",
    ],
  },
  {
    version: "v0.5.0",
    date: "2026-10",
    bullets: [
      "Current models by default. Anthropic's chat model is now Claude Sonnet 5.5 and its classifier Claude Haiku 5.5, and the list of Anthropic models in Settings is fetched live from Anthropic instead of being written into the app. OpenAI, OpenRouter and Gemini defaults move to current models too; Gemini's old default was shut down in June. A saved model that its provider has retired now says so and points to Settings, instead of failing with a raw 404.",
      "Chats keep working on models that think by default. The second message of a chat on a 5.x model could fail with \"Invalid signature in thinking block\", because earlier thinking was sent back under a changed system prompt. Earlier turns' thinking is no longer sent back, and each model's output is capped at that model's own limit.",
      "Edits you make while the assistant is working are kept, in the order you made them. A long tool such as a time-stretch no longer locks the session, so the faders, mute, markers and undo answer while it runs. If you change a track the tool is working on, the tool notices and stops rather than editing the wrong track. Edits from the timeline, mixer, markers, undo and file loads go out one after another, so two quick fader moves cannot land in the wrong order.",
      "The window no longer freezes on track edits. Twelve track commands, from gain and mute to rename and remove, waited on the session on the app's main thread.",
      "Plan cards say what each step does. A step showed only a tool name, sometimes a dangling dash, and could name a tool that does not exist. Steps now read as sentences with their values, steps naming unknown tools are dropped, and steps for tools that are not shipped yet are marked \"not available in this build\".",
      "A question is never held at a plan gate. The request classifier could read a question as an edit and hold it for approval.",
      "After a failed tool call, the next message works. A turn that ended on a tool-argument error left a tool call unanswered, and the provider then refused every later message in the chat.",
      "Local models (Ollama) get a request they can take. A small-context model was sent every tool's full description, about 15,000 tokens before the first word; it now gets a core set plus the tools the message asks for, with the rest listed by name.",
      "Ducking music under speech works without a transcript. Name the voice track and the music ducks under its audio.",
      "On Linux, API keys survive a reboot. They are stored in the Secret Service (GNOME Keyring, KWallet or KeePassXC); with none running, the app says the key lasts only until restart. A keychain that cannot be read says so, Settings no longer erases what it could not read, and the window opens while the keyring asks to be unlocked.",
      "Settings fits small windows. The tab strip scrolls when the window is narrow, and when it is short the dialog scrolls while Save stays in view.",
      "Signed macOS builds can record from the microphone.",
      "A missing ONNX Runtime is an error instead of a hang. The code that downloads a speech model and checks it against a pinned checksum is in the app, but nothing calls it yet and no model is pinned, so nothing is downloaded. Stem separation and transcription are still not shipped.",
      "Set as head in the graph is a step undo can return from, and the label lane follows undo and redo.",
    ],
  },
  {
    version: "v0.4.0",
    date: "2026-10",
    bullets: [
      "Play straight after opening a file or making an edit. Playback plays a rendered preview of the mix, and only the Preview button made one, so Space did nothing and the Play button stayed disabled until you found it. Play now renders the preview when there is none and starts as soon as it has loaded. After an edit it plays the new version; it used to play the one before.",
      "Play, pause and place the playhead with the mouse. The toolbar has a Play button, and a click on the ruler or a lane moves the playhead there. Playback used to be keyboard-only.",
      "The assistant's replies are formatted. Lists, bold, headings, code and tables render as such instead of showing their asterisks, dashes and backticks. HTML in a reply stays text, and a link shows its address beside it rather than opening in the app window.",
      "A request with many steps finishes. A request was capped at 10 tool calls without the model being told, and one mastering pass takes seven to ten, so an ordinary request could end in an error after its edits had already been made. The cap is now 20, the model knows it, and a request that reaches it ends with the assistant saying what it did and what is left.",
      "The limiter limits instead of clipping. It turned every sample over the ceiling into the ceiling, which on a sine 6 dB over a −6 dBFS ceiling is 23% distortion. It now turns the gain down at once and lets it recover over a release time you can set (80 ms by default): 0.19% on the same 1 kHz sine. It works on sample peaks, with no look-ahead.",
      "With Plan first on, nothing changes before you approve it. A request the model answered with an edit and no plan used to run straight away; the first step that would change the session is now held and shown as the plan.",
      "Undo retraces the steps you took. A version is named after its content, so an edit that returns to an earlier state, such as mute then unmute, lands on the old version, and undo used to go to that version's first parent: after loading a track, muting and unmuting, undo made the track vanish. Undo and redo now follow the path you actually took, a new edit after an undo clears redo, and opening another project starts its history fresh.",
      "A reasoning-effort setting for Anthropic models, from low to max, in Settings. Higher effort gives the model more room to think before it edits. Only Anthropic receives it, and leaving it unset sends exactly what was sent before.",
      "The window no longer freezes after the assistant's edits on Linux. Every timeline lane carried an audio player that it never plays, and on Linux each one streamed through its own media pipeline; an edit swapped them all at once, and tearing them down could deadlock the window. Lanes now draw their waveform without loading a player.",
      "A request the assistant took for a mashup is carried out. It used to print a plan as raw text in the chat and stop without making the edit.",
      "Changing the zoom while an edited track is still loading no longer breaks the timeline.",
    ],
  },
  {
    version: "v0.3.0",
    date: "2026-10",
    bullets: [
      "Playback works on Linux and macOS. The rendered mix and the chat's auditions live in a hidden folder that the narrowed file scope in 0.2.0 could not reach through, so pressing Space played nothing and showed \"403 Forbidden\". Windows was not affected.",
      "The assistant knows the session it is editing: every track's name, where its audio sits on the timeline, its level, pan and effects, and the current version. \"Fade out the last two seconds\" now fades, and \"export the current mix\" exports, where before the assistant had nothing to place them with.",
      "Ask for a track's tempo, key or loudness by name. The analysis tool took only a file path, so the assistant asked you for one.",
      "Plan first works with Ollama, Groq and Gemini, and after the first edit of a conversation. Their plan requests were sent in the wrong format, and any turn after one that used a tool sent an empty message the provider rejected, so the plan was skipped and the edit ran unapproved.",
      "A request that fails stops showing \"Thinking\", errors name the provider that failed rather than Anthropic, and a rejected API key offers Open Settings with a plain explanation instead of raw JSON.",
      "A tool call with no arguments no longer ends the turn with an error.",
      "The window stays responsive while the assistant runs a long edit. Reading the track list, editing a marker and renaming a version waited for the edit on the app's main thread, so the whole window froze until the edit finished.",
      "Edited audio is named after its track. After an edit, the clip and the status bar showed a 64-character file hash.",
      "History stays under 2 GiB. Audio that only older versions use is removed once the app has proved it can rebuild it, and it is rebuilt on demand when you undo to it, preview it or export it.",
      "Switching between A and B crossfades from the same moment instead of cutting, and a volume curve the assistant writes draws itself on.",
      "Undo works with a gain or pan slider focused, and no longer writes the slider's old value back after undoing.",
      "Opening a file the app cannot read says which file and why, instead of adding an empty track.",
    ],
  },
  {
    version: "v0.2.0",
    date: "2026-09",
    bullets: [
      "Edit audio by editing its transcript. The transcript pane shows the words with their times; cut a word from the text and the audio goes with it. It needs a transcript, and the on-device transcription that makes one has not shipped yet — in this release the transcript pane stays empty.",
      "Split an interview into one track per speaker. Each voice gets its own gain, EQ and noise treatment, and because every speaker's clips point at the same source file, playing them together is sample-identical to the original.",
      "Describe a region instead of dragging it: \"select where he talks about latency\", \"the last thirty seconds of speech\", \"bars 9 to 16\". The selection appears for you to check before anything acts on it, and a description that cannot be resolved says so rather than guessing. Phrases and speech need a transcript, so for now only bar ranges resolve.",
      "Timer record starts and stops a recording unattended, and punch-in replaces a selected region with a new take while leaving the rest untouched.",
      "Every row of the timeline shares one time axis. A shorter track, or a clip moved along the timeline, now sits under the ruler where it plays — each lane used to stretch its own audio across the pane. Zoomed in, the selection, the playhead and a drag follow what is on screen, and one scrollbar (or a horizontal swipe) pans every track, the clips, the automation and the labels together.",
      "A label lane you type into. Chapter marks are added, renamed, moved and deleted next to the audio, survive edits that shift time, and round-trip through label export.",
      "Opening a file loads it straight into the session. It used to be sent to the agent as a sentence, so with no model set up yet the waveform appeared and every edit afterwards failed.",
      "A returning user opens into their project where they left it — zoom, selection and playhead — instead of the empty state, and an edit made in the last moment before quitting is no longer undone by the saved view.",
      "Undo and redo work on macOS, and redo works at all. Labels, template applications and batch loads now move the session head they create, so undo undoes them rather than the edit before.",
      "Destructive edits keep every clip's volume automation, labels and the transcript move with cuts, silences, speed changes and stretches, and a whole track shifts together instead of clamping clip by clip.",
      "Audio fixes: the resampler's latency is compensated, the leveler applies its gain continuously rather than per block, a clip's frames are read at its own sample rate, and the effect chain is built per track so it no longer restarts at every clip.",
      "A drawn track made of several clips is written by streaming its sources a chunk at a time, instead of decoding a whole recording into memory — over a gigabyte for an hour of stereo audio.",
      "Derived audio that no version of the project refers to any more is removed when the project opens. Audio that undo history still needs is kept.",
      "Security: the app ships a Content Security Policy, the webview can read only the project's own audio rather than any file on disk, and the capability whitelist is enforced when a tool runs, not only in the list the model sees.",
      "The typefaces are bundled with the app, so it no longer contacts Google Fonts on launch.",
      "Auditions play instead of printing a file path, the spectrogram toggle draws a spectrogram, an undecodable file says so, and A/B comparison keeps the playhead when you switch sides.",
      "Project metadata — title, artist, album and more — carries through to the tags on exported MP3 and FLAC files.",
    ],
  },
  {
    version: "v0.1.0",
    date: "2026-08",
    bullets: [
      "Per-track effect chains. Track.effects round-tripped through save/load since Phase 1 and the render path refused the moment anything populated it, so a field in the model was unreachable in practice. Chains now reach the audio, with five tools to operate them — and because the processors are built once per render rather than per chunk, there is no click at the second boundary.",
      "Phase locking in the vocoder. Every bin's phase advanced independently, so bins belonging to one partial stopped agreeing and the partial alternately reinforced and cancelled itself. On a steady 2 kHz tone stretched 2x that was an envelope swinging 2.09x — 6 dB of warble on a flat input. Identity phase locking brings the same measurement to 1.003x.",
      "A stretched feature now lands where the ratio says. time_stretch put everything up to 41 ms off its mark, and the error was exactly zero at ratio 1, which is why every existing test missed it: the lead-in trim named the frame holding sample 0 at its left edge, where the analysis window weight is zero, rather than the frame holding it at its centre.",
      "storage_report. Every destructive edit writes a content-addressed WAV and none are ever deleted, so a long session grows without bound. The report splits the cost three ways — what the current version needs, what only undo is holding, and what nothing references — because collapsing them into one number is what would make a sweep look safe when it is not.",
      "Nodes record how they were made. A node carried a human label and the transform was an anonymous closure, so nothing could replay an edit. tool, parameters and engine version are now recorded — by the dispatcher, so all 81 tools are covered by one code path — which is the prerequisite for treating derived audio as a cache rather than as data.",
      "A real logo. Every icon in the repo was still the stock Tauri mark while the brand colour had been violet since the site was written.",
      "Point any provider anywhere. A base URL field per provider, so a local server on another port, a gateway or a proxy all work — LM Studio, llama.cpp and vLLM included. The model catalogue follows the same endpoint the agent uses, because listing one server's models while chat talks to another is worse than not listing.",
      "Mixer controls. Gain, pan, mute and solo are editable per track without asking the agent — none of them had a command before, so every mixing decision went through a sentence and a model round trip. Pan in particular had a balance law in the render engine and nothing in the product able to reach it.",
      "Volume automation is visible and editable. The curve, its persistence and the render integration all existed; there was no way to see or touch it. Draw points on the lane, drag them, nudge with arrows — one gesture is one undoable step, not one per pixel.",
      "Clip timeline. A track split by an interior cut used to render as one continuous lane, so the seam was invisible. Clips now appear as chips you can select, move and delete, with move_clip and remove_clip available to the agent too.",
      "align_to_beat warps audio. It recorded a beat grid that nothing read and reported success while changing nothing — the last tool in the repo doing that. It now stretches each inter-beat segment by its own ratio in a single vocoder pass, so there is no seam at the beats.",
      "Formant preservation. preserve_formants was a documented, accepted, ignored flag on two tools. Shifted voices now keep the resonances of the vocal tract where they were instead of travelling with the pitch.",
      "MP3 export, on a pure-Rust encoder — no LAME, no C dependency, no licence interaction. Bitrate is settable and defaults to 192 kbps CBR.",
      "normalize_loudness targets LUFS rather than a peak. Two files peak-normalised to the same value can differ by 10 LUFS, so peak normalisation cannot answer 'make this as loud as everything else'. Gain is capped at a true-peak ceiling and the shortfall is reported when the cap bites.",
      "Bus routing. Sends, per-bus effect chains, and the tools to reach them, so one reverb can serve several tracks.",
      "FLAC export — lossless, about half the size of WAV, sample-identical to the WAV of the same render.",
      "A time-stretch bug that could produce a full-scale output. A steady 2 kHz sine stretched 2x came back peaking at 740x the input: the overlap-add divided phase-rewritten frames by a window energy that ramps to zero at the edges. Fixed, with tests asserting that stretching redistributes energy in time and never creates any.",
      "A timing drift in every long stretch. Rounding each synthesis hop to an integer accumulated — measured on a 120-to-100 BPM warp, that put the fifth beat 10.5 ms early. Positions are now accumulated in floating point and rounded once.",
      "Ollama joins Anthropic, OpenAI, Gemini, Groq and OpenRouter — six providers, and the local one needs no key at all.",
    ],
  },
  {
    version: "v0.1.0-dev (earlier)",
    date: "2026-08",
    bullets: [
      "Audio fidelity: five tools — fade, reverse, insert_silence, copy_region and paste_region — converted seconds straight into a sample index with no interleave stride. On a stereo track that halved every span, and an odd sample count swapped left and right for the rest of the track. All five now count in frames.",
      "stereo_to_mono and mono_to_stereo wrote the source's channel count into the WAV header rather than the converted buffer's, so the result played at double speed an octave high, or half speed an octave low. The conversion maths was right all along; only the write-back was wrong.",
      "Filters no longer diverge above Nyquist. low_pass_filter(cutoff_hz: 30000) on a 44.1 kHz track put the biquad poles outside the unit circle and the render saturated into a full-scale square wave. Frequencies are now held just below Nyquist across every filter and every EQ band.",
      "Seven tools panicked outright on a reversed time range (end before start) instead of returning an error.",
      "Renders no longer drop everything after an interior cut. A track is a list of clips and the render graph only ever read the first one, so cutting ten thousand frames out of the middle of a one-second track rendered 12 000 frames instead of 38 000. Clips now mix the way tracks do, with gaps as silence.",
      "Destructive edits cover every clip of a split track. A reverb applied after an interior cut used to treat the first half and leave the second half dry, with a hard seam at the join.",
      "Cutting or splitting a clip keeps its volume automation. split_clip copied the curve across without re-basing it, so a fade-out restarted at full volume after the split; cut_range discarded the curve entirely.",
      "Extensibility: MCP servers now start automatically at launch, so a registered server contributes its tools without a manual restart. Requests are deadline-bounded and server stderr is surfaced in error messages.",
      "Agent profiles that pin a model on another provider now authenticate against that provider instead of reusing the active provider's key.",
      "MCP server editor: the args, env, and headers fields accept multi-line input correctly.",
      "Plugins: install skills and agent profiles from a GitHub repo (github:org/repo) or a local path.",
      "Eight audio skills ship pre-installed — podcast cleanup, vocal chain, loudness mastering, noise reduction, dialog enhancement, music mixing, silence cleanup, and an export guide.",
      "Agent profiles: save a model override, a tool whitelist, and a system-prompt addition, then switch between them.",
      "Skills: markdown files with always/keyword/regex triggers, editable in Settings.",
      "Memory: global and per-project notes spliced into the system prompt.",
      "Microphone recording via CPAL, captured straight to WAV.",
      "Spectrogram view toggle in the timeline.",
      "Plan steps can be edited inline before you approve them.",
      "Individual tools can be disabled from the capabilities menu.",
      "69 built-in audio tools, including reverb, echo, noise gate, limiter, de-esser, tone and noise generators, Audacity-format label import/export, and per-track export.",
      "OpenAI provider added alongside Anthropic and OpenRouter; model picker is catalogue-backed.",
      "Per-provider keychain slots; legacy unsuffixed Anthropic key still read for back-compat.",
    ],
  },
];

export default function ChangelogPage() {
  return (
    <LegalShell title="Changelog">
      <p>
        Recent landed work. The authoritative list of binaries and tags lives
        on{" "}
        <a href="https://github.com/laadtushar/edytlab/releases">
          GitHub Releases
        </a>
        .
      </p>
      {/* Flat — heading, list, heading, list — so the shell's cascade
          reveals each bullet as it arrives instead of a whole release
          at once. */}
      {entries.map((e) => (
        <Fragment key={e.version}>
          <h2>
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
          </h2>
          <ul>
            {e.bullets.map((b, i) => (
              <li key={i}>{b}</li>
            ))}
          </ul>
        </Fragment>
      ))}
    </LegalShell>
  );
}
