// Build the social-kit artifact page from the reviewed markdown copy files.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { checkInputPath, checkOutputPath } from "./paths.mjs";

/** Read the reviewed copy and write social-kit.html beside this file. */
function main() {
  // fileURLToPath, not URL.pathname: that keeps %20 for a space and a leading /C: on Windows.
  const HERE = path.dirname(fileURLToPath(import.meta.url));
  const dir = path.resolve(HERE, "..");
  const tabs = [
    { id: "linkedin", label: "LinkedIn", file: "linkedin-posts.md", limit: { re: /.*/, n: 3000, name: "LinkedIn" } },
    { id: "instagram", label: "Instagram", file: "instagram.md", limit: { re: /(?:^\d\.)|reel/i, n: 2200, name: "Instagram caption" } },
    { id: "reddit", label: "Reddit", file: "reddit.md" },
    { id: "more", label: "X, Bluesky, Mastodon, TikTok, YouTube, HN", file: "more.md" },
    { id: "calendar", label: "Two-week plan", file: "calendar.md" },
  ];
  const data = tabs.map((t) => ({ id: t.id, label: t.label, md: fs.readFileSync(checkInputPath(path.join(dir, t.file)), "utf8") }));
  const json = JSON.stringify(data).replace(/</g, "\\u003c");
  const tpl = fs.readFileSync(checkInputPath(path.join(HERE, "template.html")), "utf8");
  checkOutputPath(path.join(HERE, "social-kit.html"));
  fs.writeFileSync(path.join(HERE, "social-kit.html"), tpl.replace("__DATA__", json));
  console.log("wrote social-kit.html", fs.statSync(path.join(HERE, "social-kit.html")).size, "bytes");
}

if (process.argv.includes("--help")) {
  console.log("Usage: node marketing/social-kit/artifact/build.mjs\nBuild the reviewed copy into artifact/social-kit.html.");
} else {
  main();
}
