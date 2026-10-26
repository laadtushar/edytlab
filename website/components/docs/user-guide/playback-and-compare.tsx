/** The "playback-and-compare" page of the user guide: moved from the one long page, unchanged. */
export function PlaybackAndCompare() {
  return (
    <>
      <h2 id="playback">Playback</h2>
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

      <h2 id="markers-and-selections">Markers and Selections</h2>
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

      <h2 id="a-b-compare">A/B Compare</h2>
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
    </>
  );
}
