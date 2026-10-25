import type { Metadata } from "next";
import { Fragment } from "react";

import { LegalShell } from "@/components/landing/legal-shell";
import { pageSocial } from "@/lib/seo";

const title = "Changelog: release notes for every version";
const description =
  "What changed in each edytlab release, newest first: new audio tools, fixes and the limits that remain. Installers for every version are on GitHub Releases.";

export const metadata: Metadata = {
  title,
  description,
  alternates: { canonical: "/changelog" },
  ...pageSocial("/changelog", `${title} · edytlab`, description),
};

type Bullet = string | { text: string; href: string };

interface Entry {
  version: string;
  date: string;
  summary: string;
  bullets: Bullet[];
}

// Static, updated by hand as releases land. At most six short bullets per
// release; detail lives in the user guide and the privacy page. The full
// list of binaries and tags is on GitHub Releases.
const entries: Entry[] = [
  {
    version: "v0.9.1",
    date: "2026-10",
    summary: "Fixes the first message after signing in with ChatGPT.",
    bullets: [
      "Fixed: in v0.9.0 the first message after Sign in with ChatGPT failed with “Unsupported content type”.",
      "Changed: a similar OpenAI refusal now shows a plan card with a copyable request ID.",
    ],
  },
  {
    version: "v0.9.0",
    date: "2026-10",
    summary: "Sign in with ChatGPT (preview), and better noise reduction, tempo and key.",
    bullets: [
      { text: "New: Sign in with ChatGPT (preview) uses your Plus or Pro plan instead of an API key.", href: "/docs/user-guide#chatgpt-plan" },
      "It may not work on workspace plans; that depends on OpenAI and your admin.",
      { text: "Your audio stays local; prompts and tool results go to OpenAI. What is sent: privacy page.", href: "/privacy#signing-in-with-chatgpt" },
      "Fixed: noise reduction no longer adds a loud click at the start and end of a track.",
      "Fixed: tempo and key detection (doubled tempo from hi-hats, minor keys read as major), now with confidence.",
      "Fixed: re-downloading the speech model left about 136 MB of the old one in memory.",
    ],
  },
  {
    version: "v0.8.1",
    date: "2026-10",
    summary: "Fixes editing a voice recorded at a different sample rate from the project.",
    bullets: [
      "Fixed: nine tools put cuts, shifts, labels and transcript words in the wrong place on such tracks.",
      "Fixed: volume envelopes, such as ducking under speech, landed at the wrong time on such clips.",
      "Fixed: the session length was wrong with mixed sample rates.",
      "Changed: tools that need one rate for a track with two refuse it instead of guessing.",
    ],
  },
  {
    version: "v0.8.0",
    date: "2026-10",
    summary: "Transcription, editing by transcript, and a fix that stops renders overwriting your audio.",
    bullets: [
      { text: "New: transcription, on your computer, after a one-time model download (Settings → Account).", href: "/docs/user-guide#transcription" },
      "New: cut audio by selecting words in the Transcript tab, or ask to remove filler words.",
      { text: "Transcribing sends no audio. What reaches your AI provider: privacy page.", href: "/privacy" },
      "Fixed: batch and other renders could overwrite audio your project reads; they now refuse.",
      "If you are on v0.7.0 or older: keep copies of originals before batch renders.",
      "Fixed: batch renders could share one output name; lanes went stale after Accept B.",
    ],
  },
  {
    version: "v0.7.0",
    date: "2026-10",
    summary: "Installers carry the ONNX Runtime, plus fixes to controls, the graph and large sessions.",
    bullets: [
      "New: installers include ONNX Runtime; Settings → Account says bundled. No speech or stem model with it.",
      "New: the assistant can page through every track, bus and marker of a big session.",
      "Fixed: faders reverting, stale History graph nodes, and “Could not load the mix”.",
      "Changed: bundled skills update in existing installs; skills you edited or deleted are left alone.",
      "Fixed: a batch run could write into an open project, and a restored folder opened too early.",
      "Security: updated two libraries past advisories about distrusted certificates.",
    ],
  },
  {
    version: "v0.6.1",
    date: "2026-10",
    summary: "Fixes Save As copies, A/B compare renders and speech analysis stalls.",
    bullets: [
      "Fixed: a Save As copy lost its audio when the original was deleted or moved.",
      "If you made a copy with v0.6.0: open it once, with the original present, to repair it.",
      "Fixed: A/B compare left full-length WAVs in the system temp folder.",
      "Fixed: selecting a spoken passage or ducking held up edits while the voice was analysed.",
      "Security: updated the plugin zip library past three advisories; the app never used the affected functions.",
      "Settings → Account shows whether ONNX Runtime is installed (this build: no). Stem separation and transcription not shipped.",
    ],
  },
  {
    version: "v0.6.0",
    date: "2026-10",
    summary: "Master and bus effects, many undo and project-opening fixes, and a safer history compaction.",
    bullets: [
      "Fixed: compacting history could delete a WAV you loaded. Earlier releases have this bug: update before you compact.",
      "New: effects on the master bus and on buses, with a way to hear one before adding it.",
      "Fixed: an effect the app cannot run no longer breaks previews; it is skipped.",
      "Fixed: about ten undo, Save As, project-opening and label bugs that lost or misplaced edits.",
      "Fixed: keychain failures on Linux are now reported instead of hidden.",
      "Security: updated rustls past four advisories. Stem separation and transcription are still not shipped.",
    ],
  },
  {
    version: "v0.5.0",
    date: "2026-10",
    summary: "Current models by default, plus fixes to chats, plan cards, Ollama and Linux keychain handling.",
    bullets: [
      "Changed: Claude Sonnet 5.5 is the Anthropic default; other providers move to current models.",
      "Fixed: the second message of a chat on a 5.x model could fail with a thinking-block error.",
      "Fixed: edits you make while the assistant works are kept, in order, and no longer freeze the window.",
      "Changed: Ollama gets a smaller tool set, instead of about 15,000 tokens of descriptions.",
      "Fixed: on Linux, keys survive a reboot via the Secret Service; the app says when not.",
      "New: ducking music under speech works without a transcript. Stem separation and transcription are still not shipped.",
    ],
  },
  {
    version: "v0.4.0",
    date: "2026-10",
    summary: "Play works straight away, undo follows your path, and the limiter limits instead of clipping.",
    bullets: [
      "Fixed: Space and the new Play button play at once, and play the latest edit.",
      "Fixed: undo follows the path you took, and a new edit after undo clears redo.",
      "Fixed: the limiter clipped; it now limits.",
      "Fixed: requests capped at 10 tool calls now allow 20, and say what is left.",
      "New: a reasoning-effort setting for Anthropic models, in Settings.",
      "Fixed: Plan first now holds every edit until you approve it.",
    ],
  },
  {
    version: "v0.3.0",
    date: "2026-10",
    summary: "Playback on Linux and macOS, and an assistant that knows the session it is editing.",
    bullets: [
      "Fixed: Space played nothing (“403 Forbidden”) on Linux and macOS. Windows was not affected.",
      "New: the assistant knows every track, its level, pan and effects, and the current version.",
      "Fixed: Plan first now works with Ollama, Groq and Gemini; before, edits ran unapproved.",
      "Fixed: window freezes during long edits; errors now name the failing provider.",
      "Changed: history stays under 2 GiB; old audio is rebuilt on demand.",
      "Fixed: undo with a slider focused, and opening an unreadable file now says which and why.",
    ],
  },
  {
    version: "v0.2.0",
    date: "2026-09",
    summary: "Transcript editing, one track per speaker, region descriptions, a shared timeline axis and security hardening.",
    bullets: [
      "New: edit by transcript. Transcription had not shipped, so the pane stays empty in this release.",
      "New: one track per speaker; describe a region (only bar ranges resolve for now).",
      "New: timer record, punch-in, a label lane you type into, and MP3 and FLAC metadata tags.",
      "Fixed: one shared time axis and scrollbar; undo and redo on macOS; opening a file loads it directly.",
      "Fixed: edits keep volume automation, labels and transcript; audio fixes for latency and sample rates.",
      "Security: Content Security Policy; the webview reads only project audio; fonts are bundled.",
    ],
  },
  {
    version: "v0.1.0",
    date: "2026-08",
    summary: "Effect chains, mixer controls, bus routing, MP3 and FLAC export, and vocoder and time-stretch fixes.",
    bullets: [
      "New: per-track effect chains and bus routing, with gain, pan, mute and solo per track.",
      "New: editable volume automation, a clip timeline, a base URL per provider, and Ollama as a sixth provider.",
      "New: MP3 and FLAC export, and loudness normalisation to LUFS.",
      "Fixed: time-stretch and pitch-shift distortion, drift and timing errors; align_to_beat and preserve_formants now work.",
      "New: storage_report shows what disk use belongs to the current version, undo or nothing.",
      "Changed: every history node records the tool that made it.",
    ],
  },
  {
    version: "v0.1.0-dev (earlier)",
    date: "2026-08",
    summary: "69 built-in tools, MCP, skills, profiles, memory and recording, with many audio fixes.",
    bullets: [
      "New: 69 audio tools, microphone recording, a spectrogram toggle, and Audacity label import and export.",
      "New: MCP servers start at launch; install skills and profiles from GitHub or a local path.",
      "New: eight pre-installed skills, agent profiles, global and project memory, editable plan steps.",
      "Changed: OpenAI joins Anthropic and OpenRouter, each with its own keychain slot.",
      "Fixed: stereo edits, channel-count headers, filters above Nyquist, and reversed ranges.",
      "Fixed: renders dropped audio after an interior cut, and cuts lost volume automation.",
    ],
  },
];

