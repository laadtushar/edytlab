#!/usr/bin/env node
/**
 * Render the LinkedIn visuals.
 *
 *   node marketing/social-kit/linkedin/render.mjs              squares and carousel, from the committed shots
 *   node marketing/social-kit/linkedin/render.mjs --shots      take the app screenshots first (builds the frontend)
 *   node marketing/social-kit/linkedin/render.mjs --only post-4-privacy
 *
 * Needs `pnpm install` at the repo root (Playwright is apps/desktop's
 * dev dependency) and the Chromium Playwright installs. Nothing else, and
 * no network. Writes into `out/`: five 1200x1200 PNGs and one carousel PDF
 * of 1080x1350 pages.
 *
 * Every canvas goes through `lint.mjs` before it is written: safe area,
 * clipping, minimum text size and 4.5:1 contrast. A canvas that fails
 * stops the render with the list of what is wrong.
 */

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { lintCanvas } from "./lint.mjs";
import { appVersion, logoSvg, toolCount, tokensCss } from "./tokens.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, "../../..");
const DESKTOP = join(REPO, "apps/desktop");

const args = process.argv.slice(2);
const only = args.includes("--only") ? args[args.indexOf("--only") + 1] : null;

/** The five posts' squares, in post order (linkedin-posts.md numbering). */
const SQUARES = [
  { id: "post-1-launch" },
  { id: "post-2-before-after" },
  { id: "post-3-model-never-touches-a-sample" },
  { id: "post-4-privacy" },
  { id: "post-5-help-wanted" },
];

/** The carousel: post 3's, as a document. */
const CAROUSEL = { id: "post-3-carousel", pages: { w: 1080, h: 1350 }, minPages: 6, maxPages: 8 };

const MIN_FONT = { square: 24, page: 28 };

// ---- screenshots ----------------------------------------------------------

if (args.includes("--shots")) {
  console.log("taking the app screenshots (builds the production frontend, then drives it)...");
  const run = spawnSync(
    "pnpm",
    ["exec", "playwright", "test", "-c", "../../marketing/social-kit/linkedin/shots.config.mjs"],
    { cwd: DESKTOP, stdio: "inherit", shell: process.platform === "win32" },
  );
  if (run.status !== 0) process.exit(run.status ?? 1);
}

// ---- compose --------------------------------------------------------------

const { chromium } = createRequire(join(DESKTOP, "package.json"))("@playwright/test");

const facts = { "{{version}}": appVersion(), "{{tools}}": String(toolCount()), "{{logo}}": logoSvg() };

function compose(name) {
  const body = readFileSync(join(HERE, "templates", `${name}.html`), "utf8");
  const html = body
    .replace("{{tokens}}", `<style>\n${tokensCss()}\n</style>`)
    .replace("{{base}}", `<style>\n${readFileSync(join(HERE, "base.css"), "utf8")}\n</style>`)
    .replaceAll(/\{\{(version|tools|logo)\}\}/g, (m) => facts[m]);
  const left = /\{\{\w+\}\}/.exec(html);
  if (left) throw new Error(`${name}.html: unfilled placeholder ${left[0]}`);
  mkdirSync(join(HERE, ".build"), { recursive: true });
  const file = join(HERE, ".build", `${name}.html`);
  writeFileSync(file, html);
  return pathToFileURL(file).href;
}

async function ready(page) {
  // `load` rather than `check`: a face is fetched when first used, and a
  // template that does not use the mono would otherwise read as "not loaded".
  const faces = await page.evaluate(async () => ({
    inter: (await document.fonts.load('600 40px "Inter"')).length,
    mono: (await document.fonts.load('500 20px "Geist Mono Variable"')).length,
  }));
  if (!faces.inter || !faces.mono) throw new Error(`a typeface did not load: ${JSON.stringify(faces)}`);
  await page.evaluate(() => document.fonts.ready);
  // Images decoded: a half-loaded screenshot is a blank hole in the render.
  await page.evaluate(() =>
    Promise.all([...document.images].map((i) => i.decode().catch(() => { throw new Error(`image failed: ${i.src}`); }))),
  );
}

/**
 * The site's gradient headline is `background-clip: text`, which the PDF
 * writer draws as a clipped box, and a viewer can show the box's bottom
 * edge as a hairline under the line. For the PDF only, each gradient span
 * becomes an inline SVG `<text>` filled with the same gradient (read from
 * the computed style, so it is still the site's): vector, selectable text,
 * no clip, same size and place.
 */
