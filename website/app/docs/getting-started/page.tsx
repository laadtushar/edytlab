import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { pageSocial } from "@/lib/seo-core";
import { DocShell } from "@/components/docs/doc-shell";

const title = "Getting Started: install and edit your first file";
const description =
  "Install edytlab on macOS, Windows or Linux, connect an LLM provider with your own API key, your ChatGPT plan or Ollama (no key), and edit your first audio file by chatting.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs/getting-started" },
  ...pageSocial("/docs/getting-started", `${title} · edytlab`, description),
};

export default function GettingStartedPage() {
  return (
    <DocShell
      title="Getting Started"
      description="Install edytlab, configure your LLM provider, and edit your first audio file."
    >
      <h2>1. Download and install</h2>
      <p>
        Download the latest build from the{" "}
        <a href={siteConfig.releases} target="_blank" rel="noopener noreferrer">
          GitHub Releases page
        </a>
        .
      </p>
      <blockquote>
        <strong>These builds are unsigned.</strong> macOS and Windows both
        object on first launch. Neither download is broken; the steps below get
        you past it.
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
            <td>Drag to Applications, then run the command below.</td>
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
        The app is not damaged. macOS quarantines downloads and refuses to
        launch one Apple hasn&rsquo;t notarized. The dialog offers only{" "}
        <em>Move to Trash</em>, and right-click &rarr; Open will not help.
        Remove the flag after dragging the app to Applications:
      </p>
      <pre>
        <code>xattr -dr com.apple.quarantine /Applications/edytlab.app</code>
      </pre>
      <p>
        Then open it normally. Once per install, until notarization is set up.
        If you see a milder &ldquo;unidentified developer&rdquo; message
        instead, System Settings &rarr; Privacy &amp; Security &rarr;{" "}
        <strong>Open Anyway</strong> is enough.
      </p>

      <h3>Windows: SmartScreen</h3>
      <p>
        Click &ldquo;More info&rdquo; &rarr; &ldquo;Run anyway&rdquo; on first
        launch. The warning stays until the installers are signed.
      </p>

      <h2>2. Get an API key</h2>
      <p>
        You bring your own API key; it is stored in your OS keychain. With a
        ChatGPT Plus or Pro plan you can skip the key (v0.9.0 and later; a
        preview): see the last row and &ldquo;Using your ChatGPT plan&rdquo; in
        the <a href="/docs/user-guide#chatgpt-plan">user guide</a>.
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
          <tr>
            <td>Ollama (local)</td>
            <td>
              No key. Install{" "}
              <a href="https://ollama.com" target="_blank" rel="noopener noreferrer">
                Ollama
              </a>
              , pull a model, and edytlab talks to it on your machine.
            </td>
            <td>Free — runs on your hardware</td>
          </tr>
          <tr>
            <td>ChatGPT plan (preview, v0.9.0 and later)</td>
            <td>
              No key. In Settings, choose ChatGPT plan and{" "}
              <strong>Continue with ChatGPT</strong>; edytlab opens your
              browser to sign in. Available for ChatGPT Plus and Pro. May not
              be available on workspace plans; that depends on OpenAI and
              your admin.
            </td>
            <td>No separate bill — it uses your plan&apos;s included usage</td>
          </tr>
        </tbody>
      </table>
      <blockquote>
        <strong>Cost estimate:</strong> a typical 30-minute editing session
        uses roughly $0.05–0.50 in LLM tokens, depending on the model and the
        session. Haiku-class models cost about 20× less per token than
        Sonnet-class models. With the ChatGPT plan there is no per-token bill:
        usage counts toward your plan, and on Plus the five-hour limit is
        shared with every other app and tool that uses the plan (not on Pro).
      </blockquote>

      <h2>3. Enter your key</h2>
      <ol>
        <li>Launch edytlab.</li>
        <li>Click the gear icon (⚙) in the top-right corner.</li>
        <li>Pick your provider.</li>
        <li>
          Paste your API key (Ollama needs none). For the ChatGPT plan, choose{" "}
          <strong>Continue with ChatGPT</strong> and finish signing in in your
          browser.
        </li>
        <li>
          Optionally press <strong>Test</strong>: a minimal request checks the
          key and that the model can call tools.
        </li>
        <li>Press <strong>Save &amp; Continue</strong>.</li>
      </ol>
      <p>
        Keys go in your OS keychain (macOS Keychain, Windows Credential
        Manager, the Secret Service on Linux), never to disk or to any edytlab
        server. On a Linux desktop with no Secret Service running they are
        kept in memory until you restart, and the app tells you so, at launch
        and again at Save if the service went away after the app started.
      </p>

      <h2>4. Load your first audio file</h2>
      <ul>
        <li>
          <strong>Drag and drop</strong> a WAV, MP3, FLAC or Ogg Vorbis file
          onto the timeline. Several files at once make a multi-track session.
        </li>
        <li>
          <strong>Open button</strong>: click &ldquo;Open Audio&rdquo; in the
          empty state or the file menu.
        </li>
        <li>
          <strong>Chat</strong>: type <code>load /path/to/file.wav</code>.
        </li>
      </ul>

      <h2>5. Your first edit</h2>
      <p>Describe what you want in the chat panel. Some prompts to try:</p>
      <ul>
        <li>
          <code>Remove the silence at the beginning</code>
        </li>
        <li>
          <code>Normalize to -14 LUFS</code>
        </li>
        <li>
          <code>Take the background hiss out</code>
        </li>
        <li>
          <code>Find the silences longer than a second</code>
        </li>
        <li>
          <code>Export to /Users/me/Desktop/output.wav</code>
        </li>
      </ul>
      <p>
        The agent streams its response and shows a badge for each tool call.
        Every change is a new node in the session graph; nothing is
        destructive.
      </p>

      <h2>6. Undo and redo</h2>
      <p>Every edit is a node in a directed acyclic graph (DAG), so you can undo and redo freely:</p>
      <ul>
        <li>
          <strong>Ctrl+Z</strong> (Cmd+Z on Mac) — undo
        </li>
        <li>
          <strong>Ctrl+Y</strong> (Cmd+Y on Mac) — redo
        </li>
      </ul>
      <p>
        Unlike a linear undo stack, the DAG keeps all branches: fork a session,
        try a different edit, and switch between branches in the Graph view.
        Undo retraces the steps you took, even when an edit returned to an
        earlier state (mute, then unmute); the Graph view marks that path, and
        it is saved with the project across restarts.
      </p>

      <h2>Keyboard shortcuts</h2>
      <p>Press <code>?</code> for the full shortcuts overlay.</p>
      <table>
        <thead>
          <tr>
            <th>Key</th>
            <th>Action</th>
          </tr>
        </thead>
        <tbody>
          <tr><td>Space</td><td>Play / Pause (renders the preview first if there is none)</td></tr>
          <tr><td>L</td><td>Toggle loop playback (loops the selected region)</td></tr>
          <tr><td>Ctrl/Cmd + Z</td><td>Undo</td></tr>
          <tr><td>Ctrl/Cmd + Y</td><td>Redo</td></tr>
          <tr><td>Ctrl + Scroll</td><td>Zoom waveform</td></tr>
          <tr><td>+</td><td>Zoom in</td></tr>
          <tr><td>−</td><td>Zoom out</td></tr>
          <tr><td>0</td><td>Reset zoom</td></tr>
          <tr><td>Esc</td><td>Clear selection</td></tr>
        </tbody>
      </table>

      <h2>Next steps</h2>
      <ul>
        <li>
          <a href="/docs/user-guide">User Guide</a> — every feature
        </li>
        <li>
          <a href="/docs/tools">Audio Tools Reference</a> — all 94 tools the
          agent can call
        </li>
        <li>
          <a href="/docs/developer-guide">Developer Guide</a> — build, test,
          contribute
        </li>
      </ul>
    </DocShell>
  );
}
