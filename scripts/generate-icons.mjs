// Generates every logo asset in the repo from one geometry definition.
//
//     npm i --no-save sharp && node scripts/generate-icons.mjs
//
// `sharp` is deliberately not a dependency of anything: this runs when the
// mark changes, which is close to never, and adding a native image library
// to the workspace to support that would be a poor trade. Nothing in the
// build imports this file.
//
// The mark ("Take Two"): three amplitude bars — the material you loaded —
// then two waveforms leaving in different directions, the two versions you
// made of it. Bars in, waves out.
//
// Two decisions here came from measuring rather than taste:
//
//  * The mark is placed by its *ink* box, not its viewBox. Round stroke
//    caps push past the path coordinates, so centring on the viewBox sits
//    the artwork visibly low and left.
//  * App icons get a tile; the favicon does not. At 16px the tile's inset
//    and corner radius cost enough of the canvas that the mark collapses
//    into a violet blob. Bare on transparent, the same mark still resolves
//    into bars and two diverging arms.

import sharp from "sharp";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const REPO = new URL("..", import.meta.url).pathname.replace(/\/$/, "");
const TAURI = join(REPO, "apps/desktop/src-tauri/icons");
const PUB = join(REPO, "website/public");
const APP = join(REPO, "website/app");

// ── palette ───────────────────────────────────────────────────────────
// Web mark: mid-tone violets, so one file reads on white and on near-black.
const WEB_A = "#9D4EF5";
const WEB_B = "#6248E8";
// Tile: the app's own ground, so the brighter dark-theme pair applies.
const TILE_BG = "#0B0A0F";
const TILE_A = "#A656F6";
const TILE_B = "#7C5CF0";

// ── geometry (64×64 grid) ─────────────────────────────────────────────
const STROKE = 6;
const mark = (a, b) => `
  <g fill="${a}">
    <rect x="5"  y="26" width="6" height="12" rx="3"/>
    <rect x="14" y="20" width="6" height="24" rx="3"/>
    <rect x="23" y="25" width="6" height="14" rx="3"/>
  </g>
  <path d="M34 26 L41 18 L48 23 L57 12" fill="none" stroke="${a}"
        stroke-width="${STROKE}" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M34 38 L41 46 L48 41 L57 52" fill="none" stroke="${b}"
        stroke-width="${STROKE}" stroke-linecap="round" stroke-linejoin="round"/>`;

// Ink bounds: bars span x 5→29 and y 20→44; the arms span x 34→57 and
// y 12→52, each grown by half the stroke for the round caps.
const CAP = STROKE / 2;
const BOX = [5, 12 - CAP, 57 + CAP, 52 + CAP];

/** Transform that fits the ink box to `frac` of an S×S canvas, centred. */
function fit(S, frac) {
  const k = (frac * S) / (BOX[2] - BOX[0]);
  return {
    k,
    dx: S / 2 - ((BOX[0] + BOX[2]) / 2) * k,
    dy: S / 2 - ((BOX[1] + BOX[3]) / 2) * k,
  };
}

/** Mark alone, transparent ground. */
function markSvg(size, a = WEB_A, b = WEB_B, frac = 0.96) {
  const S = 1024;
  const { k, dx, dy } = fit(S, frac);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${S} ${S}">
  <g transform="translate(${dx.toFixed(2)},${dy.toFixed(2)}) scale(${k.toFixed(4)})">${mark(a, b)}</g>
</svg>`;
}

/**
 * Mark on a filled ground. `radius` 0 gives a hard square, which is what
 * iOS wants — it applies its own mask and a transparent or pre-rounded
 * icon shows corner artefacts.
 */
function tileSvg(size, radius = 0.225) {
  const S = 1024;
  const { k, dx, dy } = fit(S, 0.82);
  const r = Math.round(S * radius);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${S} ${S}">
  <rect width="${S}" height="${S}" rx="${r}" ry="${r}" fill="${TILE_BG}"/>
  <g transform="translate(${dx.toFixed(2)},${dy.toFixed(2)}) scale(${k.toFixed(4)})">${mark(TILE_A, TILE_B)}</g>
</svg>`;
}

