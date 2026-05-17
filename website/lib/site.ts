export const siteConfig = {
  name: "edytlab",
  title: "edytlab — Describe it. Get pro-grade audio edits.",
  description:
    "Desktop audio editor where you chat with an AI to load, cut, mix, transcribe, and render. Pure-Rust DSP, local-first, BYO LLM key.",
  url: "https://edytlab.app",
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
    "mashup",
    "stem separation",
    "Demucs",
    "Whisper",
    "Rubber Band",
    "Tauri",
    "Rust DSP",
    "local-first",
    "podcast editor",
    "music production",
    "AI mixing",
  ],
} as const;

export type SiteConfig = typeof siteConfig;
