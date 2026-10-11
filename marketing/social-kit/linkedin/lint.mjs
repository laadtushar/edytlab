/**
 * The brand check, run on every canvas before it is written.
 *
 * What it refuses, so the render fails instead of shipping the problem:
 *
 *   - text outside the safe area (a 60 px margin from every edge);
 *   - text clipped by an ancestor with `overflow` set, or cut off by the
 *     canvas edge;
 *   - text under a minimum size (a feed shows a 1200 px image at under
 *     half size, and a carousel page at about a third);
 *   - text with contrast under 4.5:1 against what is drawn behind it,
 *     measured from the rendered pixels (so the aurora and the grain count),
 *     not from the CSS colour of an ancestor;
 *   - a screenshot or panel (`[data-safe]`) outside the safe area.
 *
 * Looking at each render is still part of the job: this catches what a
 * script can measure, not whether the slide is any good.
 */

const SAFE = 60;

/** WCAG 2.x relative luminance of an sRGB colour (0-255 channels). */
function luminance([r, g, b]) {
  const lin = (v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

export function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Composite an rgba foreground over an opaque background. */
function over([r, g, b, a = 1], bg) {
  return [r * a + bg[0] * (1 - a), g * a + bg[1] * (1 - a), b * a + bg[2] * (1 - a)];
}

/** Runs in the page: every text run under `root`, with what a check needs. */
function collect({ rootSelector, index }) {
  const root = document.querySelectorAll(rootSelector)[index];
  const base = root.getBoundingClientRect();
  const rel = (r) => ({ x: r.left - base.left, y: r.top - base.top, w: r.width, h: r.height });

  const parse = (value) => {
    const m = /rgba?\(([^)]+)\)/.exec(value);
    if (!m) return null;
    const [r, g, b, a] = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
    return [r, g, b, a ?? 1];
  };

  const runs = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node.textContent.replace(/\s+/g, " ").trim();
    if (!text) continue;
    const el = node.parentElement;
    if (!el || ["SCRIPT", "STYLE"].includes(el.tagName)) continue;
    const style = getComputedStyle(el);
    if (style.visibility === "hidden" || style.display === "none") continue;

    const range = document.createRange();
    range.selectNodeContents(node);
    const rects = [...range.getClientRects()].filter((r) => r.width > 0.5 && r.height > 0.5);
    if (!rects.length) continue;

    // Clipped by an ancestor that is not `visible`?
    let clippedBy = null;
    for (let a = el; a && a !== root.parentElement; a = a.parentElement) {
      const s = getComputedStyle(a);
      if (a === root || s.overflowX !== "visible" || s.overflowY !== "visible") {
        const box = a.getBoundingClientRect();
        for (const r of rects) {
          if (r.left < box.left - 1 || r.right > box.right + 1 || r.top < box.top - 1 || r.bottom > box.bottom + 1) {
            clippedBy = a === root ? "the canvas edge" : `${a.tagName.toLowerCase()}.${a.className}`;
          }
        }
        if (clippedBy) break;
      }
    }

    // Effective opacity: text with opacity < 1 has a colour no check can trust.
    let opacity = 1;
    for (let a = el; a && a !== root.parentElement; a = a.parentElement) opacity *= Number(getComputedStyle(a).opacity);

    const gradient = style.backgroundClip === "text" || style.webkitBackgroundClip === "text";
    runs.push({
      text: text.slice(0, 48),
      rects: rects.map(rel),
      fontSize: parseFloat(style.fontSize),
      color: gradient ? null : parse(style.webkitTextFillColor !== "rgba(0, 0, 0, 0)" && style.webkitTextFillColor ? style.webkitTextFillColor : style.color),
      // A gradient's colours, for `background-clip: text`.
      stops: gradient ? [...style.backgroundImage.matchAll(/rgba?\([^)]+\)/g)].map((m) => parse(m[0])) : null,
      opacity,
      clippedBy,
      family: style.fontFamily.split(",")[0].replace(/["']/g, "").trim(),
    });
  }

  // A screenshot cut off by its frame (an `overflow: hidden` ancestor that is
  // smaller than it) is a bug, unless the crop is on purpose: `data-crops`.
  const clippedImages = [];
  for (const img of root.querySelectorAll("img")) {
    if (img.closest("[data-crops]")) continue;
    const box = img.getBoundingClientRect();
    for (let a = img.parentElement; a && a !== root.parentElement; a = a.parentElement) {
      const s = getComputedStyle(a);
      if (a !== root && s.overflowX === "visible" && s.overflowY === "visible") continue;
      const r = a.getBoundingClientRect();
      if (box.left < r.left - 1 || box.right > r.right + 1 || box.top < r.top - 1 || box.bottom > r.bottom + 1) {
        clippedImages.push(img.getAttribute("src"));
        break;
      }
    }
  }

  const safe = [...root.querySelectorAll("[data-safe]")].map((el) => ({
    label: el.getAttribute("data-safe") || el.className || el.tagName,
    rect: rel(el.getBoundingClientRect()),
  }));

  return { runs, safe, clippedImages, width: base.width, height: base.height };
}

/**
 * Most common colour in `rect` once pixels near the text colour are set
 * aside. Runs in a scratch page that decodes the screenshot.
 */
async function backgrounds(scratch, png, queries) {
  return scratch.evaluate(
    async ({ b64, queries }) => {
      const blob = await (await fetch(`data:image/png;base64,${b64}`)).blob();
      const bmp = await createImageBitmap(blob);
      const canvas = new OffscreenCanvas(bmp.width, bmp.height);
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      ctx.drawImage(bmp, 0, 0);
      const near = (px, i, c) => Math.abs(px[i] - c[0]) + Math.abs(px[i + 1] - c[1]) + Math.abs(px[i + 2] - c[2]) < 90;
      return queries.map(({ rect, avoid }) => {
        const x = Math.max(0, Math.floor(rect.x - 2));
        const y = Math.max(0, Math.floor(rect.y - 2));
        const w = Math.min(bmp.width - x, Math.ceil(rect.w + 4));
        const h = Math.min(bmp.height - y, Math.ceil(rect.h + 4));
        if (w <= 0 || h <= 0) return null;
        const px = ctx.getImageData(x, y, w, h).data;
        const buckets = new Map();
        for (let i = 0; i < px.length; i += 4) {
          if (avoid.some((c) => near(px, i, c))) continue;
          const key = ((px[i] >> 4) << 8) | ((px[i + 1] >> 4) << 4) | (px[i + 2] >> 4);
          const cur = buckets.get(key) ?? { n: 0, r: 0, g: 0, b: 0 };
          cur.n++;
          cur.r += px[i];
          cur.g += px[i + 1];
          cur.b += px[i + 2];
          buckets.set(key, cur);
        }
        let best = null;
        for (const v of buckets.values()) if (!best || v.n > best.n) best = v;
        return best ? [best.r / best.n, best.g / best.n, best.b / best.n] : null;
      });
    },
    { b64: png.toString("base64"), queries },
  );
}

/**
 * Check one canvas. `rootSelector`/`index` find it on `page`, `png` is its
 * screenshot, `scratch` is any blank page, and `opts.minFont` is the
 * smallest text the canvas may carry.
 */
export async function lintCanvas(page, scratch, png, { rootSelector, index = 0, minFont, label }) {
  const { runs, safe, clippedImages, width, height } = await page.evaluate(collect, { rootSelector, index });
  const problems = [];
  const at = (r) => `${Math.round(r.x)},${Math.round(r.y)} ${Math.round(r.w)}x${Math.round(r.h)}`;

  for (const run of runs) {
    for (const r of run.rects) {
      if (r.x < SAFE - 0.5 || r.y < SAFE - 0.5 || r.x + r.w > width - SAFE + 0.5 || r.y + r.h > height - SAFE + 0.5) {
        problems.push(`outside the ${SAFE}px safe area: "${run.text}" at ${at(r)}`);
        break;
      }
    }
    if (run.clippedBy) problems.push(`clipped by ${run.clippedBy}: "${run.text}"`);
    if (run.fontSize < minFont) problems.push(`under ${minFont}px (${run.fontSize}px): "${run.text}"`);
    if (run.opacity < 1) problems.push(`text drawn with opacity ${run.opacity}: "${run.text}" (use a colour, so contrast can be trusted)`);
    if (run.family !== "Inter" && run.family !== "Geist Mono Variable") problems.push(`unexpected typeface ${run.family}: "${run.text}"`);
  }
  for (const src of clippedImages) problems.push(`screenshot clipped by its frame: ${src}`);
  for (const s of safe) {
    const r = s.rect;
    if (r.x < SAFE - 0.5 || r.y < SAFE - 0.5 || r.x + r.w > width - SAFE + 0.5 || r.y + r.h > height - SAFE + 0.5) {
      problems.push(`${s.label} outside the safe area at ${at(r)}`);
    }
  }

  // Contrast, from pixels.
  const colours = (run) => (run.stops ?? [run.color]).filter(Boolean);
  const queries = runs.flatMap((run) =>
    run.rects.map((rect) => ({ rect, avoid: colours(run).map((c) => c.slice(0, 3)) })),
  );
  const bgs = await backgrounds(scratch, png, queries);
  let i = 0;
  let worst = Infinity;
  let worstText = "";
  for (const run of runs) {
    for (const rect of run.rects) {
      const bg = bgs[i++];
      if (!bg) continue;
      for (const c of colours(run)) {
        const ratio = contrast(over(c, bg), bg);
        if (ratio < worst) {
          worst = ratio;
          worstText = run.text;
        }
        if (ratio < 4.5) problems.push(`contrast ${ratio.toFixed(2)}:1 (needs 4.5): "${run.text}" at ${at(rect)}`);
      }
    }
  }
  return { label, problems, texts: runs.length, worstContrast: worst === Infinity ? null : worst, worstText };
}
