import type { Metadata } from "next";
import Link from "next/link";
import { BookOpen, Code2, Wrench, Zap, Terminal, FileCode2 } from "lucide-react";

import { siteConfig } from "@/lib/site";
import { DocShell } from "@/components/docs/doc-shell";

export const metadata: Metadata = {
  title: "Documentation",
  description:
    "Complete documentation for edytlab — the local-first AI audio editor. Guides for users and developers.",
  openGraph: {
    title: "edytlab Documentation",
    description:
      "Complete documentation for edytlab — installation, user guide, tools reference, and developer API.",
    url: `${siteConfig.url}/docs`,
  },
  alternates: { canonical: "/docs" },
};

const cards = [
  {
    href: "/docs/getting-started",
    icon: Zap,
    title: "Getting Started",
    desc: "Install edytlab, set up your API key, and load your first audio file in under 5 minutes.",
    audience: "User",
  },
  {
    href: "/docs/user-guide",
    icon: BookOpen,
    title: "User Guide",
    desc: "Sessions, the DAG, tracks, chat interface, markers, A/B compare, export, memory, and skills.",
    audience: "User",
  },
  {
    href: "/docs/tools",
    icon: Wrench,
    title: "Audio Tools Reference",
    desc: "All 83 tools the agent can call — cut, normalize, stem separate, transcribe, render, and more.",
    audience: "User",
  },
  {
    href: "/docs/developer-guide",
    icon: Terminal,
    title: "Developer Guide",
    desc: "Build from source, run tests, understand the architecture, and contribute new tools or providers.",
    audience: "Developer",
  },
  {
    href: "/docs/api-reference",
    icon: FileCode2,
    title: "API Reference",
    desc: "Every Tauri command, TypeScript bridge type, and SSE event — complete with signatures and examples.",
    audience: "Developer",
  },
];

export default function DocsPage() {
  return (
    <DocShell
      title="Documentation"
      description="Everything you need to use edytlab and build on top of it."
    >
      <div className="not-prose mt-8 grid gap-4 sm:grid-cols-2">
        {cards.map((card) => (
          <Link
            key={card.href}
            href={card.href}
            className="group flex flex-col gap-3 rounded-xl border border-border/60 bg-card/40 p-5 transition-colors hover:border-primary/40 hover:bg-card"
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
        chat with an AI agent to load, cut, mix, transcribe, and render audio.
        The DSP engine runs entirely on your machine — your audio never leaves
        your device. The only network traffic is the text tokens you send to
        your chosen LLM provider.
      </p>
      <p>
        edytlab supports five LLM providers out of the box:{" "}
        <strong>Anthropic</strong>, <strong>OpenAI</strong>,{" "}
        <strong>Google Gemini</strong>, <strong>Groq</strong>, and{" "}
        <strong>OpenRouter</strong>. You bring your own API key, stored in your
        OS keychain. You can switch providers at any time from Settings without
        reinstalling.
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
        edytlab is open source.{" "}
        <a href={siteConfig.github} target="_blank" rel="noopener noreferrer">
          View the source on GitHub
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