const png = (svg) => sharp(Buffer.from(svg)).png({ compressionLevel: 9 }).toBuffer();
const tile = (size) => png(tileSvg(size));

// ── ICO ───────────────────────────────────────────────────────────────
// A container of PNGs: 6-byte header, one 16-byte directory entry per
// image, then the data. Written by hand — there is no icotool here and
// this is twenty lines.
function buildIco(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // 1 = icon
  header.writeUInt16LE(images.length, 4);

  const dir = Buffer.alloc(16 * images.length);
  let offset = 6 + 16 * images.length;
  images.forEach(({ size, data }, i) => {
    const o = i * 16;
    dir.writeUInt8(size >= 256 ? 0 : size, o); // 0 encodes 256
    dir.writeUInt8(size >= 256 ? 0 : size, o + 1);
    dir.writeUInt16LE(1, o + 4); // colour planes
    dir.writeUInt16LE(32, o + 6); // bits per pixel
    dir.writeUInt32LE(data.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += data.length;
  });
  return Buffer.concat([header, dir, ...images.map((i) => i.data)]);
}

// ── ICNS ──────────────────────────────────────────────────────────────
// 'icns' + total length, then [OSType][length including this header][PNG].
// macOS 10.7 and later accept PNG payloads for these types.
function buildIcns(entries) {
  const chunks = entries.map(({ type, data }) => {
    const head = Buffer.alloc(8);
    head.write(type, 0, 4, "ascii");
    head.writeUInt32BE(data.length + 8, 4);
    return Buffer.concat([head, data]);
  });
  const body = Buffer.concat(chunks);
  const head = Buffer.alloc(8);
  head.write("icns", 0, 4, "ascii");
  head.writeUInt32BE(body.length + 8, 4);
  return Buffer.concat([head, body]);
}

// ── run ───────────────────────────────────────────────────────────────
const written = [];
const write = (path, buf) => {
  writeFileSync(path, buf);
  written.push(`${path.replace(REPO + "/", "")}  ${buf.length.toLocaleString()} B`);
};

mkdirSync(TAURI, { recursive: true });

write(join(TAURI, "32x32.png"), await tile(32));
write(join(TAURI, "128x128.png"), await tile(128));
write(join(TAURI, "128x128@2x.png"), await tile(256));
write(join(TAURI, "icon.png"), await tile(512));
for (const s of [30, 44, 71, 89, 107, 142, 150, 284, 310]) {
  write(join(TAURI, `Square${s}x${s}Logo.png`), await tile(s));
}
write(join(TAURI, "StoreLogo.png"), await tile(50));

const ico = [];
for (const size of [16, 24, 32, 48, 64, 128, 256]) ico.push({ size, data: await tile(size) });
write(join(TAURI, "icon.ico"), buildIco(ico));

const icns = [];
for (const [type, size] of [
  ["ic11", 32], ["ic12", 64], ["ic07", 128], ["ic13", 256],
  ["ic08", 256], ["ic14", 512], ["ic09", 512], ["ic10", 1024],
]) icns.push({ type, data: await tile(size) });
write(join(TAURI, "icon.icns"), buildIcns(icns));

// Website. The bare mark for anything shown against the page; a tile for
// anything the OS will place on a ground of its own choosing.
write(join(PUB, "logo.svg"), Buffer.from(markSvg(64)));
write(join(PUB, "logo.png"), await png(markSvg(512)));
write(join(PUB, "icon-192.png"), await tile(192));
write(join(PUB, "icon-512.png"), await tile(512));
write(join(APP, "icon.png"), await png(markSvg(32)));
write(join(APP, "apple-icon.png"), await png(tileSvg(180, 0)));

console.log(written.join("\n"));
console.log(`\n${written.length} files written.`);
