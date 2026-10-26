import type { Metadata } from "next";
import Link from "next/link";
import { BookOpen, Code2, Wrench, Zap, Terminal, FileCode2 } from "lucide-react";

import { siteConfig } from "@/lib/site";
import { pageSocial } from "@/lib/seo-core";
import { DocShell } from "@/components/docs/doc-shell";

const title = "Documentation: install, use and build on edytlab";
const description =
  "Complete documentation for edytlab, the local-first AI audio editor: getting started, the user guide, the tools reference, and the developer and API guides.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/docs" },
  ...pageSocial("/docs", `${title} · edytlab`, description),
};

const cards = [
  {
    href: "/docs/getting-started",
    icon: Zap,
    title: "Getting Started",
    desc: "Install edytlab, set up your API key, and load your first audio file.",
    audience: "User",
  },
  {
    href: "/docs/user-guide",
    icon: BookOpen,
    title: "User Guide",
    desc: "Sessions, tracks, chat, markers, A/B compare, export, memory and skills.",
    audience: "User",
  },
  {
    href: "/docs/tools",
    icon: Wrench,
    title: "Audio Tools Reference",
    desc: "All 94 tools the agent can call.",
    audience: "User",
  },
  {
    href: "/docs/developer-guide",
    icon: Terminal,
    title: "Developer Guide",
    desc: "Build from source, run tests, add tools or providers.",
    audience: "Developer",
  },
  {
    href: "/docs/api-reference",
    icon: FileCode2,
    title: "API Reference",
    desc: "Every Tauri command, bridge type and event.",
    audience: "Developer",
  },
];

export default function DocsPage() {
  return (
    <DocShell
      title="Documentation"
      description="Use edytlab and build on it."
    >
      <div className="not-prose mt-8 grid gap-4 sm:grid-cols-2" data-cascade-items>
        {cards.map((card, i) => (
          <Link
            key={card.href}
            href={card.href}
            data-lift
            className={`ring-hover group flex flex-col gap-3 rounded-xl border border-border/60 bg-card/40 p-4 hover:bg-card${
              /* An odd count leaves the last card alone in a row: it spans it. */
              i === cards.length - 1 && cards.length % 2 === 1 ? " sm:col-span-2" : ""
            }`}
          >
            <div className="flex items-center justify-between">
              <div className="flex size-9 items-center justify-center rounded-lg bg-primary/10 text-primary ring-1 ring-primary/20">
                <card.icon className="size-4" />
              </div>
              <span className="rounded-full border border-border/60 px-2 py-0.5 text-xs text-muted-foreground">
                {card.audience}
              </span>
            </div>
            <div>
              <p className="font-semibold group-hover:text-primary transition-colors">
                {card.title}
              </p>
              <p className="mt-1 text-sm text-muted-foreground leading-relaxed">
                {card.desc}
              </p>
            </div>
          </Link>
        ))}
      </div>

      <h2>About edytlab</h2>
      <p>
        edytlab is a local-first, open-source desktop audio editor where you
        chat with an AI agent to load, cut, mix, and render audio. The DSP
        engine runs on your machine and your audio never leaves it. The app
        sends the text you write to your chosen LLM provider, plus requests
        you start yourself: a plugin you install from a URL, an MCP server you
        add, the speech model you download from Hugging Face and
        the sign-in to OpenAI when you choose Continue with ChatGPT. See <a href="/privacy">privacy</a>.
      </p>
      <p>
        edytlab supports six API providers plus your ChatGPT plan:{" "}
        <strong>Anthropic</strong>, <strong>OpenAI</strong>,{" "}
        <strong>Google Gemini</strong>, <strong>Groq</strong>,{" "}
        <strong>OpenRouter</strong>, and <strong>Ollama</strong> for a local
        model. You bring your own API key, stored in your OS keychain (Ollama
        needs none), or sign in with ChatGPT to use your Plus or
        Pro plan instead of a key (a preview). Switch providers any time in
        Settings.
      </p>

      <h2>Platform support</h2>
      <table>
        <thead>
          <tr>
            <th>Platform</th>
            <th>Status</th>
            <th>Minimum version</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>macOS (Apple Silicon or Intel)</td>
            <td>✅ Supported (unsigned, universal binary)</td>
            <td>macOS 11.0 (Big Sur)</td>
          </tr>
          <tr>
            <td>Windows</td>
            <td>✅ Supported (unsigned)</td>
            <td>Windows 10 (with WebView2)</td>
          </tr>
          <tr>
            <td>Linux</td>
            <td>✅ Supported (unsigned)</td>
            <td>Ubuntu 22.04+ (.deb / AppImage)</td>
          </tr>
        </tbody>
      </table>

      <h2>License and source</h2>
      <p>
        <a href={siteConfig.github} target="_blank" rel="noopener noreferrer">
          The source is on GitHub
        </a>
        , released under the{" "}
        <a
          href="https://github.com/laadtushar/edytlab/blob/main/LICENSE"
          target="_blank"
          rel="noopener noreferrer"
        >
          MIT License
        </a>
        .
      </p>
    </DocShell>
  );
}
