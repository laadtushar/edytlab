import type { Metadata } from "next";
import { pageSocial } from "@/lib/seo-core";
import { DocShell } from "@/components/docs/doc-shell";

const title = "User Guide: sessions, tracks, chat and export";
const description =
  "The user guide for edytlab: sessions and the edit graph, tracks, chat, A/B compare, export, memory and skills, and what is not available yet.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs/user-guide" },
  ...pageSocial("/docs/user-guide", `${title} · edytlab`, description),
};

export default function UserGuidePage() {
  return (
    <DocShell
      title="User Guide"
      description="Complete guide to every feature in edytlab."
    >
      <h2>Sessions and the Session Graph</h2>
      <p>
        Every edit you make in edytlab creates a new <strong>session node</strong> —
        a snapshot of your audio session at that point in time. Nodes are linked
        to their parent, forming a directed acyclic graph (DAG).
      </p>
      <p>
        This means <strong>nothing is ever destructive</strong>. Your original
        audio files are never modified. You can always navigate back to any
        previous state.
      </p>
      <ul>
        <li>
          <strong>Graph view</strong> — open the <strong>Graph</strong> tab at
          the top, beside Timeline and Transcript, to see all session nodes
          drawn as a graph. Each node shows its label, the tool that created
          it and how long ago it was made. The graph redraws whenever the head
          moves, when the assistant&apos;s turn ends (so a label it set shows
          at once) and after the history is compacted, so it does not keep
          nodes that are gone.
        </li>
        <li>
          <strong>Preview a version</strong> — click a node in the graph to
          render that version&apos;s mix and play it. The node gets a dashed
          outline and your place in the history does not move: the next edit
          or undo starts from where you were, not from the node you
          previewed.
        </li>
        <li>
          <strong>Revert to any node</strong> — right-click a node in the graph
          and choose "Set as head" to jump back to that state. Undo brings you
          back.
        </li>
        <li>
          <strong>Fork</strong> — branch from any node to explore a different
          edit direction without losing your current work.
        </li>
        <li>
          <strong>Undo and redo</strong> — <code>Ctrl/Cmd+Z</code> and{" "}
          <code>Ctrl/Cmd+Shift+Z</code> retrace the steps you took, the
          agent&apos;s edits included. The path is saved with the project, so
          undo still works after you quit and reopen edytlab.
        </li>
        <li>
          <strong>Compact history</strong> — a long session can use a lot of
          disk. Ask <code>this project is huge — reclaim some disk</code> and
          the agent first reports what it would remove, then, if you confirm,
          deletes old undo steps for good together with the audio the project
          made for them inside its own folder. A file you loaded is never
          deleted, only the undo steps that named it. The most recent steps
          are kept, so ordinary undo keeps working.
        </li>
        <li>
          <strong>One writer per project folder</strong> — only one thing
          writes to a project folder at a time. Opening a project is refused,
          with a message, while a batch run is writing its own project into
          that folder, and while the folder was replaced at the same path
          (restored from a backup, or trashed and made again) and the
          assistant is still working in the old one. Wait for the
          assistant&apos;s turn to end, then open the project again. In the
          other direction, a batch run is refused up front, and starts
          nothing, if one of the projects it would write is open.
        </li>
      </ul>

      <h2>Tracks</h2>
      <p>
        A session contains one or more tracks. Each track holds a sequence of
        audio clips pointing to source files on disk. Clips are non-destructive
        — they describe a region of a source file, not a copy of it.
      </p>
      <ul>
        <li>
          <strong>Load audio</strong> — drag a file onto the timeline, click
          "Open Audio", or type <code>load /path/to/file.mp3</code> in chat.
          Supported formats: WAV, MP3, FLAC and Ogg Vorbis.
        </li>
        <li>
          <strong>Multi-track sessions</strong> — load multiple files or ask the
          agent to add a track: <code>add an empty track called drums</code>.
        </li>
        <li>
          <strong>Lane controls</strong> — each track&apos;s lane has its own
          gain slider, pan slider, <strong>mute</strong> and{" "}
          <strong>solo</strong> buttons, so you can mix by hand without asking
          the agent. Double-click a pan slider to return it to the centre. A
          mute silences a track without removing it from the session, and a
          solo plays only the soloed tracks. One drag, or one click, is one
          step in the history, and a control you have just set stays where you
          put it until the session confirms it, even if an earlier change is
          still being read back.
        </li>
      </ul>

      <h2>Effects, Buses and the Master</h2>
      <p>
        Ask for an effect on a track and it goes into that track&apos;s effect
        chain. The chain is applied when the session is rendered and your
        source audio is never rewritten, so you can change an effect&apos;s
        settings, switch it off or take it out later. Effects run in the order
        they appear in the chain.
      </p>
      <ul>
        <li>
          <strong>The master bus</strong> — every track is summed there, so an
          effect on it processes the whole mix and keeps doing so when you
          edit a track later: <code>put a limiter on the master at -1 dB</code>.
        </li>
        <li>
          <strong>Buses</strong> — a bus is a shared chain that tracks send
          into. Ask for one with <code>create a bus called Drums</code>, send
          tracks to it, and put effects on it:{" "}
          <code>put a low-pass at 8 kHz on the Drums bus</code>. A bus effect
          is heard only when a track sends to the bus.
        </li>
        <li>
          <strong>Hear it first</strong> — <code>what would a 1 kHz low-pass
          sound like on the vocal?</code> plays a few seconds in the chat with
          the effect added, on a track, the master or a bus, and changes
          nothing. Ask for the same effect to keep it.
        </li>
        <li>
          <strong>Five effects work in a chain today</strong> — gain, limiter
          and the low-pass, high-pass and notch filters. Reverb, echo, EQ,
          the compressor and the rest are separate edits that bake their
          result into a new version of the track instead, and the agent says
          so if you ask for one of them in a chain.
        </li>
        <li>
          <strong>Setting names</strong> — a gain has <code>db</code>, a
          limiter <code>ceiling_db</code> and <code>release_ms</code>, the
          low-pass and high-pass filters <code>cutoff_hz</code>, and the notch{" "}
          <code>center_hz</code> and <code>q</code>. The assistant is told
          these, and a setting name an effect does not have is refused, with
          the right names in the message, rather than saved and ignored.
        </li>
      </ul>
      <p>
        A project saved by an older build can still hold an effect in a chain
        that this build cannot run. Rendering leaves it out rather than
        failing, and the window says so: a notice above the timeline names the
        effect and where it sits, and stays while the version on screen holds
        it. Export Selection adds a note naming the file and what it was
        written without, and the A/B compare bar lists each side&apos;s left-out
        effects, marked A or B, because a side with a reverb it cannot play
        sounds the same as one without. Remove or bypass the effect and the
        notice goes.
      </p>

      <h2>Session Templates</h2>
      <p>
        When no audio is loaded, click <strong>Start from template</strong> to
        choose from a set of pre-configured session layouts:
      </p>
      <ul>
        <li>
          <strong>Podcast</strong> — Host and Guest tracks, each at −3 dB.
        </li>
        <li>
          <strong>Music</strong> — Lead, Harmony, Bass and Drums tracks, with
          the harmony panned a little to the left.
        </li>
        <li>
          <strong>Interview</strong> — Interviewer and Subject tracks, panned
          slightly apart.
        </li>
      </ul>
      <p>
        A template makes the empty tracks with a starting gain and pan. It adds
        no audio, effects or skills. Skills still switch on when your messages
        match their triggers, whether or not you used a template. You can
        modify the session freely after choosing one.
      </p>

      <h2>The Chat Interface</h2>
      <p>
        The chat panel on the right is where you direct the agent. Type your
        editing instruction in plain English — no commands to memorise.
      </p>
      <p>
        Type <code>/</code> in the chat input to see an inline dropdown of all
        available commands — press arrow keys to navigate, Enter to select.
      </p>
      <p>The agent:</p>
      <ul>
        <li>Streams its response in real time as it processes.</li>
        <li>Shows a tool badge for each operation it executes.</li>
        <li>Tells you what it did and why after each set of operations.</li>
        <li>
          Understands follow-up instructions in the same conversation:{" "}
          <code>actually make it 2 dB higher instead</code>.
        </li>
      </ul>

      <h3>What makes a good prompt</h3>
      <ul>
        <li>
          <strong>Be specific about the track</strong> when you have multiple:{" "}
          <code>normalize track 1 to -14 LUFS</code>.
        </li>
        <li>
          <strong>Use time references</strong>: <code>cut from 1:30 to 2:00</code>{" "}
          or <code>remove the first 5 seconds</code>.
        </li>
        <li>
          <strong>Chain multiple operations</strong> in one message: the agent
          plans and executes the full chain.
        </li>
        <li>
          <strong>Reference markers</strong> you have set:{" "}
          <code>cut everything before the chorus marker</code>.
        </li>
      </ul>

      <h3>Big sessions</h3>
      <p>
        Each turn, the assistant is given a short summary of the session: its
        tracks, buses, master effects and markers. The summary has a size
        limit, 8,000 bytes for the hosted providers and 2,300 for Ollama, so a
        big project still fits. When a list is too long it is cut, the summary
        says how many it left out, and the assistant can read the rest a page
        at a time with the <code>list_session</code> tool. The tool only
        reads, so Plan first never holds it for approval. With Ollama the
        assistant is always sent the tool when a list was cut.
      </p>

      <h3>Plan first</h3>
      <p>
        Turn on the <strong>Plan</strong> toggle beside the chat box when you
        want to see what the agent will do before it does it. With it on,{" "}
        <strong>no edit runs until you approve it</strong>. The toggle stays
        as you left it when you quit and reopen edytlab, so a session you
        protected is still protected after a restart.
      </p>
      <ul>
        <li>
          The agent shows a plan card. <strong>Run</strong> approves it,{" "}
          <strong>Discard</strong> ends the turn without applying anything, and{" "}
          <strong>Edit</strong> lets you reword a step before you run it.
        </li>
        <li>
          If the agent answers without a plan, the first edit it is about to
          make is shown on the same card, with its exact tool and settings (for
          example <code>reverse</code>, <code>track: 0</code>), before anything
          changes. Approve it to carry on; once you have, the rest of that turn
          runs without asking again. Discard it and nothing runs, its badge reads{" "}
          <em>not run</em>, and the agent is told you declined.
        </li>
        <li>
          If you reword a step on that card instead, nothing runs: the agent
          gets your wording and proposes again, and you approve that.
        </li>
        <li>
          Questions that only read the session, like the tempo, the loudness
          or the spectrum of a track, run without asking. So do hearing an
          effect before you add it and rendering a preview: they change
          nothing in your project, and what they save is a disposable copy
          that is cleared as it grows.
        </li>
      </ul>

      <h2>Playback</h2>
      <ul>
        <li>
          <strong>Space</strong> — play / pause from the current position. The
          Play button at the top of the timeline does the same. Right after you
          open a file or make an edit there is no preview of the new version
          yet, so Play renders it first and starts as soon as it is ready; press
          it again while it renders to cancel. <strong>Preview</strong> in the
          chat panel renders the preview without playing it. The mix you are
          playing is kept in the project&apos;s preview cache and is not
          cleared out while it is in use, so switching to the Graph or
          Transcript tab and back does not lose it. If the file has gone
          anyway, the window renders it again, once.
        </li>
        <li>
          <strong>L</strong> — toggle loop mode. When active, playback loops
          within the selected region.
        </li>
        <li>
          <strong>Click the waveform</strong> — jump to that position.
        </li>
        <li>
          <strong>Ctrl+scroll</strong> or <strong>+/−/0</strong> — zoom the
          waveform in/out/reset.
        </li>
        <li>
          <strong>Drag on the waveform</strong> — create a selection range (used
          for range export, loop playback, and as agent context).
        </li>
      </ul>

      <h2>Markers and Selections</h2>
      <p>
        Markers let you annotate points and regions in your session. The agent
        can reference them by name.
      </p>
      <ul>
        <li>
          Ask the agent to add markers:{" "}
          <code>mark the verse start at 0:30</code>.
        </li>
        <li>
          Drag on the waveform to select a region. The selection is passed as
          context to the agent so it knows where to operate.
        </li>
        <li>
          Use <strong>Esc</strong> to clear the selection.
        </li>
        <li>
          <strong>The label lane</strong> — the lane under the tracks is
          where you name chapters and regions yourself, without a round trip
          through the agent. Double-click empty space in it, type a name where
          the label will sit and press Enter (Esc cancels); clicking the ruler
          opens the same name box at that point. Double-click a label to
          rename it, drag it to move it (a region moves whole and keeps its
          length), and right-click it to delete it. Each one is a single step
          in the history, so undo takes it back. Labels stay with the audio
          when an edit shifts time — a cut, a trim, an inserted silence, a
          paste, a speed change or a stretch — and you can ask the agent to
          export them as Audacity label text, or import some.
        </li>
      </ul>

      <h2>A/B Compare</h2>
      <p>
        Compare any two session nodes side by side with audio playback of both.
        Useful for deciding between two mixes or comparing before/after an edit.
      </p>
      <ol>
        <li>
          Ask the agent:{" "}
          <code>compare the current version with the one before the reverb</code>,
          or right-click a node in the Graph view and choose "Compare with…"
          to compare it, as B, with the current version.
        </li>
        <li>
          The A/B compare bar appears at the top. Toggle between A and B to hear
          each version.
        </li>
        <li>
          Click <strong>Accept B</strong> to make the B node the new session
          head, or close the bar (<strong>×</strong>) to keep the current
          head.
        </li>
      </ol>
      <p>
        Both versions are rendered into the project&apos;s preview cache, so a
        version you have just played is not rendered again. They stay there
        while the bar is open, even when the two are too long to fit in the
        cache&apos;s 1 GiB together, as an hour-long mix is, and the cache
        clears back down to its size when you close the bar.
      </p>

      <h2>Stem Separation</h2>
      <p>
        <strong>Not available in this build.</strong> Stem separation is
        designed to split a mixed track into vocals, drums, bass, and other
        instruments with Demucs, running entirely on your device — but
        inference ships as a stub, so asking the agent to{" "}
        <code>separate the stems on track 1</code> returns an error.
      </p>
      <p>
        There is no stem model to download and no environment variable that
        changes this. The decoder is what is missing, not the model file:
        supplying a valid Demucs export loads it and then still fails.
      </p>

      <h2>Transcription</h2>
      <p>
        <strong>New in v0.8.0.</strong> Ask the agent to{" "}
        <code>transcribe track 1</code> and it writes out what was said, with
        the start and end time of every word, as a new version of the session.
        It runs on your computer, on the CPU, with the Whisper base speech
        model. No audio is sent anywhere, and transcribing makes no network
        request.
      </p>
      <p>
        The model is not installed with the app. The first time, open Settings
        → Account and press <strong>Download</strong> beside the Speech model
        row (about 139 MB; see Settings → Account below). Until it says{" "}
        <code>Ready</code>, asking for a transcript shows a card under the
        assistant&rsquo;s tool call that says what is missing and has a button
        for Settings. The agent is told not to download or install anything
        itself, and cannot: only your click downloads the model. A build from
        source also needs the ONNX Runtime library, which the installers carry.
      </p>
      <ul>
        <li>
          <strong>What it transcribes.</strong> A track as it sits on the
          timeline (the word times are then seconds of the session), or an
          audio file that is not in the session, by its path (the times are the
          file&rsquo;s). A new transcription replaces the session&rsquo;s
          earlier one.
        </li>
        <li>
          <strong>Language.</strong> Leave it out and the model detects the
          language, or ask for one by its two-letter code, such as{" "}
          <code>en</code>, <code>de</code>, <code>fr</code> or{" "}
          <code>es</code>. The model is the multilingual Whisper base; we have
          checked it only on a short synthetic English recording, so read the
          words before you cut on them.
        </li>
        <li>
          <strong>Speed and Cancel.</strong> A progress strip counts the
          30-second windows of the recording, and Cancel stops it without
          changing the session. How long it takes depends on your CPU; on the
          machine we measured, seven seconds of audio took about three
          seconds the first time. Asking again for the same audio is
          answered from a cache. The whole recording is decoded into memory
          first, which for an hour of stereo audio at 44.1 kHz is about 1.3
          GB.
        </li>
        <li>
          <strong>The Transcript tab.</strong> It shows the words with their
          times. Select words and press Delete to cut that audio and close the
          gap. A transcript remembers the track it is of, so the tab cuts only
          that track and says why when it will not.
        </li>
        <li>
          <strong>Editing by words.</strong> Ask the agent to cut a passage
          (<code>cut_words</code>), to find and remove filler words such as
          um and uh (<code>remove_fillers</code>, which reports what it found
          and changes nothing until you say to apply it), or to select a
          phrase (<code>select_region</code>). Each edit is one undoable step.
        </li>
        <li>
          <strong>What reaches your AI provider.</strong> The words come back
          to the agent as a tool result, so the text of what was said (a
          bounded amount for each call, never the audio) goes to the provider
          you chose, and stays on your computer with Ollama.
        </li>
      </ul>
      <p>
        Everything else in the editor — the timeline, effects, mixing, export —
        works without a transcript or the model.
      </p>

      <h2>Export</h2>
      <p>Export the session to a WAV, FLAC or MP3 file:</p>
      <ul>
        <li>
          <strong>Full session:</strong>{" "}
          <code>export to /Users/me/Desktop/final.wav</code>
        </li>
        <li>
          <strong>Selection only:</strong> select a region on the waveform, then{" "}
          <code>export the selection to /path/output.wav</code>
        </li>
        <li>
          <strong>Specific range:</strong>{" "}
          <code>export seconds 30 to 90 to /path/chorus.wav</code>
        </li>
      </ul>
      <p>
        Exports are non-destructive — the session graph is not changed. You can
        export multiple versions from different nodes without re-doing work.
      </p>

      <h2>Memory</h2>
      <p>
        edytlab has a persistent memory system that lets you store notes the
        agent remembers across all sessions (global) or just for the current
        project (project-level).
      </p>
      <ul>
        <li>
          <strong>Global memory</strong> — preferences, your name, typical
          workflows, instrument preferences.
        </li>
        <li>
          <strong>Project memory</strong> — BPM, key, speaker names, style
          notes, deadlines.
        </li>
      </ul>
      <p>
        Edit memory from Settings → Memory. The agent reads it on every turn
        but cannot write to it — no tool does — so a note like{" "}
        <code>the BPM is 128</code> goes in there by hand.
      </p>

      <h2>Skills</h2>
      <p>
        Skills extend the agent with domain-specific instructions that activate
        based on your message content. For example, a "podcast" skill might
        inject compression and normalization preferences whenever you mention
        "podcast" or "voice" in a message.
      </p>
      <p>Manage skills from Settings → Skills.</p>
      <ul>
        <li>
          <strong>Trigger types:</strong> always (injected every turn), keywords,
          or regex pattern.
        </li>
        <li>
          <strong>Body:</strong> Markdown instructions added to the agent's
          system prompt when the skill matches.
        </li>
      </ul>

      <h3>Skills that come with edytlab</h3>
      <p>
        edytlab ships a few skills of its own (podcast cleanup, music mixing,
        loudness mastering and others) and puts them in your skills folder,{" "}
        <code>~/.edytlab/skills/</code>, the first time it runs. When a release
        corrects one or adds one, the app brings your folder up to date at
        launch, except for what you have changed:
      </p>
      <ul>
        <li>
          A bundled skill you have not edited is replaced by the newer text.
        </li>
        <li>
          One you have edited stays as you left it. The newer text is saved
          beside it as <code>&lt;name&gt;.bundled.md</code>, a file the app does
          not load, so you can compare the two and copy across what you want.
        </li>
        <li>One you have deleted stays deleted.</li>
        <li>A skill you wrote yourself is never touched.</li>
      </ul>
      <p>
        Saving a skill in the editor, even to turn it off, counts as an edit.
        To stop getting a bundled skill's updates, edit it or delete it.
      </p>

      <h2>Agent Profiles</h2>
      <p>
        Profiles let you configure the agent's model, tool set, and behaviour
        for different workflows. Examples:
      </p>
      <ul>
        <li>
          <strong>Podcast profile</strong> — uses a fast, cheap model; restricts
          tools to load, cut, normalize, trim, render.
        </li>
        <li>
          <strong>Mastering profile</strong> — uses Claude Sonnet; has access to
          all tools; injects mastering instructions into the system prompt.
        </li>
      </ul>
      <p>Set the active profile from Settings → Agents.</p>

      <h2>Tips and Keyboard Shortcuts</h2>
      <ul>
        <li>
          Press <code>?</code> at any time to open the full keyboard shortcut
          overlay.
        </li>
        <li>
          Type <code>/</code> in the chat to browse all commands with
          autocomplete — navigate with arrow keys, confirm with Enter.
        </li>
        <li>
          Drag a selection on the waveform, then press <code>L</code> to loop
          just that region during playback.
        </li>
        <li>
          Drag multiple audio files onto the timeline at once to create a
          multi-track session in one step.
        </li>
      </ul>

      <h2>LLM Provider and Model</h2>
      <p>
        Switch providers or models at any time from Settings → Account. No
        restart needed. Your conversation history carries over.
      </p>
      <ul>
        <li>
          <strong>Anthropic Claude Sonnet</strong> — the default; best for
          complex multi-track arrangements and long planning chains.
        </li>
        <li>
          <strong>Anthropic Claude Haiku</strong> — fastest and cheapest; good
          for simple edits.
        </li>
        <li>
          <strong>OpenRouter</strong> — access to 50+ models including
          open-weight options at lower cost.
        </li>
        <li>
          <strong>OpenAI</strong> — reliable tool use; the default is GPT-6
          Luna.
        </li>
        <li>
          <strong>Google Gemini</strong> and <strong>Groq</strong> — hosted
          alternatives, each with its own key.
        </li>
        <li>
          <strong>Ollama</strong> — a local model on your own machine; no key,
          and not even the chat leaves the computer.
        </li>
        <li>
          <strong>ChatGPT plan</strong> (v0.9.0 and later, a preview) — no key:
          the assistant runs on the usage included in your ChatGPT Plus or Pro
          plan. See{" "}
          <a href="#chatgpt-plan">Using your ChatGPT plan</a> below.
        </li>
      </ul>
      <p>
        With Anthropic selected, Settings → Account also has a{" "}
        <strong>Reasoning effort</strong> setting: Default, Low, Medium, High,
        Extra high or Max. Higher effort makes Claude think longer before it
        answers, which is slower and uses more tokens; Default leaves it to the
        model. The ChatGPT plan has its own setting, described below; other
        providers do not have one.
      </p>
      <p>
        At the foot of Settings → Account, one line names the{" "}
        <strong>ONNX Runtime</strong> library the app found, for example{" "}
        <code>ONNX Runtime: 1.23.2 (bundled)</code>, with the file and the
        places it looked below it. It says{" "}
        <code>not installed in this build</code> when there is none, or why the
        one it found cannot be used, such as a macOS older than 13.4. The
        installers of every release from v0.7.0 carry ONNX Runtime 1.23.2, so
        an installed app says <code>bundled</code>; v0.6.1 and earlier, and a
        build from source, say <code>not installed in this build</code>. It is
        a status line: it installs nothing, and it does not mean that any
        on-device audio model is available. No model comes with the runtime,
        and stem separation still returns an error. The speech model has its own
        row below it. Only the library needs macOS 13.4; the app itself runs on macOS 11 and later.
      </p>
      <p>
        From v0.8.0 (v0.7.0 has no such row), the{" "}
        <strong>Speech model</strong> row below it offers the model
        transcription uses, Whisper base (about 139 MB, from Hugging
        Face). <strong>Download</strong> fetches its three files from{" "}
        <code>huggingface.co</code>, with a progress bar and a{" "}
        <strong>Cancel</strong> button, checks each against its SHA-256 hash,
        and keeps them in the <code>models</code> folder inside the
        app&rsquo;s local data folder (<code>app.edytlab.desktop</code> in your
        user data directory; on Windows that is under{" "}
        <code>%LOCALAPPDATA%</code>, so it does not roam with your profile).
        Cancel keeps what has arrived, and Download carries on from there; if
        the download fails, the row says why and offers <strong>Retry</strong>.
        When the model is ready the row says <code>Ready</code> and offers{" "}
        <strong>Remove</strong>, which asks first and deletes only that
        model&rsquo;s files. Nothing downloads unless you press Download: not
        at launch, not when Settings opens, and not when the assistant asks.
        The requests carry your IP address, the file name, and a User-Agent of
        the form <code>edytlab/&lt;version&gt;</code>, so Hugging Face learns
        that edytlab is asking and which version, and nothing about your audio
        (see the <a href="/privacy">privacy page</a>). Once the row says{" "}
        <code>Ready</code>, ask the assistant to transcribe a track (see
        Transcription above); the model is read from your computer and
        transcribing makes no request.
      </p>
      <p>
        The runtime&rsquo;s licence and third-party notices are installed with
        it, in <code>licenses/onnxruntime/</code>: inside{" "}
        <code>edytlab.app/Contents/Resources</code> on macOS, in{" "}
        <code>/usr/lib/edytlab</code> on Linux, and in the install folder on
        Windows, which also has the Visual C++ runtime the library needs and
        its notice in <code>licenses/vc-runtime/</code>.
      </p>
      <p>
        The speech model is not installed with the app, so it has no notice in
        the install folder. Whisper base is published by OpenAI under the
        Apache-2.0 licence (the Whisper code is MIT), and the files are an ONNX
        export of it by the onnx-community on Hugging Face, pinned to one
        commit. The model cards are{" "}
        <a href="https://huggingface.co/openai/whisper-base">
          openai/whisper-base
        </a>{" "}
        and{" "}
        <a href="https://huggingface.co/onnx-community/whisper-base_timestamped">
          onnx-community/whisper-base_timestamped
        </a>
        , the second of which states no licence of its own. edytlab fetches
        the files at run time when you click Download and does not
        redistribute them.
      </p>

      <h2 id="chatgpt-plan">Using your ChatGPT plan</h2>
      <p>
        From v0.9.0, instead of an API key edytlab can run the assistant on a
        ChatGPT plan. It uses OpenAI&apos;s{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source">
          Sign in with ChatGPT
        </a>{" "}
        for open-source apps, which OpenAI documents as a{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations">
          preview
        </a>
        . The assistant&apos;s requests then count toward the usage included in
        your plan, with no per-token bill. Available for ChatGPT Plus and Pro.
        May not be available on workspace plans; that depends on OpenAI and
        your admin. edytlab is not affiliated with OpenAI, which owns the
        ChatGPT name.
      </p>

      <h3>Sign in</h3>
      <ol>
        <li>
          Open Settings (the gear icon) &rarr; Account and choose{" "}
          <strong>ChatGPT plan</strong>. A card takes the place of the key,
          address, model and Test fields; there is no key to save.
        </li>
        <li>
          Choose <strong>Continue with ChatGPT</strong>. edytlab opens your
          browser at OpenAI&apos;s sign-in page, and the card reads
          &ldquo;Finish signing in in your browser&hellip;&rdquo; with a{" "}
          <strong>Cancel</strong> button. The sign-in can take up to five
          minutes.
        </li>
        <li>
          Sign in to the ChatGPT account you want, review what edytlab is
          asking for, and approve. The card then reads &ldquo;Signed in as
          &hellip;&rdquo; with a <strong>Using ChatGPT plan</strong> badge.
        </li>
      </ol>
      <p>
        If your browser shows an error such as &ldquo;organization hasn&apos;t
        enabled access&rdquo; or &ldquo;This app is unavailable&rdquo;, that
        page is OpenAI&apos;s and never reaches edytlab: this account or
        workspace can&apos;t use ChatGPT plan sign-in. After about 20 seconds
        the card says so. Use a personal Plus or Pro account, or an API key.
      </p>

      <h3>The card</h3>
      <p>Once you are signed in, the card offers:</p>
      <ul>
        <li>
          A <strong>model</strong> list, read from your account (the first
          model on it is chosen if you have not picked one), and, when the
          chosen model lists reasoning levels, a{" "}
          <strong>Reasoning effort</strong> list. Higher levels think longer
          and use more of your plan.
        </li>
        <li>
          <strong>Manage usage</strong>, which opens ChatGPT&apos;s Settings
          &rarr; Usage page in your browser. There you see what edytlab and
          your other apps have used and set a weekly limit for edytlab.
          edytlab shows no usage meter of its own.
        </li>
        <li>
          <strong>Sign out</strong>, which asks OpenAI to end the sign-in and
          removes it from this computer.
        </li>
        <li>
          <strong>Use another account</strong>, which signs the first account
          out once the second has signed in.
        </li>
        <li>
          <strong>Start over</strong> (it asks first), which signs out and
          forgets how edytlab was registered with ChatGPT, so the next sign-in
          registers it again. The old entry may stay in ChatGPT&apos;s
          settings until you disconnect it there.
        </li>
      </ul>
      <p>
        If you signed in but turned off plan usage for edytlab on OpenAI&apos;s
        page, the card says so and offers{" "}
        <strong>Turn plan usage back on</strong>. If edytlab cannot read its
        saved sign-in, or the keychain only keeps it until you restart (on
        Linux with no Secret Service running, say), the card says that too.
      </p>
      <p>
        <strong>Signing out here does not disconnect edytlab from ChatGPT.</strong>{" "}
        The card repeats this each time you sign out or start over, because
        OpenAI&apos;s Help Center says that signing out of an app &ldquo;does
        not disconnect it from ChatGPT or stop its authorized use of your
        plan&rdquo;. To disconnect edytlab, open ChatGPT settings, then
        Security and login, then Sign in with ChatGPT, select edytlab and
        choose Disconnect (
        <a href="https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites">
          OpenAI Help Center
        </a>
        ). If edytlab could not confirm that OpenAI ended the sign-in, the
        card tells you to do exactly that.
      </p>

      <h3>Limits and what happens when you reach them</h3>
      <ul>
        <li>
          Each step the assistant takes draws on your plan, and on Plus the
          five-hour limit is shared with every other app and tool that uses
          it; it does not apply to Pro (
          <a href="https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions">
            OpenAI
          </a>
          ). To spend less of it, edytlab skips the small extra request it
          makes with other providers to guess the kind of edit before each
          message. Plan first works as with every provider.
        </li>
        <li>
          An app can reach its own weekly limit while your plan still has
          usage. When a request is refused for usage, Chat shows a card with{" "}
          <strong>Manage usage</strong>, the request ID with a{" "}
          <strong>Copy</strong> button (to quote to OpenAI Support) and{" "}
          <strong>Copy request for admin</strong>, a short note you can send
          an administrator of a workspace. There is no Retry: as OpenAI&apos;s
          Help Center puts it, signing in again or repeatedly retrying does
          not restore usage, and edytlab does not retry on its own or switch to
          another provider or an API key. Letting apps use ChatGPT credits
          once the plan&apos;s usage runs out is a separate choice you make in
          ChatGPT, and edytlab&apos;s limit there must be 100%.
        </li>
        <li>
          When the session ends (it was disconnected, or OpenAI would not
          renew it), Chat shows &ldquo;Your ChatGPT session ended. Continue
          with ChatGPT to finish this message.&rdquo; <strong>Continue with
          ChatGPT</strong> signs you in again and sends your message once more,
          or, if the assistant had already started work, lets it carry on from
          where it stopped. Cancel keeps the card and the message. The message
          is held in memory only, so quitting edytlab forgets it.
        </li>
        <li>
          Other refusals (an account or workspace OpenAI does not allow,
          plan usage turned off, ChatGPT not answering, a request the plan
          route does not accept) get one plain sentence and the request ID.
          ChatGPT not answering offers <strong>Try again</strong>, and plan
          usage turned off offers <strong>Open Settings</strong>.
        </li>
      </ul>
      <p>
        What the sign-in sends and keeps is on the{" "}
        <a href="/privacy#signing-in-with-chatgpt">privacy page</a>.
      </p>
    </DocShell>
  );
}
