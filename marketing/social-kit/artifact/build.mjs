// Build the social-kit artifact page from the reviewed markdown copy files.
import fs from "node:fs";
import path from "node:path";
const dir = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const tabs = [
  { id: "linkedin", label: "LinkedIn", file: "linkedin-posts.md", limit: { re: /.*/, n: 3000, name: "LinkedIn" } },
  { id: "instagram", label: "Instagram", file: "instagram.md", limit: { re: /^\d\.|reel/i, n: 2200, name: "Instagram caption" } },
  { id: "reddit", label: "Reddit", file: "reddit.md" },
  { id: "more", label: "X, Bluesky, Mastodon, TikTok, YouTube, HN", file: "more.md" },
  { id: "calendar", label: "Two-week plan", file: "calendar.md" },
];
const data = tabs.map((t) => ({ id: t.id, label: t.label, md: fs.readFileSync(path.join(dir, t.file), "utf8") }));
const json = JSON.stringify(data).replace(/</g, "\\u003c");
const tpl = fs.readFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), "template.html"), "utf8");
fs.writeFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), "social-kit.html"), tpl.replace("__DATA__", json));
console.log("wrote social-kit.html", fs.statSync(path.join(path.dirname(new URL(import.meta.url).pathname), "social-kit.html")).size, "bytes");
