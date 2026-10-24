#!/usr/bin/env node
/**
 * Render the social images: six themes at seven canvas sizes, 42 PNGs in out/.
 *
 *   node marketing/social-kit/images/render.mjs                     all 42
 *   node marketing/social-kit/images/render.mjs --only 4            one theme (its number)
 *   node marketing/social-kit/images/render.mjs --only square       one canvas (id prefix)
 *   node marketing/social-kit/images/render.mjs --sheets <dir>      also write one contact sheet per theme
 *
 * It reuses the LinkedIn kit's pipeline: the website's tokens (tokens.mjs) and
 * the brand check (lint.mjs) are imported from ../linkedin, and base.css is
 * included as it is. The app frames are in frames/, taken from the site's own
 * videos by extract-frames.sh. Needs Playwright and its Chromium (a dev
 * dependency of apps/desktop, or any `playwright` that `require` can find),
 * and no network.
 *
 * Every canvas goes through the LinkedIn lint (safe area, clipping, text of at
 * least 24 px, 4.5:1 contrast from the rendered pixels) and, for the vertical
 * Story canvas, a check that no text is in the top 260 px or the bottom 350 px
 * (the platforms draw their own UI there). A canvas that fails stops the
 * render with the list.
 */

import { createRequire } from "node:module";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { lintCanvas } from "../linkedin/lint.mjs";
import { logoSvg, toolCount, tokensCss } from "../linkedin/tokens.mjs";
import { CANVASES, THEMES } from "./themes.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const LINKEDIN = join(HERE, "../linkedin");
const REPO = join(HERE, "../../..");

const args = process.argv.slice(2);
const flag = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : null);
const only = flag("--only");
const sheetsDir = flag("--sheets");

const MIN_FONT = 24;

function loadPlaywright() {
  for (const from of [import.meta.url, pathToFileURL(join(REPO, "apps/desktop/package.json")).href]) {
    for (const name of ["playwright", "@playwright/test"]) {
      try {
        return createRequire(from)(name).chromium;
      } catch {
        /* try the next */
      }
    }
  }
  throw new Error("Playwright is not installed: run `pnpm install` at the repo root.");
}

/** A PNG's width and height from its header. */
function pngSize(file) {
  const b = readFileSync(file);
  return { w: b.readUInt32BE(16), h: b.readUInt32BE(20) };
}

const tools = String(toolCount());
const fill = (s) => s.replaceAll("{{tools}}", tools);

function tagsHtml(tags = []) {
  return tags
    .map((t) => {
      const side = t.align === "right" ? `right:${((1 - t.x) * 100).toFixed(1)}%` : `left:${(t.x * 100).toFixed(1)}%`;
      const at = t.edge ? "" : `;top:${(t.y * 100).toFixed(1)}%`;
      return `<div class="tag ${t.tone ?? ""} ${t.edge ?? ""}" style="${side}${at}">${t.text}</div>`;
    })
    .join("");
}

