// Build the launch hub page from the reviewed markdown copy files.
//
//   node marketing/social-kit/artifact/launch-hub.mjs
//
// Reads the marketing copy, takes every postable text block out of it (verbatim, whitespace trimmed),
// and writes marketing/social-kit/artifact/out/launch-hub/ (git-ignored):
//   index.html     the page (a fragment: no doctype, html, head or body tags)
//   files.json     published path -> absolute path of the file to publish beside the page
//   existing-files.txt   the theme images (img/...) that the published page already holds, so they are not copied
//   img/ linkedin/ posters/   the copied files, at their published paths
// Nothing here publishes anything. See README.md beside this file.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const MK = path.resolve(HERE, "../.."); // marketing/
const REPO = path.resolve(MK, "..");
const OUT = path.join(HERE, "out", "launch-hub");

// ---------------------------------------------------------------- reading markdown
const read = (rel) => fs.readFileSync(path.join(MK, rel), "utf8").replace(/\r\n/g, "\n");
const lines = (rel) => read(rel).split("\n");

/** Every fenced block, with the nearest headings and the label line above it. */
function fenced(rel) {
  const ls = lines(rel);
  const out = [];
  let h1 = "", h2 = "", h3 = "", label = "";
  for (let i = 0; i < ls.length; i++) {
    const l = ls[i];
    if (/^# /.test(l)) { h1 = l.slice(2); }
    else if (/^## /.test(l)) { h2 = l.slice(3); h3 = ""; label = ""; }
    else if (/^### /.test(l)) { h3 = l.slice(4).replace(/`/g, ""); label = ""; }
    else if (/^```/.test(l)) {
      const body = [];
      i++;
      while (i < ls.length && !/^```/.test(ls[i])) body.push(ls[i++]);
      out.push({ h1, h2, h3, label, text: body.join("\n").trim() });
    } else if (/^\*\*[^*]+\*\*/.test(l)) {
      label = l.replace(/\*\*/g, "").trim();
    } else if (/^[A-Za-z][^`|]{2,90}:$/.test(l) && !/^[-*\d]/.test(l)) {
      label = l.trim();
    }
  }
  return out;
}
function pick(blocks, where, n = 0) {
  const hit = blocks.filter((b) =>
    (!where.h2 || where.h2.test(b.h2)) && (!where.h3 || where.h3.test(b.h3)) && (!where.label || where.label.test(b.label)));
  if (!hit[n]) throw new Error("no block for " + JSON.stringify(where, (k, v) => (v instanceof RegExp ? String(v) : v)) + " #" + n);
  return hit[n].text;
}
/** Lines of a ## section (without its heading), up to the next ## heading. */
function section(rel, h2re) {
  const ls = lines(rel);
  const i = ls.findIndex((l) => /^## /.test(l) && h2re.test(l.slice(3)));
  if (i < 0) throw new Error(rel + ": no section " + h2re);
  let j = i + 1;
  while (j < ls.length && !/^## /.test(ls[j])) j++;
  return ls.slice(i + 1, j);
}
function tableRows(ls) {
  const rows = ls.filter((l) => /^\|/.test(l)).map((l) => l.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim()));
  return rows.filter((r) => !r.every((c) => /^:?-+:?$/.test(c)));
}
function bullets(ls) {
  const out = [];
  for (const l of ls) {
    if (/^(- |\d+\. )/.test(l)) out.push(l.replace(/^(- |\d+\. )/, ""));
    else if (/^\s{2,}\S/.test(l) && out.length) out[out.length - 1] += " " + l.trim();
  }
  return out;
}
/** First paragraph that follows a bold label like **Rules note:** */
function labelled(blockText, name) {
  const m = new RegExp("\\*\\*" + name + ":?\\*\\*:?\\s*([\\s\\S]*?)(?:\\n\\n|$)").exec(blockText);
  if (!m) return "";
  return m[1].replace(/\s*\n\s*/g, " ").trim();
}

// ---------------------------------------------------------------- html helpers
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const ext = (href, text) => `<a href="${esc(href)}" target="_blank" rel="noopener">${text}</a>`;
function inline(md) {
  let s = esc(md);
  s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
  s = s.replace(/\[([^\]]+)\]\((https?:[^)]+)\)/g, (m, t, u) => ext(u.replace(/&amp;/g, "&"), t));
  s = s.replace(/\[([^\]]+)\]\([^)]+\)/g, "$1"); // relative repo links: text only
  s = s.replace(/&lt;(https?:\/\/[^&]+?)&gt;/g, (m, u) => ext(u, u.replace(/^https?:\/\//, "")));
  s = s.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");
  return s;
}
const table = (rows, header = true) => {
  const [h, ...b] = rows;
  return `<div class="tablewrap"><table>${header ? `<thead><tr>${h.map((c) => `<th>${inline(c)}</th>`).join("")}</tr></thead>` : ""}<tbody>${(header ? b : rows).map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`;
};
const ul = (items) => `<ul class="plain">${items.map((i) => `<li>${inline(i)}</li>`).join("")}</ul>`;

// ---------------------------------------------------------------- the cards
const ids = new Set();
let cardN = 0;
/** One copyable text block. opts: limit, mode ("x"), utm ("src|content"), desc, badge, meta (html), nolimit */
function card(id, title, text, opts = {}) {
  if (ids.has(id)) throw new Error("duplicate id " + id);
  ids.add(id);
  const tid = "t-" + id;
  cardN++;
  const attrs = [`id="${tid}"`, `class="txt"`, `tabindex="0"`];
  if (opts.limit) attrs.push(`data-limit="${opts.limit}"`); else attrs.push(`data-nolimit="1"`);
  if (opts.mode) attrs.push(`data-mode="${opts.mode}"`);
  if (opts.utm) attrs.push(`data-utm="${opts.utm}"`);
  if (opts.desc) attrs.push(`data-desc="1"`);
  const n = countOf(text, opts);
  const meter = opts.limit ? `${n} / ${opts.limit}${n > opts.limit ? " (over)" : ""}` : `${n} characters`;
  const over = opts.limit && n > opts.limit ? " over" : "";
  return `<div class="post" id="${id}"><div class="post-head"><span class="post-title">${esc(title)}${opts.badge ? `<em>${esc(opts.badge)}</em>` : ""}</span><span class="meter${over}">${meter}</span><button type="button" class="copy" data-for="${tid}">Copy</button></div><div ${attrs.join(" ")}>${esc(text)}</div>${opts.meta ? `<div class="meta">${opts.meta}</div>` : ""}</div>`;
}
const segmenter = new Intl.Segmenter();
function countOf(text, opts) {
  let t = text;
  if (opts.desc) { const i = t.indexOf("Description: "); if (i >= 0) t = t.slice(i + 13); }
  if (opts.mode === "x") t = t.replace(/https?:\/\/\S+/g, "x".repeat(23));
  return Array.from(segmenter.segment(t)).length;
}
const h3 = (t, id) => `<h3${id ? ` id="${id}"` : ""}>${esc(t)}</h3>`;
const sourceNote = (rel, extra = "") => `<p class="sub">Source: <code>marketing/${rel}</code>${extra ? " " + extra : ""}</p>`;

// ---------------------------------------------------------------- constants
const SITE = "https://www.edytlab.com";
const SIZES = ["square-1080x1080", "portrait-1080x1350", "story-1080x1920", "x-1600x900", "linkedin-1200x627", "youtube-1280x720", "pinterest-1000x1500"];
const PLATFORMS = [
  // name as in captions.md, utm_source, size, limit, mode
  ["X", "x", "x-1600x900", 280, "x"],
  ["LinkedIn", "linkedin", "linkedin-1200x627", 3000],
  ["Instagram", "instagram", "portrait-1080x1350", 2200],
  ["Facebook", "facebook", "square-1080x1080", 63206],
  ["Threads", "threads", "square-1080x1080", 500],
  ["Bluesky", "bluesky", "x-1600x900", 300],
  ["TikTok", "tiktok", "story-1080x1920", 2200],
  ["YouTube", "youtube", "youtube-1280x720", 5000, null, true],
  ["Pinterest", "pinterest", "pinterest-1000x1500", 500, null, true],
];
const THEME_UTM = ["describe-the-edit", "dj-transition", "club-intro", "local-audio", "versions", "open-source"];
const THEME_VIDEO = [
  "launch video (a typed request becoming tool calls); reel-highlights for vertical platforms on day 1",
  "reel-beatmatched-transition", "reel-extended-club-intro", "image only (no video of its own)", "the videos page", "image only; link GitHub",
];

const files = {}; // published path -> absolute source
const addFile = (pub, src) => {
  if (!fs.existsSync(src)) throw new Error("missing file " + src);
  files[pub] = src;
};

// ---------------------------------------------------------------- source blocks
const cap = (() => {
  const ls = lines("social-kit/images/captions.md");
  const themes = [];
  let cur = null, plat = null;
  for (let i = 0; i < ls.length; i++) {
    const l = ls[i];
    const th = /^## (\d)\. (.+)$/.exec(l);
    if (th) { cur = { n: +th[1], title: th[2], image: "", videoLine: "", caps: {} }; themes.push(cur); plat = null; continue; }
    if (!cur) continue;
    if (/^Image:/.test(l)) { cur.image = l; continue; }
    const pm = /^\*\*(X|LinkedIn|Instagram|Facebook|Threads|Bluesky|TikTok|YouTube|Pinterest)\*\*$/.exec(l);
    if (pm) {
      const body = [];
      let j = i + 1;
      while (j < ls.length && ls[j].trim() !== "" && ls[j] !== "---") body.push(ls[j++]);
      cur.caps[pm[1]] = body.join("\n").trim();
      i = j - 1;
    }
  }
  return themes;
})();
if (cap.length !== 6) throw new Error("expected six themes in captions.md");

const LI = fenced("social-kit/linkedin-posts.md");
const MORE = fenced("social-kit/more.md");
const IG = fenced("social-kit/instagram.md");
const RD = fenced("social-kit/reddit.md");
const PH = fenced("launch/product-hunt.md");
const HNA = fenced("launch/show-hn.md");
const XT = fenced("social/x-thread.md");
const LIA = fenced("social/linkedin.md");

// ---------------------------------------------------------------- 1. checklist
const V = (slug) => `${SITE}/videos/${slug}.mp4`;
const lk = (t, href) => ({ t, href });
const vid = (slug) => lk(slug + ".mp4", `#v-${slug}`);
const pub = (rel) => lk(rel.split("/").pop(), rel);
const DAYS = [
  {
    id: "prep", title: "Before Tue 27 Oct (this weekend and Monday)",
    note: "From calendar.md, README.md (\"Before the first post\") and issue #728, step 0 and 1.",
    items: [
      ["prep-install", "Install v0.9.2 on one machine per OS you will claim (not v0.9.0: its first message after Continue with ChatGPT failed). Check the macOS xattr step in the release notes works as written.", {}],
      ["prep-chatgpt", "Sign in with a real ChatGPT plan on v0.9.2 and send a message before any post that names the ChatGPT plan. (Recorded as working on a Plus account in #727.)", {}],
      ["prep-accounts", "Decide which accounts exist and are yours to post on: LinkedIn Showcase page, X, Instagram, TikTok, YouTube, Bluesky, Threads, Facebook, Pinterest, Reddit, Hacker News, Product Hunt.", {}],
      ["prep-media", "Download the videos and images you need into one folder, and play each video once.", { jump: "videos" }],
      ["prep-youtube", "Upload launch-video.mp4 to YouTube with the theme 1 YouTube title and description. Product Hunt takes video as a YouTube link only.", { jump: "cap-1-youtube", links: [vid("launch-video")] }],
      ["prep-ph-account", "Create the Product Hunt maker account and read the live submission form (limits and topic names change).", { jump: "dest-ph" }],
      ["prep-ph-gallery", "Make the Product Hunt gallery: five images of 1280x720 (themes 1, 2, 3, 5, 4) or screenshots from the running v0.9.2 app at 1280x776.", { jump: "ph-gallery" }],
      ["prep-claude-line", "Decide the Claude Code disclosure (LinkedIn post 3, Show HN, r/rust and r/opensource include it; the kit says keep it).", {}],
      ["prep-gfi", "Label two small issues good first issue, so LinkedIn post 5 and r/opensource can name real ones (or edit that paragraph).", {}],
      ["prep-rules", "Read the sidebars of r/rust, r/LocalLLaMA, r/DJs, r/opensource, r/edmproduction and r/WeAreTheMusicMakers. Drop or move any post a sub forbids.", { jump: "rd-rules" }],
      ["prep-ig-bio", "Set the Instagram bio link and bio line.", { jump: "ig-bio" }],
      ["prep-repo", "Repo topics and social preview image (needs repo admin; README.md, \"Before the first post\").", {}],
      ["prep-phone", "Open edytlab.com on a phone and play a demo.", {}],
    ],
  },
  {
    id: "d1", title: "Tue 27 Oct: Day 1, the launch",
    note: "LinkedIn page post L1 is scheduled for 11:00 ET (15:00 UK). Own feeds use the theme 1 captions, with reel-highlights.mp4 as the vertical video. Never ask anyone to upvote, star or share.",
    items: [
      ["d1-li1", "LinkedIn page L1 at 11:00 ET / 15:00 UK: post 1 with reel-highlights.mp4, thumbnail post-1-launch.png.", { when: "11:00 ET", jump: "li-1", links: [vid("reel-highlights"), pub("linkedin/post-1-launch.png")] }],
      ["d1-li1-pin", "Pin L1's first comment right after it publishes.", { jump: "li-1-comment" }],
      ["d1-t1-text", "Theme 1 on X, Bluesky, Threads, Facebook and Pinterest (image posts; X 12:00 to 18:00 local).", { jump: "theme-1" }],
      ["d1-t1-video", "Theme 1 on Instagram, TikTok and YouTube Shorts with reel-highlights.mp4 (Instagram 13:00 to 19:00, TikTok 14:00 to 18:00 local). YouTube: publish the launch video.", { jump: "cap-1-instagram", links: [vid("reel-highlights")] }],
      ["d1-mastodon", "Mastodon post (reel-highlights.mp4 with alt text).", { jump: "masto" }],
      ["d1-newsletter", "Newsletter and Discord announcement (links only).", { jump: "nl-body" }],
    ],
  },
  {
    id: "d2", title: "Wed 28 Oct: Show HN day",
    note: "One community post that needs you per day. Show HN goes out at 12:00 UTC (12:00 UK, 08:00 ET). Stay four to six hours.",
    items: [
      ["d2-hn-submit", "Show HN at 12:00 UTC: submit https://edytlab.com (no UTM) with the recommended title.", { when: "12:00 UTC", jump: "hn-title-0" }],
      ["d2-hn-comment", "Post the first comment as soon as the submission is up.", { jump: "hn-comment" }],
      ["d2-hn-stay", "Stay in the thread for four to six hours; answers are ready below the comment.", { jump: "hn-answers" }],
      ["d2-t2-text", "Theme 2 on X, Bluesky, Threads, Facebook and Pinterest.", { jump: "theme-2" }],
      ["d2-t2-video", "Theme 2 on Instagram, TikTok and Shorts with reel-beatmatched-transition.mp4.", { jump: "cap-2-instagram", links: [vid("reel-beatmatched-transition")] }],
    ],
  },
  {
    id: "d3", title: "Thu 29 Oct",
    note: "L2 at 11:00 ET (15:00 UK). r/rust comes after Hacker News has found the problems.",
    items: [
      ["d3-li2", "LinkedIn page L2 at 11:00 ET / 15:00 UK: post 2 with reel-beatmatched-transition.mp4, thumbnail post-2-before-after.png.", { when: "11:00 ET", jump: "li-2", links: [vid("reel-beatmatched-transition"), pub("linkedin/post-2-before-after.png")] }],
      ["d3-li2-pin", "Pin L2's first comment.", { jump: "li-2-comment" }],
      ["d3-t4", "Theme 4 (your audio stays on your computer) on every own feed except LinkedIn; image only.", { jump: "theme-4" }],
      ["d3-rust", "r/rust engineering notes (read the sidebar first; stay in the thread).", { jump: "rd-rust" }],
    ],
  },
  {
    id: "d4", title: "Fri 30 Oct",
    note: "Suggested day for these two; the kit asks only that Reddit posts come after Show HN and never share a day with it.",
    items: [
      ["d4-localllama", "r/LocalLLaMA local-model data request (after Show HN and r/rust).", { jump: "rd-localllama" }],
      ["d4-x-thread", "X thread with reel-highlights.mp4 on post 1 (links in the last post).", { jump: "x-1", links: [vid("reel-highlights")] }],
      ["d4-replies", "Answer every comment; file issues for what people found; fix anything a post got wrong.", {}],
    ],
  },
  {
    id: "w2a", title: "Mon 2 and Tue 3 Nov: Product Hunt",
    note: "Product Hunt goes live at 12:01am Pacific, which is 08:01 UK on 3 Nov. Give it its own community day. L3 is a document post: the carousel PDF.",
    items: [
      ["w2-t3", "Mon 2 Nov: theme 3 (extend a club intro) on own feeds; Instagram, TikTok and Shorts with reel-extended-club-intro.mp4.", { jump: "theme-3", links: [vid("reel-extended-club-intro")] }],
      ["w2-youtube-long", "YouTube long form: the three full DJ recordings back to back (check the chapter times against the upload).", { jump: "yt-long" }],
      ["w2-ph-live", "Tue 3 Nov, 00:01 PT: Product Hunt launch live (name, tagline, description, topics, gallery, YouTube video).", { when: "00:01 PT", jump: "dest-ph" }],
      ["w2-ph-comment", "Post the maker's first comment.", { jump: "ph-comment" }],
      ["w2-ph-stay", "Be present for the first several hours and answer every comment with specifics. Pin the unsigned-build note in your first reply to anyone who reports an install warning. No counts of users, downloads or stars anywhere.", {}],
      ["w2-li3", "LinkedIn page L3 at 11:00 ET / 16:00 UK: post 3 as a document post, post-3-carousel.pdf titled \"How edytlab is built\" (delete the one video sentence).", { when: "11:00 ET", jump: "li-3-doc", links: [pub("linkedin/post-3-carousel.pdf")] }],
      ["w2-li3-pin", "Pin L3's first comment.", { jump: "li-3-comment" }],
    ],
  },
  {
    id: "w2b", title: "Wed 4 to Fri 6 Nov",
    note: "Music communities come after you know how the project reads to strangers. Drop any whose sidebar forbids self-promotion.",
    items: [
      ["w2-djs", "Wed 4 Nov: r/DJs show-and-tell (reel-beatmatched-transition.mp4 or text).", { jump: "rd-dj", links: [vid("reel-beatmatched-transition")] }],
      ["w2-li4", "Thu 5 Nov: LinkedIn page L4 at 11:00 ET / 16:00 UK: post 4 with reel-extended-club-intro.mp4, thumbnail post-4-privacy.png.", { when: "11:00 ET", jump: "li-4", links: [vid("reel-extended-club-intro"), pub("linkedin/post-4-privacy.png")] }],
      ["w2-li4-pin", "Pin L4's first comment.", { jump: "li-4-comment" }],
      ["w2-t5", "Thu 5 to Fri 6 Nov: theme 5 (every edit is a version) on own feeds except LinkedIn; link goes to /videos.", { jump: "theme-5" }],
      ["w2-edm", "Fri 6 Nov: r/edmproduction (reel-extended-club-intro.mp4).", { jump: "rd-edm", links: [vid("reel-extended-club-intro")] }],
    ],
  },
  {
    id: "w3", title: "Mon 9 to Thu 12 Nov",
    note: "L5 and L6 are on Tuesday and Thursday at 11:00 ET (16:00 UK).",
    items: [
      ["w3-t6", "Mon 9 Nov: theme 6 (free and open source) on own feeds except LinkedIn; link GitHub.", { jump: "theme-6" }],
      ["w3-wtmm", "Mon 9 Nov: r/WeAreTheMusicMakers (reel-extended-club-intro.mp4 or text).", { jump: "rd-wtmm" }],
      ["w3-li5", "Tue 10 Nov: LinkedIn page L5 at 11:00 ET / 16:00 UK: post 5 with reel-teaser.mp4, thumbnail post-5-help-wanted.png.", { when: "11:00 ET", jump: "li-5", links: [vid("reel-teaser"), pub("linkedin/post-5-help-wanted.png")] }],
      ["w3-li5-pin", "Pin L5's first comment.", { jump: "li-5-comment" }],
      ["w3-opensource", "Wed 11 Nov: r/opensource contributors wanted, if the sidebar allows it (link GitHub, not a reel).", { jump: "rd-opensource" }],
      ["w3-li6", "Thu 12 Nov: LinkedIn page L6 at 11:00 ET / 16:00 UK: the theme 5 LinkedIn caption without its link line, with the 1200x627 image.", { when: "11:00 ET", jump: "li-6", links: [lk("5-every-edit-is-a-version__linkedin-1200x627.png", "img/5-every-edit-is-a-version__linkedin-1200x627.png")] }],
      ["w3-li6-pin", "Pin L6's first comment (the /videos link with its UTM).", { jump: "li-6-comment" }],
      ["w3-review", "Review: GitHub release downloads, stars, issues opened. Decide what to post next. Fill the record at the end of calendar.md.", {}],
    ],
  },
];
const CHECK_ITEMS = [];
const checklistHtml = DAYS.map((d) => {
  const lis = d.items.map(([id, label, o]) => {
    CHECK_ITEMS.push(id);
    const links = [];
    if (o.jump) links.push(`<a href="#${o.jump}">Jump to text</a>`);
    for (const l of o.links || []) links.push(l.href.startsWith("#") ? `<a href="${l.href}">${esc(l.t)}</a>` : `<a href="${esc(l.href)}" target="_blank" rel="noopener">${esc(l.t)}</a>`);
    return `<li><label><input type="checkbox" data-ck="${id}"><span class="ck-label">${o.when ? `<span class="when">${esc(o.when)}</span>` : ""}${esc(label)}</span></label><span class="ck-links">${links.join("")}</span></li>`;
  }).join("");
  return `<div class="day" id="day-${d.id}"><h3>${esc(d.title)} <small class="day-count"></small></h3><p class="daynote">${esc(d.note)}</p><ul class="ck">${lis}</ul></div>`;
}).join("");
const JUMPS = DAYS.flatMap((d) => d.items.flatMap(([, , o]) => [o.jump, ...(o.links || []).filter((l) => l.href.startsWith("#")).map((l) => l.href.slice(1))].filter(Boolean)));

// ---------------------------------------------------------------- 2. posts
// --- Product Hunt
function phHtml() {
  const nameSec = section("launch/product-hunt.md", /^Name$/).find((l) => l.trim()) || "";
  const tagline = pick(PH, { h2: /^Tagline/ });
  const alts = section("launch/product-hunt.md", /^Tagline/).filter((l) => /^- `/.test(l)).map((l) => l.replace(/^- `|`$/g, ""));
  const desc = pick(PH, { h2: /^Description/ });
  const first = pick(PH, { h2: /^Maker's first comment/ });
  const gal = tableRows(section("launch/product-hunt.md", /^Gallery/));
  const topics = bullets(section("launch/product-hunt.md", /^Topics/));
  const notes = bullets(section("launch/product-hunt.md", /^Launch-day notes/));
  const galleryOptionA = [1, 2, 3, 5, 4].map((n) => {
    const t = cap[n - 1];
    const p = `img/${n}-${slugOf(t)}__youtube-1280x720.png`;
    return `<figure><a href="${p}" target="_blank" rel="noopener"><img src="${p}" alt="Theme ${n}: ${esc(t.title)}, 1280x720" loading="lazy" width="1280" height="720" style="--ar:16/9"></a><figcaption><b>${[1, 2, 3, 5, 4].indexOf(n) + 1}. ${esc(t.title)}</b><a href="${p}" target="_blank" rel="noopener">Open 1280x720</a></figcaption></figure>`;
  }).join("");
  return `<div class="dest" id="dest-ph">${h3("Product Hunt")}${sourceNote("launch/product-hunt.md")}
<div class="note info"><strong>Where it goes:</strong> Website field ${ext("https://edytlab.com", "edytlab.com")} (UTM <code>utm_source=producthunt</code> is allowed there), download link ${ext("https://github.com/laadtushar/edytlab/releases/latest", "releases/latest")}, source ${ext("https://github.com/laadtushar/edytlab", "github.com/laadtushar/edytlab")}. Live 12:01am Pacific, on its own day. Check the live form before you paste: field names and limits change.</div>
${card("ph-name", "Name", nameSec.trim())}
${card("ph-tagline", "Tagline (60 or fewer)", tagline, { limit: 60 })}
<div class="card"><p><strong>Alternates</strong></p></div>
${alts.map((a, i) => card("ph-tagline-alt-" + i, "Tagline alternate " + (i + 1), a, { limit: 60 })).join("")}
${card("ph-desc", "Description (260 or fewer)", desc, { limit: 260 })}
${card("ph-comment", "Maker's first comment", first, { nolimit: true })}
${h3("Gallery plan", "ph-gallery")}
<p class="sub">The repo has no screenshots made for the listing. Two ways to fill five slots; Product Hunt recommends 1270x760, needs two or more images, and takes video as a YouTube link only (see <code>images/LAUNCH.md</code>).</p>
<h4>Option A: the theme images at 1280x720, in this order</h4>
<div class="gal">${galleryOptionA}</div>
<p class="sub">Keep the "Real app, real Claude" labels. Put the launch video's YouTube link in the video slot. Expect a little letterboxing: none of the images is exactly 1270x760.</p>
<h4>Option B: capture from the running v0.9.2 app (1280x776), as product-hunt.md plans</h4>
${table(gal)}
<h4>Topics</h4>${ul(topics)}
<h4>Launch-day notes</h4>${ul(notes)}</div>`;
}
const THEME_SLUGS = ["describe-the-edit", "dj-transition-in-one-sentence", "extend-a-club-intro", "your-audio-stays-on-your-computer", "every-edit-is-a-version", "free-and-open-source"];
function slugOf(t) { return THEME_SLUGS[t.n - 1]; }

// --- Show HN
function hnHtml() {
  const moreTitles = bullets(section("social-kit/more.md", /^6\. Show HN/).filter((l) => /^\d\. `/.test(l)));
  const hnaTitles = bullets(section("launch/show-hn.md", /^Title/).filter((l) => /^\d\. `/.test(l)));
  const raw = [...hnaTitles, ...moreTitles].map((l) => /^`(.+)` \((\d+)\)$/.exec(l)).filter(Boolean).map((m) => m[1]);
  const titles = [...new Set(raw)];
  const REC = "Show HN: edytlab – desktop audio editor where an LLM calls 94 DSP tools";
  if (!titles.includes(REC)) throw new Error("recommended HN title not found");
  const first = pick(MORE, { h2: /^6\. Show HN/, label: /^First comment/ });
  const qa = tableRows(section("social-kit/more.md", /^6\. Show HN/));
  const alt = (() => {
    const ls = section("launch/show-hn.md", /^First comment/);
    const a = ls.indexOf("---"), b = ls.indexOf("---", a + 1);
    return ls.slice(a + 1, b).join("\n").trim();
  })();
  const altQa = tableRows(section("launch/show-hn.md", /^Questions/));
  return `<div class="dest" id="dest-hn">${h3("Show HN")}${sourceNote("social-kit/more.md", "(section 6, refreshed for v0.9.2), with launch/show-hn.md as the alternate")}
<div class="note info"><strong>Where it goes:</strong> submit ${ext("https://edytlab.com", "https://edytlab.com")} with no UTM, at <strong>12:00 UTC</strong>. Post the first comment as soon as it is up and stay for several hours. Never on the same day as a Reddit post. Do not ask anyone to upvote.</div>
${titles.map((t, i) => card("hn-title-" + (i === titles.indexOf(REC) ? 0 : i + 1), "Title" + (t === REC ? "" : " option"), t, { limit: 80, badge: t === REC ? "recommended" : "" })).join("")}
${card("hn-comment", "First comment (post it yourself, right after submitting)", first, { nolimit: true })}
${h3("Answers to have ready", "hn-answers")}
<p class="sub">Likely questions, with the answer from more.md. Copy the answer cell.</p>
${qa.slice(1).map((r, i) => card("hn-a-" + i, r[0], r[1], { nolimit: true })).join("")}
<details class="alt"><summary>Alternate: the longer first comment from launch/show-hn.md</summary>${card("hn-comment-alt", "First comment, longer version", alt, { nolimit: true, badge: "alternate" })}
<p class="sub">Its questions table (alternate answers):</p>${table(altQa)}</details></div>`;
}

// --- Reddit
const SUBS = [
  ["rust", "6. r/rust"], ["localllama", "5. r/LocalLLaMA"], ["dj", "1. r/DJs"], ["opensource", "4. r/opensource"], ["edm", "3. r/edmproduction"], ["wtmm", "2. r/WeAreTheMusicMakers"],
];
function redditHtml() {
  const before = bullets(section("social-kit/reddit.md", /^Before every post/));
  const sub = SUBS.map(([slug, h]) => {
    const sec = section("social-kit/reddit.md", new RegExp("^" + h.replace(/[.\/]/g, "\\$&") + "$")).join("\n");
    const title = pick(RD, { h2: new RegExp("^" + h.replace(/[.\/]/g, "\\$&") + "$"), label: /^Title/ });
    const body = pick(RD, { h2: new RegExp("^" + h.replace(/[.\/]/g, "\\$&") + "$"), label: /^Body/ });
    const attach = labelled(sec, "Attach"), flair = labelled(sec, "Suggested flair"), rules = labelled(sec, "Rules note");
    const name = h.replace(/^\d\. /, "");
    return `<div id="rd-${slug}">${h3(name)}
<div class="note"><strong>Rules note.</strong> ${inline(rules)}</div>
<p class="sub"><strong>Attach:</strong> ${inline(attach)}<br><strong>Flair:</strong> ${inline(flair)}</p>
${card(`rd-${slug}-title`, `${name}: title`, title, { limit: 300 })}${card(`rd-${slug}-body`, `${name}: body`, body, { limit: 40000 })}</div>`;
  }).join("");
  return `<div class="dest" id="dest-reddit">${h3("Reddit")}${sourceNote("social-kit/reddit.md", "(six posts, in the order the calendar runs them)")}
<div class="note" id="rd-rules"><strong>Self-promotion rules, before every post.</strong>${ul(before)}<p>No live sidebar could be read when this copy was written: treat every rules note below as unverified and re-read the rules the day you post.</p></div>${sub}</div>`;
}

// --- LinkedIn
function linkedinHtml() {
  const pages = [1, 2, 3, 4, 5];
  const heads = ["1. Launch: what it is", "2. The DJ workflow, before and after", "3. The build story", "4. Privacy and local-first, with the Ollama option", "5. Open source: help wanted"];
  const sched = ["L1 Tue 27 Oct 11:00 ET (15:00 UK)", "L2 Thu 29 Oct 11:00 ET (15:00 UK)", "L3 Tue 3 Nov 11:00 ET (16:00 UK)", "L4 Thu 5 Nov 11:00 ET (16:00 UK)", "L5 Tue 10 Nov 11:00 ET (16:00 UK)"];
  const attach = ["reel-highlights.mp4", "reel-beatmatched-transition.mp4", "post-3-carousel.pdf (document post)", "reel-extended-club-intro.mp4", "reel-teaser.mp4"];
  const thumbs = ["linkedin/post-1-launch.png", "linkedin/post-2-before-after.png", "", "linkedin/post-4-privacy.png", "linkedin/post-5-help-wanted.png"];
  let out = "";
  pages.forEach((n, i) => {
    const h2 = new RegExp("^" + heads[i].replace(/[.\/]/g, "\\$&"));
    const post = pick(LI, { h2 });
    const pin = pick(LI, { h2, label: /^Pin this comment/ });
    const meta = `<p><strong>${esc(sched[i])}.</strong> Attach ${esc(attach[i])}${thumbs[i] ? `; thumbnail <a href="${thumbs[i]}" target="_blank" rel="noopener">${thumbs[i].split("/").pop()}</a>` : ""}. Link goes in the pinned comment, not the body (post 1 has one in the body).</p>`;
    out += `<div id="li-${n}">${h3(`LinkedIn ${n}: ${heads[i].replace(/^\d\. /, "")}`)}`;
    out += card(`li-${n}-post`, `Post ${n}`, post, { limit: 3000, meta });
    if (n === 3) {
      const SENT = " The video shows it sequencing a three-track mix with them.";
      if (!post.includes(SENT)) throw new Error("L3 video sentence not found");
      out += card("li-3-doc", "Post 3 as a document post (carousel PDF)", post.replace(SENT, ""), { limit: 3000, badge: "derived", meta: `<p>Derived: <code>linkedin-posts.md</code> says to delete the one sentence that says what the video shows when you post the PDF instead of the reel. Nothing else changed. Document title: "How edytlab is built".</p>` });
    }
    if (n === 1) {
      out += card("li-1-short", "Shorter version (under 90 words), same video", pick(LI, { h2, label: /^Shorter version/ }), { limit: 3000 });
    }
    out += card(`li-${n}-comment`, `Post ${n}: pin this comment`, pin, { limit: 1250 });
    out += `</div>`;
  });
  // L6
  const t5 = cap[4].caps.LinkedIn;
  const tl = t5.split("\n");
  const urlLine = tl[tl.length - 1];
  if (!/^https:\/\/edytlab\.com\/videos$/.test(urlLine)) throw new Error("theme 5 LinkedIn caption does not end in the videos link");
  const body6 = tl.slice(0, -1).join("\n");
  const com6 = "https://edytlab.com/videos?utm_source=linkedin&utm_medium=social&utm_campaign=launch&utm_content=versions";
  out += `<div id="li-6">${h3("LinkedIn 6: Every edit is a version (theme 5)")}
${card("li-6-post", "L6 post body (theme 5 LinkedIn caption, link line removed)", body6, { limit: 3000, badge: "derived", meta: `<p><strong>L6 Thu 12 Nov 11:00 ET (16:00 UK).</strong> Image: <a href="img/5-every-edit-is-a-version__linkedin-1200x627.png" target="_blank" rel="noopener">5-every-edit-is-a-version__linkedin-1200x627.png</a>. The caption as it stands in captions.md is under Images, theme 5. LinkedIn takes the link in the first comment, so its last line is removed here, as issue #728 says.</p>` })}
${card("li-6-comment", "L6: first comment", com6, { limit: 1250, badge: "derived" })}</div>`;
  // alternate post
  const alt = pick(LIA, { h2: /^Post/ });
  const altc = pick(LIA, { h2: /^First comment/ });
  out += `<details class="alt"><summary>Alternate: the single v0.9.2 DJ-demo post (social/linkedin.md)</summary>${card("lia-post", "Alternate post: v0.9.2 and the DJ demo", alt, { limit: 3000, badge: "alternate" })}${card("lia-comment", "Alternate post: first comment", altc, { limit: 1250, badge: "alternate" })}<p class="sub">Not in the L1 to L6 schedule. Use it only instead of one of those, never as well (no text is posted twice).</p></details>`;
  return `<div class="dest" id="dest-linkedin">${h3("LinkedIn page")}${sourceNote("social-kit/linkedin-posts.md", "(posts 1 to 5) and social-kit/images/captions.md (L6). Times are from issue #728.")}${out}</div>`;
}

// --- X
function xHtml() {
  const h2 = /^1\. X thread/;
  const labels = [/^1\/7/, /^2\/7/, /^3\/7/, /^4\/7/, /^5\/7/, /^6\/7/, /^7\/7/];
  let out = labels.map((re, i) => card(`x-${i + 1}`, `Post ${i + 1} of 7`, pick(MORE, { h2, label: re }), { limit: 280, mode: "x", badge: i === 0 ? "attach reel-highlights.mp4" : "" })).join("");
  out += card("x-single", "Single post (attach reel-teaser.mp4 or reel-highlights.mp4)", pick(MORE, { h2, label: /^Single post/ }), { limit: 280, mode: "x" });
  let alt = "";
  for (let i = 1; i <= 8; i++) alt += card(`xa-${i}`, `Alternate thread: post ${i} of 8`, pick(XT, { label: new RegExp("^" + i + "/8") }), { limit: 280, mode: "x", badge: i === 1 ? "attach the 2:45 DJ demo" : "" });
  return `<div class="dest" id="dest-x">${h3("X thread")}${sourceNote("social-kit/more.md", "(section 1). Each post is counted against 280, and a link counts as 23. The theme captions for X are under Images.")}${out}
<details class="alt"><summary>Alternate: the 8-post thread built around the v0.9.2 release (social/x-thread.md)</summary>${alt}<p class="sub">Use one thread or the other, not both. Check its post 6 against the latest release notes before posting.</p></details></div>`;
}

// --- Instagram / TikTok / Shorts / YouTube
function igHtml() {
  const bio = pick(IG, { label: /^Bio line/ });
  const heads = [/^1\. `reel-highlights/, /^2\. `reel-beatmatched/, /^3\. `reel-extended/, /^4\. `reel-mini-mix/, /^5\. `reel-teaser/];
  const names = ["reel-highlights.mp4 (the lead reel)", "reel-beatmatched-transition.mp4", "reel-extended-club-intro.mp4", "reel-mini-mix.mp4", "reel-teaser.mp4"];
  let out = card("ig-bio", "Bio line (under 150 characters)", bio, { limit: 150 });
  heads.forEach((re, i) => {
    out += `<div id="ig-${i + 1}">${h3(`Instagram ${i + 1}: ${names[i]}`)}${card(`ig-${i + 1}-caption`, "Caption", pick(IG, { h2: re, label: /^Caption/ }), { limit: 2200 })}${card(`ig-${i + 1}-alt`, "Alt text", pick(IG, { h2: re, label: /^Alt text/ }), { nolimit: true })}</div>`;
  });
  const stories = bullets(section("social-kit/instagram.md", /^Story frames/));
  out += h3("Story frames") + ul(stories);
  // TikTok / Shorts
  const tk = [["reel-highlights.mp4", "reel-highlights"], ["reel-beatmatched-transition.mp4", "reel-beatmatched-transition"], ["reel-extended-club-intro.mp4", "reel-extended-club-intro"], ["reel-mini-mix.mp4", "reel-mini-mix"], ["reel-teaser.mp4 (12 to 15 seconds)", "reel-teaser"]];
  let t = "";
  tk.forEach(([nm, slug], i) => {
    const h2 = /^4\. TikTok and YouTube Shorts/, h3re = new RegExp("^" + slug + "\\.mp4");
    t += `<div id="tt-${slug}">${h3(`TikTok and Shorts: ${nm}`)}${card(`tt-${slug}-tiktok`, "TikTok caption", pick(MORE, { h2, h3: h3re, label: /^TikTok/ }), { limit: 2200 })}${card(`tt-${slug}-title`, "YouTube Shorts title (under 100)", pick(MORE, { h2, h3: h3re, label: /^(YouTube )?Shorts title/ }), { limit: 100 })}${card(`tt-${slug}-desc`, "YouTube Shorts description", pick(MORE, { h2, h3: h3re, label: /^(YouTube )?Shorts description/ }), { limit: 5000 })}</div>`;
  });
  return { ig: out, tt: t };
}
function ytLong() {
  const h2 = /^5\. YouTube long-form/;
  const titles = bullets(section("social-kit/more.md", h2).filter((l) => /^\d\. `/.test(l))).map((s) => s.replace(/^`|`$/g, ""));
  const body = MORE.filter((b) => h2.test(b.h2));
  return `<div id="yt-long">${h3("YouTube long form: the three full recordings")}${titles.map((t, i) => card("yt-title-" + i, "Title option " + (i + 1), t, { limit: 100 })).join("")}${card("yt-desc", "Description (with chapters)", body[0].text, { limit: 5000 })}${card("yt-tags", "Tags (comma separated)", body[1].text, { nolimit: true })}<p class="sub">The chapter times (0:00, 2:45, 4:40) are arithmetic on the durations; check them against the real upload.</p></div>`;
}

// --- Bluesky, Mastodon, Threads
function socialHtml() {
  const bsky = pick(MORE, { h2: /^2\. Bluesky/ });
  const masto = pick(MORE, { h2: /^3\. Mastodon/ });
  const alt = pick(IG, { h2: /^1\. `reel-highlights/, label: /^Alt text/ });
  return `<div class="dest" id="dest-social">${h3("Bluesky, Mastodon and Threads")}${sourceNote("social-kit/more.md", "(sections 2 and 3)")}
${card("bsky", "Bluesky (300 or fewer; attach reel-highlights.mp4)", bsky, { limit: 300 })}
${card("masto", "Mastodon (500 or fewer; attach reel-highlights.mp4 and add alt text)", masto, { limit: 500 })}
<p class="sub">Alt text for the Mastodon video, from instagram.md:</p>${card("masto-alt", "Mastodon: alt text", alt, { nolimit: true })}
<div class="note info"><strong>Threads</strong> has no post of its own in the kit: use the Threads caption of the day's theme (500 or fewer), under <a href="#images">Images</a>. Bluesky also has a caption for each theme there.</div></div>`;
}

// --- Newsletter and Discord
function newsletterHtml() {
  const h2 = /^7\. Newsletter/;
  const subj = bullets(section("social-kit/more.md", h2).filter((l) => /^\d\. `/.test(l))).map((s) => s.replace(/^`|`$/g, ""));
  return `<div class="dest" id="dest-news">${h3("Newsletter and Discord")}${sourceNote("social-kit/more.md", "(section 7)")}
${subj.map((s, i) => card("nl-subj-" + i, "Subject option " + (i + 1), s, { nolimit: true })).join("")}
${card("nl-body", "Newsletter body", pick(MORE, { h2, label: /^Body/ }), { nolimit: true })}
${card("discord", "Discord (short)", pick(MORE, { h2, label: /^Discord/ }), { limit: 2000 })}</div>`;
}

// --- Press kit
function pressHtml() {
  const one = section("press-kit.md", /^One-sentence description/).join("\n").trim();
  const boiler = section("press-kit.md", /^Boilerplate/).filter((l) => l.trim());
  const bio = section("press-kit.md", /^Short bios by length/).filter((l) => /^\*\*\d+ words:\*\*/.test(l));
  const facts = tableRows(section("press-kit.md", /^Fact sheet/));
  const notShipped = bullets(section("press-kit.md", /^Fact sheet/).join("\n").split("### What is not shipped")[1].split("###")[0].split("\n"));
  const lines1 = section("press-kit.md", /^Boilerplate/);
  const boilerText = lines1.join("\n").trim();
  const quote = section("press-kit.md", /^Approved descriptions/).filter((l) => /^>/.test(l)).map((l) => l.replace(/^> ?/, "")).join(" ").replace(/\s+/g, " ").trim();
  const linksTop = [];
  return `<div class="dest" id="dest-press">${h3("Press kit")}${sourceNote("press-kit.md")}
${card("press-one", "One-sentence description", one, { nolimit: true, meta: "<p>The file wraps the sentence at 80 columns; the line break is kept here, as in the file.</p>" })}
${card("press-boiler", "Boilerplate", boilerText, { nolimit: true, meta: "<p>The file wraps the paragraph at 80 columns; the line breaks are kept here, as in the file.</p>" })}
${bio.map((b, i) => card("press-bio-" + i, b.match(/\d+ words/)[0] + " bio", b.replace(/^\*\*\d+ words:\*\* /, ""), { nolimit: true })).join("")}
${card("press-quote", "Approved description of the demo (the only one to quote)", quote, { nolimit: true, badge: "derived", meta: "<p>The blockquote in the file, with its line breaks joined into one paragraph.</p>" })}
<h4>Fact sheet</h4>${table(facts)}
<h4>What is not shipped (state this plainly if asked)</h4>${ul(notShipped)}</div>`;
}

// ---------------------------------------------------------------- 3. images
function imagesHtml() {
  const nav = cap.map((t) => `<button type="button" data-n="${t.n}" aria-pressed="${t.n === 1}">${t.n}. ${esc(t.title)}</button>`).join("");
  const themes = cap.map((t) => {
    const slug = `${t.n}-${slugOf(t)}`;
    const vp = /Videos? page: <([^>]+)>/.exec(t.image);
    const videoPage = vp ? vp[1] : "";
    const sizeOpts = SIZES.map((s) => `<option value="${s}"${s === "x-1600x900" ? " selected" : ""}>${s}</option>`).join("");
    const caps = PLATFORMS.map(([name, src, size, limit, mode, desc]) => {
      const text = t.caps[name];
      if (!text) throw new Error(`theme ${t.n}: no ${name} caption`);
      const lower = name.toLowerCase();
      return card(`cap-${t.n}-${lower}`, `${name}`, text, { limit, mode, desc, utm: `${src}|${THEME_UTM[t.n - 1]}`, meta: `<p>Image: <a class="pv" data-size="${size}" href="img/${slug}__${size}.png" target="_blank" rel="noopener">${size}</a></p>` });
    }).join("");
    return `<div class="theme" id="theme-${t.n}" data-theme-n="${t.n}" data-slug="${slug}">
<div class="prev"><img src="img/${slug}__x-1600x900.png" width="1600" height="900" alt="Theme ${t.n}: ${esc(t.title)}" loading="lazy">
<div class="row"><label>Size <select aria-label="Image size">${sizeOpts}</select></label><a class="openlink" href="img/${slug}__x-1600x900.png" target="_blank" rel="noopener">Open</a></div>
<div class="sizes">${SIZES.map((s) => `<a href="img/${slug}__${s}.png" target="_blank" rel="noopener">${s}</a>`).join("")}</div>
<p class="sub">Video: ${esc(THEME_VIDEO[t.n - 1])}.${videoPage ? " Watch page: " + ext(videoPage, videoPage.replace(/^https?:\/\//, "")) : ""}</p></div>
<div class="caps"><h3 style="margin-top:0">${t.n}. ${esc(t.title)}</h3>${caps}</div></div>`;
  }).join("");
  const liFiles = [
    ["post-1-launch.png", "Post 1: launch", "L1"], ["post-2-before-after.png", "Post 2: before and after", "L2"],
    ["post-3-model-never-touches-a-sample.png", "Post 3: the model never touches a sample", "L3 (alongside the carousel)"], ["post-4-privacy.png", "Post 4: privacy", "L4"], ["post-5-help-wanted.png", "Post 5: help wanted", "L5"],
  ];
  const lig = liFiles.map(([f, t, l]) => `<figure><a href="linkedin/${f}" target="_blank" rel="noopener"><img src="linkedin/${f}" alt="${esc(t)}" loading="lazy" width="1200" height="1200" style="--ar:1"></a><figcaption><b>${esc(t)}</b>${esc(l)}, 1200x1200<div class="links"><a href="linkedin/${f}" target="_blank" rel="noopener">Open</a></div></figcaption></figure>`).join("")
    + `<figure><a href="linkedin/post-3-carousel.pdf" target="_blank" rel="noopener"><div class="pdf" aria-hidden="true">PDF</div></a><figcaption><b>Post 3 carousel</b>Seven 1080x1350 pages; post as a document, title "How edytlab is built"<div class="links"><a href="linkedin/post-3-carousel.pdf" target="_blank" rel="noopener">Open</a></div></figcaption></figure>`;
  return `<section class="sec" id="images"><h2>Images</h2>
<p class="sub">Six themes at seven sizes, with one caption per platform (captions.md), and the LinkedIn page images. Every frame carries its "Real app, real Claude" or "recorded live" label: crop nothing.</p>
<div class="utm"><input type="checkbox" id="utm-toggle"><label for="utm-toggle">Add UTM tags to edytlab.com links when copying a theme caption (<code>utm_source=&lt;platform&gt;&amp;utm_medium=social&amp;utm_campaign=launch&amp;utm_content=&lt;theme&gt;</code>). Off: the copy is the repo text exactly. Never on Show HN, Reddit or GitHub links.</label></div>
<div class="pills themenav" role="group" aria-label="Theme">${nav}</div>
${themes}
<h3 id="linkedin-images">LinkedIn page images</h3>
<p class="sub">From <code>marketing/social-kit/linkedin/out/</code>. The chat in each is a scripted example conversation in the real app, and each image says so.</p>
<div class="gal">${lig}</div></section>`;
}

// ---------------------------------------------------------------- 4. videos
function videosHtml() {
  const vts = fs.readFileSync(path.join(REPO, "website/lib/videos.ts"), "utf8");
  const dts = fs.readFileSync(path.join(REPO, "website/lib/demos.ts"), "utf8");
  const entries = (src) => src.split(/\n\s*slug: "/).slice(1).map((chunk) => {
    const slug = chunk.slice(0, chunk.indexOf('"'));
    const g = (k) => { const m = new RegExp("\\n\\s*" + k + ": \"?([^\",\\n]+)").exec(chunk); return m ? m[1] : ""; };
    return { slug, title: g("title"), seconds: +g("seconds"), width: +g("width"), height: +g("height") };
  });
  const V1 = entries(vts), D1 = entries(dts);
  const want = [
    ["launch-video", V1, "videos", "Launch video", "Theme 1 (describe the edit); the YouTube upload for the Product Hunt gallery."],
    ["reel-highlights", V1, "videos", "Highlights reel", "DAY-1 VERTICAL VIDEO. LinkedIn L1; Instagram, TikTok and Shorts on day 1; X thread post 1; Bluesky and Mastodon."],
    ["reel-beatmatched-transition", V1, "videos", "Beatmatch reel", "LinkedIn L2; theme 2; r/DJs."],
    ["reel-extended-club-intro", V1, "videos", "Extended intro reel", "LinkedIn L4; theme 3; r/edmproduction and r/WeAreTheMusicMakers."],
    ["reel-mini-mix", V1, "videos", "Mini-mix reel", "Instagram, TikTok and Shorts caption #4."],
    ["reel-teaser", V1, "videos", "Teaser reel", "LinkedIn L5; the X single post."],
    ["dj-beatmatched-transition", D1, "demos", "DJ demo 1", "Full recording with sound; the Product Hunt gallery video; LinkedIn alternate post; the alternate X thread."],
    ["dj-extended-club-intro", D1, "demos", "DJ demo 2", "Full recording with sound; YouTube long form."],
    ["dj-mini-mix", D1, "demos", "DJ demo 3", "Full recording with sound; YouTube long form."],
  ];
  const fmt = (s) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, "0")}`;
  const cards = want.map(([slug, list, folder, kind, used]) => {
    const e = list.find((x) => x.slug === slug);
    if (!e) throw new Error("no video entry " + slug);
    const src = path.join(REPO, `website/public/${folder}/${slug}.jpg`);
    addFile(`posters/${slug}.jpg`, src);
    const w = e.width || 1280, h = e.height || 776;
    const orient = h > w ? "vertical 9:16" : "horizontal";
    const mp4 = folder === "demos" ? `${SITE}/demos/${slug}.mp4` : `${SITE}/videos/${slug}.mp4`;
    const day1 = slug === "reel-highlights" ? '<span class="badge">Day-1 vertical video</span>' : "";
    return `<figure id="v-${slug}"><a href="${mp4}" target="_blank" rel="noopener"><img src="posters/${slug}.jpg" alt="Poster of ${esc(e.title)}" loading="lazy" width="${w}" height="${h}" style="--ar:${w}/${h}"></a><figcaption>${day1}<b>${esc(e.title)}</b>${kind}, ${fmt(e.seconds)}, ${orient}<br>${esc(used.replace("DAY-1 VERTICAL VIDEO. ", ""))}<div class="links"><a href="${mp4}" target="_blank" rel="noopener">Watch / download</a><a href="${SITE}/videos/${slug}" target="_blank" rel="noopener">Watch page</a></div></figcaption></figure>`;
  }).join("");
  return `<section class="sec" id="videos"><h2>Videos</h2>
<p class="sub">Nine videos, all on edytlab.com. "Watch / download" opens the MP4 (right-click to save). The reels carry their captions on screen and the "Real app, real Claude" label: post them uncropped. Videos that the site no longer serves are not listed.</p>
<div class="gal">${cards}</div></section>`;
}

// ---------------------------------------------------------------- 5. where / when / rules
function infoHtml() {
  const L = "social-kit/images/LAUNCH.md";
  const where = tableRows(section(L, /^Which canvas/));
  const when = tableRows(section(L, /^When to post/));
  const whenBullets = bullets(section(L, /^When to post/));
  const util = tableRows(section(L, /^Link format/));
  const dont = bullets(section(L, /^Don't/));
  const principles = bullets(section("social-kit/calendar.md", /^Principles/));
  const sprout = section(L, /^When to post/).find((l) => /^Sprout Social 2026/.test(l)) || "";
  const li = [
    ["L1", "Tue 27 Oct", "11:00", "15:00", "1. Launch: what it is", "reel-highlights.mp4", "post-1-launch.png"],
    ["L2", "Thu 29 Oct", "11:00", "15:00", "2. The DJ workflow, before and after", "reel-beatmatched-transition.mp4", "post-2-before-after.png"],
    ["L3", "Tue 3 Nov", "11:00", "16:00", "3. The build story, as a document post", "post-3-carousel.pdf (title \"How edytlab is built\")", ""],
    ["L4", "Thu 5 Nov", "11:00", "16:00", "4. Privacy and local-first, with the Ollama option", "reel-extended-club-intro.mp4", "post-4-privacy.png"],
    ["L5", "Tue 10 Nov", "11:00", "16:00", "5. Open source: help wanted", "reel-teaser.mp4", "post-5-help-wanted.png"],
    ["L6", "Thu 12 Nov", "11:00", "16:00", "Theme 5, LinkedIn caption (link in first comment)", "5-every-edit-is-a-version__linkedin-1200x627.png", ""],
  ];
  return `<section class="sec" id="where"><h2>Where each goes</h2>
<p class="sub">Which canvas, image or video, and where the link goes (<code>images/LAUNCH.md</code>). Video-first platforms post the matching reel and use the 1080x1920 image as its cover.</p>${table(where)}</section>
<section class="sec" id="when"><h2>When to post</h2>
<h3 style="margin-top:8px">The schedule (issue #728)</h3>
<p class="sub">LinkedIn page posts, in US Eastern with UK beside it. The UK moved to GMT on 25 Oct and the US moves to EST on 1 Nov, so the gap is 4 hours until 1 Nov and 5 hours after.</p>
${table([["#", "Date", "ET", "UK", "Post", "Attach", "Thumbnail"], ...li])}
<div class="note info"><strong>Own feeds</strong> (day 1 to 12 Nov, from #728): Tue 27 Oct theme 1; Wed 28 Oct theme 2; Thu 29 Oct theme 4; Mon 2 to Tue 3 Nov theme 3; Thu 5 to Fri 6 Nov theme 5 (link /videos); Mon 9 to Wed 11 Nov theme 6 (link GitHub). The day-1 vertical video is <code>reel-highlights.mp4</code>. LinkedIn's themes 1 to 4 captions are not posted: the page has its own five posts and L6.<br><strong>Community posts</strong> (Show HN, Reddit, Product Hunt) are yours to submit: Show HN Wed 28 Oct 12:00 UTC; Product Hunt Tue 3 Nov 12:01am PT; Reddit one a day, after Show HN, never on its day. The Reddit dates in the checklist are suggestions that keep to those rules.</div>
<h3>Best times by platform</h3>${table(when)}<p class="sub">${inline(sprout)}</p>${ul(whenBullets)}
<h3>Link format</h3>${table(util)}</section>
<section class="sec" id="rules"><h2>Rules</h2>
<h3 style="margin-top:8px">Don't</h3>${ul(dont)}
<h3>Principles of the calendar</h3>${ul(principles)}
<p class="sub">Facts every post respects: free and open source (MIT); macOS, Windows and Linux; the installers are unsigned; audio is edited on your computer and never uploaded, only the chat goes to the provider you choose; 6 API providers plus your ChatGPT plan, Ollama runs locally with no key; 94 tools; every edit is a node in a history graph. Stem separation is not shipped.</p></section>`;
}

// ---------------------------------------------------------------- assemble
function build() {
  fs.rmSync(OUT, { recursive: true, force: true });
  fs.mkdirSync(OUT, { recursive: true });

  // files to publish
  for (const f of ["post-1-launch.png", "post-2-before-after.png", "post-3-model-never-touches-a-sample.png", "post-4-privacy.png", "post-5-help-wanted.png", "post-3-carousel.pdf"]) {
    addFile(`linkedin/${f}`, path.join(MK, "social-kit/linkedin/out", f));
  }
  const existing = [];
  for (const t of cap) for (const s of SIZES) existing.push(`img/${t.n}-${slugOf(t)}__${s}.png`);
  for (const e of existing) if (!fs.existsSync(path.join(MK, "social-kit/images/out", e.replace("img/", "")))) throw new Error("theme image missing in the repo: " + e);

  const images = imagesHtml();
  const videos = videosHtml();
  const ig = igHtml();
  const posts = [
    ["ph", "Product Hunt", phHtml()],
    ["hn", "Show HN", hnHtml()],
    ["reddit", "Reddit", redditHtml()],
    ["linkedin", "LinkedIn", linkedinHtml()],
    ["x", "X thread", xHtml()],
    ["ig", "Instagram, TikTok, Shorts", `<div class="dest" id="dest-ig">${h3("Instagram, TikTok and YouTube Shorts")}${sourceNote("social-kit/instagram.md", "and social-kit/more.md (sections 4 and 5). Instagram does not make URLs in captions clickable: every call to action says link in bio.")}${ig.ig}${ig.tt}${ytLong()}</div>`],
    ["social", "Bluesky, Mastodon, Threads", socialHtml()],
    ["news", "Newsletter, Discord", newsletterHtml()],
    ["press", "Press kit", pressHtml()],
  ];
  const css = fs.readFileSync(path.join(HERE, "launch-hub.css"), "utf8");
  const js = fs.readFileSync(path.join(HERE, "launch-hub.client.js"), "utf8");

  const html = `<title>edytlab Launch Kit</title>
<meta name="claude:viewport-fit-sides" content="cover">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=Geist+Mono:wght@400;500&display=swap">
<style>
${css}</style>
<div class="wrap">
<header>
<p class="eyebrow">edytlab / launch kit / v0.9.2</p>
<h1>Launch edytlab with <span>real footage</span>.</h1>
<p class="lede">Every post, image and video for the relaunch, and the checklist to run it, in one place. Each text has a copy button and a count against its platform's limit. Nothing here posts anywhere by itself, and nobody is asked to upvote, star or share.</p>
<div class="facts" aria-label="Facts every post respects"><span>Free and open source (MIT)</span><span>macOS, Windows, Linux</span><span>94 tools</span><span>6 API providers plus your ChatGPT plan</span><span>Unsigned installers</span><span>Audio never uploaded</span><span>No stem separation yet</span></div>
</header>
<nav class="top" aria-label="Sections"><div class="navrow"><a href="#checklist" aria-current="true">Checklist</a><a href="#posts">Posts</a><a href="#images">Images</a><a href="#videos">Videos</a><a href="#where">Where it goes</a><a href="#when">When to post</a><a href="#rules">Rules</a></div></nav>
<section class="sec" id="checklist"><h2>Checklist</h2>
<p class="sub">Every launch step, by day. Times are from issue #728 (LinkedIn page) and the calendar. Tick as you go.</p>
<div class="progress"><strong id="ck-count" aria-live="polite">0 of ${CHECK_ITEMS.length} done</strong><div class="bar" aria-hidden="true"><i id="ck-bar"></i></div></div>
<p id="ck-note">Sign in to claude.ai to save ticks. Until then they are kept in this browser.</p>
${checklistHtml}
</section>
<section class="sec" id="posts"><h2>Posts</h2>
<p class="sub">Text is copied from the repo's marketing files, character for character. Pick a destination.</p>
<div class="pills destnav" role="group" aria-label="Destination">${posts.map(([k, l]) => `<button type="button" data-dest="dest-${k}">${esc(l)}</button>`).join("")}</div>
${posts.map(([, , h]) => h).join("\n")}
</section>
${images}
${videos}
${infoHtml()}
<footer>Generated by <code>marketing/social-kit/artifact/launch-hub.mjs</code> from the markdown in <code>marketing/</code>. To change a text, edit the markdown and run the script again.</footer>
</div>
<script>
${js}</script>
`;
  // checks
  for (const j of JUMPS) if (!ids.has(j) && !html.includes(`id="${j}"`)) throw new Error("checklist jump target missing: " + j);
  for (const bad of ["whats-new-v070", "reel-v070"]) if (html.includes(bad)) throw new Error("stale reference " + bad);

  fs.writeFileSync(path.join(OUT, "index.html"), html);
  for (const [pubPath, src] of Object.entries(files)) {
    const dst = path.join(OUT, pubPath);
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.copyFileSync(src, dst);
  }
  const map = Object.fromEntries(Object.keys(files).sort().map((p) => [p, path.join(OUT, p)]));
  fs.writeFileSync(path.join(OUT, "files.json"), JSON.stringify(map, null, 2) + "\n");
  fs.writeFileSync(path.join(OUT, "existing-files.txt"), existing.join("\n") + "\n");
  fs.writeFileSync(path.join(OUT, "checklist-ids.txt"), CHECK_ITEMS.join("\n") + "\n");

  console.log(`wrote ${path.relative(REPO, path.join(OUT, "index.html"))}: ${html.length} bytes, ${cardN} copy cards, ${CHECK_ITEMS.length} checklist items, ${Object.keys(files).length} files to publish, ${existing.length} existing theme images`);
}
build();
