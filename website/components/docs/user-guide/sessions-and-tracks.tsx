/** The "sessions-and-tracks" page of the user guide: moved from the one long page, unchanged. */
export function SessionsAndTracks() {
  return (
    <>
      <h2 id="sessions-and-the-session-graph">Sessions and the Session Graph</h2>
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

      <h2 id="tracks">Tracks</h2>
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
    </>
  );
}