async function gradientTextAsSvg(page) {
  const replaced = await page.evaluate(() => {
    const NS = "http://www.w3.org/2000/svg";
    const ctx = document.createElement("canvas").getContext("2d");
    let n = 0;
    for (const span of document.querySelectorAll(".gradient-text")) {
      const style = getComputedStyle(span);
      const rects = [...span.getClientRects()];
      if (rects.length !== 1) throw new Error(`gradient text wraps, cannot be made an SVG: "${span.textContent}"`);
      const stops = [...style.backgroundImage.matchAll(/(rgba?\([^)]+\))\s*([\d.]+%)?/g)].map((m) => ({ color: m[1], at: m[2] }));
      const angle = /(\d+)deg/.exec(style.backgroundImage)?.[1] ?? "180";
      ctx.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
      ctx.letterSpacing = style.letterSpacing;
      const text = span.textContent;
      const m = ctx.measureText(text);
      const ascent = m.fontBoundingBoxAscent;
      const descent = m.fontBoundingBoxDescent;
      const width = Math.ceil(m.width + 1);
      const height = Math.ceil(ascent + descent);

      const svg = document.createElementNS(NS, "svg");
      svg.setAttribute("width", width);
      svg.setAttribute("height", height);
      svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
      // Take the line box the span took: an inline box is its content area
      // plus half the leading (line-height minus content) above and below.
      const lineHeight = parseFloat(style.lineHeight) || (ascent + descent);
      const halfLeading = (lineHeight - (ascent + descent)) / 2;
      // An inline replaced box sits on its bottom *margin* edge, which the
      // negative margin lifts by the half-leading: lower it by that much.
      svg.style.cssText = `display:inline-block;vertical-align:${-(descent + halfLeading)}px;overflow:visible;margin:${halfLeading}px 0`;
      const id = `gt${n++}`;
      const defs = document.createElementNS(NS, "defs");
      const grad = document.createElementNS(NS, "linearGradient");
      grad.setAttribute("id", id);
      grad.setAttribute("gradientUnits", "objectBoundingBox");
      // CSS angles run clockwise from "up"; an SVG gradient vector from (x1,y1) to (x2,y2).
      const a = (Number(angle) * Math.PI) / 180;
      grad.setAttribute("x1", 0.5 - Math.sin(a) / 2);
      grad.setAttribute("y1", 0.5 + Math.cos(a) / 2);
      grad.setAttribute("x2", 0.5 + Math.sin(a) / 2);
      grad.setAttribute("y2", 0.5 - Math.cos(a) / 2);
      for (const s of stops) {
        const stop = document.createElementNS(NS, "stop");
        stop.setAttribute("offset", s.at ?? "0%");
        stop.setAttribute("stop-color", s.color);
        grad.appendChild(stop);
      }
      defs.appendChild(grad);
      const t = document.createElementNS(NS, "text");
      t.setAttribute("x", 0);
      t.setAttribute("y", ascent);
      t.setAttribute("fill", `url(#${id})`);
      t.setAttribute("style", `font:${style.fontWeight} ${style.fontSize} ${style.fontFamily};letter-spacing:${style.letterSpacing};white-space:pre`);
      t.textContent = text;
      svg.append(defs, t);
      span.replaceWith(svg);
    }
    return n;
  });
  return replaced;
}

const kb = (file) => `${(statSync(file).size / 1024).toFixed(0)} KB`;

async function main() {
  mkdirSync(join(HERE, "out"), { recursive: true });
  const browser = await chromium.launch();
  const failures = [];
  const written = [];
  try {
    const scratch = await (await browser.newContext()).newPage();

    for (const sq of SQUARES) {
      if (only && only !== sq.id) continue;
      const context = await browser.newContext({ viewport: { width: 1200, height: 1200 }, deviceScaleFactor: 1 });
      const page = await context.newPage();
      await page.goto(compose(sq.id));
      await ready(page);
      const png = await page.locator(".canvas").screenshot({ type: "png" });
      const result = await lintCanvas(page, scratch, png, { rootSelector: ".canvas", minFont: MIN_FONT.square, label: sq.id });
      report(result, failures);
      const file = join(HERE, "out", `${sq.id}.png`);
      writeFileSync(file, png);
      written.push(`${file} (${kb(file)})`);
      await context.close();
    }

    if (!only || only === CAROUSEL.id) {
      const { w, h } = CAROUSEL.pages;
      const context = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
      const page = await context.newPage();
      await page.goto(compose(CAROUSEL.id));
      await ready(page);
      const count = await page.locator(".canvas.page").count();
      if (count < CAROUSEL.minPages || count > CAROUSEL.maxPages) {
        throw new Error(`the carousel has ${count} pages; LinkedIn's sweet spot here is ${CAROUSEL.minPages} to ${CAROUSEL.maxPages}`);
      }
      for (let i = 0; i < count; i++) {
        const png = await page.locator(".canvas.page").nth(i).screenshot({ type: "png" });
        report(await lintCanvas(page, scratch, png, { rootSelector: ".canvas.page", index: i, minFont: MIN_FONT.page, label: `${CAROUSEL.id} page ${i + 1}` }), failures);
        if (process.env.KEEP_PAGES) {
          mkdirSync(join(HERE, ".build", "pages"), { recursive: true });
          writeFileSync(join(HERE, ".build", "pages", `page-${i + 1}.png`), png);
        }
      }
      const file = join(HERE, "out", `${CAROUSEL.id}.pdf`);
      // Print media, as the PDF is made.
      await page.emulateMedia({ media: "print" });
      await gradientTextAsSvg(page);
      await page.pdf({ path: file, width: `${w}px`, height: `${h}px`, printBackground: true, margin: { top: 0, right: 0, bottom: 0, left: 0 } });
      const pdfPages = (readFileSync(file, "latin1").match(/\/Type\s*\/Page[^s]/g) ?? []).length;
      if (pdfPages !== count) failures.push(`${CAROUSEL.id}.pdf has ${pdfPages} pages, the template has ${count}`);
      written.push(`${file} (${kb(file)}, ${pdfPages} pages)`);
      await context.close();
    }
  } finally {
    await browser.close();
  }

  console.log(written.join("\n"));
  if (failures.length) {
    console.error(`\n${failures.length} problem(s):\n- ${failures.join("\n- ")}`);
    process.exit(1);
  }
}

function report({ label, problems, texts, worstContrast, worstText }, failures) {
  const worst = worstContrast === null ? "n/a" : `${worstContrast.toFixed(1)}:1`;
  console.log(`${problems.length ? "FAIL" : "ok  "} ${label}: ${texts} text runs, lowest contrast ${worst} ("${worstText}")`);
  for (const p of problems) failures.push(`${label}: ${p}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