function body(theme, canvas) {
  const themeNo = theme.id.split("-")[0];
  const frameFile = `t${themeNo}-${canvas.crop}.png`;
  const { w: iw, h: ih } = pngSize(join(HERE, "frames", frameFile));
  const alt = `A frame of the edytlab app from the site's own recording: ${theme.label.toLowerCase()}.`;
  const shot = `<div class="shot" data-ratio="${(iw / ih).toFixed(4)}" data-safe="app frame"><div class="clip"><img src="../frames/${frameFile}" alt="${alt}" /></div>${tagsHtml(theme.tags?.[canvas.crop])}</div>`;
  const chips = theme.chips ? `<div class="chips">${theme.chips.map((c) => `<span class="pill">${fill(c)}</span>`).join("")}</div>` : "";
  const note = theme.note ? `<p class="note">${theme.note}</p>` : "";
  const urls = theme.url
    ? `<div class="urls"><span class="url">edytlab.com</span><span class="url quiet">${theme.url}</span></div>`
    : `<div class="urls"><span class="url">edytlab.com</span></div>`;
  const foot = `<div class="foot"><span class="label">${theme.label}</span>${urls}</div>`;
  const brand = `<div class="brandbar"><div class="wordmark">{{logo}} edytlab</div></div>`;
  const headline = `<h1 class="display"${canvas.layout === "poster" ? ` data-fit data-cap="${Math.round(canvas.head * (theme.posterScale ?? 1))}" data-maxh="${canvas.headMax}"` : ""}>${theme.headline}</h1>`;
  const lead = theme.sub && !(canvas.tight && theme.chips) ? `<p class="lead">${theme.sub}</p>` : "";

  const style = [
    `--w:${canvas.w}px`, `--h:${canvas.h}px`, `--head:${Math.round(canvas.head * (canvas.layout === "poster" ? 1 : (theme.scale ?? 1)))}px`, `--sub:${canvas.sub}px`,
    `--pad:${canvas.pad}px`, `--gap:${canvas.gap}px`,
    canvas.col ? `--col:${canvas.col}` : "",
    canvas.safeTop ? `--pad-top:${canvas.safeTop}px` : "",
    canvas.safeBottom ? `--pad-bottom:${canvas.safeBottom}px` : "",
  ].filter(Boolean).join(";");

  if (canvas.layout === "stack") {
    return `<div class="canvas img stack" style="${style}"><div class="aurora strong"></div><div class="frame" style="gap:var(--gap)">
${brand}
<div class="col"><div class="text">${headline}${lead}${chips}${note}</div><div class="fit${theme.tags?.[canvas.crop]?.some((t) => t.edge) ? " tagged" : ""}">${shot}</div>${foot}</div>
</div></div>`;
  }
  if (canvas.layout === "poster") {
    return `<div class="canvas img poster" style="${style}"><div class="aurora strong"></div><div class="frame">
<div class="col">${brand}<div class="text">${headline}${lead}${chips}${note}</div><div class="fit${theme.tags?.[canvas.crop]?.some((t) => t.edge) ? " tagged" : ""}">${shot}</div>${foot}</div>
</div></div>`;
  }
  return `<div class="canvas img side${canvas.tight ? " tight" : ""}" style="${style}"><div class="aurora strong"></div><div class="frame">
<div class="row"><div class="text-col">${brand}${headline}${lead}${chips}${note}<div class="spacer"></div>${foot}</div><div class="fit${theme.tags?.[canvas.crop]?.some((t) => t.edge) ? " tagged" : ""}">${shot}</div></div>
</div></div>`;
}

function compose(theme, canvas) {
  const template = readFileSync(join(HERE, "templates/image.html"), "utf8");
  const html = template
    .replace("{{title}}", `${theme.id} at ${canvas.id}`)
    .replace("{{tokens}}", () => `<style>\n${tokensCss()}\n</style>`)
    .replace("{{base}}", () => `<style>\n${readFileSync(join(LINKEDIN, "base.css"), "utf8")}\n</style>`)
    .replace("{{images_css}}", () => `<style>\n${readFileSync(join(HERE, "images.css"), "utf8")}\n</style>`)
    .replace("{{body}}", () => body(theme, canvas))
    .replaceAll("{{logo}}", () => logoSvg());
  const left = /\{\{\w+\}\}/.exec(html);
  if (left) throw new Error(`${theme.id}/${canvas.id}: unfilled placeholder ${left[0]}`);
  mkdirSync(join(HERE, ".build"), { recursive: true });
  const file = join(HERE, ".build", `${theme.id}__${canvas.id}.html`);
  writeFileSync(file, html);
  return pathToFileURL(file).href;
}

async function ready(page) {
  const faces = await page.evaluate(async () => ({
    inter: (await document.fonts.load('600 40px "Inter"')).length,
    mono: (await document.fonts.load('500 20px "Geist Mono Variable"')).length,
  }));
  if (!faces.inter || !faces.mono) throw new Error(`a typeface did not load: ${JSON.stringify(faces)}`);
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => Promise.all([...document.images].map((i) => i.decode().catch(() => { throw new Error(`image failed: ${i.src}`); }))));
  await page.waitForFunction(() => document.body.dataset.fitted === "1");
}

