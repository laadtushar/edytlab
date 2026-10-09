import type { Metadata } from "next";
import { siteConfig } from "@/lib/site";
import { DEFAULT_OG_IMAGE } from "@/lib/seo-core";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "Developer Guide",
  description:
    "Build edytlab from source, understand the architecture, run tests, and contribute new tools or LLM providers.",
  alternates: { canonical: "/docs/developer-guide" },
  openGraph: {
    title: "Developer Guide — edytlab Docs",
    description: "Build from source, run tests, and contribute to edytlab.",
    url: `${siteConfig.url}/docs/developer-guide`,
    images: [DEFAULT_OG_IMAGE],
  },
};

export default function DeveloperGuidePage() {
  return (
    <DocShell
      title="Developer Guide"
      description="Build edytlab from source, run tests, and contribute new tools or providers."
    >
      <h2>Prerequisites</h2>
      <table>
        <thead>
          <tr>
            <th>Tool</th>
            <th>Version</th>
            <th>Notes</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Rust</td>
            <td>1.88 (auto-installed)</td>
            <td>Pinned via <code>rust-toolchain.toml</code></td>
          </tr>
          <tr>
            <td>Node.js</td>
            <td>20+</td>
            <td>LTS recommended</td>
          </tr>
          <tr>
            <td>pnpm</td>
            <td>9.15+</td>
            <td><code>npm install -g pnpm</code></td>
          </tr>
        </tbody>
      </table>

      <h3>Platform extras</h3>
      <p>
        <strong>macOS:</strong>{" "}
        <code>xcode-select --install</code> for Xcode Command Line Tools.
      </p>
      <p>
        <strong>Windows:</strong> Visual C++ Build Tools (Desktop development
        with C++) + WebView2. Install via{" "}
        <a
          href="https://visualstudio.microsoft.com/visual-cpp-build-tools/"
          target="_blank"
          rel="noopener noreferrer"
        >
          Visual Studio Build Tools
        </a>
        .
      </p>
      <p>
        <strong>Linux (Ubuntu 22.04+):</strong>
      </p>
      <pre>
        <code>{`sudo apt-get install -y \\
  libgtk-3-dev libwebkit2gtk-4.1-dev \\
  libayatana-appindicator3-dev librsvg2-dev \\
  libssl-dev libasound2-dev patchelf`}</code>
      </pre>

      <h2>Initial setup</h2>
      <pre>
        <code>{`git clone https://github.com/laadtushar/edytlab.git
cd edytlab
pnpm install
cargo build --workspace    # first build: 5–20 min
pnpm tauri:dev             # start dev mode`}</code>
      </pre>
      <p>
        Subsequent incremental builds complete in under 30 seconds.
      </p>

      <h2>Project structure</h2>
      <pre>
        <code>{`apps/
  desktop/            Tauri shell — React frontend + Rust bridge
  cli/                Headless one-turn agent driver (E2E tests)
crates/
  ai/                 LLM providers, agent loop, keychain
  tools/              93 audio-editing tools
  session/            Session DAG data model and store
  audio-decoder/      File decode (symphonia)
  audio-dsp/          Sample-level DSP shared by tools and render
  audio-engine/       DSP graph and render
  audio-io/           cpal output (unused; playback is in the webview)
  audio-time/         Time-stretch and pitch-shift
  audio-analysis/     BPM, key, beat grid
  recorder/           Microphone capture to WAV
  ml-demucs/          Stem separation (ONNX Demucs; inference not shipped)
  ml-whisper/         Transcription (ONNX Whisper; decoder not shipped)
  ml-pipeline/        Shared ONNX runtime + model cache
  memory/             Global/project markdown memory
  skills/             User skill library
  agent_profiles/     Per-session model + tool profiles
  mcp/                MCP server lifecycle
docs/                 Comprehensive technical documentation
website/              Next.js marketing site (this site)`}</code>
      </pre>

      <h2>Running tests</h2>
      <p>Full acceptance gate (same as CI):</p>
      <pre>
        <code>{`cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --test-threads=1
pnpm --filter @edytlab/desktop test
pnpm --filter @edytlab/desktop test:slow-scheduler
pnpm --filter @edytlab/desktop typecheck
pnpm --filter @edytlab/desktop typecheck:e2e
pnpm --filter @edytlab/desktop test:e2e    # Playwright, Chromium`}</code>
      </pre>
      <p>Targeted runs:</p>
      <pre>
        <code>{`cargo test -p ai                          # single crate
cargo test -p tools test_normalize        # single test
pnpm --filter @edytlab/desktop test:watch # vitest watch mode`}</code>
      </pre>
      <blockquote>
        <code>--test-threads=1</code> is required because some AI crate tests
        share a model-catalogue cache that is not safe for concurrent access.
      </blockquote>

      <h2>Architecture overview</h2>
      <p>The system has three layers:</p>
      <ol>
        <li>
          <strong>Frontend</strong> — React 19 + Vite + Tailwind in a Tauri
          WebView. Thin UI layer; all state lives in Rust.
        </li>
        <li>
          <strong>Rust application layer</strong> — about 86 Tauri commands in{" "}
          <code>commands.rs</code>. Manages <code>AppState</code>: Store,
          Engine, Agent, Clipboard.
        </li>
        <li>
          <strong>Rust crates</strong> — AI subsystem, tool dispatcher,
          session DAG, audio engine, ML pipeline. Each is independently
          testable.
        </li>
      </ol>
      <p>
        For the full technical design, see the{" "}
        <a
          href={`${siteConfig.github}/blob/main/docs/architecture.md`}
          target="_blank"
          rel="noopener noreferrer"
        >
          architecture.md
        </a>{" "}
        in the repository.
      </p>

      <h2>Adding a new audio tool</h2>
      <ol>
        <li>
          Create <code>crates/tools/src/tool/my_tool.rs</code> implementing the{" "}
          <code>Tool</code> trait (<code>name</code>, <code>schema</code>,{" "}
          <code>invoke</code>).
        </li>
        <li>
          Register it in <code>ToolDispatcher::default_dispatcher()</code> in{" "}
          <code>crates/tools/src/dispatcher.rs</code>.
        </li>
        <li>
          Write tests covering valid input, invalid input, and edge cases.
        </li>
        <li>
          Regenerate <code>docs/tools-reference.md</code> and document the tool
          in the <a href="/docs/tools">Audio Tools Reference</a> — tests fail
          until both list it.
        </li>
      </ol>
      <p>
        Detailed step-by-step with code examples in{" "}
        <a
          href={`${siteConfig.github}/blob/main/docs/contributing.md`}
          target="_blank"
          rel="noopener noreferrer"
        >
          contributing.md §6
        </a>
        .
      </p>

      <h2>Adding a new LLM provider</h2>
      <ol>
        <li>
          Implement the <code>LlmProvider</code> trait in{" "}
          <code>crates/ai/src/provider.rs</code>.
        </li>
        <li>
          Add it to <code>SUPPORTED_PROVIDER_IDS</code> and the{" "}
          <code>provider_from_id()</code> factory.
        </li>
        <li>
          Give it an arm in <code>crates/ai/src/models.rs</code> so its model
          list loads.
        </li>
        <li>
          Update <code>ProviderId</code> in <code>tauri-bridge.ts</code> and the
          provider list in <code>Settings.tsx</code>. Keychain slots need no
          code — they are keyed by provider id.
        </li>
      </ol>
      <p>
        The trait handles auth, request serialization, and SSE stream parsing.
        See{" "}
        <a
          href={`${siteConfig.github}/blob/main/docs/contributing.md`}
          target="_blank"
          rel="noopener noreferrer"
        >
          contributing.md §7
        </a>{" "}
        for full details.
      </p>

      <h2>Commit and PR process</h2>
      <ul>
        <li>
          Branch naming: <code>claude/feature/&lt;kebab-summary&gt;</code> or{" "}
          <code>claude/fix/&lt;kebab-summary&gt;</code>.
        </li>
        <li>
          Commits follow{" "}
          <a
            href="https://www.conventionalcommits.org"
            target="_blank"
            rel="noopener noreferrer"
          >
            Conventional Commits
          </a>
          : <code>feat(tools): add spectral repair tool</code>.
        </li>
        <li>Open PRs as drafts. Flip to ready when CI passes.</li>
        <li>PRs are squash-merged. One concern per PR.</li>
        <li>
          Run the full acceptance gate before pushing. CI blocks merge on any
          failure.
        </li>
      </ul>

      <h2>CI / release pipeline</h2>
      <p>
        <code>ci.yml</code> runs on every push to main and every PR: fmt →
        clippy → cargo test → frontend tests → frontend build, on a matrix of
        macOS 14 · Windows latest · Ubuntu 22.04, plus a Playwright job
        (Chromium) and the website&rsquo;s own tests.
      </p>
      <p>
        On a green main push, <code>auto-release.yml</code> tags{" "}
        <code>v&lt;version&gt;-dev.&lt;run&gt;</code> and dispatches{" "}
        <code>release-dev.yml</code>, which builds unsigned installers for
        macOS, Windows and Linux. Signed production releases are manual
        workflow dispatches, waiting on signing certificates — every release so
        far is unsigned.
      </p>

      <h2>Documentation</h2>
      <p>
        All documentation lives in{" "}
        <code>docs/</code> in the repository:{" "}
        <a
          href={`${siteConfig.github}/tree/main/docs`}
          target="_blank"
          rel="noopener noreferrer"
        >
          github.com/laadtushar/edytlab/tree/main/docs
        </a>
        . Update docs in the same PR as the code.
      </p>
    </DocShell>
  );
}
