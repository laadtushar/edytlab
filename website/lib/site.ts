export const siteConfig = {
  name: "edytlab",
  title: "edytlab — Describe it. Get pro-grade audio edits.",
  description:
    "Free, open-source desktop audio editor: chat with an AI to load, cut, mix and render. Pure-Rust DSP on your machine, local-first, bring your own LLM key.",
  // The canonical origin: the host that serves the pages. The apex
  // (edytlab.com) answers with a redirect to this one, so it is never
  // the URL a page, a sitemap or a structured-data block should name.
  url: "https://www.edytlab.com",
  // Placeholder shown only when the GitHub API is unreachable — the real
  // version comes from `getLatestRelease()`. It matches no tag on purpose:
  // if you see it on the live site, the fetch failed.
  version: "v0.1.0-dev",
  github: "https://github.com/laadtushar/edytlab",
  // The list, not `/releases/latest`: every dev build is a prerelease, and
  // `/latest` skips prereleases, so it would hide every dev build newer
  // than the last versioned release.
  releases: "https://github.com/laadtushar/edytlab/releases",
  designSpec:
    "https://github.com/laadtushar/edytlab/blob/main/docs/specs/2026-05-05-conversational-audio-editor-design.md",
  // Search keywords are claims too: nothing here names a feature that
  // has not shipped (stem separation, vocal isolation, transcription).
  keywords: [
    "audio editor",
    "AI audio editor",
    "conversational DAW",
    "AI podcast editor",
    "pitch shift",
    "time stretch",
    "beat alignment",
    "beatmatching",
    "local-first audio",
    "offline audio editor",
    "bring your own API key",
    "Ollama",
    "open source audio editor",
    "music production AI",
    "AI mixing software",
    "podcast editing software",
    "free audio editor",
    "DAW alternative",
    "Tauri desktop app",
    "Rust DSP",
  ],
} as const;
