/** The "effects-and-templates" page of the user guide: moved from the one long page, unchanged. */
export function EffectsAndTemplates() {
  return (
    <>
      <h2 id="effects-buses-and-the-master">Effects, Buses and the Master</h2>
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

      <h2 id="session-templates">Session Templates</h2>
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
    </>
  );
}
