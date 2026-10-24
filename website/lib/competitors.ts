/**
 * What the comparison pages say about other products, written down once.
 *
 * Every fact here was read on the product's own page, and carries that
 * page's address and the day it was read. A comparison page takes its
 * claims from this file rather than writing them again, so the source
 * and the date travel with the fact: `FactsTable` prints both beside it,
 * `SourcesList` lists them at the foot, and `competitors.test.ts` fails
 * when a link is not https, when a date is missing, or when one is more
 * than 100 days old.
 *
 * The rules this file keeps (`/about` states them to readers):
 *
 * - Only what the product's own pages say. Where a page is silent the
 *   text says "the pages read do not state" and stops; it never says the
 *   product lacks something its pages do not rule out.
 * - A price is the price shown on the day, in the currency shown, with
 *   the plan's own name. A quoted phrase is the product's own wording,
 *   inside curly quotes, which the superlative guard exempts.
 * - Names are plain text. There is no logo, screenshot or wordmark
 *   anywhere in the repository (the test looks).
 * - A fact is moved to a new date only after the source is read again.
 *   Re-check every one by `REFRESH_AFTER_DAYS`; the test allows ten more.
 *
 * Read on 2026-10-22 (the day each `checkedOn` below names), with `curl`
 * and the user agent `edytlab-dev-agent`. Two Adobe pages answer that
 * with an access error, the Audition product page and its system
 * requirements page, so those two were read through a page-fetching
 * tool instead. A third, the Adobe Podcast help page that lists its free
 * plan's limits, was not read, so those limits are not stated here.
 */

/** The day-count the pages promise: "re-checked at least every 90 days". */
export const REFRESH_AFTER_DAYS = 90;

/** One fact about a product, with where it was read and when. */
export interface Sourced {
  text: string;
  /** The product's own page that states it. https, and a clean address (no utm_ parameters). */
  url: string;
  /** The day it was read there, `YYYY-MM-DD`. */
  checkedOn: string;
}

/** One sentence a comparison page may lean on. */
export interface Claim {
  /** What it is about, kebab-case and unique within a product: `claimFor` looks it up by this. */
  topic: string;
  text: string;
  sourceUrl: string;
  checkedOn: string;
}

export interface Competitor {
  /** Kebab-case. The page for a product is `/alternatives/<slug>`. */
  slug: string;
  /** The product as it names itself, in plain type. */
  name: string;
  category: string;
  platforms: Sourced;
  pricing: Sourced;
  openSource: Sourced;
  localOrCloud: Sourced;
  aiFeatures: Sourced;
  claims: Claim[];
}

const CHECKED = "2026-10-22";

