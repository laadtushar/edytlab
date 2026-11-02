/** The "skills-and-profiles" page of the user guide: moved from the one long page, unchanged. */
export function SkillsAndProfiles() {
  return (
    <>
      <h2 id="skills">Skills</h2>
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

      <h2 id="agent-profiles">Agent Profiles</h2>
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

      <h2 id="tips-and-keyboard-shortcuts">Tips and Keyboard Shortcuts</h2>
      <ul>
        <li>
          Press <code>?</code> for the full keyboard shortcut overlay.
        </li>
        <li>
          Drag a selection on the waveform, then press <code>L</code> to loop
          just that region.
        </li>
      </ul>
    </>
  );
}
