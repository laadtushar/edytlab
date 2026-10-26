/** The "chat" page of the user guide: moved from the one long page, unchanged. */
export function Chat() {
  return (
    <>
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
    </>
  );
}
