import type { Metadata } from "next";
import Link from "next/link";

import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { siteConfig } from "@/lib/site";

const title = "Local-first AI audio editor";
const description =
  "An AI audio editor that keeps your audio on your machine. DSP runs locally in Rust, only the chat leaves, and with Ollama it needs no cloud and no API key.";
const url = `${siteConfig.url}/use-cases/local-ai-audio-editor`;

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: url },
  openGraph: {
    type: "website",
    url,
    title: `${title} · edytlab`,
    description,
    siteName: siteConfig.name,
    images: [{ url: "/opengraph-image", width: 1200, height: 630, alt: "edytlab" }],
  },
  twitter: {
    card: "summary_large_image",
    title: `${title} · edytlab`,
    description,
    images: ["/opengraph-image"],
  },
};

export default function LocalAiUseCasePage() {
  return (
    <PageShell
      eyebrow="Use case"
      title="A local-first AI audio editor"
      lead="Most AI audio tools upload your files to a server. edytlab does not. The editing runs on your computer, the AI only sequences the tools, and with Ollama even the chat can stay at home."
    >
      <Prose>
        <h2>What stays on your machine, and what leaves</h2>
        <table>
          <thead>
            <tr>
              <th>Item</th>
              <th>Where it goes</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>Your audio files</td>
              <td>Nowhere. Decoded, processed and rendered locally.</td>
            </tr>
            <tr>
              <td>The edits themselves (EQ, fades, time-stretch, export)</td>
              <td>Nowhere. They run in-process in a Rust engine.</td>
            </tr>
            <tr>
              <td>Your chat messages and the agent&apos;s tool results</td>
              <td>
                The AI provider you chose. Never your raw audio. With a local
                Ollama model, they stay on your computer too.
              </td>
            </tr>
            <tr>
              <td>API keys</td>
              <td>
                Your operating system&apos;s credential store, sent only to the
                provider that owns the key.
              </td>
            </tr>
            <tr>
              <td>Usage data</td>
              <td>None. The app has no telemetry and this site has no analytics.</td>
            </tr>
          </tbody>
        </table>
        <p>
          The details are on the <Link href="/privacy">privacy page</Link>, and
          the source is public, so you can check the claims rather than take
          them on trust.
        </p>

        <h2>Why the AI does not need your audio</h2>
        <p>
          The model never sees or hears samples. It is given a description of
          the session, with measurements such as tempo, key, beat grid and
          loudness, and a list of 93 deterministic audio tools. It chooses
          which tools to call and in what order. The tools do the signal
          processing, in pure Rust, with no Python in the audio path. That
          split is what makes local-first practical: the heavy, private part
          stays on your machine, and the part that needs a language model is
          only text.
        </p>

        <h2>Run it with Ollama</h2>
        <p>
          Ollama needs no account and no key. To use it:
        </p>
        <ul>
          <li>
            Install <a href="https://ollama.com" target="_blank" rel="noopener noreferrer">Ollama</a>{" "}
            and pull a model that supports tool calling.
          </li>
          <li>Open Settings in edytlab (the gear icon) and choose Ollama.</li>
          <li>
            Press Test. edytlab sends a minimal request to check that the model
            can call tools, then save.
          </li>
        </ul>
        <p>
          The <Link href="/docs/getting-started">getting started guide</Link>{" "}
          walks through it. You can also switch between Ollama and a hosted
          provider (Anthropic, OpenAI, OpenRouter, Groq or Gemini) at any time,
          without reinstalling.
        </p>

        <h2>Be realistic about local models</h2>
        <ul>
          <li>
            The agent sends its instructions and the tool list with every
            request. One measured first request was about 15,000 tokens, and a
            local model run with an 8,192-token context refused it. Use a model
            with reliable tool calling and a context window well above that.
            Shrinking the request is tracked in{" "}
            <a
              href={`${siteConfig.github}/issues/395`}
              target="_blank"
              rel="noopener noreferrer"
            >
              issue 395
            </a>
            .
          </li>
          <li>
            On a CPU, a large request means a long wait for the first reply.
          </li>
          <li>
            &ldquo;Plan first&rdquo; mode, which shows the agent&apos;s steps
            for your approval before anything runs, is a useful guard with
            smaller models.
          </li>
          <li>
            Stem separation and transcription are not built yet, locally or
            otherwise, so nothing here offers them.
          </li>
        </ul>

        <h2>Open source, so you can verify it</h2>
        <p>
          edytlab is released under the MIT license. The audio engine, the tool
          list and the privacy behaviour are all in the{" "}
          <a href={siteConfig.github} target="_blank" rel="noopener noreferrer">
            repository
          </a>
          , and the repository&apos;s tools reference is generated from the
          same registry the agent calls, so that document cannot drift from
          what the app does.
        </p>

        <h2>Where next</h2>
        <p>
          See it work in the <Link href="/use-cases/dj">DJ blend demo</Link>,
          browse the <Link href="/docs/tools">93 tools</Link>, or read the{" "}
          <Link href="/blog">blog</Link>. The builds are unsigned for now, so
          macOS and Windows will warn on first launch; the{" "}
          <Link href="/docs/getting-started">setup guide</Link> has the steps.
        </p>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
