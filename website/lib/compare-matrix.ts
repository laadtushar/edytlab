/**
 * The matrix on `/compare`: nine products, seven columns, one cell each
 * that says what the product's own page says and links that page.
 *
 * Every cell outside edytlab's own row carries the address of the page
 * it was read on and the day it was read, and `competitors.test.ts`
 * fails when a link is not https, when a date is missing, or when one
 * is more than 100 days old. A cell for which the page read says
 * nothing is marked `absent`: its text says so ("Not described"), it
 * still links the page that was read, and it is a statement about that
 * page and not about the product. The page prints that rule above the
 * table.
 *
 * Read on 2026-10-24, the day each `checkedOn` below names. Pages were
 * fetched with `curl` and the user agent `edytlab-dev-agent`, except
 * Audionaut's home page and the AudacityMCP README, which were read
 * through a page-fetching tool with no cache. Prices are the ones the
 * pages showed, in the currency shown; a quoted phrase is the product's
 * own wording, inside curly quotes, which the superlative guard exempts.
 * Adobe Audition is not in the matrix: it has its own page,
 * `/alternatives/adobe-audition`, built from `lib/competitors.ts`.
 *
 * edytlab's row is last and has the same columns and the same markup as
 * the others. Its cells link edytlab's own docs, which a reader can
 * check against the app, so they carry no date.
 */

import { siteConfig } from "./site";

const READ = "2026-10-24";

export const COLUMNS = [
  { key: "runs", label: "Where it runs" },
  { key: "license", label: "License" },
  { key: "price", label: "Price model" },
  { key: "agent", label: "Chat or agent control" },
  { key: "stems", label: "Stem separation" },
  { key: "transcription", label: "Transcription" },
  { key: "platforms", label: "Platforms" },
] as const;

export type ColumnKey = (typeof COLUMNS)[number]["key"];

export interface Cell {
  /** What the page says, in a line or two. */
  text: string;
  /** The page it was read on (https), or, for edytlab's own row, a page of this site or of the repository. */
  href: string;
  /** The day it was read there, `YYYY-MM-DD`. Absent for edytlab's own row. */
  checkedOn?: string;
  /** The page read says nothing on this: the cell says so and is a statement about the page. */
  absent?: boolean;
  /** The link's words, for a page of this site; a page elsewhere is named by its host and path. */
  label?: string;
}

export interface MatrixRow {
  /** Kebab-case. The id of the product's section on the hub, which the `ItemList` points at. */
  id: string;
  name: string;
  /** What exactly is in the row, when the product alone is not. */
  scope?: string;
  /** A page of this site that covers the product in full, if there is one. */
  detail?: { href: string; label: string };
  cells: Record<ColumnKey, Cell>;
}

const cell = (text: string, href: string, extra: Partial<Cell> = {}): Cell => ({ text, href, checkedOn: READ, ...extra });
const absent = (href: string, text = "Not described"): Cell => cell(text, href, { absent: true });

const AUDACITY = "https://www.audacityteam.org/";
const AUDACITY_AI = "https://plugins.audacityteam.org/ai-plugins/ai-plugins";
const AUDACITY_MCP = "https://github.com/xDarkzx/Audacity-MCP";
const AUDIONAUT = "https://audionaut.app/";
const DESCRIPT_PRICING = "https://www.descript.com/pricing";
const ADOBE_PODCAST = "https://podcast.adobe.com/en/plans";
const AUPHONIC = "https://auphonic.com/";
const CLEANVOICE_PRICING = "https://cleanvoice.ai/pricing/";
const OCENAUDIO = "https://www.ocenaudio.com/features";

