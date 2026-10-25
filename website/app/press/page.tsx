import type { Metadata } from "next";
import Link from "next/link";

import { JsonLd } from "@/components/json-ld";
import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { GA_MEASUREMENT_ID } from "@/lib/consent";
import { DEFAULT_OG_IMAGE, breadcrumbJsonLd, pageAlternates, pageBreadcrumbs } from "@/lib/seo";
import { siteConfig } from "@/lib/site";

const title = "Press kit: logos, fact sheet and demo video";
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
    note: "Beatmatch and blend two tracks, 2:45, with sound",
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
      lead="A paragraph you can paste, facts checked against the source, and the logo and demo files. There is no press office; questions go to GitHub."
    >
      <JsonLd data={breadcrumbJsonLd(pageBreadcrumbs("Press kit", "/press"))} />
      <Prose>
        <h2>Boilerplate</h2>
        <blockquote>
          <p>
            edytlab is a free, open-source desktop audio editor where you edit
            by chatting with an AI agent. The agent turns a request such as
            &ldquo;time-stretch the second track to match the first, crossfade,
            and export a WAV&rdquo; into calls to 94 deterministic audio tools.
            The tools run locally in a Rust engine on the user&apos;s own
            computer, so audio files are never uploaded; only the chat goes to
            the language-model provider the user selects (Anthropic, OpenAI,
            OpenRouter, Groq, Gemini, or Ollama for a fully local setup, or the
            user&apos;s own ChatGPT Plus or Pro plan through OpenAI&apos;s
            Sign in with ChatGPT, a preview). Every
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
              <td>Tauri 2, Rust, React; pure-Rust audio DSP</td>
            </tr>
            <tr>
              <th scope="row">Tools</th>
              <td>
                94 deterministic tools the agent can call (
                <Link href="/docs/tools">full reference</Link>)
              </td>
            </tr>
            <tr>
              <th scope="row">AI providers</th>
              <td>
                Six API providers plus the user&apos;s ChatGPT plan: Anthropic,
                OpenAI, OpenRouter, Groq, Gemini, and Ollama (local, no key),
                and, from v0.9.0, Continue with ChatGPT, which uses a ChatGPT
                Plus or Pro plan&apos;s included usage instead of a key (a
                preview; it may not be available on workspace plans, which
                depends on OpenAI and your admin)
              </td>
            </tr>
            <tr>
              <th scope="row">Where audio goes</th>
              <td>
                Nowhere. Processing is local. Only chat text goes to the
                provider you choose, which includes the words a transcription
                finds but never the audio.
              </td>
            </tr>
            <tr>
              <th scope="row">Telemetry</th>
              <td>
                None in the app. This website uses anonymous Vercel analytics
                (Web Analytics and Speed Insights)
                {GA_MEASUREMENT_ID ? " and, only if you accept it in the cookie banner, Google Analytics" : ""}; see{" "}
                <Link href="/privacy">Privacy</Link>.
              </td>
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
            Stem separation is not implemented in any build. Speech
            transcription works from v0.8.0, on your computer, once the user
            has downloaded the speech model in Settings; v0.7.0 and earlier
            cannot transcribe.
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

        <p>
          More: <Link href="/use-cases/dj">AI audio editor for DJs</Link>,{" "}
          <Link href="/use-cases/local-ai-audio-editor">local-first AI audio editor</Link> and{" "}
          <Link href="/privacy">privacy</Link>.
        </p>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