export const competitors: readonly Competitor[] = [
  {
    slug: "audacity",
    name: "Audacity",
    category: "General-purpose audio editor and recorder",
    platforms: {
      text: "Windows (x86_64 and ARM64 installers), macOS (universal: Apple Silicon and Intel) and Linux (x86_64 and ARM64 AppImages). Current version 4.0.1.",
      url: "https://www.audacityteam.org/download/",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "Free: “Audacity has always been and will always remain free for everyone.”",
      url: "https://www.audacityteam.org/",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "Yes, GPL v3: its Audacity 4 page lists the license as “Free, GPL v3”.",
      url: "https://www.audacityteam.org/audacity-4/",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "A desktop app: its privacy notice covers “our desktop app, Audacity”. The notice lists three network features, each of which it says can be disabled in preferences: update checking (on by default), error reporting and a random identifier that is stored only if you opt in. Audio is not among the data it lists.",
      url: "https://www.audacityteam.org/legal/privacy-notice/",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "Optional Intel OpenVINO plugins for music separation (stems), noise suppression, music generation and continuation, and Whisper transcription. Its plugin page says these features “run 100% locally on your PC” with “no internet connection necessary”. A separate NVIDIA Broadcast denoiser works only on Windows with an NVIDIA RTX GPU.",
      url: "https://plugins.audacityteam.org/ai-plugins/ai-plugins",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "free-open-source",
        text: "Its home page says Audacity “has always been and will always remain free for everyone” and calls it “proudly open source”.",
        sourceUrl: "https://www.audacityteam.org/",
        checkedOn: CHECKED,
      },
      {
        topic: "license",
        text: "Its Audacity 4 page lists the license as “Free, GPL v3”, with the note “The same free, open-source license Audacity has always shipped under.”",
        sourceUrl: "https://www.audacityteam.org/audacity-4/",
        checkedOn: CHECKED,
      },
      {
        topic: "license-faq",
        text: "Its FAQ says Audacity is licensed under the GNU General Public License.",
        sourceUrl: "https://www.audacityteam.org/faq/",
        checkedOn: CHECKED,
      },
      {
        topic: "recording-and-effects",
        text: "Its FAQ lists, among the app’s features, recording live with a microphone or mixer, adjusting tempo without altering pitch (or the reverse), noise reduction and a spectrogram view.",
        sourceUrl: "https://www.audacityteam.org/faq/",
        checkedOn: CHECKED,
      },
      {
        topic: "daw-limits",
        text: "Its FAQ says Audacity lacks some features DAWs traditionally have, naming MIDI editing and virtual instruments.",
        sourceUrl: "https://www.audacityteam.org/faq/",
        checkedOn: CHECKED,
      },
      {
        topic: "plugins-vst",
        text: "Its home page lists plugin support: “a vast selection of third-party plugins, including VST3, Nyquist and more”.",
        sourceUrl: "https://www.audacityteam.org/",
        checkedOn: CHECKED,
      },
      {
        topic: "plugins-lv2",
        text: "Its Audacity 4 page says open-standard LV2 plugins load “right alongside VST3 and Audio Units”.",
        sourceUrl: "https://www.audacityteam.org/audacity-4/",
        checkedOn: CHECKED,
      },
      {
        topic: "ai-plugin-list",
        text: "Its plugin page lists the OpenVINO effects: music separation into drums, bass, vocals and other instruments; noise suppression; music generation and continuation; and Whisper transcription into a label track.",
        sourceUrl: "https://plugins.audacityteam.org/ai-plugins/ai-plugins",
        checkedOn: CHECKED,
      },
      {
        topic: "ai-transcription",
        text: "Its AI Transcription page says the OpenVINO Whisper Transcription effect runs OpenAI’s Whisper model “entirely on your own computer”.",
        sourceUrl: "https://www.audacityteam.org/features/transcription/",
        checkedOn: CHECKED,
      },
      {
        topic: "ai-vocal-remover",
        text: "Its Vocal Remover page describes the OpenVINO Music Separation plugin as available on Windows and Linux, and also offers a built-in Vocal Reduction and Isolation effect.",
        sourceUrl: "https://www.audacityteam.org/features/vocal-remover/",
        checkedOn: CHECKED,
      },
      {
        topic: "ai-macos-beta",
        text: "A June 2025 post says the OpenVINO plugins came to macOS with Audacity 3.7.4 and calls that release a beta of sorts. None of the pages read says whether the plugins work with Audacity 4.",
        sourceUrl: "https://www.audacityteam.org/blog/openvino-mac/",
        checkedOn: CHECKED,
      },
      {
        topic: "musehub-ai-bundles",
        text: "Its home page promotes plugins from MuseHub, one described as “Clean up spoken voice and create instant transcripts” and another as “Automatically split mixed-speaker audio into separate tracks”.",
        sourceUrl: "https://www.audacityteam.org/",
        checkedOn: CHECKED,
      },
      {
        topic: "cloud-storage",
        text: "Its FAQ says that if you found yourself paying for Audacity, one cause is signing up for additional Audacity Cloud storage.",
        sourceUrl: "https://www.audacityteam.org/faq/",
        checkedOn: CHECKED,
      },
      {
        topic: "musehub-paid",
        text: "Its Audacity 4 page says MuseHub’s “Get Effects” lists plugins, samples and oneshots, “including free and paid options”.",
        sourceUrl: "https://www.audacityteam.org/audacity-4/",
        checkedOn: CHECKED,
      },
      {
        topic: "privacy-notice",
        text: "Its privacy notice for Audacity 4 and newer, last updated 31 August 2026, says all network features are optional: update checking (on by default), error reporting and a randomly generated identifier (UUID) that is stored only if you opt in, each of which can be disabled in preferences.",
        sourceUrl: "https://www.audacityteam.org/legal/privacy-notice/",
        checkedOn: CHECKED,
      },
      {
        topic: "label-format",
        text: "Its manual says label tracks export to and import from tab-delimited plain text: start time in seconds, end time, then the label’s text, with start and end the same for a point label.",
        sourceUrl: "https://manual.audacityteam.org/man/importing_and_exporting_labels.html",
        checkedOn: CHECKED,
      },
      {
        topic: "anniversary",
        text: "Its blog marked the project’s 25th anniversary on 28 May 2025.",
        sourceUrl: "https://www.audacityteam.org/blog/25-years/",
        checkedOn: CHECKED,
      },
      {
        topic: "mcp-third-party",
        text: "The AudacityMCP project’s README says it works with Audacity 3.x and cannot control Audacity 4, which removed the scripting interface it uses; it points to a separate early-alpha project for Audacity 4 that needs a companion fork built from source.",
        sourceUrl: "https://github.com/xDarkzx/Audacity-MCP",
        checkedOn: CHECKED,
      },
    ],
  },
  {
    slug: "adobe-audition",
    name: "Adobe Audition",
    category: "Professional multitrack audio workstation",
    platforms: {
      text: "Windows 11 (version 24H2, including Windows on Arm) and macOS Sonoma (14) or later, per Adobe’s system requirements for Audition 26.5 (September 2026). The page lists no Linux requirements.",
      url: "https://helpx.adobe.com/audition/desktop/introduction/system-requirements.html",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "US$22.99/mo, annual plan billed monthly, for Audition alone; Creative Cloud Pro US$69.99/mo, or US$34.99/mo for the first 3 months for new subscribers; a free trial is offered and its length is not stated on the page.",
      url: "https://www.adobe.com/products/audition.html",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "The pages read describe a commercial subscription product and give no open-source license.",
      url: "https://www.adobe.com/products/audition.html",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "A desktop application. Adobe’s requirements page says an internet connection and registration are necessary for software activation, validation of subscriptions and access to online services.",
      url: "https://helpx.adobe.com/audition/desktop/introduction/system-requirements.html",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "The product page names Remix: “Easily and automatically rearrange any song to fit any duration with Remix in Audition.” The page does not describe chat or agent control.",
      url: "https://www.adobe.com/products/audition.html",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "overview",
        text: "Adobe describes Audition as a toolset that includes multitrack, waveform and spectral display for creating, mixing, editing and restoring audio.",
        sourceUrl: "https://www.adobe.com/products/audition.html",
        checkedOn: CHECKED,
      },
      {
        topic: "help-topics",
        text: "Its help site documents adding third-party plugins and an Essential Sound panel for editing, repairing and improving audio.",
        sourceUrl: "https://helpx.adobe.com/audition/desktop/introduction/system-requirements.html",
        checkedOn: CHECKED,
      },
      {
        topic: "remix",
        text: "Its product page names Remix: “Easily and automatically rearrange any song to fit any duration with Remix in Audition.”",
        sourceUrl: "https://www.adobe.com/products/audition.html",
        checkedOn: CHECKED,
      },
      {
        topic: "activation",
        text: "Its system requirements page says an internet connection and registration are necessary for software activation, validation of subscriptions and access to online services.",
        sourceUrl: "https://helpx.adobe.com/audition/desktop/introduction/system-requirements.html",
        checkedOn: CHECKED,
      },
      {
        topic: "help-silence-loudness",
        text: "Its help site lists the topics “Remove silences from your audio recordings” and “Matching loudness across multiple audio files”.",
        sourceUrl: "https://helpx.adobe.com/audition/desktop.html",
        checkedOn: "2026-10-24",
      },
    ],
  },
  {
    slug: "descript",
    name: "Descript",
    category: "Text-based audio and video editor with an AI co-editor",
    platforms: {
      text: "Desktop apps for macOS 14 (Sonoma) or newer and Windows 11 or newer.",
      url: "https://www.descript.com/download",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "Free plan at $0 (60 media minutes a month, 100 AI credits one time). Hobbyist $16 per person per month billed annually or $24 monthly (10 media hours, 400 AI credits a month); Creator $24 annual or $35 monthly (30 hours, 800 credits); Business $50 annual or $65 monthly (40 hours, 1,500 credits); Enterprise custom. US dollars, as shown on 22 October 2026.",
      url: "https://www.descript.com/pricing",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "The pages read describe a commercial product with free and paid plans and give no open-source license.",
      url: "https://www.descript.com/pricing",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "Its podcasting page says: “Because it’s all in the cloud, you’ll be able to view different versions and restore them if you want.” The pages read do not state where its AI features run.",
      url: "https://www.descript.com/podcasting",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "Underlord, an AI co-editor in beta: its help page says “Each prompt uses credits” and that when it runs a tool you are “charged for both the agent brain and the tool itself”. Prompts can also be sent from Zapier, the Descript API or “the Descript MCP”.",
      url: "https://help.descript.com/getting-started/underlord-beta-your-ai-co-editor-in-descript",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "text-based-editing",
        text: "Its pricing page lists “Text-based editing: Edit video by editing a doc” among its features, and transcription in 25 languages on every plan.",
        sourceUrl: "https://www.descript.com/pricing",
        checkedOn: CHECKED,
      },
      {
        topic: "ai-tools",
        text: "Its pricing page lists the AI tools Studio Sound, Remove Filler Words, Shorten Word Gaps, Edit for Clarity and Remove Retakes, with limits on the free plan.",
        sourceUrl: "https://www.descript.com/pricing",
        checkedOn: CHECKED,
      },
      {
        topic: "underlord-credits",
        text: "Its help page says Underlord is in beta and that its credit use is not fixed: “the same prompt can cost different amounts on different runs”.",
        sourceUrl: "https://help.descript.com/getting-started/underlord-beta-your-ai-co-editor-in-descript",
        checkedOn: CHECKED,
      },
    ],
  },
  {
    slug: "auphonic",
    name: "Auphonic",
    category: "Automatic audio post-production web service",
    platforms: {
      text: "Runs in the browser (upload a file or record), with an API, a command-line tool, watch folders and a Zapier integration listed on its site.",
      url: "https://auphonic.com/",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "Free for 2 hours of processed audio a month (“Productions of free users come with a jingle”). Paid recurring credit plans from 9 to 250 hours a month, and one-time credits that “never expire”. The euro and dollar prices load with JavaScript and are not stated here.",
      url: "https://auphonic.com/pricing",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "The pages read describe a hosted service with free and paid credits and give no open-source license.",
      url: "https://auphonic.com/pricing",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "Hosted: its home page asks you to upload audio or video and says “Please leave your browser open while uploading files.”",
      url: "https://auphonic.com/",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "Its home page lists noise and reverb reduction, an “Intelligent Leveler”, cutting filler words, coughs and silence, multitrack algorithms (ducking, noise gate, mic bleed removal), loudness specifications with a true peak limit, and multilingual speech-to-text.",
      url: "https://auphonic.com/",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "free-tier",
        text: "Its pricing page says Auphonic is free for 2 hours of processed audio per month and sells recurring and one-time credits.",
        sourceUrl: "https://auphonic.com/pricing",
        checkedOn: CHECKED,
      },
      {
        topic: "loudness",
        text: "Its home page describes loudness specifications: “Define a target loudness, true peak limit, MaxLRA, and more for consistency across files and compliance with audio specs.”",
        sourceUrl: "https://auphonic.com/",
        checkedOn: CHECKED,
      },
    ],
  },
  {
    slug: "audionaut",
    name: "Audionaut",
    category: "Open-source multitrack audio editor with agent control over MCP",
    platforms: {
      text: "macOS (Apple silicon, macOS 13 or later), Windows (64-bit) and Linux (x86_64 AppImage and Debian package). Version 1.6.7, 21 October 2026.",
      url: "https://audionaut.app/download",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "Free: “a free, open-source desktop application”. Its page links a sponsorship page.",
      url: "https://audionaut.app/",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "Yes, GPLv3: “Audionaut is open source under the GPLv3 license.” Its page says the code base went open source on 15 August 2026.",
      url: "https://audionaut.app/",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "A desktop app. Its stem separation “runs entirely on your machine” with a model “downloaded once on first use”. The AI assistant is an external MCP client you connect. Its privacy page says anonymous usage statistics are opt-in, never include audio or file names, and that the app checks daily whether a newer version exists unless you turn that off.",
      url: "https://audionaut.app/manual/12-privacy/",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "Speaks MCP, so an MCP-capable assistant can inspect and edit projects conversationally; its page says each change arrives as one undo step. Also built-in analysis and Auto Edit, time-stretch, and stem separation into drums, bass, other and vocals.",
      url: "https://audionaut.app/",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "stem-separation",
        text: "Its home page lists stem separation into Drums, Bass, Other and Vocals as a built-in feature, run on your machine.",
        sourceUrl: "https://audionaut.app/",
        checkedOn: CHECKED,
      },
      {
        topic: "signing",
        text: "Its download page says the macOS build is signed and notarized and that the Windows installer is not yet code-signed.",
        sourceUrl: "https://audionaut.app/download",
        checkedOn: CHECKED,
      },
    ],
  },
  {
    slug: "cleanvoice",
    name: "Cleanvoice AI",
    category: "Cloud service and API for automatic podcast cleanup",
    platforms: {
      text: "A web app, plus an API and SDKs.",
      url: "https://cleanvoice.ai/",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "30 minutes of free credits on sign-up. Pay as you go (credits valid 2 years): $11 for 5 hours, $20 for 10 hours, $45 for 30 hours. Subscription: $11 for 10 hours, $28 for 30 hours, $90 for 100 hours a month. US dollars, as shown on 22 October 2026, without VAT; the page also offers euros.",
      url: "https://cleanvoice.ai/pricing/",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "The pages read describe a hosted service with credits and give no open-source license.",
      url: "https://cleanvoice.ai/pricing/",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "Cloud: its pricing page says “We store both your original and edited files for 7 days. After 7 days, we remove your files permanently.”",
      url: "https://cleanvoice.ai/pricing/",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "Its pricing page lists a filler words remover, background noise remover, silence remover, mouth sounds and breath remover, an audio enhancer (Studio Sound), video podcast editing, timeline export, and transcription and summary.",
      url: "https://cleanvoice.ai/pricing/",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "data-location",
        text: "Its home page says “Data stored in the EU”.",
        sourceUrl: "https://cleanvoice.ai/",
        checkedOn: CHECKED,
      },
      {
        topic: "pricing-note",
        text: "Its pricing page says all plans include all features and are listed without VAT.",
        sourceUrl: "https://cleanvoice.ai/pricing/",
        checkedOn: CHECKED,
      },
    ],
  },
  {
    slug: "adobe-podcast",
    name: "Adobe Podcast",
    category: "Browser-based speech enhancement and recording",
    platforms: {
      text: "Web only: its page title reads “AI audio recording and editing, all on the web”, and Studio (beta) is described as “Record, edit, and enhance in your browser”.",
      url: "https://podcast.adobe.com/en/plans",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "Premium is part of Adobe’s Firefly Pro plan (“Unlock more with the Adobe Firefly Pro plan with access to Adobe Podcast Premium”): Enhance Speech “up to 4 hours a day, files up to 1 GB”. The plans page shows no price, offers a free trial and does not state the free plan’s limits.",
      url: "https://podcast.adobe.com/en/plans",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "The pages read describe a hosted web service and give no open-source license.",
      url: "https://podcast.adobe.com/en/plans",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "Cloud: the service runs “all on the web”, in the browser.",
      url: "https://podcast.adobe.com/en/plans",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "Enhance Speech (“Remove noise and echo from voice recordings”) and Studio (beta), which records, edits and enhances in the browser.",
      url: "https://podcast.adobe.com/en/plans",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "enhance-speech",
        text: "Its plans page lists, for Enhance Speech, video support, bulk upload, and speech, music and ambience adjustment.",
        sourceUrl: "https://podcast.adobe.com/en/plans",
        checkedOn: CHECKED,
      },
    ],
  },
  {
    slug: "ocenaudio",
    name: "ocenaudio",
    category: "Simple cross-platform audio editor",
    platforms: {
      text: "Windows 10 or later (64-bit), macOS 14.4 or later (Intel and ARM; a legacy build covers macOS 10.14 or later), and Linux 64-bit as an AppImage or Debian packages. Version 3.22.1.",
      url: "https://www.ocenaudio.com/download",
      checkedOn: CHECKED,
    },
    pricing: {
      text: "Free: “Download here for free, your ocenaudio version to the platform of your choice!”",
      url: "https://www.ocenaudio.com/download",
      checkedOn: CHECKED,
    },
    openSource: {
      text: "The download, features and about pages do not state a license or say whether the source is open.",
      url: "https://www.ocenaudio.com/whatis",
      checkedOn: CHECKED,
    },
    localOrCloud: {
      text: "A desktop app: its features page says native applications are built for each platform from a common source.",
      url: "https://www.ocenaudio.com/features",
      checkedOn: CHECKED,
    },
    aiFeatures: {
      text: "The pages read describe none. Its features page lists VST plugin support, real-time effect preview, multi-selection, a spectrogram and editing of large files.",
      url: "https://www.ocenaudio.com/features",
      checkedOn: CHECKED,
    },
    claims: [
      {
        topic: "features",
        text: "Its features page lists VST plugin support, real-time preview of effects, multi-selection editing and a spectrogram view.",
        sourceUrl: "https://www.ocenaudio.com/features",
        checkedOn: CHECKED,
      },
    ],
  },
];

