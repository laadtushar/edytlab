import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "Getting Started",
  description:
    "Install edytlab, set up your API key, and edit your first audio file in under 5 minutes.",
  alternates: { canonical: "/docs/getting-started" },
  openGraph: {
    title: "Getting Started — edytlab Docs",
    description: "Install edytlab, configure your LLM provider, and run your first session.",
    url: `${siteConfig.url}/docs/getting-started`,
  },
};

export default function GettingStartedPage() {
  return (
    <DocShell
      title="Getting Started"
      description="Install edytlab, configure your LLM provider, and edit your first audio file."
    >
      <h2>1. Download and install</h2>
      <p>
        Download the latest signed installer for your platform from the{" "}
        <a href={siteConfig.releases} target="_blank" rel="noopener noreferrer">
          GitHub Releases page
        </a>
        .
      </p>
      <table>
        <thead>
          <tr>
            <th>Platform</th>
            <th>Installer</th>
            <th>Notes</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>macOS (Apple Silicon + Intel)</td>
            <td>.dmg (Universal)</td>
            <td>Drag to Applications. Apple-notarized.</td>
          </tr>
          <tr>
            <td>Windows 10 / 11</td>
            <td>.msi or .exe (NSIS)</td>
            <td>WebView2 installed automatically if absent.</td>
          </tr>
        </tbody>
      </table>
      <blockquote>
        <strong>macOS Gatekeeper:</strong> If you see "cannot be opened because
        the developer cannot be verified", right-click the app and choose Open,
        then confirm. This prompt only appears once.
      </blockquote>
      <blockquote>
        <strong>Windows SmartScreen:</strong> Click "More info" → "Run anyway"
        on first launch. SmartScreen reputation builds over time as more users
        install the app.
      </blockquote>

      <h2>2. Get an API key</h2>
      <p>
        edytlab uses an LLM to understand your editing instructions. You bring
        your own API key — it is stored in your OS keychain and never sent to
        edytlab servers.
      </p>
      <table>
        <thead>
          <tr>
            <th>Provider</th>
            <th>Where to get a key</th>
            <th>Free tier?</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Anthropic (recommended)</td>
            <td>
              <a href="https://console.anthropic.com/settings/keys" target="_blank" rel="noopener noreferrer">
                console.anthropic.com/settings/keys
              </a>
            </td>
            <td>No — pay-as-you-go</td>
          </tr>
          <tr>
            <td>OpenRouter</td>
            <td>
              <a href="https://openrouter.ai/keys" target="_blank" rel="noopener noreferrer">
                openrouter.ai/keys
              </a>
            </td>
            <td>Yes — free models available</td>
          </tr>
          <tr>
            <td>OpenAI</td>
            <td>
              <a href="https://platform.openai.com/api-keys" target="_blank" rel="noopener noreferrer">
                platform.openai.com/api-keys
              </a>
            </td>
            <td>No — pay-as-you-go</td>
          </tr>
        </tbody>
      </table>
      <blockquote>
        <strong>Cost estimate:</strong> A typical 30-minute editing session
        consumes roughly $0.05–0.50 in LLM tokens depending on the model and
        session complexity. Haiku / GPT-4o mini cost ~10× less than Sonnet /
        GPT-4o.
      </blockquote>

      <h2>3. Enter your API key</h2>
      <ol>
        <li>Launch edytlab.</li>
        <li>Click the gear icon (⚙) in the top-right corner.</li>
        <li>
          Select your provider (Anthropic, OpenRouter, or OpenAI) from the
          dropdown.
        </li>
        <li>Paste your API key into the field and press Save.</li>
        <li>
          edytlab validates the key immediately with a 1-token test request. A
          green checkmark means you are ready.
        </li>
      </ol>
      <p>
        Keys are stored using your OS native keychain (macOS Keychain,
        Windows Credential Manager). They are never written to disk or sent to
        any edytlab server.
      </p>

      <h2>4. Load your first audio file</h2>
      <p>There are three ways to load audio:</p>
      <ul>
        <li>
          <strong>Drag and drop</strong> — drag an MP3, WAV, or FLAC file
          directly onto the timeline area. Drag multiple files at once to create
          a multi-track session automatically.
        </li>
        <li>
          <strong>Open button</strong> — click "Open Audio" in the empty state
          or the file menu.
        </li>
        <li>
          <strong>Chat</strong> — type a message like{" "}
          <code>load /path/to/file.wav</code> in the chat panel.
        </li>
      </ul>
      <p>
        Once loaded, the waveform appears in the timeline and the session is
        ready for editing.
      </p>

      <h2>5. Your first edit</h2>
      <p>
        Type in the chat panel. Describe what you want in plain English — the
        agent figures out which tools to run.
      </p>
      <p>Example prompts to try:</p>
      <ul>
        <li>
          <code>Remove the silence at the beginning</code>
        </li>
        <li>
          <code>Normalize to -14 LUFS</code>
        </li>
        <li>
          <code>Separate the stems</code>
        </li>
        <li>
          <code>Transcribe this audio</code>
        </li>
        <li>
          <code>Export to /Users/me/Desktop/output.wav</code>
        </li>
      </ul>
      <p>
        The agent streams its response in real time, showing tool call badges as
        it executes each operation. Every change creates a new node in the
        session graph — nothing is destructive.
      </p>

      <h2>6. Undo and redo</h2>
      <p>
        Every edit is stored as a node in a directed acyclic graph (DAG). You
        can undo and redo freely:
      </p>
      <ul>
        <li>
          <strong>Ctrl+Z</strong> (Cmd+Z on Mac) — undo
        </li>
        <li>
          <strong>Ctrl+Y</strong> (Cmd+Y on Mac) — redo
        </li>
      </ul>
      <p>
        Unlike a linear undo stack, the DAG preserves all branching history.
        You can fork a session, try a different edit, and switch between
        branches using the Graph view.
      </p>

      <h2>Keyboard shortcuts</h2>
      <p>
        Press <code>?</code> at any time to see the full shortcuts overlay.
        Key shortcuts:
      </p>
      <table>
        <thead>
          <tr>
            <th>Key</th>
            <th>Action</th>
          </tr>
        </thead>
        <tbody>
          <tr><td>Space</td><td>Play / Pause</td></tr>
          <tr><td>L</td><td>Toggle loop playback (loops the selected region)</td></tr>
          <tr><td>Ctrl/Cmd + Z</td><td>Undo</td></tr>
          <tr><td>Ctrl/Cmd + Y</td><td>Redo</td></tr>
          <tr><td>Ctrl + Scroll</td><td>Zoom waveform</td></tr>
          <tr><td>+</td><td>Zoom in</td></tr>
          <tr><td>−</td><td>Zoom out</td></tr>
          <tr><td>0</td><td>Reset zoom</td></tr>
          <tr><td>?</td><td>Show shortcuts</td></tr>
          <tr><td>Esc</td><td>Clear selection</td></tr>
        </tbody>
      </table>

      <h2>Next steps</h2>
      <ul>
        <li>
          <a href="/docs/user-guide">User Guide</a> — full coverage of every
          feature
        </li>
        <li>
          <a href="/docs/tools">Audio Tools Reference</a> — all 69 tools the
          agent can call
        </li>
        <li>
          <a href="/docs/developer-guide">Developer Guide</a> — build from
          source, run tests, contribute
        </li>
      </ul>
    </DocShell>
  );
}
