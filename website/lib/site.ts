export const siteConfig = {
  name: "edytlab",
  title: "edytlab — Describe it. Get pro-grade audio edits.",
  description:
    "Desktop audio editor where you chat with an AI to load, cut, mix, transcribe, and render. Pure-Rust DSP, local-first, BYO LLM key.",
  url: "https://edytlab.com",
  ogImage: "/og.png",
  // Pinned canonical version for download links until release fetch is wired up.
  version: "v0.1.0-dev",
  github: "https://github.com/laadtushar/edytlab",
  releases: "https://github.com/laadtushar/edytlab/releases/latest",
  designSpec:
    "https://github.com/laadtushar/edytlab/blob/main/docs/specs/2026-05-05-conversational-audio-editor-design.md",
  keywords: [
    "audio editor",
    "AI audio editor",
    "conversational DAW",
    "AI podcast editor",
    "stem separation",
    "vocal isolation",
    "Demucs",
    "Whisper transcription",
    "pitch shift",
    "time stretch",
    "beat alignment",
    "local-first audio",
    "offline audio editor",
    "bring your own API key",
    "open source audio editor",
    "music production AI",
    "AI mixing software",
    "podcast editing software",
    "audio transcription",
    "free audio editor",
    "DAW alternative",
    "Tauri desktop app",
    "Rust DSP",
  ],
} as const;

export type SiteConfig = typeof siteConfig;
