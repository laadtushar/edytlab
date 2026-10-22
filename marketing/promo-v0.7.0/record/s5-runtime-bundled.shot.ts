/**
 * S5. The ONNX Runtime line in Settings.
 *
 * Settings, Account tab: the line at its foot says the runtime is bundled. The
 * backend's answer is the shape `ml_runtime_status` returns for a library the
 * installer put there: `commands.rs` serialises `MlRuntimeStatusDto::Found
 * { version, path, source: Bundled }` as `{ state: "found", version, path,
 * source: "bundled" }` (the Rust test `a_found_runtime_is_reported_with_its_
 * version_path_and_source` pins that), the version is the one `ml-runtime.json`
 * pins (1.23.2), and the path is where a .deb install's resource directory puts
 * `libonnxruntime.so` (the place `e2e/backend.ts` already lists first for a
 * Linux install). What the line says is `lib/mlRuntimeLine.ts`'s wording, not
 * ours. It names no model and no feature: a runtime that loads is not stem
 * separation or transcription, which this release does not ship.
 */

import { expect, test } from "../../../apps/desktop/e2e/fixtures";
import { ok, projectWith, readyToLoad } from "../../../apps/desktop/e2e/backend";

import { LENGTH_SEC, stems, writeFixture } from "./lib/music";
import { writeNotes } from "./lib/notes";
import { SCALE, VIEWPORT, Stage } from "./lib/stage";
import { Studio, loadedTrack } from "./lib/studio";

test.use({ viewport: VIEWPORT, deviceScaleFactor: SCALE });

const NAME = "s5-runtime-bundled";

/** `MlRuntimeStatusDto::Found` for a bundled library, as serialised. */
const BUNDLED = {
  state: "found",
  version: "1.23.2",
  path: "/usr/lib/edytlab/libonnxruntime.so",
  source: "bundled",
} as const;

test(NAME, async ({ app }) => {
  const page = app.page;
  const stage = await Stage.prepare(app);

  const s = stems();
  const studio = new Studio();
  studio.tracks = [
    loadedTrack(1, "Backing", writeFixture("Music/demo-song/backing.wav", s.backing), LENGTH_SEC),
    loadedTrack(2, "Vocal", writeFixture("Music/demo-song/vocal.wav", s.vocal), LENGTH_SEC),
  ];
  studio.commit("load", { agoSec: 140 });
  studio.commit("load", { agoSec: 125 });

  // The e2e backend's model list is the one for a profile with no key saved
  // (`source: "fallback"`, with a notice saying so). This user has a key, so
  // the same models are answered as the provider's own list (`source: "live"`,
  // no notice), which is what `list_models_for` returns once a key is saved.
  const models = (readyToLoad().list_models_for as { ok: { models: unknown[] } }).ok.models;
  await app.boot({
    ...projectWith(studio.tracks, studio.head),
    ...studio.answers(),
    list_models_for: ok({ models, source: "live", notice: null }),
    ml_runtime_status: ok(BUNDLED),
  });
  await expect(page.getByTestId("timeline-lane")).toHaveCount(2);
  await app.settle();
  await page.evaluate(() => document.fonts.ready);

  await stage.start(NAME);
  await stage.wait(900);

  await stage.click(page.getByTestId("open-settings-button"));
  stage.mark("settings opening");
  await expect(page.getByTestId("settings-card")).toBeVisible();
  await stage.wait(1300);

  // The line is at the foot of the Account tab's scrolling body.
  const line = page.getByTestId("settings-ml-runtime");
  await expect(line).toHaveText("ONNX Runtime: 1.23.2 (bundled)");
  const body = page.getByTestId("settings-body");
  const box = (await body.boundingBox())!;
  const overflow = await body.evaluate((el) => el.scrollHeight - el.clientHeight);
  if (overflow > 2) {
    await stage.wheel({ x: box.x + box.width / 2, y: box.y + box.height / 2 }, overflow, 1500);
  } else {
    await stage.hover(line, { at: { x: 0.97 }, moveMs: 700 });
  }
  stage.mark("runtime line in view");
  await stage.wait(3200);

  await expect(page.getByTestId("settings-ml-runtime-detail")).toHaveText("/usr/lib/edytlab/libonnxruntime.so");
  const done = await stage.finish();
  writeNotes(NAME, { ...done, status: BUNDLED, text: await line.textContent(), scrolledPx: overflow });
});
