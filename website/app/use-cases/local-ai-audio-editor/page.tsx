import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import {
  DEFAULT_OG_IMAGE,
  articleJsonLd,
  breadcrumbJsonLd,
  pageAlternates,
  pageBreadcrumbs,
} from "@/lib/seo";
import { GA_MEASUREMENT_ID } from "@/lib/consent";
import { localOllama } from "@/lib/blog-posts-audio-2/local-ollama";
import { siteConfig } from "@/lib/site";

const title = "Local-first AI audio editor: audio stays local";
const description =
  "An AI audio editor that keeps your audio on your machine. DSP runs locally in Rust, only the chat leaves, and with Ollama it needs no cloud and no API key.";
const path = "/use-cases/local-ai-audio-editor";
const url = `${siteConfig.url}/use-cases/local-ai-audio-editor`;
// The day the page was first committed (`git log --diff-filter=A`). The
// date it last changed is `PAGE_UPDATED[path]` in `lib/seo-core.ts`.
const published = "2026-10-09";

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/use-cases/local-ai-audio-editor"),
  openGraph: {
    type: "website",
    url,
    title: `${title} · edytlab`,
    description,
    siteName: siteConfig.name,
    images: [DEFAULT_OG_IMAGE],
  },
  twitter: {
    card: "summary_large_image",
    title: `${title} · edytlab`,
    description,
    images: [DEFAULT_OG_IMAGE.url],
  },
};

export default function LocalAiUseCasePage() {
  return (
    <PageShell
      eyebrow="Use case"
      title="A local-first AI audio editor"
      lead="edytlab does not upload your audio. The editing runs on your computer, the AI only sequences the tools, and with Ollama even the chat can stay at home."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("A local-first AI audio editor", path))} />
      <JsonLd data={articleJsonLd({ path, headline: title, description, datePublished: published })} />
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
              <td>Your audio files, and the edits made to them (EQ, fades, time-stretch, export)</td>
              <td>Nowhere. Decoded, processed and rendered on your computer.</td>
            </tr>
            <tr>
              <td>Your chat messages and the agent&apos;s tool results</td>
              <td>
                The AI provider you chose. Never your raw audio. A transcript
                is one of these results: the words the speech model found (a
                bounded amount for each call) reach the provider as text, and
                with a local Ollama model they stay on your computer too.
              </td>
            </tr>
            <tr>
              <td>API keys</td>
              <td>
                Your operating system&apos;s credential store, sent only to the
                provider that owns the key. The ChatGPT plan has no key.
              </td>
            </tr>
            <tr>
              <td>Signing in with ChatGPT (v0.9.0 and later; optional)</td>
              <td>
                Only if you click Continue with ChatGPT. Your browser goes to
                auth.openai.com, which edytlab also uses to exchange and renew
                the sign-in and to revoke it when you sign out. The assistant
                then talks to api.openai.com with your ChatGPT access token
                instead of a key: the same text as for any provider, never your
                raw audio. The refresh token stays in your operating system&apos;s
                credential store, and a small registration file (host ID,
                client ID, your email as a label) stays in the app&apos;s config
                folder. Signing out in edytlab does not disconnect it in
                ChatGPT; the{" "}
                <Link href="/privacy#signing-in-with-chatgpt">privacy page</Link>{" "}
                says how.
              </td>
            </tr>
            <tr>
              <td>The speech model (about 139 MB; v0.8.0 and later)</td>
              <td>
                Downloaded from Hugging Face (huggingface.co) only when you
                click Download in Settings, never automatically. Hugging Face
                sees your IP address, the file you asked for and that the
                request comes from edytlab (and which version), and nothing
                about your audio. Transcribing with it runs on your computer
                and makes no request of its own.
              </td>
            </tr>
            <tr>
              <td>Usage data from the app</td>
              <td>None. The app sends no telemetry.</td>
            </tr>
            <tr>
              <td>Visits to this website</td>
              <td>
                Recorded by anonymous Vercel analytics (Web Analytics and
                Speed Insights){GA_MEASUREMENT_ID ? " and, only if you accept it in the cookie banner, Google Analytics" : ""}, which the
                website loads and the app does not. See the <Link href="/privacy">privacy page</Link>.
              </td>
            </tr>
          </tbody>
        </table>
        <p>
          The <Link href="/privacy">privacy page</Link> has the details, and the{" "}
          <a href={siteConfig.github} target="_blank" rel="noopener noreferrer">
            source is public
          </a>
          , so you can check the claims.
        </p>

        <h2>Why the AI does not need your audio</h2>
        <p>
          The model never sees or hears samples. It is given a description of the session (tempo, key, beat grid, loudness)
          and a list of deterministic audio tools, and chooses which to call and in what order. The tools do the signal
          processing on your machine, so the private part stays local and the part that needs a language model is only text.
        </p>

        <h2>Run it with Ollama</h2>
        <p>
          Ollama needs no account and no key. Install <a href="https://ollama.com" target="_blank" rel="noopener noreferrer">Ollama</a>{" "}
          and pull a model that supports tool calling, then choose Ollama in edytlab&apos;s Settings and press Test, which sends a
          minimal request to check that the model can call tools. The{" "}
          <Link href="/docs/getting-started">getting started guide</Link> walks through it, and you can switch between Ollama
          and a hosted provider at any time, without reinstalling.
        </p>

        <h2>Be realistic about local models</h2>
        <ul>
          <li>
            With Ollama the agent sends a smaller set of tools: the common
            editing tools, plus any you have named or the assistant has just
            proposed. The names of the rest go in the prompt, so the model can
            still call them. A big project&apos;s session summary is cut to fit
            too, and the model reads the rest a page at a time. Ollama trims
            an over-long prompt and the app shows nothing, so if replies start
            to forget earlier messages, raise the model&apos;s context length or
            reopen the project to start the conversation afresh. The{" "}
            <Link href={`/blog/${localOllama.slug}`}>Ollama post</Link> has the
            limits in full.
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
            Stem separation is not built yet, locally or otherwise, so
            nothing here offers it. Transcription runs on your computer&apos;s
            CPU, whichever model you chat with.
          </li>
        </ul>

        <h2>Where next</h2>
        <p>
          See it work in the <Link href="/use-cases/dj">DJ blend demo</Link> or browse the{" "}
          <Link href="/docs/tools">tools reference</Link>. The builds are unsigned for now, so macOS and Windows will warn on
          first launch; the <Link href="/docs/getting-started">setup guide</Link> has the steps.
        </p>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
