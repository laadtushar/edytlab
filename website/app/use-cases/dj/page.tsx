import type { Metadata } from "next";
import Link from "next/link";

import { DemoCard } from "@/components/landing/real-demos";
import { PageLinks, PageShell, Prose } from "@/components/landing/page-shell";
import { demos } from "@/lib/demos";
import { DEFAULT_OG_IMAGE, pageAlternates } from "@/lib/seo";
import { siteConfig } from "@/lib/site";

const title = "AI audio editor for DJs: beatmatch, blend and master";
const description =
  "Prep blends by describing them. edytlab's AI agent beatmatches two tracks, crossfades, masters to a loudness target and exports a file. Free and open source.";
const url = `${siteConfig.url}/use-cases/dj`;

export const metadata: Metadata = {
  title,
  description,
  alternates: pageAlternates("/use-cases/dj"),
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

export default function DjUseCasePage() {
  return (
    <PageShell
      eyebrow="Use case"
      title="AI audio editor for DJs"
      lead="Describe a blend, a longer intro or a loudness target in plain English. An AI agent runs real audio tools on your files, on your computer, and you can A/B every take before you keep one."
    >
      <section aria-labelledby="dj-demo" className="mt-12">
        <h2 id="dj-demo" className="text-2xl font-semibold">
          A real beatmatch, recorded
        </h2>
        <p className="mt-4 leading-relaxed text-foreground/90">
          This is a screen recording of the desktop app, not a mock-up. The DJ
          asks for a transition between two tracks and the agent does the rest,
          ending in an exported file.
        </p>
        <div className="mt-6 grid gap-6">
          {demos.map((demo, i) => (
            <DemoCard key={demo.slug} demo={demo} index={i} />
          ))}
        </div>
      </section>

      <Prose>
        <h2>What you can ask for</h2>
        <p>
          Each of these is a tool the agent can call. The full list is in the{" "}
          <Link href="/docs/tools">tools reference</Link>.
        </p>
        <ul>
          <li>
            <strong>Beatmatching.</strong>{" "}
            It reads each track&apos;s tempo and
            beat grid, then time-stretches one to match the other without
            changing its pitch. For a track whose timing drifts, it can warp
            the audio so the beats land on a grid.
          </li>
          <li>
            <strong>Key matching.</strong>{" "}
            It reads each track&apos;s key and
            can shift pitch in semitones without changing the length. This is
            most reliable within about a fifth.
          </li>
          <li>
            <strong>Extended intros and edits.</strong>{" "}
            Copy a region, repeat
            it, paste it elsewhere, cut a range out or move a clip. Because the
            agent has the beat grid it can talk in bars rather than seconds,
            as in the recording above, where the incoming track starts 8 bars
            before the outgoing one ends.
          </li>
          <li>
            <strong>Transitions.</strong>{" "}
            Fades, crossfades, and low-pass or
            high-pass filters on a track over the overlap, plus EQ, reverb and
            echo.
          </li>
          <li>
            <strong>Loudness and delivery.</strong>{" "}
            It measures integrated
            loudness (EBU R128), compresses and limits, brings the mix to a
            target such as &minus;14 LUFS, and exports WAV, FLAC or MP3.
          </li>
        </ul>

        <h2>Try the blend more than one way</h2>
        <p>
          Every edit is saved as a node in a branching history. Fork the session,
          ask for the incoming track to start 4 bars earlier, then A/B the two
          takes and keep the one you like. Nothing you try is lost, and undo
          steps back through it.
        </p>

        <h2>Your files stay on your computer</h2>
        <p>
          The audio processing runs locally in a Rust engine. Tracks are never
          uploaded. Only the chat goes to the AI provider you pick (Anthropic,
          OpenAI, OpenRouter, Groq or Gemini), or nowhere at all if you run a
          local model with Ollama. See the{" "}
          <Link href="/use-cases/local-ai-audio-editor">
            local-first AI audio editor
          </Link>{" "}
          page for how that works.
        </p>

        <h2>What it does not do</h2>
        <ul>
          <li>
            It is a prep tool, not a live one. There are no decks, controllers
            or real-time mixing.
          </li>
          <li>
            It cannot split a track into stems or an acapella. Stem separation
            is not built yet.
          </li>
          <li>
            Beat and key detection are software estimates. A track with a
            drifting tempo or a long ambient intro can be off, and in the
            recording above the agent itself points out a start where the bars
            land a beat apart.
          </li>
          <li>
            Time-stretch and pitch-shift use a phase vocoder written for the
            project. It is clean on sustained material, but dense mixes can
            sound smeared, and the further the change is from 1.0 the more it
            shows.
          </li>
          <li>
            It needs an AI provider: your own key for a hosted one, or Ollama
            for a local model. The current builds are unsigned, so macOS and
            Windows warn on first launch;{" "}
            <Link href="/docs/getting-started">the setup guide</Link> has the
            steps.
          </li>
        </ul>

        <h2>Get started</h2>
        <p>
          edytlab is free and open source (MIT) for macOS, Windows and Linux.
          Download it from the{" "}
          <Link href={siteConfig.releases}>releases page</Link>, follow the{" "}
          <Link href="/docs/getting-started">getting started guide</Link>, and
          read the <Link href="/docs/user-guide">user guide</Link> for the
          other things the agent can do. Posts about how it works are on the{" "}
          <Link href="/blog">blog</Link>.
        </p>
      </Prose>
      <PageLinks />
    </PageShell>
  );
}
