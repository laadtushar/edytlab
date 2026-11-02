/** The "export-and-memory" page of the user guide: moved from the one long page, unchanged. */
export function ExportAndMemory() {
  return (
    <>
      <h2 id="export">Export</h2>
      <p>Ask the agent to export the whole session or a range to WAV, AIFF, FLAC, MP3 or OGG:</p>
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
        The Export Selection button also saves the selected region as WAV. Exports do not change the session graph, so you can export any node. Choose a destination outside the project audio folders and away from files the project reads.
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