export const matrix: readonly MatrixRow[] = [
  {
    id: "audacity",
    name: "Audacity",
    scope: "with its optional OpenVINO AI plugins",
    detail: { href: "/alternatives/audacity", label: "Audacity alternative" },
    cells: {
      runs: cell("Its AI plugins \u201crun 100% locally on your PC\u201d, per its plugin page.", AUDACITY_AI),
      license: cell("GPL v3.", "https://www.audacityteam.org/audacity-4/"),
      price: cell("Free.", AUDACITY),
      agent: absent(AUDACITY),
      stems: cell("A plugin, Music Separation, splits a track into drums, bass, vocals and other instruments.", AUDACITY_AI),
      transcription: cell("A plugin, Whisper Transcription, writes a label track.", AUDACITY_AI),
      platforms: cell("Windows, macOS and Linux.", AUDACITY),
    },
  },
  {
    id: "audacity-mcp",
    name: "Audacity with the AudacityMCP server",
    scope: "a separate, third-party project",
    detail: { href: "/alternatives/audacity#chat-control-in-audacity", label: "Chat control inside Audacity" },
    cells: {
      runs: cell(
        "Drives Audacity on your computer, with no cloud for audio processing, per its README. The assistant is your own MCP client.",
        AUDACITY_MCP,
      ),
      license: cell("Apache License 2.0.", AUDACITY_MCP),
      price: cell("Its README speaks of keeping the project “maintained and free for everyone”.", AUDACITY_MCP),
      agent: cell("Yes: MCP, with a tool set its README counts at 144. It works with Audacity 3.x and cannot control Audacity 4.", AUDACITY_MCP),
      stems: absent(AUDACITY_MCP),
      transcription: cell("Experimental: faster-whisper, offline, set up separately.", AUDACITY_MCP),
      platforms: cell("Install steps for Windows, macOS and Linux.", AUDACITY_MCP),
    },
  },
  {
    id: "audionaut",
    name: "Audionaut",
    scope: "an open-source multitrack editor",
    cells: {
      runs: cell("A desktop app. Its stems are made on your machine. The assistant is your own MCP client.", AUDIONAUT),
      license: cell("GPLv3.", AUDIONAUT),
      price: cell("Free.", AUDIONAUT),
      agent: cell("Yes: MCP. Each change from an assistant arrives as one undo step.", AUDIONAUT),
      stems: cell("Yes: drums, bass, other and vocals, on your machine.", AUDIONAUT),
      transcription: absent(AUDIONAUT),
      platforms: cell("macOS, Windows and Linux.", AUDIONAUT),
    },
  },
  {
    id: "descript",
    name: "Descript",
    scope: "with its Underlord AI co-editor",
    cells: {
      runs: cell(
        "Cloud-based projects: its podcasting page says “Because it’s all in the cloud”. Where its AI runs is not stated.",
        "https://www.descript.com/podcasting",
      ),
      license: absent(DESCRIPT_PRICING, "No open-source license stated"),
      price: cell("A free plan, and paid plans from $16 a month per person billed annually.", DESCRIPT_PRICING),
      agent: cell(
        "Yes: Underlord, an AI co-editor in beta. Each prompt uses credits.",
        "https://help.descript.com/getting-started/underlord-beta-your-ai-co-editor-in-descript",
      ),
      stems: absent(DESCRIPT_PRICING),
      transcription: cell("Yes: transcription in 25 languages, and edits are made in the transcript.", DESCRIPT_PRICING),
      platforms: cell("Desktop apps for macOS 14 or later and Windows 11 or later.", "https://www.descript.com/download"),
    },
  },
  {
    id: "adobe-podcast",
    name: "Adobe Podcast",
    scope: "Enhance Speech and Studio",
    cells: {
      runs: cell("Cloud: “all on the web”, in the browser.", ADOBE_PODCAST),
      license: absent(ADOBE_PODCAST, "No open-source license stated"),
      price: cell("Premium comes with the Adobe Firefly Pro plan. The plans page shows no price.", ADOBE_PODCAST),
      agent: absent(ADOBE_PODCAST),
      stems: absent(ADOBE_PODCAST),
      transcription: absent(ADOBE_PODCAST),
      platforms: cell("Web.", ADOBE_PODCAST),
    },
  },
  {
    id: "auphonic",
    name: "Auphonic",
    scope: "automatic audio post-production",
    cells: {
      runs: cell("Hosted: you upload audio or video to the service.", AUPHONIC),
      license: absent("https://auphonic.com/pricing", "No open-source license stated"),
      price: cell("Free for 2 hours of audio a month; paid recurring plans and one-time credits.", "https://auphonic.com/pricing"),
      agent: absent(AUPHONIC),
      stems: absent(AUPHONIC),
      transcription: cell("Yes: multilingual speech-to-text, with a transcript editor.", AUPHONIC),
      platforms: cell("Web, with an API, watch folders and a command-line tool.", AUPHONIC),
    },
  },
  {
    id: "cleanvoice",
    name: "Cleanvoice AI",
    scope: "podcast cleanup as a service",
    cells: {
      runs: cell("Cloud: it stores your original and edited files for 7 days, then removes them.", CLEANVOICE_PRICING),
      license: absent(CLEANVOICE_PRICING, "No open-source license stated"),
      price: cell("A free trial, then pay-as-you-go credits or a monthly subscription.", CLEANVOICE_PRICING),
      agent: absent(CLEANVOICE_PRICING),
      stems: absent(CLEANVOICE_PRICING),
      transcription: cell("Yes: transcription and summary.", CLEANVOICE_PRICING),
      platforms: cell("A web app, plus an API.", CLEANVOICE_PRICING),
    },
  },
  {
    id: "ocenaudio",
    name: "ocenaudio",
    scope: "a simple cross-platform editor",
    cells: {
      runs: cell("A desktop app: native applications for each platform.", OCENAUDIO),
      license: absent(OCENAUDIO, "No license stated"),
      price: cell("A free download.", "https://www.ocenaudio.com/download"),
      agent: absent(OCENAUDIO),
      stems: absent(OCENAUDIO),
      transcription: absent(OCENAUDIO),
      platforms: cell("Windows, macOS and Linux.", OCENAUDIO),
    },
  },
  {
    id: "edytlab",
    name: "edytlab",
    scope: "this site’s project",
    detail: { href: "/docs/getting-started", label: "Getting started" },
    cells: {
      runs: {
        text: "A desktop app. Audio is processed on your computer; the chat goes to the AI provider you pick, or stays on your machine with Ollama.",
        href: "/privacy",
        label: "Privacy",
      },
      license: { text: "MIT.", href: `${siteConfig.github}/blob/main/LICENSE`, label: "LICENSE on GitHub" },
      price: {
        text: "The app is free. A hosted AI provider bills you for tokens; Ollama is free and runs on your machine.",
        href: "/docs/getting-started",
        label: "Getting started",
      },
      agent: {
        text: "Yes: a built-in agent calls the editing tools from your request. It is also an MCP client.",
        href: "/docs/tools",
        label: "Tools reference",
      },
      stems: {
        text: "Not yet. The tool exists and returns an error today (#385).",
        href: `${siteConfig.github}/issues/385`,
        label: "Issue #385 on GitHub",
      },
      transcription: {
        text: "Yes, on your computer, after a one-time download of the speech model (about 139 MB) that you start from Settings.",
        href: "/docs/user-guide",
        label: "User guide",
      },
      platforms: { text: "macOS, Windows and Linux. The installers are unsigned.", href: "/docs/getting-started", label: "Getting started" },
    },
  },
];

/** True for a link that leaves the site. */
export function isExternal(href: string): boolean {
  return /^https?:\/\//.test(href);
}

/** `2026-10-24` as "24 Oct 2026": the short form the matrix has room for. */
export function shortDate(iso: string): string {
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const [year, month, day] = iso.split("-").map(Number);
  return `${day} ${months[month - 1]} ${year}`;
}

/** The newest day any cell outside edytlab's row was read: the page's "last verified". */
export function matrixCheckedOn(): string {
  const days = matrix.flatMap((row) => Object.values(row.cells).map((c) => c.checkedOn)).filter((d): d is string => Boolean(d));
  return days.reduce((a, b) => (a > b ? a : b));
}
