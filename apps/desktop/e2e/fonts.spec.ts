/**
 * The app draws in its own typefaces, from its own bundle (#333).
 *
 * The fonts came from a Google Fonts `@import` placed after
 * `@import "tailwindcss"`. Tailwind expands its import inline, which left
 * the font import behind thousands of rules — and CSS ignores an
 * `@import` that is not at the top. The build dropped it, so Geist,
 * Geist Mono and Instrument Serif never loaded, in dev or in a release,
 * and the brand type fell back to system fonts. Moving the import up
 * would have loaded them from Google on every launch: failing offline,
 * and sending each user's address to a third party.
 *
 * `document.fonts.check()` cannot prove any of this: it answers true when
 * no face matches at all. `document.fonts.load()` returns the faces it
 * actually loaded, so an empty answer means the face does not exist.
 */

import { readyToLoad } from "./backend";
import { expect, test } from "./fixtures";

/** Every face the design tokens in `styles.css` ask for. */
const FACES = [
  '300 16px "Geist Variable"',
  '400 16px "Geist Variable"',
  '700 16px "Geist Variable"',
  '400 16px "Geist Mono Variable"',
  '600 16px "Geist Mono Variable"',
  'normal 400 16px "Instrument Serif"',
  'italic 400 16px "Instrument Serif"',
];

test("every typeface the design uses loads from the app itself", async ({ app }) => {
  const offOrigin: string[] = [];
  app.page.on("request", (req) => {
    const { hostname, protocol } = new URL(req.url());
    const local = hostname === "127.0.0.1" || hostname === "localhost";
    if (!local && (protocol === "http:" || protocol === "https:")) offOrigin.push(req.url());
  });

  await app.boot(readyToLoad());
  const page = app.page;
  await expect(page.getByTestId("empty-state")).toBeVisible();

  for (const face of FACES) {
    const loaded = await page.evaluate(async (f) => {
      const faces = await document.fonts.load(f);
      return faces.filter((ff) => ff.status === "loaded").length;
    }, face);
    expect(loaded, `a loaded face for ${face}`).toBeGreaterThan(0);
  }

  // And the tokens name them, so the loaded faces are the ones drawn.
  const heading = page.getByTestId("empty-state").getByRole("heading", { level: 1 });
  expect(await heading.evaluate((el) => getComputedStyle(el).fontFamily)).toContain(
    "Instrument Serif",
  );
  expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toContain(
    "Geist Variable",
  );

  await page.waitForLoadState("networkidle");
  expect(offOrigin, "requests that left the machine").toEqual([]);
});
