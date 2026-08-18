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
        Download the latest build for your platform from the{" "}
        <a href={siteConfig.releases} target="_blank" rel="noopener noreferrer">
          GitHub Releases page
        </a>
        .
      </p>
      <blockquote>
        <strong>These are unsigned developer previews.</strong> Code-signing
        certificates aren&rsquo;t provisioned yet, so macOS and Windows will
        both object on first launch. Neither download is broken — the steps
        below get you past it.
      </blockquote>
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
            <td>macOS (Apple Silicon or Intel)</td>
            <td>.dmg (universal)</td>
            <td>
              One universal binary covers both architectures. Drag to
              Applications, then run the command below.
            </td>
          </tr>
          <tr>
            <td>Windows 10 / 11</td>
            <td>.msi or .exe (NSIS)</td>
            <td>WebView2 installed automatically if absent.</td>
          </tr>
          <tr>
            <td>Linux</td>
            <td>.deb or AppImage</td>
            <td>x86_64. No signing step needed.</td>
          </tr>
        </tbody>
      </table>

      <h3>macOS: &ldquo;edytlab is damaged and can&rsquo;t be opened&rdquo;</h3>
      <p>
        The app is not damaged. macOS attaches a quarantine flag to anything
        downloaded from the internet, and refuses to launch a quarantined app
        that Apple hasn&rsquo;t notarized. Because the refusal is phrased as
        damage, the dialog offers only <em>Move to Trash</em> — there is no
        &ldquo;Open Anyway&rdquo; button to click, and right-click &rarr; Open
        will not help either.
      </p>
      <p>Remove the flag after dragging the app to Applications:</p>
      <pre>
        <code>xattr -dr com.apple.quarantine /Applications/edytlab.app</code>
      </pre>
      <p>
        Then open it normally. You only need to do this once per install. If
        you see a milder &ldquo;unidentified developer&rdquo; message instead,
        System Settings &rarr; Privacy &amp; Security &rarr;{" "}
        <strong>Open Anyway</strong> is enough on its own.
      </p>
      <p>
        This goes away once notarization is set up; until then every release
        needs the command above.
      </p>

      <h3>Windows: SmartScreen</h3>
      <p>
        Click &ldquo;More info&rdquo; &rarr; &ldquo;Run anyway&rdquo; on first
        launch. SmartScreen reputation is earned through download volume and
        cannot be shortcut, so this warning persists until the installers are
        signed.
      </p>

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
            <td>Google Gemini</td>
            <td>
              <a href="https://aistudio.google.com/apikey" target="_blank" rel="noopener noreferrer">
                aistudio.google.com/apikey
              </a>
            </td>
            <td>Yes — free tier available</td>
          </tr>
          <tr>
            <td>Groq</td>
            <td>
              <a href="https://console.groq.com/keys" target="_blank" rel="noopener noreferrer">
                console.groq.com/keys
              </a>
            </td>
            <td>Yes — free tier available</td>
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
          Select your provider (Anthropic, OpenAI, Gemini, Groq or OpenRouter) from the
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
          <a href="/docs/tools">Audio Tools Reference</a> — all 87 tools the
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