export function competitorBySlug(slug: string): Competitor {
  const found = competitors.find((c) => c.slug === slug);
  if (!found) throw new Error(`no competitor with the slug "${slug}"`);
  return found;
}

/** One of a product's claims, by its topic. Throws when there is none, so a page cannot cite a claim that is not written down. */
export function claimFor(c: Competitor, topic: string): Claim {
  const found = c.claims.find((claim) => claim.topic === topic);
  if (!found) throw new Error(`${c.slug} has no claim on "${topic}"`);
  return found;
}

/** The newest `checkedOn` among a product's facts: "last verified" on its pages. */
function checkDays(c: Competitor): string[] {
  return [
    c.platforms.checkedOn,
    c.pricing.checkedOn,
    c.openSource.checkedOn,
    c.localOrCloud.checkedOn,
    c.aiFeatures.checkedOn,
    ...c.claims.map((claim) => claim.checkedOn),
  ];
}

export function newestCheck(c: Competitor): string {
  return checkDays(c).reduce((a, b) => (a > b ? a : b));
}

/** The day the least recently checked fact was read: what a "checked on" line can promise for every fact. */
export function oldestCheck(c: Competitor): string {
  return checkDays(c).reduce((a, b) => (a < b ? a : b));
}

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/**
 * `2026-10-22` as "22 October 2026". Written from the string, not from a
 * `Date`, so the server and the browser print the same day whatever
 * their time zones.
 */
export function displayDate(iso: string): string {
  const [year, month, day] = iso.split("-").map(Number);
  return `${day} ${MONTHS[month - 1]} ${year}`;
}

/**
 * A source link as a reader should see it: `audacityteam.org/download`.
 * The host without `www.`, then the path without its trailing slash.
 */
export function hostAndPath(url: string): string {
  const { hostname, pathname } = new URL(url);
  const path = pathname.replace(/\/+$/, "");
  return `${hostname.replace(/^www\./, "")}${path}`;
}
