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
          <strong>Graph view</strong> — click the graph icon in the toolbar to
          see all session nodes visualised as a tree. Each node shows its label,
          the tool that created it, and its timestamp.
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
          Supported formats: MP3, WAV, FLAC.
        </li>
        <li>
          <strong>Multi-track sessions</strong> — load multiple files or ask the
          agent to add a track: <code>add an empty track called drums</code>.
        </li>
        <li>
          <strong>Track gain</strong> — adjust the volume of any track
          independently.
        </li>
        <li>
          <strong>Mute</strong> — silence a track without removing it from the
          session.
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
          <strong>Podcast</strong> — host + guest tracks, pre-configured for
          voice normalization and noise reduction.
        </li>
        <li>
          <strong>Music</strong> — lead, harmony, bass, and drums tracks.
        </li>
        <li>
          <strong>Interview</strong> — interviewer + subject tracks.
        </li>
      </ul>
      <p>
        Templates create the track structure and inject matching skills
        automatically. You can still modify the session freely after choosing a
        template.
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
          chat panel renders the preview without playing it.
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
        There is no model download or environment variable that changes this.
        The decoder is what is missing, not the model file: supplying a valid
        Demucs export loads it and then still fails.
      </p>

      <h2>Transcription</h2>
      <p>
        <strong>Not available in this build.</strong> Transcription is designed
        to run on-device with an ONNX Whisper export, producing word-level
        timestamps stored in the session — but the decoder ships as a stub, so
        asking the agent to <code>transcribe track 1</code> returns an error.
        There is no model download or environment variable that changes this
        yet.
      </p>
      <p>
        Text-based editing works from a transcript, so it is unavailable for
        the same reason. Everything else in the editor — the timeline, effects,
        mixing, export — is unaffected.
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
      </ul>
      <p>
        With Anthropic selected, Settings → Account also has a{" "}
        <strong>Reasoning effort</strong> setting: Default, Low, Medium, High,
        Extra high or Max. Higher effort makes Claude think longer before it
        answers, which is slower and uses more tokens; Default leaves it to the
        model. Other providers do not have the setting.
      </p>
      <p>
        At the foot of Settings → Account, one line names the{" "}
        <strong>ONNX Runtime</strong> library the app found, for example{" "}
        <code>ONNX Runtime: 1.23.2 (bundled)</code>, with the file and the
        places it looked below it. It says{" "}
        <code>not installed in this build</code> when there is none, or why the
        one it found cannot be used, such as a macOS older than 13.4. The
        installers of every release after v0.6.1 carry ONNX Runtime 1.23.2, so
        an installed app says <code>bundled</code>; v0.6.1 and earlier, and a
        build from source, say <code>not installed in this build</code>. It is
        a status line: it installs nothing, and it does not mean that any
        on-device audio model is available.
      </p>
      <p>
        The runtime&rsquo;s licence and third-party notices are installed with
        it, in <code>licenses/onnxruntime/</code>: inside{" "}
        <code>edytlab.app/Contents/Resources</code> on macOS, in{" "}
        <code>/usr/lib/edytlab</code> on Linux, and in the install folder on
        Windows, which also has the Visual C++ runtime the library needs and
        its notice in <code>licenses/vc-runtime/</code>.
      </p>
    </DocShell>
  );
}
