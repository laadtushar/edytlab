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
      description="Every feature in edytlab."
    >
      <h2>Sessions and the Session Graph</h2>
      <p>
        Every edit creates a <strong>session node</strong>, a snapshot of the
        session. Nodes link to their parent in a directed acyclic graph (DAG),
        so <strong>nothing is destructive</strong>: your original audio files
        are never modified and you can return to any earlier state.
      </p>
      <ul>
        <li>
          <strong>Graph view</strong> — the <strong>Graph</strong> tab, beside
          Timeline and Transcript, draws all session nodes with their label,
          the tool that made them and their age.
        </li>
        <li>
          <strong>Preview a version</strong> — click a node in the graph to
          render that version&apos;s mix and play it. The node gets a dashed
          outline and your place in the history does not move: the next edit
          or undo starts from where you were.
        </li>
        <li>
          <strong>Revert to any node</strong> — right-click a node in the graph
          and choose "Set as head" to jump back to that state. Undo brings you
          back.
        </li>
        <li>
          <strong>Fork</strong> — branch from any node to try another direction.
        </li>
        <li>
          <strong>Undo and redo</strong> — <code>Ctrl/Cmd+Z</code> and{" "}
          <code>Ctrl/Cmd+Shift+Z</code> retrace the steps you took, the
          agent&apos;s edits included. The path is saved with the project, so
          it survives a restart.
        </li>
        <li>
          <strong>Compact history</strong> — ask{" "}
          <code>this project is huge — reclaim some disk</code> and the agent
          reports what it would remove, then, if you confirm, deletes old undo
          steps for good, with the audio the project made for them. A file you
          loaded is never deleted. The most recent steps stay, so ordinary
          undo keeps working.
        </li>
        <li>
          <strong>One writer per project folder</strong> — opening a project is
          refused while a batch run is writing its own project into that
          folder, and while the folder was replaced at the same path (restored
          from a backup, or trashed and made again) and the assistant is still
          working in the old one. Wait for the turn to end, then open it again.
          A batch run is likewise refused, and starts nothing, if one of the
          projects it would write is open.
        </li>
      </ul>

      <h2>Tracks</h2>
      <p>
        A session has one or more tracks, each a sequence of clips. A clip
        describes a region of a source file on disk, not a copy of it.
      </p>
      <ul>
        <li>
          <strong>Load audio</strong> — drag a file onto the timeline, click
          &ldquo;Open Audio&rdquo;, or type <code>load /path/to/file.mp3</code>.
          Formats: WAV, MP3, FLAC and Ogg Vorbis.
        </li>
        <li>
          <strong>Multi-track sessions</strong> — load several files or ask:{" "}
          <code>add an empty track called drums</code>.
        </li>
        <li>
          <strong>Lane controls</strong> — each track&apos;s lane has its own
          gain slider, pan slider, <strong>mute</strong> and{" "}
          <strong>solo</strong> buttons, so you can mix by hand. Double-click a
          pan slider to centre it. A mute silences a track without removing
          it; a solo plays only the soloed tracks. One drag or click is one
          history step, and a control you have just set stays put until the
          session confirms it.
        </li>
      </ul>

      <h2>Effects, Buses and the Master</h2>
      <p>
        An effect you ask for goes into the track&apos;s effect chain. The
        chain is applied at render and your source audio is never rewritten, so
        you can change, switch off or remove an effect later. Effects run in
        chain order.
      </p>
      <ul>
        <li>
          <strong>The master bus</strong> — every track is summed there, so an
          effect on it processes the whole mix, including later edits:{" "}
          <code>put a limiter on the master at -1 dB</code>.
        </li>
        <li>
          <strong>Buses</strong> — a shared chain that tracks send into:{" "}
          <code>create a bus called Drums</code>, send tracks to it, then{" "}
          <code>put a low-pass at 8 kHz on the Drums bus</code>. A bus effect
          is heard only when a track sends to it.
        </li>
        <li>
          <strong>Hear it first</strong> — <code>what would a 1 kHz low-pass
          sound like on the vocal?</code> plays a few seconds in the chat, on a
          track, the master or a bus, and changes nothing. Ask again to keep it.
        </li>
        <li>
          <strong>Five effects work in a chain today</strong> — gain, limiter
          and the low-pass, high-pass and notch filters. Reverb, echo, EQ, the
          compressor and the rest are separate edits that bake their result
          into a new version of the track, and the agent says so if you ask
          for one in a chain.
        </li>
        <li>
          <strong>Setting names</strong> — a gain has <code>db</code>, a
          limiter <code>ceiling_db</code> and <code>release_ms</code>, the
          low-pass and high-pass filters <code>cutoff_hz</code>, and the notch{" "}
          <code>center_hz</code> and <code>q</code>. A setting name an effect
          does not have is refused, with the right names in the message.
        </li>
      </ul>
      <p>
        A project saved by an older build can hold an effect this build cannot
        run. Rendering leaves it out rather than failing, and says so: a notice
        above the timeline names the effect and where it sits, Export
        Selection notes what the file was written without, and the A/B compare
        bar lists each side&apos;s left-out effects, marked A or B. Remove or
        bypass the effect and the notice goes.
      </p>

      <h2>Session Templates</h2>
      <p>
        With no audio loaded, <strong>Start from template</strong> offers:
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
        A template makes empty tracks with a starting gain and pan, and adds no
        audio, effects or skills. Each track shows as an empty lane until you
        put audio on it. Skills switch on when your messages match
        their triggers, template or not. Change the session freely afterwards.
      </p>

      <h2>The Chat Interface</h2>
      <p>
        The chat panel on the right is where you direct the agent, in plain
        English. Type <code>/</code> for a dropdown of commands (arrow keys,
        Enter). The agent streams its response, shows a badge for each tool it
        runs, says what it did and why, and takes follow-ups:{" "}
        <code>actually make it 2 dB higher instead</code>.
      </p>

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
          <strong>Chain operations</strong> in one message; the agent plans the
          whole chain.
        </li>
        <li>
          <strong>Reference markers</strong> you have set:{" "}
          <code>cut everything before the chorus marker</code>.
        </li>
      </ul>

      <h3>Big sessions</h3>
      <p>
        Each turn the assistant gets a short summary of the session: tracks,
        buses, master effects and markers, capped at 8,000 bytes for the
        hosted providers and 2,300 for Ollama. A list that is too long is cut,
        the summary says how many were left out, and the assistant reads the
        rest a page at a time with the <code>list_session</code> tool. It only
        reads, so Plan first never holds it for approval. With Ollama the tool
        is always sent when a list was cut.
      </p>

      <h3>Plan first</h3>
      <p>
        With the <strong>Plan</strong> toggle above the chat box on,{" "}
        <strong>no edit runs until you approve it</strong>. The toggle keeps
        its setting across restarts.
      </p>
      <ul>
        <li>
          The agent shows a plan card. <strong>Run</strong> approves it,{" "}
          <strong>Discard</strong> ends the turn without applying anything, and{" "}
          <strong>Edit</strong> lets you reword a step before you run it.
        </li>
        <li>
          If the agent answers without a plan, the card shows the first edit it
          is about to make, with its exact tool and settings (for example{" "}
          <code>reverse</code>, <code>track: 0</code>). Approve it and the rest
          of that turn runs without asking again. Discard it and nothing runs,
          its badge reads <em>not run</em>, and the agent is told you declined.
        </li>
        <li>
          If you reword a step instead, nothing runs: the agent proposes again
          and you approve that.
        </li>
        <li>
          Questions that only read the session (tempo, loudness, spectrum) run
          without asking. So do hearing an effect before you add it and
          rendering a preview: they change nothing in your project, and what
          they save is a disposable copy cleared as it grows.
        </li>
      </ul>

      <h2>Playback</h2>
      <ul>
        <li>
          <strong>Space</strong> — play / pause, like the Play button above the
          timeline. After you open a file or edit, Play renders the new preview
          first and starts when it is ready; press it again to cancel.{" "}
          <strong>Preview</strong> in the chat panel renders without playing.
          The mix you are playing is held in the preview cache, so switching to
          the Graph or Transcript tab and back does not lose it; if the file
          has gone anyway, it is rendered again once.
        </li>
        <li>
          <strong>L</strong> — toggle loop; playback loops the selected region.
        </li>
        <li>
          <strong>Click the waveform</strong> — jump to that position.
        </li>
        <li>
          <strong>Ctrl+scroll</strong> or <strong>+/−/0</strong> — zoom the
          waveform in/out/reset.
        </li>
        <li>
          <strong>Drag on the waveform</strong> — select a range (for range
          export, loop playback and agent context).
        </li>
      </ul>

      <h2>Markers and Selections</h2>
      <p>
        Markers annotate points and regions; the agent can refer to them by name.
      </p>
      <ul>
        <li>
          Ask the agent to add markers:{" "}
          <code>mark the verse start at 0:30</code>.
        </li>
        <li>
          Drag on the waveform to select a region; the agent gets it as context.
        </li>
        <li>
          Use <strong>Esc</strong> to clear the selection.
        </li>
        <li>
          <strong>The label lane</strong> — the lane under the tracks is where
          you name chapters and regions yourself. Double-click empty space in
          it, type a name and press Enter (Esc cancels); clicking the ruler
          opens the same name box. Double-click a label to rename it, drag it
          to move it (a region keeps its length), right-click to delete it.
          Each is one history step. Labels stay with the audio when an edit
          shifts time (a cut, trim, inserted silence, paste, speed change or
          stretch), and the agent can export them as Audacity label text, or
          import some.
        </li>
      </ul>

      <h2>A/B Compare</h2>
      <p>
        Compare any two session nodes and play both.
      </p>
      <ol>
        <li>
          Ask the agent:{" "}
          <code>compare the current version with the one before the reverb</code>,
          or right-click a node in the Graph view and choose &ldquo;Compare
          with…&rdquo; to compare it, as B, with the current version.
        </li>
        <li>
          The A/B compare bar appears at the top. Toggle between A and B to
          hear each.
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
        while the bar is open, even when the two together exceed the
        cache&apos;s 1 GiB (an hour-long mix does); closing the bar clears it
        back down.
      </p>

      <h2>Stem Separation</h2>
      <p>
        <strong>Not available in this build.</strong> Stem separation is meant
        to split a track into vocals, drums, bass and other with Demucs, on
        your device, but inference ships as a stub, so asking the agent to{" "}
        <code>separate the stems on track 1</code> returns an error. There is
        no stem model to download and no environment variable that changes
        this: the decoder is what is missing, not the model file.
      </p>

      <h2 id="transcription">Transcription</h2>
      <p>
        Ask the agent to <code>transcribe track 1</code> and it
        writes what was said, with the start and end time of every word, as a
        new version of the session. It runs on your CPU with the Whisper base
        speech model. No audio is sent anywhere, and transcribing makes no
        network request.
      </p>
      <p>
        The model is not installed with the app. The first time, open Settings
        → Account and press <strong>Download</strong> beside the Speech model
        row (about 139 MB; see below). Until it says <code>Ready</code>, a
        transcript request shows a card saying what is missing, with a button
        for Settings. The agent cannot download the model: only your click
        does. A build from source also needs the ONNX Runtime library, which
        the installers carry.
      </p>
      <ul>
        <li>
          <strong>What it transcribes.</strong> A track as it sits on the
          timeline (word times are session seconds), or an audio file outside
          the session, by path (times are the file&rsquo;s). A new transcription
          replaces the earlier one.
        </li>
        <li>
          <strong>Language.</strong> The model detects it, or ask for one by
          two-letter code (<code>en</code>, <code>de</code>, <code>fr</code>,{" "}
          <code>es</code>). The model is multilingual Whisper base, checked
          only on a short synthetic English recording, so read the words before
          you cut on them.
        </li>
        <li>
          <strong>Speed and Cancel.</strong> A progress strip counts the
          30-second windows, and Cancel stops it without changing the session.
          Speed depends on your CPU: on our test machine seven seconds of audio
          took about three seconds the first time, and the same audio again is
          answered from a cache. The whole recording is decoded into memory
          first: about 1.3 GB for an hour of 44.1 kHz stereo.
        </li>
        <li>
          <strong>The Transcript tab.</strong> It shows the words with their
          times. Select words and press Delete to cut that audio and close the
          gap. The tab cuts only the track the transcript is of, and says why
          when it will not.
        </li>
        <li>
          <strong>Editing by words.</strong> Ask the agent to cut a passage
          (<code>cut_words</code>), find and remove fillers such as um and uh
          (<code>remove_fillers</code>, which changes nothing until you say to
          apply it), or select a phrase (<code>select_region</code>). Each edit
          is one undoable step.
        </li>
        <li>
          <strong>What reaches your AI provider.</strong> The words come back as
          a tool result, so the text of what was said (a bounded amount per
          call, never the audio) goes to the provider you chose, and stays on
          your computer with Ollama.
        </li>
      </ul>
      <p>The rest of the editor works without a transcript or the model.</p>

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
        Exports do not change the session graph, so you can export any node.
      </p>

      <h2>Memory</h2>
      <p>
        Notes the agent remembers across all sessions (global) or for one
        project.
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
        Edit memory in Settings → Memory. The agent reads it every turn but
        cannot write to it, so a note like <code>the BPM is 128</code> goes in
        by hand.
      </p>

      <h2>Skills</h2>
      <p>
        Skills add instructions to the agent when your message matches. A
        &ldquo;podcast&rdquo; skill might inject compression and normalization
        preferences whenever you mention &ldquo;podcast&rdquo; or
        &ldquo;voice&rdquo;. Manage them in Settings → Skills.
      </p>
      <ul>
        <li>
          <strong>Trigger types:</strong> always (injected every turn), keywords,
          or regex pattern.
        </li>
        <li>
          <strong>Body:</strong> Markdown added to the agent&apos;s system
          prompt when the skill matches.
        </li>
      </ul>

      <h3>Skills that come with edytlab</h3>
      <p>
        edytlab puts a few skills of its own (podcast cleanup, music mixing,
        loudness mastering and others) in <code>~/.edytlab/skills/</code> on
        first run, and brings them up to date at launch, except for what you
        have changed:
      </p>
      <ul>
        <li>
          A bundled skill you have not edited is replaced by the newer text.
        </li>
        <li>
          One you have edited stays as you left it; the newer text is saved
          beside it as <code>&lt;name&gt;.bundled.md</code>, which the app does
          not load, so you can compare and copy across.
        </li>
        <li>One you have deleted stays deleted.</li>
        <li>A skill you wrote yourself is never touched.</li>
      </ul>
      <p>
        Saving a skill in the editor, even to turn it off, counts as an edit.
        To stop a bundled skill&apos;s updates, edit or delete it.
      </p>

      <h2>Agent Profiles</h2>
      <p>
        Profiles set the agent&apos;s model, tool set and behaviour per workflow,
        in Settings → Agents. Examples:
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

      <h2>Tips and Keyboard Shortcuts</h2>
      <ul>
        <li>
          Press <code>?</code> for the full keyboard shortcut overlay.
        </li>
        <li>
          Drag a selection on the waveform, then press <code>L</code> to loop
          just that region.
        </li>
      </ul>

      <h2>LLM Provider and Model</h2>
      <p>
        Switch providers or models in Settings → Account, with no restart. Your
        conversation history carries over.
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
          <strong>ChatGPT plan</strong> (a preview) — no key:
          the assistant runs on the usage included in your ChatGPT Plus or Pro
          plan. See{" "}
          <a href="#chatgpt-plan">Using your ChatGPT plan</a> below.
        </li>
      </ul>
      <p>
        With Anthropic selected, Settings → Account also has a{" "}
        <strong>Reasoning effort</strong> setting: Default, Low, Medium, High,
        Extra high or Max. Higher effort makes Claude think longer, which is
        slower and uses more tokens; Default leaves it to the model. The
        ChatGPT plan has its own, described below; other providers have none.
      </p>
      <p>
        At the foot of Settings → Account, one line names the{" "}
        <strong>ONNX Runtime</strong> library the app found, for example{" "}
        <code>ONNX Runtime: 1.23.2 (bundled)</code>, with the file and the
        places it looked below it. It says{" "}
        <code>not installed in this build</code> when there is none, or why the
        one it found cannot be used, such as a macOS older than 13.4 (only the
        library needs it; the app runs on macOS 11 and later). An installed app says{" "}
        <code>bundled</code>; a build from source says{" "}
        <code>not installed in this build</code>. It is a status line: it
        installs nothing and does not mean any on-device audio model is
        available. No model comes with the runtime, and stem separation still
        returns an error.
      </p>
      <p>
        The{" "}
        <strong>Speech model</strong> row below it offers Whisper base (about
        139 MB, from Hugging Face). <strong>Download</strong> fetches its three
        files from <code>huggingface.co</code>, with a progress bar and a{" "}
        <strong>Cancel</strong> button, checks each against its SHA-256 hash,
        and keeps them in the <code>models</code> folder of the app&rsquo;s
        local data folder (<code>app.edytlab.desktop</code>; on Windows under{" "}
        <code>%LOCALAPPDATA%</code>, so it does not roam). Cancel keeps what
        has arrived and Download carries on from there; a failure shows why,
        with <strong>Retry</strong>. When the row says <code>Ready</code> it
        offers <strong>Remove</strong>, which asks first and deletes only that
        model&rsquo;s files. Nothing downloads unless you press Download: not
        at launch, not when Settings opens, not when the assistant asks. The
        requests carry your IP address, the file name, and a User-Agent of the
        form <code>edytlab/&lt;version&gt;</code>, so Hugging Face learns that
        edytlab is asking and which version, and nothing about your audio (see
        the <a href="/privacy">privacy page</a>).
      </p>
      <p>
        The runtime&rsquo;s licence and notices are installed with it, in{" "}
        <code>licenses/onnxruntime/</code>: inside{" "}
        <code>edytlab.app/Contents/Resources</code> on macOS, in{" "}
        <code>/usr/lib/edytlab</code> on Linux, and in the install folder on
        Windows, which also has the Visual C++ runtime the library needs and
        its notice in <code>licenses/vc-runtime/</code>.
      </p>
      <p>
        The speech model has no notice in the install folder, since it is not
        installed with the app. Whisper base is published by OpenAI under the
        Apache-2.0 licence (the Whisper code is MIT); the files are an ONNX
        export by the onnx-community on Hugging Face, pinned to one commit. The
        model cards are{" "}
        <a href="https://huggingface.co/openai/whisper-base">
          openai/whisper-base
        </a>{" "}
        and{" "}
        <a href="https://huggingface.co/onnx-community/whisper-base_timestamped">
          onnx-community/whisper-base_timestamped
        </a>
        , the second of which states no licence of its own. edytlab fetches the
        files when you click Download and does not redistribute them.
      </p>

      <h2 id="chatgpt-plan">Using your ChatGPT plan</h2>
      <p>
        edytlab can run the assistant on a ChatGPT plan instead of
        an API key, using OpenAI&apos;s{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source">
          Sign in with ChatGPT
        </a>{" "}
        for open-source apps, which OpenAI documents as a{" "}
        <a href="https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations">
          preview
        </a>
        . Requests count toward the usage included in your plan, with no
        per-token bill. Available for ChatGPT Plus and Pro. May not be
        available on workspace plans; that depends on OpenAI and your admin.
        edytlab is not affiliated with OpenAI, which owns the ChatGPT name.
      </p>

      <h3>Sign in</h3>
      <ol>
        <li>
          Open Settings (the gear icon) &rarr; Account and choose{" "}
          <strong>ChatGPT plan</strong>. A card replaces the key, address,
          model and Test fields.
        </li>
        <li>
          Choose <strong>Continue with ChatGPT</strong>. edytlab opens
          OpenAI&apos;s sign-in page in your browser, and the card reads
          &ldquo;Finish signing in in your browser&hellip;&rdquo; with a{" "}
          <strong>Cancel</strong> button. You have up to five minutes.
        </li>
        <li>
          Sign in, review what edytlab asks for, and approve. The card reads
          &ldquo;Signed in as &hellip;&rdquo; with a{" "}
          <strong>Using ChatGPT plan</strong> badge.
        </li>
      </ol>
      <p>
        A browser error such as &ldquo;organization hasn&apos;t enabled
        access&rdquo; or &ldquo;This app is unavailable&rdquo; is OpenAI&apos;s
        and never reaches edytlab: this account or workspace can&apos;t use
        ChatGPT plan sign-in. After about 20 seconds the card says so. Use a
        personal Plus or Pro account, or an API key.
      </p>

      <h3>The card</h3>
      <ul>
        <li>
          A <strong>model</strong> list, read from your account (the first is
          chosen if you have not picked one), and, when the model lists
          reasoning levels, a <strong>Reasoning effort</strong> list. Higher
          levels think longer and use more of your plan.
        </li>
        <li>
          <strong>Manage usage</strong>, which opens ChatGPT&apos;s Settings
          &rarr; Usage page in your browser, where you see what edytlab and
          your other apps have used and set a weekly limit for edytlab.
          edytlab has no usage meter of its own.
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
          registers again. The old entry may stay in ChatGPT&apos;s settings
          until you disconnect it there.
        </li>
      </ul>
      <p>
        If you turned off plan usage for edytlab on OpenAI&apos;s page, the card
        says so and offers <strong>Turn plan usage back on</strong>. It also
        says when edytlab cannot read its saved sign-in, or the keychain keeps
        it only until you restart (Linux with no Secret Service, say).
      </p>
      <p>
        <strong>Signing out here does not disconnect edytlab from ChatGPT.</strong>{" "}
        The card says so each time, because OpenAI&apos;s Help Center says that
        signing out of an app &ldquo;does not disconnect it from ChatGPT or
        stop its authorized use of your plan&rdquo;. To disconnect edytlab,
        open ChatGPT settings, then Security and login, then Sign in with
        ChatGPT, select edytlab and choose Disconnect (
        <a href="https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites">
          OpenAI Help Center
        </a>
        ). If edytlab could not confirm that OpenAI ended the sign-in, the
        card tells you to do this.
      </p>

      <h3>Limits and what happens when you reach them</h3>
      <ul>
        <li>
          Each step the assistant takes draws on your plan, and on Plus the
          five-hour limit is shared with every other app and tool that uses
          the plan; it does not apply to Pro (
          <a href="https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions">
            OpenAI
          </a>
          ). To spend less, edytlab skips the small extra request it makes with
          other providers to guess the kind of edit before each message. Plan
          first works as with every provider.
        </li>
        <li>
          An app can reach its own weekly limit while your plan still has
          usage. When a request is refused for usage, Chat shows a card with{" "}
          <strong>Manage usage</strong>, the request ID with a{" "}
          <strong>Copy</strong> button (to quote to OpenAI Support) and{" "}
          <strong>Copy request for admin</strong>, a short note for a workspace
          administrator. There is no Retry: signing in again or retrying does
          not restore usage, and edytlab does not retry or switch to another
          provider or an API key. Letting apps use ChatGPT credits once the
          plan&apos;s usage runs out is a separate choice you make in ChatGPT,
          and edytlab&apos;s limit there must be 100%.
        </li>
        <li>
          When the session ends (disconnected, or OpenAI would not renew it),
          Chat shows &ldquo;Your ChatGPT session ended. Continue with ChatGPT
          to finish this message.&rdquo; <strong>Continue with ChatGPT</strong>{" "}
          signs you in again and resends your message, or, if the assistant had
          started work, lets it carry on from where it stopped. Cancel keeps
          the card and the message. The message is held in memory only, so
          quitting edytlab forgets it.
        </li>
        <li>
          Other refusals (an account or workspace OpenAI does not allow, plan
          usage turned off, ChatGPT not answering, a request the plan route
          does not accept) get one sentence and the request ID. ChatGPT not
          answering offers <strong>Try again</strong>; plan usage turned off
          offers <strong>Open Settings</strong>.
        </li>
      </ul>
      <p>
        What the sign-in sends and keeps is on the{" "}
        <a href="/privacy#signing-in-with-chatgpt">privacy page</a>.
      </p>
    </DocShell>
  );
}