/** The platforms draw over the top and bottom of a vertical canvas: no text there. */
async function zoneProblems(page, canvas) {
  if (!canvas.safeTop) return [];
  return page.evaluate(({ top, bottom, h }) => {
    const out = [];
    const walker = document.createTreeWalker(document.querySelector(".canvas"), NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      if (!n.textContent.trim()) continue;
      const r = document.createRange();
      r.selectNodeContents(n);
      for (const q of r.getClientRects()) {
        if (q.top < top - 0.5 || q.bottom > h - bottom + 0.5) out.push(`text in the platform zone (${Math.round(q.top)}-${Math.round(q.bottom)}): "${n.textContent.trim().slice(0, 40)}"`);
      }
    }
    return out;
  }, { top: canvas.safeTop, bottom: canvas.safeBottom, h: canvas.h });
}

const kb = (file) => `${(statSync(file).size / 1024).toFixed(0)} KB`;

async function sheets(browser, written) {
  mkdirSync(sheetsDir, { recursive: true });
  const page = await (await browser.newContext({ viewport: { width: 2400, height: 1500 } })).newPage();
  for (const theme of THEMES) {
    const files = written.filter((w) => w.theme === theme.id);
    if (!files.length) continue;
    const cells = files.map((f) => `<figure><img src="${pathToFileURL(f.file).href}"><figcaption>${f.canvas.id}</figcaption></figure>`).join("");
    const sheetFile = join(HERE, ".build", `sheet-${theme.id}.html`);
    writeFileSync(sheetFile, `<style>body{margin:0;padding:24px;background:#18181b;font:20px sans-serif;color:#ddd}div{display:flex;flex-wrap:wrap;gap:24px;align-items:flex-start}figure{margin:0}img{height:560px;width:auto;display:block;border-radius:8px}figcaption{margin-top:6px}</style><div>${cells}</div>`);
    await page.goto(pathToFileURL(sheetFile).href);
    await page.waitForFunction(() => [...document.images].every((i) => i.complete));
    const out = join(sheetsDir, `contact-${theme.id}.png`);
    await page.screenshot({ path: out, fullPage: true });
    console.log(`sheet ${out}`);
  }
}

async function main() {
  const chromium = loadPlaywright();
  mkdirSync(join(HERE, "out"), { recursive: true });
  const browser = await chromium.launch();
  const failures = [];
  const written = [];
  try {
    const scratch = await (await browser.newContext()).newPage();
    for (const theme of THEMES) {
      for (const canvas of CANVASES) {
        if (only && !(theme.id.startsWith(`${only}-`) || canvas.id.startsWith(only))) continue;
        const context = await browser.newContext({ viewport: { width: canvas.w, height: canvas.h }, deviceScaleFactor: 1 });
        const page = await context.newPage();
        await page.goto(compose(theme, canvas));
        await ready(page);
        const png = await page.locator(".canvas").screenshot({ type: "png" });
        const label = `${theme.id} ${canvas.id}`;
        const result = await lintCanvas(page, scratch, png, { rootSelector: ".canvas", minFont: MIN_FONT, label });
        const problems = [...result.problems, ...(await zoneProblems(page, canvas))];
        const worst = result.worstContrast === null ? "n/a" : `${result.worstContrast.toFixed(1)}:1`;
        console.log(`${problems.length ? "FAIL" : "ok  "} ${label}: ${result.texts} text runs, lowest contrast ${worst}`);
        for (const p of problems) failures.push(`${label}: ${p}`);
        const file = join(HERE, "out", `${theme.id}__${canvas.id}.png`);
        writeFileSync(file, png);
        written.push({ file, theme: theme.id, canvas, size: kb(file) });
        await context.close();
      }
    }
    if (sheetsDir) await sheets(browser, written);
  } finally {
    await browser.close();
  }
  console.log(written.map((w) => `${w.file} (${w.size})`).join("\n"));
  if (failures.length) {
    console.error(`\n${failures.length} problem(s):\n- ${failures.join("\n- ")}`);
    process.exit(1);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
