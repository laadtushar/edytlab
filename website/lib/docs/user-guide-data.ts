/**
 * The pages of the user guide. `/docs/user-guide` is an index of these and
 * `/docs/user-guide/<slug>` is one of them, made by the one route
 * `app/docs/user-guide/[section]/page.tsx` from this array.
 *
 * Unlike the tools and API references, whose pages are lists of records, a
 * guide page is prose, so the data holds only what the index, the pager,
 * the sitemap and the old-link redirect need (a slug, a title, one
 * sentence, the headings), and the prose itself is one component per page
 * in `components/docs/user-guide/`, found by the same slug in
 * `components/docs/user-guide/index.ts`.
 *
 * `headings` are the `h2`s of the page, each with the id it carries. A
 * page with one heading shows none (the page's own title says it), and so
 * lists none.
 */

export interface GuideHeading {
  /** The element id of the `h2` on the page, and the hash a link to it uses. */
  id: string;
  title: string;
}

export interface GuidePage {
  /** The last segment of the page's path: `/docs/user-guide/<slug>`. */
  slug: string;
  title: string;
  /** One sentence: the card on the index, the page's lead, its meta description. */
  summary: string;
  headings: GuideHeading[];
}

export const guidePages: GuidePage[] = [
  {
    slug: "sessions-and-tracks",
    title: "Sessions and Tracks",
    summary:
      "The session graph, undo and redo, and how audio is loaded onto tracks and mixed with lane controls.",
    headings: [
      { id: "sessions-and-the-session-graph", title: "Sessions and the Session Graph" },
      { id: "tracks", title: "Tracks" },
    ],
  },
  {
    slug: "effects-and-templates",
    title: "Effects, Buses and Templates",
    summary: "Effect chains, buses and the master bus, and the session templates you start from.",
    headings: [
      { id: "effects-buses-and-the-master", title: "Effects, Buses and the Master" },
      { id: "session-templates", title: "Session Templates" },
    ],
  },
  {
    slug: "chat",
    title: "The Chat Interface",
    summary: "How to direct the agent in plain English: prompts, big sessions and Plan first.",
    headings: [],
  },
  {
    slug: "playback-and-compare",
    title: "Playback, Markers and A/B Compare",
    summary: "Playing the mix, marking places and ranges, and comparing two versions side by side.",
    headings: [
      { id: "playback", title: "Playback" },
      { id: "markers-and-selections", title: "Markers and Selections" },
      { id: "a-b-compare", title: "A/B Compare" },
    ],
  },
  {
    slug: "speech",
    title: "Stem Separation and Transcription",
    summary: "What stem separation does today, and transcribing speech with the Whisper model you download.",
    headings: [
      { id: "stem-separation", title: "Stem Separation" },
      { id: "transcription", title: "Transcription" },
    ],
  },
  {
    slug: "export-and-memory",
    title: "Export and Memory",
    summary: "Rendering your session to a file, and what the agent remembers between sessions.",
    headings: [
      { id: "export", title: "Export" },
      { id: "memory", title: "Memory" },
    ],
  },
  {
    slug: "skills-and-profiles",
    title: "Skills, Profiles and Shortcuts",
    summary: "Reusable skills, agent profiles, and the tips and keyboard shortcuts worth knowing.",
    headings: [
      { id: "skills", title: "Skills" },
      { id: "agent-profiles", title: "Agent Profiles" },
      { id: "tips-and-keyboard-shortcuts", title: "Tips and Keyboard Shortcuts" },
    ],
  },
  {
    slug: "providers",
    title: "LLM Provider and Model",
    summary: "Choosing a provider and model, where your keys are kept, and the speech model download.",
    headings: [],
  },
  {
    slug: "chatgpt-plan",
    title: "Using your ChatGPT plan",
    summary: "Running the assistant on a ChatGPT Plus or Pro plan with Sign in with ChatGPT, a preview.",
    headings: [],
  },
];
