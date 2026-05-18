import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "User Guide",
  description:
    "Complete user guide for edytlab — sessions, tracks, chat interface, A/B compare, export, memory, and skills.",
  alternates: { canonical: "/docs/user-guide" },
  openGraph: {
    title: "User Guide — edytlab Docs",
    description: "Everything you need to know to use edytlab effectively.",
    url: `${siteConfig.url}/docs/user-guide`,
  },
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
          <strong>Revert to any node</strong> — click a node in the graph and
          select "Set as head" to jump back to that state.
        </li>
        <li>
          <strong>Fork</strong> — branch from any node to explore a different
          edit direction without losing your current work.
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

      <h2>Playback</h2>
      <ul>
        <li>
          <strong>Space</strong> — play / pause from the current position.
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
          or use the Graph view to select two nodes and click "Compare".
        </li>
        <li>
          The A/B compare bar appears at the top. Toggle between A and B to hear
          each version.
        </li>
        <li>
          Click <strong>Accept B</strong> to make the B node the new session
          head, or <strong>Dismiss</strong> to keep the current head.
        </li>
      </ol>

      <h2>Stem Separation</h2>
      <p>
        edytlab integrates Demucs to separate a mixed track into individual
        stems — vocals, drums, bass, and other instruments — running entirely
        on your device.
      </p>
      <p>
        Ask the agent: <code>separate the stems on track 1</code>. The model
        downloads automatically on first use (~80 MB). Processing takes roughly
        45 seconds per minute of audio on a modern CPU. Apple Neural Engine and
        NVIDIA CUDA acceleration reduce this significantly.
      </p>
      <p>
        The four stems appear as new tracks in your session. You can then edit
        each independently.
      </p>

      <h2>Transcription</h2>
      <p>
        edytlab uses Whisper large-v3 to transcribe spoken audio to text with
        word-level timestamps. The model runs on-device (~1.5 GB, downloaded on
        first use).
      </p>
      <p>
        Ask the agent: <code>transcribe track 1</code>. A 60-minute file
        transcribes in approximately 4–8 minutes on CPU. The transcript is
        stored in the session and can be referenced by the agent.
      </p>

      <h2>Export</h2>
      <p>Export the session to a WAV file:</p>
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
        Access memory from Settings → Memory. The agent can also write to memory
        directly: <code>remember that the BPM is 128</code>.
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

      <h2>Agent Profiles</h2>
      <p>
        Profiles let you configure the agent's model, tool set, and behaviour
        for different workflows. Examples:
      </p>
      <ul>
        <li>
          <strong>Podcast profile</strong> — uses a fast, cheap model; restricts
          tools to load, cut, normalize, trim, transcribe, render.
        </li>
        <li>
          <strong>Mastering profile</strong> — uses Claude Sonnet; has access to
          all tools; injects mastering instructions into the system prompt.
        </li>
      </ul>
      <p>Set the active profile from Settings → Agent Profiles.</p>

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
        Switch providers or models at any time from Settings → Provider. No
        restart needed. Your conversation history carries over.
      </p>
      <ul>
        <li>
          <strong>Anthropic Claude Sonnet 4.6</strong> — best for complex
          multi-track arrangements and long planning chains.
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
          <strong>OpenAI GPT-4o</strong> — reliable tool use; good general
          performance.
        </li>
      </ul>
    </DocShell>
  );
}
