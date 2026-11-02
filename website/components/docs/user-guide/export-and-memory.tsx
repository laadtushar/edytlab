/** The "export-and-memory" page of the user guide: moved from the one long page. */
export function ExportAndMemory() {
  return (
    <>
      <h2 id="export">Export</h2>
      <ul>
        <li>
          <strong>Full session:</strong> ask for it, as a WAV, AIFF, FLAC,
          MP3 or OGG (Opus) file:{" "}
          <code>export to /Users/me/Desktop/final.mp3</code>
        </li>
        <li>
          <strong>Part of it:</strong> select a region on the waveform, then
          click <strong>Export Selection</strong> above the chat. It writes
          a WAV of that region.
        </li>
      </ul>
      <p>
        Exports do not change the session graph, so you can export any node.
        An export is refused if its path is audio the project itself plays or
        stores (a file you loaded, or anything inside the project&rsquo;s{" "}
        <code>.audiograph</code> or <code>generated</code> folders), because
        it would replace that audio; choose another path.
      </p>

      <h2 id="memory">Memory</h2>
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
    </>
  );
}
