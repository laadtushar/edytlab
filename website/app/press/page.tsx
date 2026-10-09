import type { Metadata } from "next";
import Link from "next/link";

import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { DEFAULT_OG_IMAGE, pageAlternates } from "@/lib/seo";
import { siteConfig } from "@/lib/site";

const title = "Press kit";
const description =
  "Boilerplate, fact sheet, logos and demo video for writing about edytlab, the free, open-source desktop audio editor you drive by chatting with an AI agent.";
const url = `${siteConfig.url}/press`;

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/press"),
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

/** Files that already live in `public/`. Each is served at its path. */
const assets = [
  { label: "Logo (SVG)", href: "/logo.svg", note: "Vector, for print and large sizes" },
  { label: "Logo (PNG)", href: "/logo.png", note: "512 × 512" },
  { label: "Icon 512", href: "/icon-512.png", note: "512 × 512" },
  { label: "Icon 192", href: "/icon-192.png", note: "192 × 192" },
  {
    label: "Demo video (MP4)",
    href: "/demos/dj-beatmatched-transition.mp4",
    note: "Beatmatch and blend two tracks, 3:08, with sound",
  },
  {
    label: "Demo video poster (JPG)",
    href: "/demos/dj-beatmatched-transition.jpg",
    note: "1280 × 776",
  },
] as const;

export default function PressPage() {
  return (
    <PageShell
      eyebrow="Press"
      title="edytlab press kit"
      lead="Everything in one place for writing about edytlab: a paragraph you can paste, facts that have been checked against the source, and the logo and demo files. There is no press office; questions go to GitHub."
    >
      <Prose>
        <h2>Boilerplate</h2>
        <blockquote>
          <p>
            edytlab is a free, open-source desktop audio editor where you edit
            by chatting with an AI agent. The agent turns a request such as
            &ldquo;time-stretch the second track to match the first, crossfade,
            and export a WAV&rdquo; into calls to 93 deterministic audio tools.
            The tools run locally in a Rust engine on the user&apos;s own
            computer, so audio files are never uploaded; only the chat goes to
            the language-model provider the user selects (Anthropic, OpenAI,
            OpenRouter, Groq, Gemini, or Ollama for a fully local setup). Every
            edit is saved as a node in a branchable history, with undo and A/B
            comparison. edytlab runs on macOS, Windows and Linux, is released
            under the MIT license, and its source is at
            github.com/laadtushar/edytlab.
          </p>
        </blockquote>

        <h2>Fact sheet</h2>
        <table>
          <tbody>
            <tr>
              <th scope="row">What it is</th>
              <td>A desktop audio editor driven by an AI agent</td>
            </tr>
            <tr>
              <th scope="row">Platforms</th>
              <td>
                macOS (one universal build for Apple Silicon and Intel),
                Windows 10 and 11, Linux
              </td>
            </tr>
            <tr>
              <th scope="row">Installers</th>
              <td>.dmg, .msi and NSIS .exe, .deb and AppImage</td>
            </tr>
            <tr>
              <th scope="row">Signing</th>
              <td>
                Not signed or notarized yet, so macOS and Windows warn on first
                launch. The release notes give the steps.
              </td>
            </tr>
            <tr>
              <th scope="row">License</th>
              <td>MIT</td>
            </tr>
            <tr>
              <th scope="row">Built with</th>
              <td>Tauri 2, Rust, React</td>
            </tr>
            <tr>
              <th scope="row">Audio engine</th>
              <td>
                Pure-Rust DSP, including a phase vocoder written for the
                project for time-stretch and pitch-shift
              </td>
            </tr>
            <tr>
              <th scope="row">Tools</th>
              <td>
                93 deterministic tools the agent can call (
                <Link href="/docs/tools">full reference</Link>)
              </td>
            </tr>
            <tr>
              <th scope="row">AI providers</th>
              <td>
                Anthropic, OpenAI, OpenRouter, Groq, Gemini, and Ollama (local,
                no key)
              </td>
            </tr>
            <tr>
              <th scope="row">Where audio goes</th>
              <td>
                Nowhere. Processing is local. Only chat text goes to the
                provider you choose.
              </td>
            </tr>
            <tr>
              <th scope="row">Telemetry</th>
              <td>None in the app; no analytics on this website</td>
            </tr>
            <tr>
              <th scope="row">History</th>
              <td>Branchable graph of edits with fork, A/B compare and undo</td>
            </tr>
            <tr>
              <th scope="row">Safety option</th>
              <td>
                &ldquo;Plan first&rdquo; shows the agent&apos;s steps for
                approval before they run
              </td>
            </tr>
            <tr>
              <th scope="row">Export formats</th>
              <td>WAV, FLAC, MP3</td>
            </tr>
          </tbody>
        </table>

        <h2>What is not shipped</h2>
        <p>Please state this plainly if you cover edytlab.</p>
        <ul>
          <li>
            Stem separation and speech transcription are not implemented in any
            build. Editing by transcript depends on transcription, so it does
            not work either.
          </li>
          <li>
            It edits files offline. It is not a live DJ tool and it is not a
            DAW: there is no MIDI and no VST or CLAP plugin hosting.
          </li>
          <li>There is no web or mobile version.</li>
        </ul>
        <p>
          There are no user counts, download statistics, benchmarks,
          testimonials or awards to cite.
        </p>

        <h2>Logos and demo video</h2>
        <p>
          These files are in the project&apos;s MIT-licensed repository. There
          are no separate brand guidelines. Write the name in lowercase:
          edytlab.
        </p>
        <ul>
          {assets.map((a) => (
            <li key={a.href}>
              <a href={a.href} download>
                {a.label}
              </a>{" "}
              &mdash; {a.note}
            </li>
          ))}
        </ul>
        <p>
          Screenshots of the interface are not published as files. The{" "}
          <Link href="/#demos">demo video</Link> shows the real app, and the
          release builds are free to download and capture yourself.
        </p>

        <h2>Contact</h2>
        <p>
          Open an issue at{" "}
          <a
            href={`${siteConfig.github}/issues`}
            target="_blank"
            rel="noopener noreferrer"
          >
            github.com/laadtushar/edytlab/issues
          </a>
          . The maintainer is{" "}
          <a href="https://github.com/laadtushar" target="_blank" rel="noopener noreferrer">
            @laadtushar
          </a>{" "}
          on GitHub.
        </p>

        <h2>More</h2>
        <ul>
          <li>
            <Link href="/docs">Documentation</Link>
          </li>
          <li>
            <Link href="/blog">Blog</Link>
          </li>
          <li>
            <Link href="/use-cases/dj">AI audio editor for DJs</Link>
          </li>
          <li>
            <Link href="/use-cases/local-ai-audio-editor">
              Local-first AI audio editor
            </Link>
          </li>
          <li>
            <Link href="/privacy">Privacy</Link>
          </li>
        </ul>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