/** The newest releases are shown in full; older ones start collapsed. */
const OPEN_RELEASES = 3;

function Bullets({ items }: { items: Bullet[] }) {
  return (
    <ul>
      {items.map((b, i) => (
        <li key={i}>
          {typeof b === "string" ? (
            b
          ) : (
            <>
              {b.text} <a href={b.href}>Details</a>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}

export default function ChangelogPage() {
  const recent = entries.slice(0, OPEN_RELEASES);
  const older = entries.slice(OPEN_RELEASES);
  return (
    <LegalShell title="Changelog" path="/changelog">
      <p>
        Newest first. Installers and tags are on{" "}
        <a href="https://github.com/laadtushar/edytlab/releases">
          GitHub Releases
        </a>
        .
      </p>
      {/* Flat — heading, summary, list — so the shell's cascade reveals
          each part as it arrives instead of a whole release at once. */}
      {recent.map((e) => (
        <Fragment key={e.version}>
          <h2>
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
          </h2>
          <p>{e.summary}</p>
          <Bullets items={e.bullets} />
        </Fragment>
      ))}
      <h2>Older releases</h2>
      {older.map((e) => (
        <details key={e.version} className="mt-3 rounded-lg border border-border/50 px-4 py-2">
          <summary className="cursor-pointer font-semibold">
            {e.version}
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.date}
            </span>
            <span className="ml-3 text-sm font-normal text-muted-foreground">
              {e.summary}
            </span>
          </summary>
          <Bullets items={e.bullets} />
        </details>
      ))}
    </LegalShell>
  );
}
